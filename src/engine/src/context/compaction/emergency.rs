//! Emergency compact summary — deterministic fallback used only for overflow recovery.

use super::summarizer::types::SummarizerInput;
use super::summarizer::types::SummarizerOutput;
use crate::context::compaction::types::FileOps;

/// Generate a deterministic emergency summary using compact memory extraction.
pub fn summarize(input: &SummarizerInput) -> SummarizerOutput {
    let mut sections: Vec<String> = Vec::new();

    // Section 1: Overview
    sections.push(format!(
        "[Context compacted: {} messages removed]",
        input.evicted_count
    ));

    // Section 2: Completed user requests
    if !input.completed_requests.is_empty() {
        let mut s = String::from("Completed requests (do not revisit):");
        for req in &input.completed_requests {
            s.push_str("\n- ");
            s.push_str(req);
        }
        sections.push(s);
    }

    // Section 3: File operations
    let file_section = format_file_ops(&input.file_ops);
    if !file_section.is_empty() {
        sections.push(file_section);
    }

    // Section 4: Environment discoveries
    if !input.env_discoveries.is_empty() {
        let mut s = String::from("Environment:");
        for e in &input.env_discoveries {
            s.push_str("\n- ");
            s.push_str(e);
        }
        sections.push(s);
    }

    // Section 5: Turn prefix context (if split turn)
    if let Some(prefix) = input.turn_prefix.as_deref() {
        if !prefix.is_empty() {
            sections.push(format!("Current turn context (prefix removed):\n{prefix}"));
        }
    }

    // Section 6: Last assistant conclusion
    if let Some(ref conclusion) = input.last_conclusion {
        sections.push(format!("Last assistant conclusion:\n{conclusion}"));
    }

    let summary = sections.join("\n\n");
    SummarizerOutput { summary }
}

fn format_file_ops(file_ops: &FileOps) -> String {
    let mut sections: Vec<String> = Vec::new();

    let modified = file_ops.modified();
    if !modified.is_empty() {
        let mut s = String::from("Files modified:");
        for f in &modified {
            s.push_str("\n- ");
            s.push_str(f);
        }
        sections.push(s);
    }

    let read_only = file_ops.read_only();
    if !read_only.is_empty() {
        let mut s = String::from("Files read:");
        for f in &read_only {
            s.push_str("\n- ");
            s.push_str(f);
        }
        sections.push(s);
    }

    sections.join("\n")
}
