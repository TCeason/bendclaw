//! Compact scenario DSL — simulate multi-round agent loops and verify compact behavior.
//!
//! A `Scenario` describes a session as a sequence of phases. Each phase adds
//! messages then runs compact. Invariants are checked per-round and at the end.
//!
//! # Example
//!
//! ```rust,ignore
//! scenario("image-heavy session")
//!     .budget(160_000)
//!     .system_tokens(4_000)
//!     .tool_overhead(45_000)
//!     .seed(vec![
//!         Turn::user("start"),
//!         Turn::user_image("look", "/tmp/s1.png"),
//!         Turn::user_image("look", "/tmp/s2.png"),
//!     ])
//!     .phase(20, Turn::tool("bash", 200))
//!     .assert_no_toothpaste()
//!     .assert_images_stripped_when_pressured()
//!     .assert_context_bounded(0.95)
//!     .run();
//! ```

use evotengine::context::*;
use evotengine::types::*;

// ---------------------------------------------------------------------------
// Turn builders
// ---------------------------------------------------------------------------

#[derive(Clone, Debug)]
pub enum Turn {
    User(String),
    /// User message with repeated padding to control size.
    UserLarge {
        prefix: String,
        padding: String,
        repeat: usize,
    },
    UserImage {
        text: String,
        path: String,
    },
    ToolCall {
        tool_name: String,
        output_size: usize,
    },
    AssistantText(String),
}

impl Turn {
    pub fn user(text: &str) -> Self {
        Self::User(text.into())
    }
    pub fn user_large(prefix: &str, padding: &str, repeat: usize) -> Self {
        Self::UserLarge {
            prefix: prefix.into(),
            padding: padding.into(),
            repeat,
        }
    }
    pub fn user_image(text: &str, path: &str) -> Self {
        Self::UserImage {
            text: text.into(),
            path: path.into(),
        }
    }
    pub fn tool(name: &str, output_size: usize) -> Self {
        Self::ToolCall {
            tool_name: name.into(),
            output_size,
        }
    }
    pub fn assistant(text: &str) -> Self {
        Self::AssistantText(text.into())
    }
}

// ---------------------------------------------------------------------------
// Scenario builder
// ---------------------------------------------------------------------------

pub struct Scenario {
    name: String,
    budget: usize,
    system_tokens: usize,
    tool_overhead: usize,
    keep_recent: usize,
    keep_first: usize,
    max_messages: usize,
    /// Multiplier to simulate provider tokenizer reporting higher counts than
    /// tiktoken (e.g. 1.3 means provider sees 30% more tokens). Default: 1.0.
    estimated_token_multiplier: f64,
    seed: Vec<Turn>,
    phases: Vec<Phase>,
    assertions: Vec<Assertion>,
}

struct Phase {
    count: usize,
    turn: Turn,
}

#[derive(Clone, Debug)]
enum Assertion {
    /// No round where est > trigger but compact saves nothing.
    NoToothpaste,
    /// After all phases, images outside recent window are stripped.
    ImagesStrippedWhenPressured,
    /// Context never exceeds this fraction of budget.
    ContextBounded(f64),
    /// Final message count stays below this.
    MessageCountBelow(usize),
    /// Compact fires at least once (level > 0 or actions non-empty).
    CompactionFires,
    /// L2 eviction (message drops) occurs at least once.
    DropsMessages,
    /// Final tokens are within budget.
    FinalWithinBudget,
}

pub fn scenario(name: &str) -> Scenario {
    Scenario {
        name: name.into(),
        budget: 160_000,
        system_tokens: 4_000,
        tool_overhead: 0,
        keep_recent: 10,
        keep_first: 2,
        max_messages: 386,
        estimated_token_multiplier: 1.0,
        seed: vec![],
        phases: vec![],
        assertions: vec![],
    }
}

