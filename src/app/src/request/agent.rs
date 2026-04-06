use std::sync::Arc;

use bend_engine::provider::AnthropicProvider;
use bend_engine::provider::ModelConfig;
use bend_engine::provider::OpenAiCompatProvider;
use tokio::sync::mpsc;
use tokio::sync::RwLock;

use crate::conf::ProviderKind;
use crate::error::Result;
use crate::request::into_agent_messages;
use crate::request::RequestOptions;

enum AgentState {
    Live {
        agent: Box<Option<bend_engine::Agent>>,
    },
    Scripted {
        events_to_send: Vec<bend_engine::AgentEvent>,
        final_messages: Vec<bend_engine::AgentMessage>,
    },
}

pub struct RequestAgent {
    state: RwLock<AgentState>,
}

impl Default for RequestAgent {
    fn default() -> Self {
        Self {
            state: RwLock::new(AgentState::Live {
                agent: Box::new(None),
            }),
        }
    }
}

impl RequestAgent {
    pub fn new() -> Arc<Self> {
        Arc::new(Self::default())
    }

    pub fn scripted(
        events_to_send: Vec<bend_engine::AgentEvent>,
        final_messages: Vec<bend_engine::AgentMessage>,
    ) -> Arc<Self> {
        Arc::new(Self {
            state: RwLock::new(AgentState::Scripted {
                events_to_send,
                final_messages,
            }),
        })
    }

    pub async fn start(
        &self,
        options: RequestOptions,
    ) -> Result<mpsc::UnboundedReceiver<bend_engine::AgentEvent>> {
        let mut state = self.state.write().await;
        match &mut *state {
            AgentState::Live { agent } => {
                let prior_messages = into_agent_messages(&options.transcript);
                let prompt = options.prompt.clone();
                let mut runtime = build_agent(&options, prior_messages);
                let rx = runtime.prompt(prompt).await;
                **agent = Some(runtime);
                Ok(rx)
            }
            AgentState::Scripted { events_to_send, .. } => {
                let events = events_to_send.clone();
                let (tx, rx) = mpsc::unbounded_channel();
                tokio::spawn(async move {
                    for event in events {
                        let _ = tx.send(event);
                    }
                });
                Ok(rx)
            }
        }
    }

    pub async fn take_messages(&self) -> Vec<bend_engine::AgentMessage> {
        let mut state = self.state.write().await;
        match &mut *state {
            AgentState::Live { agent } => {
                if let Some(agent) = agent.as_mut().as_mut() {
                    agent.finish().await;
                    return agent.messages().to_vec();
                }
                Vec::new()
            }
            AgentState::Scripted { final_messages, .. } => final_messages.clone(),
        }
    }

    pub async fn close(&self) {
        let mut state = self.state.write().await;
        match &mut *state {
            AgentState::Live { agent } => {
                if let Some(agent) = agent.as_ref().as_ref() {
                    agent.abort();
                }
            }
            AgentState::Scripted { .. } => {}
        }
    }
}

fn build_agent(
    options: &RequestOptions,
    prior_messages: Vec<bend_engine::AgentMessage>,
) -> bend_engine::Agent {
    let mut model_config = match options.llm.provider {
        ProviderKind::Anthropic => ModelConfig::anthropic(&options.llm.model, &options.llm.model),
        ProviderKind::OpenAi => ModelConfig::openai(&options.llm.model, &options.llm.model),
    };
    if let Some(base_url) = &options.llm.base_url {
        model_config.base_url = base_url.clone();
    }

    let mut system_prompt = format!(
        "You are a helpful assistant. Working directory: {}",
        options.cwd
    );
    if let Some(extra) = &options.append_system_prompt {
        system_prompt.push('\n');
        system_prompt.push_str(extra);
    }

    let mut agent = match options.llm.provider {
        ProviderKind::Anthropic => bend_engine::Agent::new(AnthropicProvider)
            .with_model(&options.llm.model)
            .with_api_key(&options.llm.api_key)
            .with_model_config(model_config)
            .with_system_prompt(system_prompt)
            .with_tools(bend_engine::tools::default_tools())
            .with_messages(prior_messages),
        ProviderKind::OpenAi => bend_engine::Agent::new(OpenAiCompatProvider)
            .with_model(&options.llm.model)
            .with_api_key(&options.llm.api_key)
            .with_model_config(model_config)
            .with_system_prompt(system_prompt)
            .with_tools(bend_engine::tools::default_tools())
            .with_messages(prior_messages),
    };

    if let Some(max_turns) = options.max_turns {
        agent = agent.with_execution_limits(bend_engine::context::ExecutionLimits {
            max_turns: max_turns as usize,
            ..Default::default()
        });
    }

    agent
}
