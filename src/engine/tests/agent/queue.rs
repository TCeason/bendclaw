//! Prompt queue behavior tests.

use evotengine::agent::RunHandle;
use evotengine::AgentMessage;
use evotengine::Message;
use evotengine::PromptQueue;
use evotengine::PromptQueueError;

fn user(text: &str) -> AgentMessage {
    AgentMessage::Llm(Message::user(text))
}

fn message_text(message: &AgentMessage) -> String {
    match message {
        AgentMessage::Llm(Message::User { content, .. }) => content
            .iter()
            .filter_map(|content| match content {
                evotengine::Content::Text { text } => Some(text.as_str()),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join("\n"),
        _ => String::new(),
    }
}

#[test]
fn queue_entries_have_stable_ids_and_versions() {
    let queue = PromptQueue::new();
    let first = queue.enqueue(user("first"));
    let second = queue.enqueue(user("second"));

    assert_ne!(first.id, second.id);
    assert_eq!(first.version, 0);
    assert_eq!(queue.list().len(), 2);

    let updated = queue.update(&first.id, 0, user("updated"));
    match updated {
        Ok(entry) => {
            assert_eq!(entry.id, first.id);
            assert_eq!(entry.version, 1);
            assert_eq!(message_text(&entry.message), "updated");
        }
        Err(error) => panic!("update failed: {error}"),
    }
}

#[test]
fn stale_queue_update_is_rejected() {
    let queue = PromptQueue::new();
    let entry = queue.enqueue(user("first"));
    if let Err(error) = queue.update(&entry.id, 0, user("second")) {
        panic!("initial update failed: {error}");
    }
    let stale = queue.update(&entry.id, 0, user("stale"));
    assert!(matches!(
        stale,
        Err(PromptQueueError::VersionConflict {
            expected: 0,
            actual: 1
        })
    ));
}

#[test]
fn queue_entries_can_be_reordered_with_version_checks() {
    let queue = PromptQueue::new();
    let first = queue.enqueue(user("first"));
    let second = queue.enqueue(user("second"));
    let third = queue.enqueue(user("third"));

    let moved = queue.move_up(&second.id, second.version);
    match moved {
        Ok(entry) => assert_eq!(entry.version, 1),
        Err(error) => panic!("move failed: {error}"),
    }
    assert_eq!(
        queue
            .list()
            .iter()
            .map(|entry| message_text(&entry.message))
            .collect::<Vec<_>>(),
        ["second", "first", "third"]
    );
    assert!(matches!(
        queue.move_down(&second.id, second.version),
        Err(PromptQueueError::VersionConflict { .. })
    ));
    assert!(!first.id.is_empty());
    assert!(!third.id.is_empty());
}

#[test]
fn follow_up_can_be_promoted_to_steering() {
    let handle = RunHandle::noop();
    let queued = handle.follow_up(user("do this next"));
    let promoted = handle.send_follow_up_now(&queued.id, Some(queued.version));

    match promoted {
        Ok(entry) => {
            assert_eq!(entry.id, queued.id);
            assert_eq!(entry.version, 1);
        }
        Err(error) => panic!("promotion failed: {error}"),
    }
    assert!(handle.follow_up_entries().is_empty());
    let steering = handle.steering_entries();
    assert_eq!(steering.len(), 1);
    assert_eq!(message_text(&steering[0].message), "do this next");
}
