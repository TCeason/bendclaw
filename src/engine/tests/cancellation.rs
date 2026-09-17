//! Cancellation is enforced by the runner, not delegated to a provider.
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use evotengine::agent_loop;
use evotengine::provider::ProviderError;
use evotengine::provider::StreamConfig;
use evotengine::provider::StreamEvent;
use evotengine::provider::StreamOutcome;
use evotengine::provider::StreamProvider;
use evotengine::types::AgentContext;
use evotengine::AgentLoopConfig;
use evotengine::AgentMessage;
use evotengine::Message;
use evotengine::PathGuard;
use tokio::sync::mpsc;
use tokio::sync::Notify;
use tokio_util::sync::CancellationToken;

mod fixtures;

struct UncooperativeProvider {
    started: Arc<Notify>,
    dropped: Arc<Notify>,
}

struct OnDrop(Arc<Notify>);
impl Drop for OnDrop {
    fn drop(&mut self) {
        self.0.notify_one();
    }
}

#[async_trait]
impl StreamProvider for UncooperativeProvider {
    async fn stream(
        &self,
        _config: StreamConfig,
        _tx: mpsc::UnboundedSender<StreamEvent>,
        _cancel: CancellationToken,
    ) -> Result<StreamOutcome, ProviderError> {
        Err(ProviderError::Other("bounded path required".into()))
    }

    async fn stream_bounded(
        &self,
        _config: StreamConfig,
        _tx: mpsc::Sender<StreamEvent>,
        _cancel: CancellationToken,
    ) -> Result<StreamOutcome, ProviderError> {
        let _guard = OnDrop(self.dropped.clone());
        self.started.notify_one();
        std::future::pending().await
    }
}

#[tokio::test]
async fn interrupt_drops_uncooperative_bounded_provider() -> Result<(), Box<dyn std::error::Error>>
{
    let started = Arc::new(Notify::new());
    let dropped = Arc::new(Notify::new());
    let mut config: AgentLoopConfig = fixtures::agent_harness::make_config(
        evotengine::provider::mock::MockProvider::text("unused"),
    );
    config.provider = Arc::new(UncooperativeProvider {
        started: started.clone(),
        dropped: dropped.clone(),
    });
    let mut context = AgentContext {
        system_prompt: "test".into(),
        messages: vec![],
        tools: vec![],
        cwd: std::path::PathBuf::new(),
        path_guard: Arc::new(PathGuard::open()),
        prompt_cache_key: None,
    };
    let cancel = CancellationToken::new();
    let (tx, _rx) = mpsc::unbounded_channel();
    let run_cancel = cancel.clone();
    let run = async move {
        agent_loop(
            vec![AgentMessage::Llm(Message::user("hi"))],
            &mut context,
            &config,
            tx,
            run_cancel,
        )
        .await
    };
    let interrupt = async {
        started.notified().await;
        cancel.cancel();
    };
    tokio::time::timeout(Duration::from_secs(1), async {
        tokio::join!(run, interrupt)
    })
    .await?;
    tokio::time::timeout(Duration::from_secs(1), dropped.notified()).await?;
    Ok(())
}
