#[derive(Debug, Clone)]
pub struct RunRequest {
    pub prompt: String,
    pub session_id: Option<String>,
    pub max_turns: Option<u32>,
    pub append_system_prompt: Option<String>,
}

impl RunRequest {
    pub fn new(prompt: String) -> Self {
        Self {
            prompt,
            session_id: None,
            max_turns: None,
            append_system_prompt: None,
        }
    }
}
