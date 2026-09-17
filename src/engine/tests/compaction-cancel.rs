//! A summary must not depend on the provider observing cancellation.
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use evotengine::context::compaction::summarizer::llm;
use evotengine::context::compaction::summarizer::SummarizerContext;
use evotengine::context::compaction::summarizer::SummarizerError;
use evotengine::context::compaction::summarizer::SummarizerInput;
use evotengine::provider::ProviderError;
use evotengine::provider::StreamConfig;
use evotengine::provider::StreamEvent;
use evotengine::provider::StreamOutcome;
use evotengine::provider::StreamProvider;
use tokio::sync::mpsc;
use tokio::sync::Notify;
use tokio_util::sync::CancellationToken;

struct ParkedProvider(Arc<Notify>);

#[async_trait]
impl StreamProvider for ParkedProvider {
    async fn stream(
        &self,
        _config: StreamConfig,
        _tx: mpsc::UnboundedSender<StreamEvent>,
        _cancel: CancellationToken,
    ) -> Result<StreamOutcome, ProviderError> {
        self.0.notify_one();
        std::future::pending().await
    }
}

#[tokio::test]
async fn compaction_cancel_interrupts_parked_llm_summarizer(
) -> Result<(), Box<dyn std::error::Error>> {
    let started = Arc::new(Notify::new());
    let ctx = SummarizerContext {
        provider: Arc::new(ParkedProvider(started.clone())),
        model: "test-model".into(),
        api_key: "test".into(),
        thinking_level: evotengine::ThinkingLevel::Off,
        system_prompt: String::new(),
        tools: vec![],
        max_tokens: None,
        cache_config: Default::default(),
        prompt_cache_key: None,
        model_config: None,
    };
    let input = SummarizerInput {
        conversation: "[User]: compact this".into(),
        turn_prefix: None,
        previous_summary: None,
        custom_instructions: None,
        request_max_bytes: 100_000,
        file_ops: Default::default(),
        evicted_count: 2,
        completed_requests: vec![],
        env_discoveries: vec![],
        last_conclusion: None,
    };
    let cancel = CancellationToken::new();
    let work = llm::summarize(input, &ctx, 1024, cancel.clone());
    let interrupt = async {
        started.notified().await;
        cancel.cancel();
    };
    let (result, ()) = tokio::time::timeout(Duration::from_secs(1), async {
        tokio::join!(work, interrupt)
    })
    .await?;
    assert!(matches!(result, Err(SummarizerError::Cancelled)));
    Ok(())
}
