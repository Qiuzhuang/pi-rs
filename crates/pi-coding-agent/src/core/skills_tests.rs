//! Port of `test/skills.test.ts`.

use super::*;
use crate::core::source_info::SourceOrigin;

fn fixtures_dir() -> String {
    format!("{}/tests/fixtures/skills", env!("CARGO_MANIFEST_DIR"))
}

fn collision_fixtures_dir() -> String {
    format!("{}/tests/fixtures/skills-collision", env!("CARGO_MANIFEST_DIR"))
}

fn load_fixture(name: &str) -> LoadSkillsResult {
    load_skills_from_dir(&LoadSkillsFromDirOptions {
        dir: node_join(&[&fixtures_dir(), name]),
        source: "test".to_string(),
    })
}

fn has_diagnostic(result: &LoadSkillsResult, text: &str) -> bool {
    result.diagnostics.iter().any(|d| d.message.contains(text))
}

fn test_skill(name: &str, description: &str, file_path: &str, disable: bool) -> Skill {
    Skill {
        name: name.to_string(),
        description: description.to_string(),
        file_path: file_path.to_string(),
        base_dir: node_dirname(file_path),
        source_info: create_synthetic_source_info(
            file_path,
            &CreateSyntheticSourceInfoOptions {
                source: "test".to_string(),
                scope: None,
                origin: None,
                base_dir: None,
            },
        ),
        disable_model_invocation: disable,
    }
}

#[test]
fn loads_a_valid_skill() {
    let result = load_fixture("valid-skill");
    assert_eq!(result.skills.len(), 1);
    assert_eq!(result.skills[0].name, "valid-skill");
    assert_eq!(
        result.skills[0].description,
        "A valid skill for testing purposes."
    );
    assert_eq!(result.skills[0].source_info.source, "test");
    assert!(!result.skills[0].disable_model_invocation);
    assert!(result.diagnostics.is_empty());
}

#[test]
fn allows_names_that_do_not_match_parent_directory() {
    let result = load_fixture("name-mismatch");
    assert_eq!(result.skills.len(), 1);
    assert_eq!(result.skills[0].name, "different-name");
    assert!(!has_diagnostic(&result, "does not match parent directory"));
}

#[test]
fn warns_on_invalid_names() {
    let result = load_fixture("invalid-name-chars");
    assert_eq!(result.skills.len(), 1);
    assert!(has_diagnostic(&result, "invalid characters"));

    let result = load_fixture("long-name");
    assert_eq!(result.skills.len(), 1);
    assert!(has_diagnostic(&result, "exceeds 64 characters"));

    let result = load_fixture("consecutive-hyphens");
    assert_eq!(result.skills.len(), 1);
    assert!(has_diagnostic(&result, "consecutive hyphens"));
}

#[test]
fn skips_skills_without_description() {
    let result = load_fixture("missing-description");
    assert!(result.skills.is_empty());
    assert!(has_diagnostic(&result, "description is required"));

    let result = load_fixture("no-frontmatter");
    assert!(result.skills.is_empty());
    assert!(has_diagnostic(&result, "description is required"));
}

#[test]
fn ignores_unknown_frontmatter_fields() {
    let result = load_fixture("unknown-field");
    assert_eq!(result.skills.len(), 1);
    assert!(result.diagnostics.is_empty());
}

#[test]
fn loads_nested_skills_and_prefers_root_skill() {
    let result = load_fixture("nested");
    assert_eq!(result.skills.len(), 1);
    assert_eq!(result.skills[0].name, "child-skill");
    assert!(result.diagnostics.is_empty());

    let result = load_fixture("root-skill-preferred");
    assert_eq!(result.skills.len(), 1);
    assert_eq!(result.skills[0].name, "root-skill-preferred");
    assert_eq!(result.skills[0].description, "Root skill should win.");
    assert!(result.diagnostics.is_empty());
}

#[test]
fn warns_and_skips_invalid_yaml() {
    let result = load_fixture("invalid-yaml");
    assert!(result.skills.is_empty());
    assert!(has_diagnostic(&result, "at line"));
}

#[test]
fn preserves_multiline_descriptions() {
    let result = load_fixture("multiline-description");
    assert_eq!(result.skills.len(), 1);
    assert!(result.skills[0].description.contains('\n'));
    assert!(
        result.skills[0]
            .description
            .contains("This is a multiline description.")
    );
    assert!(result.diagnostics.is_empty());
}