impl Scenario {
    pub fn budget(mut self, tokens: usize) -> Self {
        self.budget = tokens;
        self
    }
    pub fn system_tokens(mut self, tokens: usize) -> Self {
        self.system_tokens = tokens;
        self
    }
    pub fn tool_overhead(mut self, tokens: usize) -> Self {
        self.tool_overhead = tokens;
        self
    }
    pub fn keep_recent(mut self, n: usize) -> Self {
        self.keep_recent = n;
        self
    }
    pub fn keep_first(mut self, n: usize) -> Self {
        self.keep_first = n;
        self
    }
    pub fn max_messages(mut self, n: usize) -> Self {
        self.max_messages = n;
        self
    }
    /// Simulate provider tokenizer reporting higher counts than tiktoken.
    /// E.g. 1.3 means estimated_tokens = message_tokens * 1.3.
    pub fn estimated_token_multiplier(mut self, m: f64) -> Self {
        self.estimated_token_multiplier = m;
        self
    }

    pub fn seed(mut self, turns: Vec<Turn>) -> Self {
        self.seed = turns;
        self
    }

    pub fn phase(mut self, count: usize, turn: Turn) -> Self {
        self.phases.push(Phase { count, turn });
        self
    }

    pub fn assert_no_toothpaste(mut self) -> Self {
        self.assertions.push(Assertion::NoToothpaste);
        self
    }
    pub fn assert_images_stripped_when_pressured(mut self) -> Self {
        self.assertions.push(Assertion::ImagesStrippedWhenPressured);
        self
    }
    pub fn assert_context_bounded(mut self, fraction: f64) -> Self {
        self.assertions.push(Assertion::ContextBounded(fraction));
        self
    }
    pub fn assert_message_count_below(mut self, n: usize) -> Self {
        self.assertions.push(Assertion::MessageCountBelow(n));
        self
    }
    pub fn assert_compaction_fires(mut self) -> Self {
        self.assertions.push(Assertion::CompactionFires);
        self
    }
    pub fn assert_drops_messages(mut self) -> Self {
        self.assertions.push(Assertion::DropsMessages);
        self
    }
    pub fn assert_final_within_budget(mut self) -> Self {
        self.assertions.push(Assertion::FinalWithinBudget);
        self
    }

    // -----------------------------------------------------------------------
    // Execution
    // -----------------------------------------------------------------------

    pub fn run(self) {
        let config = ContextConfig {
            max_context_tokens: self.budget,
            system_prompt_tokens: self.system_tokens,
            keep_recent: self.keep_recent,
            keep_first: self.keep_first,
            max_messages: self.max_messages,
            ..Default::default()
        };

        let mut messages: Vec<AgentMessage> = Vec::new();
        let mut tool_id = 0usize;
        let mut toothpaste_count = 0usize;
        let mut max_context_ratio: f64 = 0.0;
        let mut compaction_fired = false;
        let mut dropped_messages = false;

        // Apply seed
        for turn in &self.seed {
            Self::apply_turn(turn, &mut messages, &mut tool_id);
        }

        // Run phases
        for phase in &self.phases {
            for _ in 0..phase.count {
                Self::apply_turn(&phase.turn, &mut messages, &mut tool_id);

                // Compute estimated_tokens: apply multiplier to simulate provider divergence
                let message_tokens = total_tokens(&messages);
                let estimated_tokens =
                    (message_tokens as f64 * self.estimated_token_multiplier) as usize;
                let budget_state = CompactionBudgetState { estimated_tokens };
                let result = compact_messages(messages, &config, &budget_state);

                // Track toothpaste: high pressure + no savings
                let budget_tokens = self.budget.saturating_sub(self.system_tokens);
                let trigger = budget_tokens * config.compact_trigger_pct as usize / 100;
                if (message_tokens > trigger || estimated_tokens > trigger)
                    && result.stats.actions.is_empty()
                {
                    toothpaste_count += 1;
                }

                // Track compaction activity
                if !result.stats.actions.is_empty() {
                    compaction_fired = true;
                }
                if result.stats.messages_dropped > 0 {
                    dropped_messages = true;
                }

                // Track context ratio (simulate provider view: messages + overhead)
                let provider_view = total_tokens(&result.messages) + self.tool_overhead;
                let ratio = provider_view as f64 / self.budget as f64;
                if ratio > max_context_ratio {
                    max_context_ratio = ratio;
                }

                messages = result.messages;
            }
        }

        // Check assertions
        for assertion in &self.assertions {
            match assertion {
                Assertion::NoToothpaste => {
                    assert!(
                        toothpaste_count == 0,
                        "[{}] toothpaste detected: {} rounds with high context but no savings",
                        self.name,
                        toothpaste_count
                    );
                }
                Assertion::ImagesStrippedWhenPressured => {
                    let recent_boundary = messages.len().saturating_sub(self.keep_recent);
                    let old_images = messages
                        .iter()
                        .enumerate()
                        .take(recent_boundary)
                        .filter(|(_, m)| {
                            matches!(m,
                                AgentMessage::Llm(Message::User { content, .. })
                                if content.iter().any(|c| matches!(c, Content::Image { .. }))
                            )
                        })
                        .count();
                    // Allow at most 1 old image (microcompact_keep_images default)
                    assert!(
                        old_images <= 1,
                        "[{}] expected old images stripped, but {} remain outside recent window",
                        self.name,
                        old_images
                    );
                }
                Assertion::ContextBounded(max_fraction) => {
                    assert!(
                        max_context_ratio <= *max_fraction + 0.05, // 5% tolerance for estimation
                        "[{}] context exceeded bound: peak {:.1}% > {:.1}%",
                        self.name,
                        max_context_ratio * 100.0,
                        max_fraction * 100.0
                    );
                }
                Assertion::MessageCountBelow(max_count) => {
                    assert!(
                        messages.len() < *max_count,
                        "[{}] message count {} >= limit {}",
                        self.name,
                        messages.len(),
                        max_count
                    );
                }
                Assertion::CompactionFires => {
                    assert!(
                        compaction_fired,
                        "[{}] compaction should fire at least once",
                        self.name
                    );
                }
                Assertion::DropsMessages => {
                    assert!(
                        dropped_messages,
                        "[{}] L2 eviction should drop messages at least once",
                        self.name
                    );
                }
                Assertion::FinalWithinBudget => {
                    let final_tokens = total_tokens(&messages);
                    assert!(
                        final_tokens <= self.budget,
                        "[{}] final tokens ({}) exceed budget ({})",
                        self.name,
                        final_tokens,
                        self.budget
                    );
                }
            }
        }
    }

