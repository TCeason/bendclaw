use std::fs;
use std::path::Path;

use evot::agent::prompt::skill::load_fs_skills;
use evot::agent::prompt::skill::load_skills;
use evot::agent::prompt::skill::SkillLoadError;
use tempfile::TempDir;

fn create_skill(dir: &Path, name: &str, description: &str) {
    let skill_dir = dir.join(name);
    fs::create_dir_all(&skill_dir).unwrap();
    fs::write(
        skill_dir.join("SKILL.md"),
        format!(
            "---\nname: {name}\ndescription: {description}\n---\n\n# Instructions\n\nDo stuff.\n"
        ),
    )
    .unwrap();
}

#[test]
fn load_from_directory() {
    let tmp = TempDir::new().unwrap();
    create_skill(tmp.path(), "weather", "Get weather");
    create_skill(tmp.path(), "git", "Git ops");

    let specs = load_fs_skills(&[tmp.path().to_path_buf()]).unwrap();
    assert_eq!(specs.len(), 2);
    assert_eq!(specs[0].name, "git");
    assert_eq!(specs[1].name, "weather");
    assert_eq!(specs[1].description, "Get weather");
    assert!(specs[1].instructions.contains("# Instructions"));
}

#[test]
fn name_comes_from_directory_not_frontmatter() {
    let tmp = TempDir::new().unwrap();
    let skill_dir = tmp.path().join("my-tool");
    fs::create_dir_all(&skill_dir).unwrap();
    fs::write(
        skill_dir.join("SKILL.md"),
        "---\nname: different-name\ndescription: A tool\n---\n\nBody.\n",
    )
    .unwrap();

    let specs = load_fs_skills(&[tmp.path().to_path_buf()]).unwrap();
    assert_eq!(specs.len(), 1);
    assert_eq!(specs[0].name, "my-tool");
}

#[test]
fn later_dirs_override_earlier() {
    let dir1 = TempDir::new().unwrap();
    let dir2 = TempDir::new().unwrap();
    create_skill(dir1.path(), "weather", "Old weather");
    create_skill(dir2.path(), "weather", "New weather");

    let specs = load_fs_skills(&[dir1.path().to_path_buf(), dir2.path().to_path_buf()]).unwrap();
    assert_eq!(specs.len(), 1);
    assert_eq!(specs[0].description, "New weather");
}

#[test]
fn skips_nonexistent_dirs() {
    let specs = load_fs_skills(&[std::path::PathBuf::from("/nonexistent/path")]).unwrap();
    assert!(specs.is_empty());
}

#[test]
fn skips_dirs_without_skill_md() {
    let tmp = TempDir::new().unwrap();
    fs::create_dir_all(tmp.path().join("empty-skill")).unwrap();

    let specs = load_fs_skills(&[tmp.path().to_path_buf()]).unwrap();
    assert!(specs.is_empty());
}

#[test]
fn error_on_missing_frontmatter() {
    let tmp = TempDir::new().unwrap();
    let skill_dir = tmp.path().join("bad");
    fs::create_dir_all(&skill_dir).unwrap();
    fs::write(skill_dir.join("SKILL.md"), "No frontmatter here.").unwrap();

    let err = load_fs_skills(&[tmp.path().to_path_buf()]).unwrap_err();
    assert!(matches!(err, SkillLoadError::InvalidFrontmatter { .. }));
}

#[test]
fn error_on_missing_description() {
    let tmp = TempDir::new().unwrap();
    let skill_dir = tmp.path().join("bad");
    fs::create_dir_all(&skill_dir).unwrap();
    fs::write(skill_dir.join("SKILL.md"), "---\nname: bad\n---\n\nBody.\n").unwrap();

    let err = load_fs_skills(&[tmp.path().to_path_buf()]).unwrap_err();
    assert!(matches!(err, SkillLoadError::MissingField { .. }));
}

