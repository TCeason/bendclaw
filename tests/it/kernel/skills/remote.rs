use std::collections::HashMap;
use std::sync::Arc;
use std::sync::Mutex;

use anyhow::Result;
use bendclaw::kernel::skills::remote::repository::DatabendSkillRepository;
use bendclaw::kernel::skills::remote::repository::SkillRepository;
use bendclaw::kernel::skills::skill::Skill;
use bendclaw::kernel::skills::skill::SkillFile;
use bendclaw::kernel::skills::skill::SkillRequirements;
use bendclaw::kernel::skills::skill::SkillScope;
use bendclaw::kernel::skills::skill::SkillSource;

use crate::common::fake_databend::paged_rows;
use crate::common::fake_databend::FakeDatabend;

#[derive(Clone)]
struct RemoteState {
    databases: Arc<Mutex<HashMap<String, HashMap<String, StoredSkill>>>>,
}

#[derive(Clone)]
struct StoredSkill {
    skill: Skill,
    sha256: String,
}

fn quoted_values(sql: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut chars = sql.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch != '\'' {
            continue;
        }
        let mut value = String::new();
        while let Some(next) = chars.next() {
            if next == '\'' {
                if chars.peek() == Some(&'\'') {
                    value.push('\'');
                    chars.next();
                    continue;
                }
                break;
            }
            value.push(next);
        }
        out.push(value);
    }
    out
}

fn make_skill(agent_id: &str, name: &str, creator: &str, body: &str) -> Skill {
    Skill {
        name: name.to_string(),
        version: "1.0.0".to_string(),
        description: format!("skill {name}"),
        scope: SkillScope::Private,
        source: SkillSource::Agent,
        user_id: agent_id.to_string(),
        created_by: Some(creator.to_string()),
        last_used_by: None,
        timeout: 45,
        executable: true,
        parameters: vec![],
        content: format!("# {name}"),
        files: vec![SkillFile {
            path: "scripts/run.sh".to_string(),
            body: body.to_string(),
        }],
        requires: Some(SkillRequirements {
            bins: vec!["bash".to_string()],
            env: vec!["API_TOKEN".to_string()],
        }),
        manifest: None,
    }
}

fn skill_rows(skills: impl Iterator<Item = Skill>) -> bendclaw::storage::pool::QueryResponse {
    let data = skills
        .map(|skill| {
            vec![
                serde_json::Value::String(skill.name),
                serde_json::Value::String(skill.version),
                serde_json::Value::String(skill.scope.as_str().to_string()),
                serde_json::Value::String(skill.source.as_str().to_string()),
                serde_json::Value::String(skill.user_id),
                serde_json::Value::String(skill.created_by.unwrap_or_default()),
                serde_json::Value::String(skill.description),
                serde_json::Value::String(skill.timeout.to_string()),
                serde_json::Value::String(skill.executable.to_string()),
                serde_json::Value::String(skill.content),
            ]
        })
        .collect();
    bendclaw::storage::pool::QueryResponse {
        id: String::new(),
        state: "Succeeded".to_string(),
        error: None,
        data,
        next_uri: None,
        final_uri: None,
        schema: Vec::new(),
    }
}

fn file_rows(skill: &Skill) -> bendclaw::storage::pool::QueryResponse {
    let data = skill
        .files
        .iter()
        .map(|file| {
            vec![
                serde_json::Value::String(file.path.clone()),
                serde_json::Value::String(file.body.clone()),
            ]
        })
        .collect();
    bendclaw::storage::pool::QueryResponse {
        id: String::new(),
        state: "Succeeded".to_string(),
        error: None,
        data,
        next_uri: None,
        final_uri: None,
        schema: Vec::new(),
    }
}

fn checksum_rows(
    skills: impl Iterator<Item = StoredSkill>,
) -> bendclaw::storage::pool::QueryResponse {
    let data = skills
        .map(|stored| {
            vec![
                serde_json::Value::String(stored.skill.name.clone()),
                serde_json::Value::String(stored.sha256),
            ]
        })
        .collect();
    bendclaw::storage::pool::QueryResponse {
        id: String::new(),
        state: "Succeeded".to_string(),
        error: None,
        data,
        next_uri: None,
        final_uri: None,
        schema: Vec::new(),
    }
}

