use std::sync::Arc;

use anyhow::Result;

use crate::mocks::llm::MockLLMProvider;

fn prompt_storage_and_skills() -> (
    Arc<bendclaw::kernel::agent_store::AgentStore>,
    Arc<bendclaw::kernel::skills::store::SkillStore>,
) {
    let rt = tokio::runtime::Runtime::new().expect("runtime");
    rt.block_on(async {
        let pool = crate::common::setup::pool().await.expect("test pool");
        let llm: Arc<dyn bendclaw::llm::provider::LLMProvider> =
            Arc::new(MockLLMProvider::with_text("ok"));
        let storage = Arc::new(bendclaw::kernel::agent_store::AgentStore::new(
            pool.clone(),
            llm,
        ));
        let databases = Arc::new(
            bendclaw::storage::AgentDatabases::new(pool.clone(), "test_").expect("databases"),
        );
        let dir = std::env::temp_dir().join(format!("bendclaw-prompt-{}", ulid::Ulid::new()));
        let _ = std::fs::create_dir_all(&dir);
        let skills = crate::mocks::skill::test_skill_store(databases, dir);
        (storage, skills)
    })
}

#[tokio::test]
async fn prompt_builder_build_with_injected_identity() -> Result<()> {
    use bendclaw::kernel::run::prompt::PromptBuilder;

    let (storage, skills) = prompt_storage_and_skills();
    let prompt = PromptBuilder::new(storage, skills)
        .with_identity("You are a test agent.")
        .build("nonexistent-agent", "u1", "s1")
        .await?;
    assert!(prompt.contains("You are a test agent."));
    Ok(())
}

#[tokio::test]
async fn prompt_builder_build_with_injected_soul() -> Result<()> {
    use bendclaw::kernel::run::prompt::PromptBuilder;

    let (storage, skills) = prompt_storage_and_skills();
    let prompt = PromptBuilder::new(storage, skills)
        .with_soul("Be helpful and concise.")
        .build("nonexistent-agent", "u1", "s1")
        .await?;
    assert!(prompt.contains("Be helpful and concise."));
    assert!(prompt.contains("## Soul"));
    Ok(())
}

#[tokio::test]
async fn prompt_builder_build_with_injected_runtime() -> Result<()> {
    use bendclaw::kernel::run::prompt::PromptBuilder;

    let (storage, skills) = prompt_storage_and_skills();
    let prompt = PromptBuilder::new(storage, skills)
        .with_runtime("Host: testhost | OS: linux (x86_64)")
        .build("nonexistent-agent", "u1", "s1")
        .await?;
    assert!(prompt.contains("## Runtime"));
    assert!(prompt.contains("testhost"));
    Ok(())
}

#[tokio::test]
async fn prompt_builder_build_with_injected_learnings() -> Result<()> {
    use bendclaw::kernel::run::prompt::PromptBuilder;

    let (storage, skills) = prompt_storage_and_skills();
    let prompt = PromptBuilder::new(storage, skills)
        .with_learnings("- Always check logs first\n")
        .build("nonexistent-agent", "u1", "s1")
        .await?;
    assert!(prompt.contains("## Learnings"));
    assert!(prompt.contains("Always check logs first"));
    Ok(())
}

#[tokio::test]
async fn prompt_builder_build_with_injected_recent_errors() -> Result<()> {
    use bendclaw::kernel::run::prompt::PromptBuilder;

    let (storage, skills) = prompt_storage_and_skills();
    let prompt = PromptBuilder::new(storage, skills)
        .with_recent_errors("- `shell`: command not found\n")
        .build("nonexistent-agent", "u1", "s1")
        .await?;
    assert!(prompt.contains("## Recent Errors"));
    assert!(prompt.contains("command not found"));
    Ok(())
}

#[tokio::test]
async fn prompt_builder_build_with_tools() -> Result<()> {
    use bendclaw::kernel::run::prompt::PromptBuilder;
    use bendclaw::llm::tool::ToolSchema;

    let (storage, skills) = prompt_storage_and_skills();
    let tools = Arc::new(vec![
        ToolSchema::new("shell", "Run shell commands", serde_json::json!({})),
        ToolSchema::new("file_read", "Read files", serde_json::json!({})),
    ]);
    let prompt = PromptBuilder::new(storage, skills)
        .with_tools(tools)
        .build("nonexistent-agent", "u1", "s1")
        .await?;
    assert!(prompt.contains("## Available Tools"));
    assert!(prompt.contains("shell"));
    assert!(prompt.contains("file_read"));
    Ok(())
}

#[tokio::test]
async fn prompt_builder_empty_setters_skip_layers() -> Result<()> {
    use bendclaw::kernel::run::prompt::PromptBuilder;

    let (storage, skills) = prompt_storage_and_skills();
    let prompt = PromptBuilder::new(storage, skills)
        .with_identity("")
        .with_soul("")
        .with_runtime("")
        .with_learnings("")
        .with_recent_errors("")
        .build("nonexistent-agent", "u1", "s1")
        .await?;
    assert!(!prompt.contains("## Soul"));
    assert!(!prompt.contains("## Learnings"));
    assert!(!prompt.contains("## Recent Errors"));
    Ok(())
}

#[tokio::test]
async fn prompt_builder_runtime_falls_back_to_env() -> Result<()> {
    use bendclaw::kernel::run::prompt::PromptBuilder;

    let (storage, skills) = prompt_storage_and_skills();
    let prompt = PromptBuilder::new(storage, skills)
        .build("nonexistent-agent", "u1", "s1")
        .await?;
    assert!(prompt.contains("## Runtime"));
    Ok(())
}