#[test]
fn loads_all_skills_from_fixture_directory() {
    let result = load_skills_from_dir(&LoadSkillsFromDirOptions {
        dir: fixtures_dir(),
        source: "test".to_string(),
    });
    assert!(result.skills.len() >= 6);
}

#[test]
fn returns_empty_for_missing_directory() {
    let result = load_skills_from_dir(&LoadSkillsFromDirOptions {
        dir: "/non/existent/path".to_string(),
        source: "test".to_string(),
    });
    assert!(result.skills.is_empty());
    assert!(result.diagnostics.is_empty());
}

#[test]
fn parses_disable_model_invocation() {
    let result = load_fixture("disable-model-invocation");
    assert_eq!(result.skills.len(), 1);
    assert_eq!(result.skills[0].name, "disable-model-invocation");
    assert!(result.skills[0].disable_model_invocation);
    assert!(!has_diagnostic(&result, "unknown frontmatter field"));
}

#[test]
fn format_returns_empty_for_no_skills() {
    assert_eq!(format_skills_for_prompt(&[], None), "");
}

#[test]
fn formats_skills_as_xml() {
    let skills = vec![test_skill(
        "test-skill",
        "A test skill.",
        "/path/to/skill/SKILL.md",
        false,
    )];
    let result = format_skills_for_prompt(&skills, None);
    assert!(result.contains("<available_skills>"));
    assert!(result.contains("</available_skills>"));
    assert!(result.contains("<skill>"));
    assert!(result.contains("<name>test-skill</name>"));
    assert!(result.contains("<description>A test skill.</description>"));
    assert!(result.contains("<location>/path/to/skill/SKILL.md</location>"));
    let intro = &result[..result.find("<available_skills>").unwrap()];
    assert!(intro.contains("The following skills provide specialized instructions"));
    assert!(intro.contains("Use the read tool to load a skill's file"));

    let bash = format_skills_for_prompt(&skills, Some(FormatSkillsForPromptFileReadTool::Bash));
    assert!(bash.contains("Use bash to load a skill's file"));
    let indirect =
        format_skills_for_prompt(&skills, Some(FormatSkillsForPromptFileReadTool::Indirect));
    assert!(indirect.contains("\nLoad a skill's file when the task matches its description."));
}

#[test]
fn escapes_xml_special_characters() {
    let skills = vec![test_skill(
        "test-skill",
        "A skill with <special> & \"characters\".",
        "/path/to/skill/SKILL.md",
        false,
    )];
    let result = format_skills_for_prompt(&skills, None);
    assert!(result.contains("&lt;special&gt;"));
    assert!(result.contains("&amp;"));
    assert!(result.contains("&quot;characters&quot;"));
}

#[test]
fn formats_multiple_skills_and_excludes_hidden() {
    let skills = vec![
        test_skill("skill-one", "First skill.", "/path/one/SKILL.md", false),
        test_skill("skill-two", "Second skill.", "/path/two/SKILL.md", false),
        test_skill("hidden-skill", "A hidden skill.", "/path/hidden/SKILL.md", true),
    ];
    let result = format_skills_for_prompt(&skills, None);
    assert!(result.contains("<name>skill-one</name>"));
    assert!(result.contains("<name>skill-two</name>"));
    assert!(!result.contains("<name>hidden-skill</name>"));
    assert_eq!(result.matches("<skill>").count(), 2);

    assert_eq!(format_skills_for_prompt(&skills[2..], None), "");
}

fn empty_agent_dir() -> String {
    format!("{}/tests/fixtures/empty-agent", env!("CARGO_MANIFEST_DIR"))
}

fn empty_cwd() -> String {
    format!("{}/tests/fixtures/empty-cwd", env!("CARGO_MANIFEST_DIR"))
}

#[test]
fn load_skills_from_explicit_paths() {
    let result = load_skills(&LoadSkillsOptions {
        cwd: empty_cwd(),
        agent_dir: empty_agent_dir(),
        skill_paths: vec![node_join(&[&fixtures_dir(), "valid-skill"])],
        include_defaults: true,
    });
    assert_eq!(result.skills.len(), 1);
    assert_eq!(result.skills[0].source_info.scope, SourceScope::Temporary);
    assert_eq!(result.skills[0].source_info.origin, SourceOrigin::TopLevel);
    assert!(result.diagnostics.is_empty());
}

