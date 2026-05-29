//! Tests for the per-file mutation queue.

use std::sync::Arc;
use std::time::Duration;

use evotengine::tools::file::mutex::acquire_file_lock;
use tokio::sync::Barrier;
use tokio::time::sleep;

#[tokio::test]
async fn serializes_same_path() {
    let tmp = std::env::temp_dir().join("evot-fmq-serialize.txt");
    tokio::fs::write(&tmp, b"x").await.expect("seed");

    let counter = Arc::new(tokio::sync::Mutex::new(0u32));
    let max = Arc::new(tokio::sync::Mutex::new(0u32));

    let mut handles = Vec::new();
    for _ in 0..4 {
        let path = tmp.clone();
        let counter = counter.clone();
        let max = max.clone();
        handles.push(tokio::spawn(async move {
            let _g = acquire_file_lock(&path).await;
            {
                let mut c = counter.lock().await;
                *c += 1;
                let mut m = max.lock().await;
                if *c > *m {
                    *m = *c;
                }
            }
            sleep(Duration::from_millis(20)).await;
            let mut c = counter.lock().await;
            *c -= 1;
        }));
    }
    for h in handles {
        h.await.expect("join");
    }
    assert_eq!(
        *max.lock().await,
        1,
        "same-path locks must serialize, observed concurrent holders"
    );

    let _ = std::fs::remove_file(tmp);
}

#[tokio::test]
async fn allows_different_paths_in_parallel() {
    let a = std::env::temp_dir().join("evot-fmq-a.txt");
    let b = std::env::temp_dir().join("evot-fmq-b.txt");
    tokio::fs::write(&a, b"x").await.expect("seed a");
    tokio::fs::write(&b, b"x").await.expect("seed b");

    // If the locks were shared, the barrier would deadlock — both tasks must
    // hold their respective locks at the same time to release each other.
    let barrier = Arc::new(Barrier::new(2));

    let ba = barrier.clone();
    let pa = a.clone();
    let ta = tokio::spawn(async move {
        let _g = acquire_file_lock(&pa).await;
        ba.wait().await;
    });
    let bb = barrier.clone();
    let pb = b.clone();
    let tb = tokio::spawn(async move {
        let _g = acquire_file_lock(&pb).await;
        bb.wait().await;
    });

    tokio::time::timeout(Duration::from_secs(2), async {
        ta.await.expect("ta");
        tb.await.expect("tb");
    })
    .await
    .expect("different paths must run in parallel");

    let _ = std::fs::remove_file(a);
    let _ = std::fs::remove_file(b);
}

#[tokio::test]
async fn reacquires_after_release() {
    let tmp = std::env::temp_dir().join("evot-fmq-reacquire.txt");
    tokio::fs::write(&tmp, b"x").await.expect("seed");

    {
        let _g = acquire_file_lock(&tmp).await;
    }
    // After drop, acquiring again must succeed quickly without contention.
    let _g = tokio::time::timeout(Duration::from_millis(500), acquire_file_lock(&tmp))
        .await
        .expect("re-acquire after release");

    let _ = std::fs::remove_file(tmp);
}
