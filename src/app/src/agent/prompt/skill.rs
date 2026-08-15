use std::collections::HashMap;
use std::fs;
use std::path::Path;
use std::path::PathBuf;

use evot_engine::SkillSpec;
use serde::Deserialize;

// ---------------------------------------------------------------------------
// Builtin skills — compiled into the binary via include_str!()
// ---------------------------------------------------------------------------

struct BuiltinDef {
    name: &'static str,
    content: &'static str,
}

const BUILTINS: &[BuiltinDef] = &[
    BuiltinDef {
        name: "review",
        content: include_str!("prompts/review.md"),
    },
    BuiltinDef {
        name: "harden",
        content: include_str!("prompts/harden.md"),
    },
    BuiltinDef {
        name: "opencli",
        content: include_str!("prompts/opencli.md"),
    },
    BuiltinDef {
        name: "humanize",
        content: include_str!("prompts/humanize.md"),
    },
    BuiltinDef {
        name: "memory",
        content: include_str!("prompts/memory.md"),
    },
];

/// Parse builtin skill definitions into `SkillSpec` values.
/// Returns specs with an empty `base_dir` (no filesystem path).
fn builtin_specs() -> Result<Vec<SkillSpec>, SkillLoadError> {
    BUILTINS
        .iter()
        .map(|def| {
            let path = PathBuf::from(format!("<builtin:{}>", def.name));
            let description = parse_frontmatter(def.content, &path)?;
            let instructions = strip_frontmatter(def.content).to_string();
            Ok(SkillSpec {
                name: def.name.to_string(),
                description,
                instructions,
                base_dir: PathBuf::new(),
            })
        })
        .collect()
}

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

#[derive(Debug, thiserror::Error)]
pub enum SkillLoadError {
    #[error("IO error reading {path}: {source}")]
    Io {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("SKILL.md in {path} missing required frontmatter field: {field}")]
    MissingField { path: PathBuf, field: &'static str },
    #[error("SKILL.md in {path} has invalid frontmatter: {detail}")]
    InvalidFrontmatter { path: PathBuf, detail: String },
    #[error("unknown skill '{name}'. Available: {available}")]
    UnknownSkill { name: String, available: String },
}

// ---------------------------------------------------------------------------
// Public loader — builtin first, then filesystem (same name overrides)
// ---------------------------------------------------------------------------

pub fn load_skills(dirs: &[impl AsRef<Path>]) -> Result<Vec<SkillSpec>, SkillLoadError> {
    let mut by_name: HashMap<String, SkillSpec> = builtin_specs()?
        .into_iter()
        .map(|s| (s.name.clone(), s))
        .collect();

    for dir in dirs {
        let dir = dir.as_ref();
        if !dir.exists() {
            continue;
        }
        match load_skills_from_dir(dir) {
            Ok(specs) => {
                for spec in specs {
                    by_name.insert(spec.name.clone(), spec);
                }
            }
            Err(e) => {
                tracing::warn!("failed to load skills from {}: {e}", dir.display());
            }
        }
    }

    let mut specs: Vec<SkillSpec> = by_name.into_values().collect();
    specs.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(specs)
}

pub fn load_skills_by_name(
    dirs: &[impl AsRef<Path>],
    names: &[String],
) -> Result<Vec<SkillSpec>, SkillLoadError> {
    if names.is_empty() {
        return Ok(Vec::new());
    }
    let skills = load_skills(dirs)?;
    let available = skills
        .iter()
        .map(|skill| skill.name.clone())
        .collect::<Vec<_>>();
    let mut by_name: HashMap<String, SkillSpec> = skills
        .into_iter()
        .map(|skill| (skill.name.clone(), skill))
        .collect();
    let mut selected = Vec::with_capacity(names.len());
    for name in names {
        let Some(skill) = by_name.remove(name) else {
            return Err(SkillLoadError::UnknownSkill {
                name: name.clone(),
                available: available.join(", "),
            });
        };
        selected.push(skill);
    }
    selected.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(selected)
}

/// Load skills from filesystem directories only (no builtins).
pub fn load_fs_skills(dirs: &[impl AsRef<Path>]) -> Result<Vec<SkillSpec>, SkillLoadError> {
    let mut by_name: HashMap<String, SkillSpec> = HashMap::new();

    for dir in dirs {
        let dir = dir.as_ref();
        if !dir.exists() {
            continue;
        }
        let specs = load_skills_from_dir(dir)?;
        for spec in specs {
            by_name.insert(spec.name.clone(), spec);
        }
    }

    let mut specs: Vec<SkillSpec> = by_name.into_values().collect();
    specs.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(specs)
}

fn load_skills_from_dir(dir: &Path) -> Result<Vec<SkillSpec>, SkillLoadError> {
    let mut specs = Vec::new();

    let entries = fs::read_dir(dir).map_err(|e| SkillLoadError::Io {
        path: dir.to_path_buf(),
        source: e,
    })?;

    for entry in entries {
        let entry = entry.map_err(|e| SkillLoadError::Io {
            path: dir.to_path_buf(),
            source: e,
        })?;
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }

        let skill_md = path.join("SKILL.md");
        if !skill_md.exists() {
            continue;
        }

        let content = fs::read_to_string(&skill_md).map_err(|e| SkillLoadError::Io {
            path: skill_md.clone(),
            source: e,
        })?;

        let description = parse_frontmatter(&content, &skill_md)?;

        let name = path
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .to_string();

        let base_dir = fs::canonicalize(&path).unwrap_or(path);
        let instructions = strip_frontmatter(&content).to_string();

        specs.push(SkillSpec {
            name,
            description,
            instructions,
            base_dir,
        });
    }

    specs.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(specs)
}

#[derive(Deserialize)]
struct SkillFrontmatter {
    description: Option<String>,
}

fn split_frontmatter(content: &str) -> Result<(&str, &str), &'static str> {
    let trimmed = content.trim_start();
    let after_open = trimmed
        .strip_prefix("---\r\n")
        .or_else(|| trimmed.strip_prefix("---\n"))
        .ok_or("missing opening ---")?;

    let mut offset = 0;
    for segment in after_open.split_inclusive('\n') {
        let line = segment.trim_end_matches(['\r', '\n']);
        if line == "---" {
            return Ok((&after_open[..offset], &after_open[offset + segment.len()..]));
        }
        offset += segment.len();
    }

    Err("missing closing ---")
}

fn parse_frontmatter(content: &str, path: &Path) -> Result<String, SkillLoadError> {
    let (yaml_block, _) =
        split_frontmatter(content).map_err(|detail| SkillLoadError::InvalidFrontmatter {
            path: path.to_path_buf(),
            detail: detail.into(),
        })?;
    let frontmatter: SkillFrontmatter =
        serde_yaml::from_str(yaml_block).map_err(|error| SkillLoadError::InvalidFrontmatter {
            path: path.to_path_buf(),
            detail: error.to_string(),
        })?;
    let description = frontmatter
        .description
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .ok_or(SkillLoadError::MissingField {
            path: path.to_path_buf(),
            field: "description",
        })?;

    Ok(description)
}

fn strip_frontmatter(content: &str) -> &str {
    split_frontmatter(content)
        .map(|(_, body)| body)
        .unwrap_or(content)
}