#[test]
fn load_skills_warns_for_missing_path() {
    let result = load_skills(&LoadSkillsOptions {
        cwd: empty_cwd(),
        agent_dir: empty_agent_dir(),
        skill_paths: vec!["/non/existent/path".to_string()],
        include_defaults: true,
    });
    assert!(result.skills.is_empty());
    assert!(has_diagnostic(&result, "does not exist"));
}

#[test]
fn load_skills_expands_tilde() {
    let home = dirs::home_dir().unwrap();
    let home_skills = home.join(".pi/agent/skills");
    let with_tilde = load_skills(&LoadSkillsOptions {
        cwd: empty_cwd(),
        agent_dir: empty_agent_dir(),
        skill_paths: vec!["~/.pi/agent/skills".to_string()],
        include_defaults: true,
    });
    let without_tilde = load_skills(&LoadSkillsOptions {
        cwd: empty_cwd(),
        agent_dir: empty_agent_dir(),
        skill_paths: vec![home_skills.to_string_lossy().into_owned()],
        include_defaults: true,
    });
    assert_eq!(with_tilde.skills.len(), without_tilde.skills.len());
}

#[test]
fn load_skills_reports_collisions_and_keeps_first() {
    let result = load_skills(&LoadSkillsOptions {
        cwd: empty_cwd(),
        agent_dir: empty_agent_dir(),
        skill_paths: vec![
            node_join(&[&collision_fixtures_dir(), "first"]),
            node_join(&[&collision_fixtures_dir(), "second"]),
        ],
        include_defaults: false,
    });
    assert_eq!(result.skills.len(), 1);
    assert!(result.skills[0].file_path.contains("/first/"));
    let collisions: Vec<_> = result
        .diagnostics
        .iter()
        .filter(|d| d.r#type == ResourceDiagnosticType::Collision)
        .collect();
    assert_eq!(collisions.len(), 1);
    assert_eq!(collisions[0].message, "name \"calendar\" collision");
    let collision = collisions[0].collision.as_ref().unwrap();
    assert!(collision.winner_path.contains("/first/"));
    assert!(collision.loser_path.contains("/second/"));
}

#[test]
fn respects_ignore_files() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().to_str().unwrap();
    for dir in ["keep", "skip", "nested/inner"] {
        let skill_dir = temp.path().join(dir);
        std::fs::create_dir_all(&skill_dir).unwrap();
        std::fs::write(
            skill_dir.join("SKILL.md"),
            format!(
                "---\nname: {}\ndescription: test\n---\nBody",
                dir.replace('/', "-")
            ),
        )
        .unwrap();
    }
    std::fs::write(temp.path().join(".gitignore"), "skip/\n# comment\n").unwrap();
    std::fs::write(temp.path().join("nested").join(".ignore"), "/inner\n").unwrap();
    let result = load_skills_from_dir(&LoadSkillsFromDirOptions {
        dir: root.to_string(),
        source: "test".to_string(),
    });
    let names: Vec<&str> = result.skills.iter().map(|s| s.name.as_str()).collect();
    assert_eq!(names, vec!["keep"]);
}

#[test]
fn root_markdown_files_need_descriptions() {
    let temp = tempfile::tempdir().unwrap();
    std::fs::write(
        temp.path().join("plain.md"),
        "# Not a skill\n\nNo frontmatter",
    )
    .unwrap();
    std::fs::write(
        temp.path().join("my-skill.md"),
        "---\ndescription: Root file skill\n---\nBody",
    )
    .unwrap();
    let result = load_skills_from_dir(&LoadSkillsFromDirOptions {
        dir: temp.path().to_str().unwrap().to_string(),
        source: "user".to_string(),
    });
    assert_eq!(result.skills.len(), 1);
    // Name falls back to the parent directory name.
    assert_eq!(
        result.skills[0].name,
        node_basename(temp.path().to_str().unwrap())
    );
    assert_eq!(result.skills[0].source_info.source, "local");
    assert_eq!(result.skills[0].source_info.scope, SourceScope::User);
}
