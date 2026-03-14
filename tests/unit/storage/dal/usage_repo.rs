use anyhow::Result;
use bendclaw::storage::UsageRepo;

use crate::common::fake_databend::paged_rows;
use crate::common::fake_databend::FakeDatabend;

#[tokio::test]
async fn usage_repo_save_batch_generates_valid_sql() -> Result<()> {
    let fake = FakeDatabend::new(|sql, _db| {
        assert!(sql.starts_with("INSERT INTO usage"));
        assert!(sql.contains("prompt_tokens"));
        Ok(paged_rows(&[], None, None))
    });
    let repo = UsageRepo::new(fake.pool());
    let record = bendclaw::storage::UsageRecord {
        id: "u-1".into(),
        agent_id: "a-1".into(),
        user_id: "user-1".into(),
        session_id: "s-1".into(),
        run_id: "r-1".into(),
        provider: "anthropic".into(),
        model: "claude-3".into(),
        model_role: "main".into(),
        prompt_tokens: 100,
        completion_tokens: 50,
        reasoning_tokens: 0,
        total_tokens: 150,
        cache_read_tokens: 10,
        cache_write_tokens: 5,
        ttft_ms: 200,
        cost: 0.003,
        created_at: "2026-03-11T00:00:00Z".into(),
    };
    repo.save_batch(&[record]).await?;
    assert_eq!(fake.calls().len(), 1);
    Ok(())
}

#[tokio::test]
async fn usage_repo_save_batch_empty_is_noop() -> Result<()> {
    let fake = FakeDatabend::new(|_sql, _db| {
        panic!("no SQL should be issued for empty batch");
    });
    let repo = UsageRepo::new(fake.pool());
    repo.save_batch(&[]).await?;
    assert!(fake.calls().is_empty());
    Ok(())
}

#[tokio::test]
async fn usage_repo_summary_by_user_generates_valid_sql() -> Result<()> {
    let fake = FakeDatabend::new(|sql, _db| {
        assert!(sql.contains("SUM(prompt_tokens)"));
        assert!(sql.contains("user_id = 'user-1'"));
        Ok(paged_rows(
            &[&["100", "50", "0", "150", "10", "5", "0.003", "1"]],
            None,
            None,
        ))
    });
    let repo = UsageRepo::new(fake.pool());
    let summary = repo.summary_by_user("user-1").await?;
    assert_eq!(summary.total_tokens, 150);
    Ok(())
}

#[tokio::test]
async fn usage_repo_summary_by_agent_day_generates_valid_sql() -> Result<()> {
    let fake = FakeDatabend::new(|sql, _db| {
        assert!(sql.contains("agent_id = 'a-1'"));
        assert!(sql.contains("TO_DATE('2026-03-11')"));
        Ok(paged_rows(
            &[&["100", "50", "0", "150", "10", "5", "0.003", "1"]],
            None,
            None,
        ))
    });
    let repo = UsageRepo::new(fake.pool());
    let summary = repo.summary_by_agent_day("a-1", "2026-03-11").await?;
    assert_eq!(summary.total_tokens, 150);
    Ok(())
}

#[tokio::test]
async fn usage_repo_daily_by_agent_generates_valid_sql() -> Result<()> {
    let fake = FakeDatabend::new(|sql, _db| {
        assert!(sql.contains("TO_VARCHAR(TO_DATE(created_at))"));
        assert!(sql.contains("agent_id = 'a-1'"));
        assert!(sql.contains("INTERVAL 7 DAY"));
        assert!(sql.contains("GROUP BY day"));
        Ok(paged_rows(
            &[&["2026-03-11", "100", "50", "150", "0.003", "5"]],
            None,
            None,
        ))
    });
    let repo = UsageRepo::new(fake.pool());
    let daily = repo.daily_by_agent("a-1", 7).await?;
    assert_eq!(daily.len(), 1);
    assert_eq!(daily[0].date, "2026-03-11");
    assert_eq!(daily[0].total_tokens, 150);
    Ok(())
}
