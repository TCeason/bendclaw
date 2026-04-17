use std::sync::Arc;

use evot::agent::session::Session;
use evot::agent::session_locator::SessionLocator;
use evot::storage::MemoryStorage;
use evot::types::TranscriptItem;

#[tokio::test]
async fn open_or_create_returns_same_session_id() {
    let storage: Arc<dyn evot::storage::Storage> = Arc::new(MemoryStorage::new());
    let locator = SessionLocator::new("test", "chat:c1:user:u1");

    let s1 = Session::open_or_create(&locator, "/tmp", "model-1", storage.clone())
        .await
        .expect("first open_or_create");
    let s2 = Session::open_or_create(&locator, "/tmp", "model-1", storage.clone())
        .await
        .expect("second open_or_create");

    assert_eq!(s1.session_id().await, s2.session_id().await);
}

#[tokio::test]
async fn open_or_create_different_locator_different_session() {
    let storage: Arc<dyn evot::storage::Storage> = Arc::new(MemoryStorage::new());
    let loc_a = SessionLocator::new("test", "chat:c1:user:u1");
    let loc_b = SessionLocator::new("test", "chat:c1:user:u2");

    let sa = Session::open_or_create(&loc_a, "/tmp", "model-1", storage.clone())
        .await
        .expect("session a");
    let sb = Session::open_or_create(&loc_b, "/tmp", "model-1", storage.clone())
        .await
        .expect("session b");

    assert_ne!(sa.session_id().await, sb.session_id().await);
}

#[tokio::test]
async fn open_or_create_restores_transcript_after_drop() {
    let storage: Arc<dyn evot::storage::Storage> = Arc::new(MemoryStorage::new());
    let locator = SessionLocator::new("test", "chat:c1:user:u1");

    // First: create session and write a transcript item
    {
        let session = Session::open_or_create(&locator, "/tmp", "model-1", storage.clone())
            .await
            .expect("create session");

        let item = TranscriptItem::user_from_content(&[evot_engine::Content::Text {
            text: "hello from turn 1".to_string(),
        }]);
        session.write_items(vec![item]).await.expect("write items");
        session.save().await.expect("save session");
    }
    // Session Arc is dropped here — simulates restart

    // Second: open_or_create should recover the same session with transcript
    let restored = Session::open_or_create(&locator, "/tmp", "model-1", storage.clone())
        .await
        .expect("restore session");

    let transcript = restored.transcript().await;
    assert!(
        !transcript.is_empty(),
        "transcript should be restored after drop"
    );
    assert_eq!(transcript.len(), 1);
}

#[tokio::test]
async fn open_or_create_updates_model_on_existing_session() {
    let storage: Arc<dyn evot::storage::Storage> = Arc::new(MemoryStorage::new());
    let locator = SessionLocator::new("test", "chat:c1:user:u1");

    let s1 = Session::open_or_create(&locator, "/tmp", "model-old", storage.clone())
        .await
        .expect("create with old model");
    s1.save().await.expect("save");

    let s2 = Session::open_or_create(&locator, "/tmp", "model-new", storage.clone())
        .await
        .expect("open with new model");

    assert_eq!(s2.meta().await.model, "model-new");
}

#[tokio::test]
async fn topic_and_dm_locators_produce_different_sessions() {
    let storage: Arc<dyn evot::storage::Storage> = Arc::new(MemoryStorage::new());
    let dm = SessionLocator::new("feishu", "chat:c1:user:u1");
    let topic = SessionLocator::new("feishu", "chat:c1:topic:omt_abc");

    let s_dm = Session::open_or_create(&dm, "/tmp", "m", storage.clone())
        .await
        .expect("dm session");
    let s_topic = Session::open_or_create(&topic, "/tmp", "m", storage.clone())
        .await
        .expect("topic session");

    assert_ne!(s_dm.session_id().await, s_topic.session_id().await);
}
