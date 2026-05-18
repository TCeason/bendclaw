//! SessionGoal — autonomous task execution bound to a session.
//!
//! A goal is verified after each engine turn. The verifier determines whether
//! the condition is met or should continue.

use chrono::Utc;
use serde::Deserialize;
use serde::Serialize;

// ---------------------------------------------------------------------------
// GoalStatus — lifecycle states
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GoalStatus {
    /// Goal is being actively pursued; auto-continuation eligible.
    Active,
    /// User-paused; auto-continuation suppressed until resumed.
    Paused,
    /// Verifier determined the condition is satisfied.
    Met,
    /// Historical terminal status from older goal verifiers.
    Impossible,
    /// Budget exhausted (token, iteration, or time limit reached).
    Exhausted,
}

impl GoalStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Active => "active",
            Self::Paused => "paused",
            Self::Met => "met",
            Self::Impossible => "impossible",
            Self::Exhausted => "exhausted",
        }
    }

    pub fn is_terminal(&self) -> bool {
        matches!(self, Self::Met | Self::Impossible | Self::Exhausted)
    }
}

// ---------------------------------------------------------------------------
// GoalBudget — resource limits
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GoalBudget {
    /// Max billable tokens. `None` = no limit.
    #[serde(default)]
    pub max_tokens: Option<u64>,
    /// Max outer-loop iterations. `None` = no limit.
    #[serde(default)]
    pub max_iterations: Option<u32>,
    /// Max wall-clock seconds. `None` = no limit.
    #[serde(default)]
    pub max_seconds: Option<u64>,
}

impl Default for GoalBudget {
    fn default() -> Self {
        Self {
            max_tokens: Some(200_000),
            max_iterations: Some(50),
            max_seconds: Some(1800),
        }
    }
}

// ---------------------------------------------------------------------------
// GoalProgress — cumulative resource usage
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct GoalProgress {
    pub tokens_used: u64,
    pub iterations: u32,
    pub seconds_used: u64,
    /// Last reason reported by goal verification.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_reason: Option<String>,
}

// ---------------------------------------------------------------------------
// GoalTask — planned execution steps
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GoalTaskStatus {
    Pending,
    InProgress,
    Completed,
}

impl GoalTaskStatus {
    pub fn is_open(self) -> bool {
        matches!(self, Self::Pending | Self::InProgress)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GoalTask {
    pub id: u32,
    pub title: String,
    pub status: GoalTaskStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub started_at: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub completed_at: Option<String>,
}

impl GoalTask {
    pub fn new(id: u32, title: String, status: GoalTaskStatus) -> Self {
        Self {
            id,
            title,
            status,
            started_at: None,
            completed_at: None,
        }
    }
}

// ---------------------------------------------------------------------------
// SessionGoal — persisted on SessionMeta
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionGoal {
    /// The goal condition to verify.
    pub condition: String,
    pub status: GoalStatus,
    pub budget: GoalBudget,
    pub progress: GoalProgress,
    #[serde(default)]
    pub tasks: Vec<GoalTask>,
    pub created_at: String,
    pub updated_at: String,
}

impl SessionGoal {
    pub fn new(condition: String, budget: GoalBudget) -> Self {
        let now = Utc::now().to_rfc3339();
        Self {
            condition,
            status: GoalStatus::Active,
            budget,
            progress: GoalProgress::default(),
            tasks: Vec::new(),
            created_at: now.clone(),
            updated_at: now,
        }
    }

    pub fn touch(&mut self) {
        self.updated_at = Utc::now().to_rfc3339();
    }

    pub fn completed_task_count(&self) -> usize {
        self.tasks
            .iter()
            .filter(|task| task.status == GoalTaskStatus::Completed)
            .count()
    }

    pub fn current_task(&self) -> Option<&GoalTask> {
        self.tasks
            .iter()
            .find(|task| task.status == GoalTaskStatus::InProgress)
            .or_else(|| {
                self.tasks
                    .iter()
                    .find(|task| task.status == GoalTaskStatus::Pending)
            })
    }

    pub fn has_open_tasks(&self) -> bool {
        self.tasks.iter().any(|task| task.status.is_open())
    }

    /// Check if any budget dimension is exhausted.
    pub fn is_budget_exhausted(&self) -> bool {
        let b = &self.budget;
        let p = &self.progress;
        if b.max_tokens.is_some_and(|m| p.tokens_used >= m) {
            return true;
        }
        if b.max_iterations.is_some_and(|m| p.iterations >= m) {
            return true;
        }
        if b.max_seconds.is_some_and(|m| p.seconds_used >= m) {
            return true;
        }
        false
    }

    pub fn remaining_tokens(&self) -> Option<u64> {
        self.budget
            .max_tokens
            .map(|b| b.saturating_sub(self.progress.tokens_used))
    }
}
