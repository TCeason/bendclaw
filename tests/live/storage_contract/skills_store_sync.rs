use std::sync::Arc;

use anyhow::Context as _;
use anyhow::Result;
use bendclaw::kernel::skills::remote::repository::DatabendSkillRepository;
use bendclaw::kernel::skills::remote::repository::SkillRepository;
use bendclaw::kernel::skills::skill::Skill;
use bendclaw::kernel::skills::skill::SkillFile;
use bendclaw::kernel::skills::skill::SkillRequirements;
use bendclaw::kernel::skills::skill::SkillScope;
use bendclaw::kernel::skills::skill::SkillSource;
use bendclaw::kernel::skills::store::SkillStore;
use bendclaw::storage::AgentDatabases;
use bendclaw::storage::Pool;
use tempfile::TempDir;

use crate::common::setup::require_api_config;
use crate::common::setup::uid;
use crate::common::setup::TestContext;

const SKILLS_MIGRATION: &str = include_str!("../../../migrations/base/skills.sql");

fn make_skill(agent_id: &str, name: &str, creator: &str) -> Skill {
    Skill {
        name: name.to_string(),
        version: "1.0.0".to_string(),
        description: "remote skill".to_string(),
        scope: SkillScope::Agent,
        source: SkillSource::Agent,
        agent_id: Some(agent_id.to_string()),
        created_by: Some(creator.to_string()),
        timeout: 45,
        executable: true,
        parameters: vec![],
        content: "# Remote Skill".to_string(),
        files: vec![
            SkillFile {
                path: "references/usage.md".to_string(),
                body: "# Usage".to_string(),
            },
            SkillFile {
                path: "scripts/run.sh".to_string(),
                body: "#!/usr/bin/env bash\necho sync".to_string(),
            },
        ],
        requires: Some(SkillRequirements {
            bins: vec!["bash".to_string()],
            env: vec!["API_TOKEN".to_string()],
        }),
        manifest: None,
    }
}

async fn setup_databases(prefix: &str, agent_ids: &[&str]) -> Result<Arc<AgentDatabases>> {
    let (base_url, token, warehouse) = require_api_config()?;
    if token.is_empty() {
        anyhow::bail!("missing Databend token");
    }
    let root = Pool::new(&base_url, &token, &warehouse)?;
    let databases = Arc::new(AgentDatabases::new(root.clone(), prefix)?);

    // Ensure evotai_meta registry exists
    root.exec("CREATE DATABASE IF NOT EXISTS evotai_meta")
        .await?;
    let meta_pool = root.with_database("evotai_meta")?;
    meta_pool
        .exec(include_str!("../../../migrations/org/agents.sql"))
        .await?;

    for agent_id in agent_ids {
        let db_name = databases.agent_database_name(agent_id)?;
        root.exec(&format!("CREATE DATABASE IF NOT EXISTS `{db_name}`"))
            .await?;
        let pool = root.with_database(&db_name)?;
        run_migration(&pool, SKILLS_MIGRATION).await?;

        // Register agent in evotai_meta
        meta_pool
            .exec(&format!(
                "INSERT INTO evotai_agents (agent_id, database_name, status) \
             VALUES ('{agent_id}', '{db_name}', 'active') \
             ON DUPLICATE KEY UPDATE status = 'active'"
            ))
            .await?;
    }

    Ok(databases)
}

async fn setup_databases_or_skip(
    prefix: &str,
    agent_ids: &[&str],
) -> Result<Option<Arc<AgentDatabases>>> {
    match setup_databases(prefix, agent_ids).await {
        Ok(databases) => Ok(Some(databases)),
        Err(error) => {
            eprintln!("skipping store_sync test: {error}");
            Ok(None)
        }
    }
}

async fn run_migration(pool: &Pool, sql: &str) -> Result<()> {
    for stmt in sql.split(';') {
        let stmt = stmt.trim();
        let has_code = stmt
            .lines()
            .any(|line| !line.trim().is_empty() && !line.trim().starts_with("--"));
        if !has_code {
            continue;
        }
        pool.exec(stmt)
            .await
            .with_context(|| format!("migration statement failed:\n{stmt}"))?;
    }
    Ok(())
}

