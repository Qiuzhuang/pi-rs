//! Ports of `test/sdk-session-manager.test.ts` and `test/sdk-skills.test.ts`.

use std::sync::Arc;

use pi_coding_agent::core::extensions::loader::create_extension_runtime;
use pi_coding_agent::core::extensions::types::LoadExtensionsResult;
use pi_coding_agent::core::resource_loader::{
    ResourceExtensionPaths, ResourceLoader, ResourceLoaderGetAgentsFilesResult,
    ResourceLoaderGetAppendSystemPromptSourcesResult, ResourceLoaderGetPromptsResult,
    ResourceLoaderGetSkillsResult, ResourceLoaderGetSystemPromptSourceResult,
    ResourceLoaderGetThemesResult, ResourceLoaderReloadOptions,
};
use pi_coding_agent::core::sdk::{CreateAgentSessionOptions, create_agent_session};
use pi_coding_agent::core::session_manager::SessionManager;
use pi_coding_agent::core::skills::Skill;
use pi_coding_agent::core::source_info::{CreateSyntheticSourceInfoOptions, create_synthetic_source_info};
use serde_json::json;

struct TempDir(String);

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn temp(prefix: &str) -> TempDir {
    let dir = std::env::temp_dir()
        .join(format!("{prefix}-{}", uuid::Uuid::new_v4()))
        .to_string_lossy()
        .into_owned();
    std::fs::create_dir_all(&dir).unwrap();
    TempDir(dir)
}

fn sonnet() -> Option<pi_ai::types::Model> {
    pi_ai::compat::get_model("anthropic", "claude-sonnet-4-5")
}

async fn run_bash(session: &pi_coding_agent::core::agent_session::AgentSession, command: &str) -> String {
    let tool = session
        .agent()
        .tools()
        .into_iter()
        .find(|t| t.name() == "bash")
        .expect("bash tool");
    let result = tool
        .execute(
            "test",
            json!({"command": command}),
            tokio_util::sync::CancellationToken::new(),
            Arc::new(|_| {}),
        )
        .await
        .unwrap();
    result
        .content
        .iter()
        .filter_map(|c| c.as_text().map(|t| t.text.clone()))
        .collect()
}

#[tokio::test]
async fn uses_agent_dir_for_the_default_persisted_session_path() {
    let t = temp("pi-sdk-session-test");
    let cwd = format!("{}/project", t.0);
    let agent_dir = format!("{}/agent", t.0);
    std::fs::create_dir_all(&cwd).unwrap();
    std::fs::create_dir_all(&agent_dir).unwrap();

    let session = create_agent_session(Some(&CreateAgentSessionOptions {
        cwd: Some(cwd.clone()),
        agent_dir: Some(agent_dir.clone()),
        model: sonnet(),
        ..Default::default()
    }))
    .await
    .unwrap()
    .session;

    let safe_path = format!(
        "--{}--",
        cwd.trim_start_matches(['/', '\\']).replace(['/', '\\', ':'], "-")
    );
    let expected = format!("{agent_dir}/sessions/{safe_path}");
    let manager = session.session_manager();
    assert_eq!(manager.get_session_dir(), expected);
    assert!(manager.get_session_file().unwrap().starts_with(&format!("{expected}/")));
    session.dispose();
}

#[tokio::test]
async fn keeps_an_explicit_session_manager_override() {
    let t = temp("pi-sdk-session-test");
    let session_manager = SessionManager::in_memory(Some(&t.0), None, None).unwrap();
    let session = create_agent_session(Some(&CreateAgentSessionOptions {
        cwd: Some(t.0.clone()),
        agent_dir: Some(format!("{}/agent", t.0)),
        model: sonnet(),
        session_manager: Some(session_manager.clone()),
        ..Default::default()
    }))
    .await
    .unwrap()
    .session;

    assert!(Arc::ptr_eq(&session.session_manager(), &session_manager));
    assert!(!session.session_manager().is_persisted());
    session.dispose();
}

#[tokio::test]
async fn derives_cwd_from_an_explicit_session_manager_when_cwd_is_omitted() {
    let t = temp("pi-sdk-session-test");
    let session_cwd = format!("{}/session-project", t.0);
    std::fs::create_dir_all(&session_cwd).unwrap();
    let session_manager = SessionManager::in_memory(Some(&session_cwd), None, None).unwrap();
    let session = create_agent_session(Some(&CreateAgentSessionOptions {
        agent_dir: Some(format!("{}/agent", t.0)),
        model: sonnet(),
        session_manager: Some(session_manager.clone()),
        ..Default::default()
    }))
    .await
    .unwrap()
    .session;

    assert!(Arc::ptr_eq(&session.session_manager(), &session_manager));
    assert!(session.system_prompt().contains(&format!("<cwd>\n{session_cwd}\n</cwd>")));
    let output = run_bash(&session, "pwd").await;
    assert_eq!(
        std::fs::canonicalize(output.trim()).unwrap(),
        std::fs::canonicalize(&session_cwd).unwrap()
    );
    session.dispose();
}

