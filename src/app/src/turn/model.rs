#[derive(Debug, Clone)]
pub struct TurnRequest {
    pub prompt: String,
    pub session_id: Option<String>,
    pub max_turns: Option<u32>,
    pub append_system_prompt: Option<String>,
}

impl TurnRequest {
    pub fn new(prompt: String) -> Self {
        Self {
            prompt,
            session_id: None,
            max_turns: None,
            append_system_prompt: None,
        }
    }
}

#[derive(Debug, Clone)]
pub struct TurnResult {
    pub session_id: String,
    pub run_id: String,
}
