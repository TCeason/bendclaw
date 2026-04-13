use std::collections::HashSet;

use bendengine::context::*;
use bendengine::types::*;

/// Assert no orphan tool_call / tool_result in a message list.
pub fn assert_no_orphan_tool_pairs(messages: &[AgentMessage]) {
    let mut call_ids = HashSet::new();
    let mut result_ids = HashSet::new();
    for msg in messages {
        match msg {
            AgentMessage::Llm(Message::Assistant { content, .. }) => {
                for c in content {
                    if let Content::ToolCall { id, .. } = c {
                        call_ids.insert(id.clone());
                    }
                }
            }
            AgentMessage::Llm(Message::ToolResult { tool_call_id, .. }) => {
                result_ids.insert(tool_call_id.clone());
            }
            _ => {}
        }
    }
    assert_eq!(
        call_ids,
        result_ids,
        "orphan detected: unmatched calls={:?}, unmatched results={:?}",
        call_ids.difference(&result_ids).collect::<Vec<_>>(),
        result_ids.difference(&call_ids).collect::<Vec<_>>(),
    );
}

/// Assert all actions match the expected level's methods.
pub fn assert_actions_match_level(level: u8, actions: &[CompactionAction]) {
    for action in actions {
        // LifecycleCleared runs at every level (level 0 cleanup)
        if action.method == CompactionMethod::LifecycleCleared {
            continue;
        }
        match level {
            0 => panic!("level 0 should only have LifecycleCleared actions"),
            1 => assert!(
                action.method == CompactionMethod::Outline
                    || action.method == CompactionMethod::HeadTail,
                "level 1 action should be Outline or HeadTail, got {:?}",
                action.method
            ),
            2 => assert_eq!(
                action.method,
                CompactionMethod::Summarized,
                "level 2 action should be Summarized"
            ),
            3 => assert_eq!(
                action.method,
                CompactionMethod::Dropped,
                "level 3 action should be Dropped"
            ),
            _ => panic!("unexpected level {}", level),
        }
    }
}