fn fake_pool(state: &RemoteState, prefix: &str) -> bendclaw::storage::Pool {
    let state = state.clone();
    let prefix = prefix.to_string();
    let fake = FakeDatabend::new(move |sql, database| {
        let db_name = database.unwrap_or_default().to_string();
        let mut databases = state.databases.lock().expect("remote state");

        if sql.contains("evotai_meta.evotai_agents") {
            let mut names: Vec<_> = databases.keys().cloned().collect();
            names.sort();
            let rows: Vec<Vec<serde_json::Value>> = names
                .into_iter()
                .filter(|name| name.starts_with(&prefix))
                .map(|name| {
                    vec![serde_json::Value::String(
                        name.strip_prefix(&prefix).unwrap_or(&name).to_string(),
                    )]
                })
                .collect();
            return Ok(bendclaw::storage::pool::QueryResponse {
                id: String::new(),
                state: "Succeeded".to_string(),
                error: None,
                data: rows,
                next_uri: None,
                final_uri: None,
                schema: Vec::new(),
            });
        }

        if sql.starts_with("DELETE FROM skill_files WHERE ")
            || sql.starts_with("DELETE FROM skills WHERE ")
        {
            let values = quoted_values(sql);
            if let Some(skills) = databases.get_mut(&db_name) {
                if let Some(name) = values.first() {
                    skills.remove(name);
                }
            }
            return Ok(paged_rows(&[], None, None));
        }

        if sql.starts_with("INSERT INTO skills ") {
            let values = quoted_values(sql);
            let skill = Skill {
                name: values[0].clone(),
                version: values[1].clone(),
                description: values[6].clone(),
                scope: SkillScope::parse(&values[2]),
                source: SkillSource::parse(&values[3]),
                user_id: values[4].clone(),
                created_by: Some(values[5].clone()),
                last_used_by: None,
                timeout: 45,
                executable: true,
                parameters: vec![],
                content: values[7].clone(),
                files: Vec::new(),
                requires: None,
                manifest: None,
            };
            databases
                .entry(db_name.clone())
                .or_default()
                .insert(skill.name.clone(), StoredSkill {
                    skill,
                    sha256: values[8].clone(),
                });
            return Ok(paged_rows(&[], None, None));
        }

        if sql.starts_with("INSERT INTO skill_files ") {
            let values = quoted_values(sql);
            let skill_name = values[0].clone();
            let path = values[3].clone();
            let body = values[4].clone();
            if let Some(stored) = databases
                .get_mut(&db_name)
                .and_then(|skills| skills.get_mut(&skill_name))
            {
                stored.skill.files = vec![SkillFile { path, body }];
            }
            return Ok(paged_rows(&[], None, None));
        }

        if sql.starts_with("SELECT name, version, scope, source, agent_id, created_by, description, timeout, executable, content FROM skills WHERE name = ") {
            let name = quoted_values(sql).first().cloned().unwrap_or_default();
            let row = databases
                .get(&db_name)
                .and_then(|skills| skills.get(&name))
                .map(|stored| stored.skill.clone());
            return Ok(skill_rows(row.into_iter()));
        }

        if sql.starts_with("SELECT file_path, file_body FROM skill_files WHERE skill_name = ") {
            let name = quoted_values(sql).first().cloned().unwrap_or_default();
            let row = databases
                .get(&db_name)
                .and_then(|skills| skills.get(&name))
                .map(|stored| stored.skill.clone());
            return Ok(row.map_or_else(|| paged_rows(&[], None, None), |skill| file_rows(&skill)));
        }

        if sql.starts_with("SELECT name, version, scope, source, agent_id, created_by, description, timeout, executable, content FROM skills WHERE enabled = TRUE") {
            let rows = databases
                .get(&db_name)
                .map(|skills| {
                    skills
                        .values()
                        .map(|stored| stored.skill.clone())
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default();
            return Ok(skill_rows(rows.into_iter()));
        }

        if sql.starts_with("SELECT name, sha256 FROM skills WHERE enabled = TRUE") {
            let rows = databases
                .get(&db_name)
                .map(|skills| skills.values().cloned().collect::<Vec<_>>())
                .unwrap_or_default();
            return Ok(checksum_rows(rows.into_iter()));
        }

        Ok(paged_rows(&[], None, None))
    });
    fake.pool()
}

#[tokio::test]
async fn remote_repository_roundtrip_on_fake_databend() -> Result<()> {
    let state = RemoteState {
        databases: Arc::new(Mutex::new(HashMap::new())),
    };
    let pool = fake_pool(&state, "test_").with_database("test_agent-a")?;
    let repo = DatabendSkillRepository::new(pool);
    let skill = make_skill(
        "agent-a",
        "remote-tool",
        "user-1",
        "#!/usr/bin/env bash\necho first",
    );

    repo.save(&skill).await?;

    let fetched = repo
        .get("remote-tool")
        .await?
        .ok_or_else(|| anyhow::anyhow!("skill missing after save"))?;
    assert_eq!(fetched.created_by.as_deref(), Some("user-1"));
    assert_eq!(fetched.files.len(), 1);
    assert_eq!(fetched.files[0].path, "scripts/run.sh");

    let listed = repo.list().await?;
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].name, "remote-tool");

    let checksums = repo.checksums().await?;
    assert_eq!(checksums.get("remote-tool"), Some(&skill.compute_sha256()));

    repo.remove("remote-tool", Some("agent-a")).await?;
    assert!(repo.get("remote-tool").await?.is_none());
    Ok(())
}