    // -----------------------------------------------------------------------
    // Turn application
    // -----------------------------------------------------------------------

    fn apply_turn(turn: &Turn, messages: &mut Vec<AgentMessage>, tool_id: &mut usize) {
        match turn {
            Turn::User(text) => {
                messages.push(AgentMessage::Llm(Message::user(text)));
            }
            Turn::UserLarge {
                prefix,
                padding,
                repeat,
            } => {
                let text = format!("{} {}", prefix, padding.repeat(*repeat));
                messages.push(AgentMessage::Llm(Message::user(&text)));
            }
            Turn::UserImage { text, path } => {
                messages.push(AgentMessage::Llm(Message::User {
                    content: vec![Content::Text { text: text.clone() }, Content::Image {
                        mime_type: "image/png".into(),
                        source: ImageSource::Base64 {
                            data: "base64data".into(),
                            path: Some(path.clone()),
                        },
                    }],
                    timestamp: 0,
                }));
            }
            Turn::ToolCall {
                tool_name,
                output_size,
            } => {
                *tool_id += 1;
                let id = format!("tc-{tool_id}");
                messages.push(AgentMessage::Llm(Message::Assistant {
                    content: vec![Content::ToolCall {
                        id: id.clone(),
                        name: tool_name.clone(),
                        arguments: serde_json::json!({}),
                    }],
                    stop_reason: StopReason::ToolUse,
                    model: "test".into(),
                    provider: "test".into(),
                    usage: Usage::default(),
                    timestamp: 0,
                    error_message: None,
                    response_id: None,
                }));
                messages.push(AgentMessage::Llm(Message::ToolResult {
                    tool_call_id: id,
                    tool_name: tool_name.clone(),
                    content: vec![Content::Text {
                        text: "x".repeat(*output_size),
                    }],
                    is_error: false,
                    timestamp: 0,
                    retention: Retention::Normal,
                }));
            }
            Turn::AssistantText(text) => {
                messages.push(AgentMessage::Llm(Message::Assistant {
                    content: vec![Content::Text { text: text.clone() }],
                    stop_reason: StopReason::Stop,
                    model: "test".into(),
                    provider: "test".into(),
                    usage: Usage::default(),
                    timestamp: 0,
                    error_message: None,
                    response_id: None,
                }));
            }
        }
    }
}