#[test]
fn error_on_empty_description() {
    let tmp = TempDir::new().unwrap();
    let skill_dir = tmp.path().join("bad");
    fs::create_dir_all(&skill_dir).unwrap();
    fs::write(
        skill_dir.join("SKILL.md"),
        "---\nname: bad\ndescription:\n---\n\nBody.\n",
    )
    .unwrap();

    let err = load_fs_skills(&[tmp.path().to_path_buf()]).unwrap_err();
    assert!(matches!(err, SkillLoadError::MissingField { .. }));
}

#[test]
fn strips_frontmatter_from_instructions() {
    let tmp = TempDir::new().unwrap();
    create_skill(tmp.path(), "test-skill", "A test");

    let specs = load_fs_skills(&[tmp.path().to_path_buf()]).unwrap();
    assert!(!specs[0].instructions.contains("---"));
    assert!(specs[0].instructions.contains("# Instructions"));
}

#[test]
fn handles_quoted_description() {
    let tmp = TempDir::new().unwrap();
    let skill_dir = tmp.path().join("quoted");
    fs::create_dir_all(&skill_dir).unwrap();
    fs::write(
        skill_dir.join("SKILL.md"),
        "---\nname: quoted\ndescription: \"A quoted desc\"\n---\n\nBody.\n",
    )
    .unwrap();

    let specs = load_fs_skills(&[tmp.path().to_path_buf()]).unwrap();
    assert_eq!(specs[0].description, "A quoted desc");
}

// ---------------------------------------------------------------------------
// Builtin skill tests
// ---------------------------------------------------------------------------

#[test]
fn builtin_review_skill_loaded() {
    // load_skills with no dirs should still return builtins
    let empty: Vec<std::path::PathBuf> = vec![];
    let specs = load_skills(&empty).unwrap();
    let review = specs.iter().find(|s| s.name == "review");
    assert!(review.is_some(), "builtin review skill should be present");
    let review = review.unwrap();
    assert!(!review.description.is_empty());
    assert!(review.instructions.contains("# Code Review"));
    assert!(review.base_dir.as_os_str().is_empty());
}

#[test]
fn builtin_harden_skill_loaded() -> Result<(), Box<dyn std::error::Error>> {
    let empty: Vec<std::path::PathBuf> = vec![];
    let specs = load_skills(&empty)?;
    let harden = match specs.iter().find(|s| s.name == "harden") {
        Some(skill) => skill,
        None => return Err("builtin harden skill should be present".into()),
    };
    assert!(!harden.description.is_empty());
    assert!(harden.instructions.contains("# Harden"));
    assert!(harden.base_dir.as_os_str().is_empty());
    Ok(())
}

#[test]
fn builtin_humanize_skill_loaded() -> Result<(), Box<dyn std::error::Error>> {
    let empty: Vec<std::path::PathBuf> = vec![];
    let specs = load_skills(&empty)?;
    let humanize = match specs.iter().find(|s| s.name == "humanize") {
        Some(skill) => skill,
        None => return Err("builtin humanize skill should be present".into()),
    };
    assert!(!humanize.description.is_empty());
    assert!(humanize.instructions.contains("# Humanize"));
    assert!(humanize.base_dir.as_os_str().is_empty());
    Ok(())
}

#[test]
fn fs_skill_overrides_builtin() {
    let tmp = TempDir::new().unwrap();
    create_skill(tmp.path(), "review", "Custom review");

    let specs = load_skills(&[tmp.path().to_path_buf()]).unwrap();
    let review = specs.iter().find(|s| s.name == "review").unwrap();
    assert_eq!(review.description, "Custom review");
    assert!(
        !review.base_dir.as_os_str().is_empty(),
        "fs skill should have a base_dir"
    );
}

#[test]
fn filesystem_skill_error_does_not_drop_builtins() -> Result<(), Box<dyn std::error::Error>> {
    let tmp = TempDir::new()?;
    let skill_dir = tmp.path().join("bad");
    fs::create_dir_all(&skill_dir)?;
    fs::write(skill_dir.join("SKILL.md"), "No frontmatter here.")?;

    let specs = load_skills(&[tmp.path().to_path_buf()])?;
    assert!(specs.iter().any(|s| s.name == "review"));
    assert!(specs.iter().any(|s| s.name == "harden"));
    assert!(specs.iter().all(|s| s.name != "bad"));
    Ok(())
}