#[tokio::test]
async fn exposes_current_session_state_to_the_built_in_bash_tool() {
    let t = temp("pi-sdk-session-test");
    let cwd = format!("{}/project", t.0);
    std::fs::create_dir_all(&cwd).unwrap();
    let model = sonnet().unwrap();
    let session = create_agent_session(Some(&CreateAgentSessionOptions {
        cwd: Some(cwd),
        agent_dir: Some(format!("{}/agent", t.0)),
        model: Some(model.clone()),
        thinking_level: Some(pi_agent::types::ThinkingLevel::High),
        ..Default::default()
    }))
    .await
    .unwrap()
    .session;
    assert!(session.session_file().is_some());
    assert!(
        session
            .system_prompt()
            .contains("You can inspect PI_* environment variables for current model and session details.")
    );

    let output = run_bash(
        &session,
        r#"printf '%s\n' "$PI_SESSION_ID" "$PI_SESSION_FILE" "$PI_PROVIDER" "$PI_MODEL" "$PI_REASONING_LEVEL""#,
    )
    .await;
    let lines: Vec<String> = output.trim().split('\n').map(str::to_string).collect();
    assert_eq!(
        lines,
        vec![
            session.session_id(),
            session.session_file().unwrap(),
            model.provider.clone(),
            model.id.clone(),
            session.thinking_level().as_str().to_string(),
        ]
    );
    session.dispose();
}

// ---------------------------------------------------------------------------
// sdk-skills
// ---------------------------------------------------------------------------

fn skills_dir() -> TempDir {
    let t = temp("pi-sdk-test");
    let skill_dir = format!("{}/skills/test-skill", t.0);
    std::fs::create_dir_all(&skill_dir).unwrap();
    std::fs::write(
        format!("{skill_dir}/SKILL.md"),
        "---\nname: test-skill\ndescription: A test skill for SDK tests.\n---\n\n# Test Skill\n\nThis is a test skill.\n",
    )
    .unwrap();
    t
}

/// A resource loader returning only `skills` (TS object literal loaders).
struct SkillsLoader {
    skills: Vec<Skill>,
}

#[async_trait::async_trait]
impl ResourceLoader for SkillsLoader {
    fn get_extensions(&self) -> LoadExtensionsResult {
        LoadExtensionsResult {
            extensions: Vec::new(),
            errors: Vec::new(),
            warnings: None,
            runtime: create_extension_runtime(),
        }
    }
    fn get_skills(&self) -> ResourceLoaderGetSkillsResult {
        ResourceLoaderGetSkillsResult {
            skills: self.skills.clone(),
            diagnostics: Vec::new(),
        }
    }
    fn get_prompts(&self) -> ResourceLoaderGetPromptsResult {
        ResourceLoaderGetPromptsResult::default()
    }
    fn get_themes(&self) -> ResourceLoaderGetThemesResult {
        ResourceLoaderGetThemesResult::default()
    }
    fn get_agents_files(&self) -> ResourceLoaderGetAgentsFilesResult {
        ResourceLoaderGetAgentsFilesResult {
            agents_files: Vec::new(),
        }
    }
    fn get_system_prompt(&self) -> Option<String> {
        None
    }
    fn get_system_prompt_source(&self) -> Option<ResourceLoaderGetSystemPromptSourceResult> {
        None
    }
    fn get_append_system_prompt(&self) -> Vec<String> {
        Vec::new()
    }
    fn get_append_system_prompt_sources(&self) -> Vec<ResourceLoaderGetAppendSystemPromptSourcesResult> {
        Vec::new()
    }
    fn extend_resources(&self, _paths: &ResourceExtensionPaths) {}
    async fn reload(&self, _options: Option<&ResourceLoaderReloadOptions>) -> anyhow::Result<()> {
        Ok(())
    }
}

async fn skills_session(
    dir: &str,
    resource_loader: Option<Arc<dyn ResourceLoader>>,
) -> Arc<pi_coding_agent::core::agent_session::AgentSession> {
    create_agent_session(Some(&CreateAgentSessionOptions {
        cwd: Some(dir.to_string()),
        agent_dir: Some(dir.to_string()),
        session_manager: Some(SessionManager::in_memory(Some(dir), None, None).unwrap()),
        resource_loader,
        ..Default::default()
    }))
    .await
    .unwrap()
    .session
}

#[tokio::test]
async fn discovers_skills_by_default_and_exposes_them_on_the_session() {
    let t = skills_dir();
    let session = skills_session(&t.0, None).await;

    let skills = session.resource_loader().get_skills().skills;
    assert!(!skills.is_empty());
    assert!(skills.iter().any(|s| s.name == "test-skill"));
    session.dispose();
}

#[tokio::test]
async fn has_empty_skills_when_the_resource_loader_returns_none() {
    let t = skills_dir();
    let session = skills_session(&t.0, Some(Arc::new(SkillsLoader { skills: Vec::new() }))).await;

    let result = session.resource_loader().get_skills();
    assert!(result.skills.is_empty());
    assert!(result.diagnostics.is_empty());
    session.dispose();
}

#[tokio::test]
async fn uses_provided_skills_when_the_resource_loader_supplies_them() {
    let t = skills_dir();
    let custom = Skill {
        name: "custom-skill".into(),
        description: "A custom skill".into(),
        file_path: "/fake/path/SKILL.md".into(),
        base_dir: "/fake/path".into(),
        source_info: create_synthetic_source_info(
            "/fake/path/SKILL.md",
            &CreateSyntheticSourceInfoOptions {
                source: "sdk".into(),
                scope: None,
                origin: None,
                base_dir: None,
            },
        ),
        disable_model_invocation: false,
    };
    let session = skills_session(&t.0, Some(Arc::new(SkillsLoader { skills: vec![custom.clone()] }))).await;

    let result = session.resource_loader().get_skills();
    assert_eq!(
        serde_json::to_value(&result.skills).unwrap(),
        serde_json::to_value(vec![custom]).unwrap()
    );
    assert!(result.diagnostics.is_empty());
    session.dispose();
}
