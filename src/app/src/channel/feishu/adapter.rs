use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use tokio::sync::Mutex;
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;

use super::config::FeishuChannelConfig;
use super::config::FEISHU_MAX_MESSAGE_LEN;
use super::token::TokenCache;
use crate::agent::Agent;
use crate::agent::QueryRequest;
use crate::agent::RunEventPayload;
use crate::channel::Channel;
use crate::error::Result;

pub struct FeishuChannel {
    config: FeishuChannelConfig,
    session_map: SessionMap,
    client: reqwest::Client,
    token_cache: TokenCache,
}

impl FeishuChannel {
    pub fn new(config: FeishuChannelConfig) -> Self {
        Self {
            config,
            session_map: SessionMap::new(),
            client: reqwest::Client::new(),
            token_cache: TokenCache::new(),
        }
    }

    pub fn spawn(
        conf: FeishuChannelConfig,
        agent: Arc<Agent>,
        cancel: CancellationToken,
    ) -> JoinHandle<()> {
        let ch = Arc::new(Self::new(conf));
        tokio::spawn(async move {
            if let Err(e) = ch.run(agent, cancel).await {
                tracing::error!(channel = "feishu", error = %e, "channel exited");
            }
        })
    }

    async fn handle_message(&self, agent: &Agent, msg: super::message::ParsedMessage) {
        let session_id = self.session_map.resolve_or_create(&msg.chat_id).await;
        tracing::info!(
            channel = "feishu",
            chat_id = %msg.chat_id,
            sender_id = %msg.sender_id,
            session_id = %session_id,
            "received message"
        );

        let request = QueryRequest::text(&msg.text).session_id(Some(session_id));
        let reply = match agent.query(request).await {
            Ok(mut stream) => collect_reply(&mut stream).await,
            Err(e) => {
                tracing::error!(channel = "feishu", error = %e, "agent query failed");
                format!("Error: {e}")
            }
        };

        if reply.is_empty() {
            return;
        }

        // Truncate if too long
        let reply = if reply.len() > FEISHU_MAX_MESSAGE_LEN {
            let mut truncated = reply[..FEISHU_MAX_MESSAGE_LEN].to_string();
            truncated.push_str("\n\n... (truncated)");
            truncated
        } else {
            reply
        };

        if let Err(e) = super::outbound::send_text(
            &self.client,
            &self.token_cache,
            &self.config.app_id,
            &self.config.app_secret,
            &msg.chat_id,
            &reply,
        )
        .await
        {
            tracing::error!(channel = "feishu", chat_id = %msg.chat_id, error = %e, "send failed");
        }
    }
}

#[async_trait]
impl Channel for FeishuChannel {
    fn name(&self) -> &'static str {
        "feishu"
    }

    async fn run(self: Arc<Self>, agent: Arc<Agent>, cancel: CancellationToken) -> Result<()> {
        tracing::info!(channel = "feishu", "channel started");

        let mut attempt: u32 = 0;
        loop {
            if cancel.is_cancelled() {
                break;
            }

            let self_ref = self.clone();
            let agent_ref = agent.clone();
            let result = super::ws::ws_receive_loop(
                &self.client,
                &self.config.app_id,
                &self.config.app_secret,
                &self.token_cache,
                &self.config,
                &cancel,
                |msg| {
                    let self_inner = self_ref.clone();
                    let agent_inner = agent_ref.clone();
                    async move {
                        self_inner.handle_message(&agent_inner, msg).await;
                    }
                },
            )
            .await;

            if cancel.is_cancelled() {
                break;
            }

            match result {
                Ok(()) => {
                    tracing::info!(channel = "feishu", "websocket closed cleanly, reconnecting");
                    attempt = 0;
                }
                Err(e) => {
                    tracing::warn!(channel = "feishu", error = %e, attempt, "websocket error");
                    attempt = attempt.saturating_add(1);
                }
            }

            // Exponential backoff: 1s, 2s, 4s, 8s, ... max 60s
            let backoff = Duration::from_secs(
                1u64.saturating_mul(2u64.saturating_pow(attempt.min(6)))
                    .min(60),
            );
            tracing::info!(
                channel = "feishu",
                backoff_secs = backoff.as_secs(),
                "reconnecting"
            );

            tokio::select! {
                _ = cancel.cancelled() => break,
                _ = tokio::time::sleep(backoff) => {}
            }
        }

        tracing::info!(channel = "feishu", "channel stopped");
        Ok(())
    }
}

// ── Collect final reply text from QueryStream ──

async fn collect_reply(stream: &mut crate::agent::QueryStream) -> String {
    let mut parts = Vec::new();
    while let Some(event) = stream.next().await {
        if let RunEventPayload::AssistantDelta {
            delta: Some(delta), ..
        } = &event.payload
        {
            if !delta.is_empty() {
                parts.push(delta.clone());
            }
        }
    }
    parts.join("")
}

// ── channel-private session state ──

struct SessionMap {
    inner: Mutex<HashMap<String, String>>,
}

impl SessionMap {
    fn new() -> Self {
        Self {
            inner: Mutex::new(HashMap::new()),
        }
    }

    async fn resolve_or_create(&self, chat_id: &str) -> String {
        let mut map = self.inner.lock().await;
        if let Some(id) = map.get(chat_id) {
            return id.clone();
        }
        let session_id = uuid::Uuid::new_v4().to_string();
        map.insert(chat_id.to_string(), session_id.clone());
        session_id
    }
}
