use tracing;

use super::pool::Pool;

const AGENT_MIGRATIONS: &[&str] = &[
    include_str!("../../migrations/0001_sessions.sql"),
    include_str!("../../migrations/0002_runs.sql"),
    include_str!("../../migrations/0003_agent.sql"),
    include_str!("../../migrations/0004_memory.sql"),
    include_str!("../../migrations/0005_skills.sql"),
    include_str!("../../migrations/0006_traces.sql"),
    include_str!("../../migrations/0007_variables.sql"),
    include_str!("../../migrations/0008_tasks.sql"),
    include_str!("../../migrations/0009_feedback.sql"),
    include_str!("../../migrations/0010_channels.sql"),
    include_str!("../../migrations/0011_recall.sql"),
];

/// Run all agent migrations against the pool's current database.
pub async fn run_agent(pool: &Pool) {
    run_statements(pool, AGENT_MIGRATIONS, "agent").await;
}

/// Run a list of raw SQL migrations against the current database.
/// All migrations run strictly sequentially to avoid race conditions
/// (e.g. a DROP in one file racing with a CREATE in another).
async fn run_statements(pool: &Pool, migrations: &[&str], scope: &str) {
    for sql in migrations {
        for stmt in sql.split(';').filter(|s| !s.trim().is_empty()) {
            let stmt = stmt.trim();
            if let Err(e) = pool.exec(stmt).await {
                tracing::info!(scope, error = %e, "migration statement skipped (may already exist)");
            }
        }
    }
    tracing::info!(scope, count = migrations.len(), "migrations completed");
}
