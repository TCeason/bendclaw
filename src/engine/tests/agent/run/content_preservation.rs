use evotengine::provider::MockProvider;
use evotengine::provider::ProviderError;
use evotengine::provider::StreamConfig;
use evotengine::provider::StreamEvent;
use evotengine::provider::StreamOutcome;
use evotengine::provider::StreamProvider;
use evotengine::types::AgentContext;
use evotengine::*;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

use crate::fixtures::agent_harness::collect_events;
use crate::fixtures::agent_harness::make_config;

struct ChunkedTextProvider {
    text: String,
}

#[async_trait::async_trait]
impl StreamProvider for ChunkedTextProvider {
    async fn stream(
        &self,
        config: StreamConfig,
        tx: mpsc::UnboundedSender<StreamEvent>,
        cancel: CancellationToken,
    ) -> Result<StreamOutcome, ProviderError> {
        let (mock_tx, mut mock_rx) = mpsc::unbounded_channel();
        let outcome = MockProvider::text(self.text.clone())
            .stream(config, mock_tx, cancel)
            .await?;
        while let Some(event) = mock_rx.recv().await {
            match event {
                StreamEvent::TextDelta {
                    content_index,
                    delta,
                } => {
                    for character in delta.chars() {
                        tx.send(StreamEvent::TextDelta {
                            content_index,
                            delta: character.to_string(),
                        })
                        .map_err(|error| ProviderError::Other(error.to_string()))?;
                    }
                }
                event => tx
                    .send(event)
                    .map_err(|error| ProviderError::Other(error.to_string()))?,
            }
        }
        Ok(outcome)
    }
}

fn assistant_text(message: &AgentMessage) -> String {
    match message {
        AgentMessage::Llm(Message::Assistant {
            content,
            stop_reason,
            ..
        }) => {
            assert_eq!(*stop_reason, StopReason::Stop);
            content
                .iter()
                .filter_map(|block| match block {
                    Content::Text { text } => Some(text.as_str()),
                    _ => None,
                })
                .collect()
        }
        _ => panic!("expected assistant message"),
    }
}

#[tokio::test]
async fn assistant_content_preservation_across_stream_finalization_and_context() {
    let cases = [
        "Example:\n\n1. `<system-reminder>` is a tag name.\n\nKeep the rest of the answer.",
        "| # | Content |\n|---|---|\n| 0 | `<system>` tag |\n\nKeep the rest of the answer.",
        "Inline `<system>literal</system>` and `<system-reminder>example</system-reminder>` remain.",
        "```xml\n<system-reminder>example</system-reminder>\n<system>example</system>\n```\nKeep the conclusion.",
        "Plain prose: <system>example</system> and <system-reminder>example</system-reminder>. End.",
        "An unclosed <system-reminder> is discussed here. Keep everything after it.",
        "<system>literal content</system>",
        "Continue: keep this prefix.",
        "Next step: keep this prefix.",
        "Next: keep this prefix.",
        "Status:\nkeep this prefix.",
        "  \nStatus: preserve leading and trailing whitespace.\n  ",
        "<systematic>not a system tag</systematic>",
    ];

    for text in cases {
        let mut config = make_config(MockProvider::text("unused"));
        config.provider = std::sync::Arc::new(ChunkedTextProvider { text: text.into() });
        let mut context = AgentContext {
            system_prompt: "test".into(),
            messages: Vec::new(),
            tools: Vec::new(),
            cwd: std::path::PathBuf::new(),
            path_guard: std::sync::Arc::new(PathGuard::open()),
            prompt_cache_key: None,
        };
        let (tx, rx) = mpsc::unbounded_channel();
        let messages = agent_loop(
            vec![AgentMessage::Llm(Message::user("Explain the example."))],
            &mut context,
            &config,
            tx,
            CancellationToken::new(),
        )
        .await;
        let events = collect_events(rx);
        let streamed: String = events
            .iter()
            .filter_map(|event| match event {
                AgentEvent::MessageUpdate {
                    delta: StreamDelta::Text { delta, .. },
                    ..
                } => Some(delta.as_str()),
                _ => None,
            })
            .collect();
        assert_eq!(streamed, text);
        let completed: Vec<_> = events
            .iter()
            .filter_map(|event| match event {
                AgentEvent::MessageEnd { message } if message.role() == "assistant" => {
                    Some(assistant_text(message))
                }
                _ => None,
            })
            .collect();
        assert_eq!(
            completed,
            vec![text.to_string()],
            "final event changed provider text"
        );
        assert_eq!(messages.len(), 2);
        assert_eq!(
            assistant_text(&messages[1]),
            text,
            "returned message changed provider text"
        );
        assert_eq!(context.messages.len(), 2);
        assert_eq!(
            assistant_text(&context.messages[1]),
            text,
            "context changed provider text"
        );
    }
}