#[tokio::test]
async fn refresh_mirrors_remote_skill_and_exposes_full_data() -> Result<()> {
    let ctx = TestContext::setup().await?;
    let agent_id = uid("agent");
    let prefix = ctx.prefix().to_string();
    let Some(databases) = setup_databases_or_skip(&prefix, &[&agent_id]).await? else {
        return Ok(());
    };
    let repo = DatabendSkillRepository::new(databases.agent_pool(&agent_id)?);

    let skill_name = uid("remote-skill");
    let skill = make_skill(&agent_id, &skill_name, "user-1");
    repo.save(&skill).await?;

    let workspace = TempDir::new()?;
    let store = SkillStore::new(databases, workspace.path().to_path_buf(), None);
    store.refresh().await?;

    let loaded = store
        .get(&agent_id, &skill_name)
        .ok_or_else(|| anyhow::anyhow!("skill not found after refresh"))?;
    assert_eq!(loaded.created_by.as_deref(), Some("user-1"));
    assert_eq!(loaded.timeout, 45);
    // SkillStore loads from filesystem: scripts/ + references/ → 2 files.
    // SKILL.md is parsed into skill.content, not included in files.
    assert_eq!(loaded.files.len(), 2);
    let file_paths: Vec<&str> = loaded.files.iter().map(|f| f.path.as_str()).collect();
    assert!(file_paths.contains(&"references/usage.md"));
    assert!(file_paths.contains(&"scripts/run.sh"));
    assert_eq!(
        store.read_skill(&agent_id, &format!("{skill_name}/references/usage.md")),
        Some("# Usage".to_string())
    );
    assert!(store
        .host_script_path(&agent_id, &skill_name)
        .is_some_and(|path| path.exists()));

    Ok(())
}

#[tokio::test]
async fn refresh_removes_stale_remote_skill_directory() -> Result<()> {
    let ctx = TestContext::setup().await?;
    let agent_id = uid("agent");
    let prefix = ctx.prefix().to_string();
    let Some(databases) = setup_databases_or_skip(&prefix, &[&agent_id]).await? else {
        return Ok(());
    };
    let repo = DatabendSkillRepository::new(databases.agent_pool(&agent_id)?);

    let skill_name = uid("remote-skill");
    repo.save(&make_skill(&agent_id, &skill_name, "user-1"))
        .await?;

    let workspace = TempDir::new()?;
    let store = SkillStore::new(databases.clone(), workspace.path().to_path_buf(), None);
    store.refresh().await?;

    let remote_dir = workspace
        .path()
        .join("agents")
        .join(&agent_id)
        .join("skills")
        .join(".remote")
        .join(&skill_name);
    assert!(remote_dir.exists());

    repo.remove(&skill_name, Some(&agent_id)).await?;
    store.refresh().await?;

    assert!(store.get(&agent_id, &skill_name).is_none());
    assert!(!remote_dir.exists());
    Ok(())
}

#[tokio::test]
async fn save_writes_skill_md_to_skill_files() -> Result<()> {
    let ctx = TestContext::setup().await?;
    let agent_id = uid("agent");
    let prefix = ctx.prefix().to_string();
    let Some(databases) = setup_databases_or_skip(&prefix, &[&agent_id]).await? else {
        return Ok(());
    };
    let repo = DatabendSkillRepository::new(databases.agent_pool(&agent_id)?);

    let skill_name = uid("skill-md-test");
    repo.save(&make_skill(&agent_id, &skill_name, "user-1"))
        .await?;

    // Load via repository (DB path) — this is what the console file tree uses.
    let loaded = repo
        .get(&skill_name)
        .await?
        .ok_or_else(|| anyhow::anyhow!("skill not found in DB"))?;

    // 2 explicit files + SKILL.md from skill.content = 3 total.
    let file_paths: Vec<&str> = loaded.files.iter().map(|f| f.path.as_str()).collect();
    assert!(file_paths.contains(&"references/usage.md"));
    assert!(file_paths.contains(&"scripts/run.sh"));
    assert!(file_paths.contains(&"SKILL.md"));
    assert_eq!(loaded.files.len(), 3);

    let skill_md = loaded.files.iter().find(|f| f.path == "SKILL.md").unwrap();
    assert_eq!(skill_md.body, "# Remote Skill");
    Ok(())
}

#[tokio::test]
async fn save_with_empty_content_does_not_create_skill_md() -> Result<()> {
    let ctx = TestContext::setup().await?;
    let agent_id = uid("agent");
    let prefix = ctx.prefix().to_string();
    let Some(databases) = setup_databases_or_skip(&prefix, &[&agent_id]).await? else {
        return Ok(());
    };
    let repo = DatabendSkillRepository::new(databases.agent_pool(&agent_id)?);

    let skill_name = uid("empty-content");
    let mut skill = make_skill(&agent_id, &skill_name, "user-1");
    skill.content = String::new();

    repo.save(&skill).await?;

    let loaded = repo
        .get(&skill_name)
        .await?
        .ok_or_else(|| anyhow::anyhow!("skill not found in DB"))?;

    // Only the 2 explicit files — no SKILL.md since content was empty.
    let file_paths: Vec<&str> = loaded.files.iter().map(|f| f.path.as_str()).collect();
    assert!(!file_paths.contains(&"SKILL.md"));
    assert_eq!(loaded.files.len(), 2);
    Ok(())
}
