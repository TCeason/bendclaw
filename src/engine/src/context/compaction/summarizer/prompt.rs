//! Summarization prompts — system prompt and user prompt templates.

/// System prompt for all summarization calls.
pub const SYSTEM_PROMPT: &str = "\
You are a context summarization assistant. Your task is to read a conversation \
between a user and an AI coding assistant, then produce a structured summary \
following the exact format specified.\n\n\
Do NOT continue the conversation. Do NOT respond to any questions in the \
conversation. ONLY output the structured summary.";

/// Initial summarization prompt (no previous summary).
pub const INITIAL_PROMPT: &str = "\
The messages above are a conversation to summarize. Create a structured context \
checkpoint summary that another LLM will use to continue the work.\n\n\
Use this EXACT format:\n\n\
## Goal\n\
[What is the user trying to accomplish? Can be multiple items if the session covers different tasks.]\n\n\
## Constraints & Preferences\n\
- [Any constraints, preferences, or requirements mentioned by user]\n\
- [Or \"(none)\" if none were mentioned]\n\n\
## Progress\n\
### Done\n\
- [x] [Completed tasks/changes]\n\n\
### In Progress\n\
- [ ] [Current work]\n\n\
### Blocked\n\
- [Issues preventing progress, if any]\n\n\
## Key Decisions\n\
- **[Decision]**: [Brief rationale]\n\n\
## Next Steps\n\
1. [Ordered list of what should happen next]\n\n\
## Critical Context\n\
- [Any data, examples, or references needed to continue]\n\
- [Or \"(none)\" if not applicable]\n\n\
Keep each section concise. Preserve exact file paths, function names, and error messages.";

/// Update prompt (merge new messages into existing summary).
pub const UPDATE_PROMPT: &str = "\
The messages above are NEW conversation messages to incorporate into the existing \
summary provided in <previous-summary> tags.\n\n\
Update the existing structured summary with new information. RULES:\n\
- PRESERVE all existing information from the previous summary\n\
- ADD new progress, decisions, and context from the new messages\n\
- UPDATE the Progress section: move items from \"In Progress\" to \"Done\" when completed\n\
- UPDATE \"Next Steps\" based on what was accomplished\n\
- PRESERVE exact file paths, function names, and error messages\n\
- If something is no longer relevant, you may remove it\n\n\
Use this EXACT format:\n\n\
## Goal\n\
[Preserve existing goals, add new ones if the task expanded]\n\n\
## Constraints & Preferences\n\
- [Preserve existing, add new ones discovered]\n\n\
## Progress\n\
### Done\n\
- [x] [Include previously done items AND newly completed items]\n\n\
### In Progress\n\
- [ ] [Current work - update based on progress]\n\n\
### Blocked\n\
- [Current blockers - remove if resolved]\n\n\
## Key Decisions\n\
- **[Decision]**: [Brief rationale] (preserve all previous, add new)\n\n\
## Next Steps\n\
1. [Update based on current state]\n\n\
## Critical Context\n\
- [Preserve important context, add new if needed]\n\n\
Keep each section concise. Preserve exact file paths, function names, and error messages.";

/// Turn prefix summarization prompt (for split turns).
pub const TURN_PREFIX_PROMPT: &str = "\
This is the PREFIX of a turn that was too large to keep. The SUFFIX (recent work) is retained.\n\n\
Summarize the prefix to provide context for the retained suffix:\n\n\
## Original Request\n\
[What did the user ask for in this turn?]\n\n\
## Early Progress\n\
- [Key decisions and work done in the prefix]\n\n\
## Context for Suffix\n\
- [Information needed to understand the retained recent work]\n\n\
Be concise. Focus on what's needed to understand the kept suffix.";

/// Append optional user guidance to the task instructions, outside the
/// `<conversation>` data block.
fn with_custom_instructions(mut prompt: String, instructions: Option<&str>) -> String {
    if let Some(instructions) = instructions.filter(|value| !value.trim().is_empty()) {
        prompt.push_str("\n\nAdditional focus: ");
        prompt.push_str(instructions.trim());
    }
    prompt
}

/// Build the user message for initial summarization.
pub fn format_initial(conversation: &str, custom_instructions: Option<&str>) -> String {
    with_custom_instructions(
        format!("<conversation>\n{conversation}\n</conversation>\n\n{INITIAL_PROMPT}"),
        custom_instructions,
    )
}

/// Build the user message for incremental update.
pub fn format_update(
    conversation: &str,
    previous_summary: &str,
    custom_instructions: Option<&str>,
) -> String {
    with_custom_instructions(
        format!(
            "<conversation>\n{conversation}\n</conversation>\n\n\
             <previous-summary>\n{previous_summary}\n</previous-summary>\n\n\
             {UPDATE_PROMPT}"
        ),
        custom_instructions,
    )
}

/// Build the user message for turn prefix summarization.
pub fn format_turn_prefix(turn_prefix_text: &str) -> String {
    format!("<conversation>\n{turn_prefix_text}\n</conversation>\n\n{TURN_PREFIX_PROMPT}")
}
