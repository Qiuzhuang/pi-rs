//! Port of `packages/coding-agent/test/default-tools-setting.test.ts`: the `defaultTools` setting,
//! `tools`/`excludeTools`/`noTools` precedence, reload (#10245) and resume (#10785, #11062, #11075).

use std::sync::Arc;

use pi_agent::types::{AgentMessage, AgentToolResult};
use pi_coding_agent::core::agent_session::{AgentSession, AgentSessionNavigateTreeOptions, ExtensionBindings};
use pi_coding_agent::core::agent_session_services::{
    CreateAgentSessionFromServicesOptions, CreateAgentSessionServicesOptions, create_agent_session_from_services,
    create_agent_session_services,
};
use pi_coding_agent::core::extensions::types::{
    ExtensionAPIRef, ExtensionFactory, ExtensionHandler, InlineExtension, ToolDefinition,
};
use pi_coding_agent::core::resource_loader::{DefaultResourceLoader, DefaultResourceLoaderOptions, ResourceLoader};
use pi_coding_agent::core::sdk::{CreateAgentSessionOptions, CreateAgentSessionOptionsNoTools, create_agent_session};
use pi_coding_agent::core::session_manager::SessionManager;
use pi_coding_agent::core::settings_manager::SettingsManager;
use serde_json::{Value, json};

struct Dirs {
    _temp: tempfile::TempDir,
    temp_dir: String,
    agent_dir: String,
}

fn dirs() -> Dirs {
    let temp = tempfile::tempdir().unwrap();
    let temp_dir = temp.path().to_string_lossy().into_owned();
    let agent_dir = format!("{temp_dir}/agent");
    std::fs::create_dir_all(&agent_dir).unwrap();
    Dirs { _temp: temp, temp_dir, agent_dir }
}

#[derive(Default)]
struct ToolOptions {
    tools: Option<Vec<&'static str>>,
    exclude_tools: Option<Vec<&'static str>>,
    no_tools: Option<CreateAgentSessionOptionsNoTools>,
    custom_tools: Option<Vec<ToolDefinition>>,
}

fn tool(name: &str, default_active: Option<bool>) -> ToolDefinition {
    let mut definition = ToolDefinition::new(
        name,
        name,
        name,
        json!({ "type": "object", "properties": {} }),
        |_id, _params, _signal, _update, _ctx| async { Ok(AgentToolResult::text("ok", Some(json!({})))) },
    );
    definition.default_active = default_active;
    definition
}

fn extension(setup: impl Fn(ExtensionAPIRef) -> anyhow::Result<()> + Send + Sync + 'static) -> InlineExtension {
    let setup = Arc::new(setup);
    let factory: ExtensionFactory = Arc::new(move |pi| {
        let setup = setup.clone();
        Box::pin(async move { setup(pi) })
    });
    InlineExtension::Factory(factory)
}

fn inactive_tool() -> InlineExtension {
    extension(|pi| {
        pi.register_tool(tool("inactive_tool", Some(false)))?;
        Ok(())
    })
}

fn inactive_and_active_tools() -> InlineExtension {
    extension(|pi| {
        pi.register_tool(tool("inactive_tool", Some(false)))?;
        pi.register_tool(tool("active_tool", None))?;
        Ok(())
    })
}

fn names(values: &[&str]) -> Vec<String> {
    values.iter().map(|v| v.to_string()).collect()
}

fn sorted(mut values: Vec<String>) -> Vec<String> {
    values.sort();
    values
}

async fn create_with(
    dirs: &Dirs,
    settings_manager: Arc<SettingsManager>,
    session_manager: Arc<SessionManager>,
    options: ToolOptions,
    extensions: Vec<InlineExtension>,
) -> anyhow::Result<Arc<AgentSession>> {
    let resource_loader = Arc::new(DefaultResourceLoader::new(&DefaultResourceLoaderOptions {
        cwd: dirs.temp_dir.clone(),
        agent_dir: dirs.agent_dir.clone(),
        settings_manager: Some(settings_manager.clone()),
        extension_factories: Some(extensions),
        ..Default::default()
    }));
    resource_loader.reload(None).await?;
    Ok(create_agent_session(Some(&CreateAgentSessionOptions {
        cwd: Some(dirs.temp_dir.clone()),
        agent_dir: Some(dirs.agent_dir.clone()),
        model: pi_ai::compat::get_model("anthropic", "claude-sonnet-4-5"),
        settings_manager: Some(settings_manager),
        session_manager: Some(session_manager),
        resource_loader: Some(resource_loader as Arc<dyn ResourceLoader>),
        tools: options.tools.map(|t| names(&t)),
        exclude_tools: options.exclude_tools.map(|t| names(&t)),
        no_tools: options.no_tools,
        custom_tools: options.custom_tools,
        ..Default::default()
    }))
    .await?
    .session)
}

fn in_memory_session(dirs: &Dirs) -> Arc<SessionManager> {
    SessionManager::in_memory(Some(&dirs.temp_dir), None, None).unwrap()
}

async fn create_session(
    dirs: &Dirs,
    default_tools: &[&str],
    options: ToolOptions,
    extensions: Vec<InlineExtension>,
) -> anyhow::Result<Arc<AgentSession>> {
    let settings = SettingsManager::in_memory_json(
        json!({ "defaultTools": default_tools }).as_object().unwrap().clone(),
        None,
    );
    create_with(dirs, settings, in_memory_session(dirs), options, extensions).await
}

fn all_tool_names(session: &AgentSession) -> Vec<String> {
    session.get_all_tools().into_iter().map(|t| t.name).collect()
}

#[tokio::test]
async fn uses_the_configured_list_as_the_initial_built_in_selection() {
    let dirs = dirs();
    let session = create_session(&dirs, &["grep", "find"], ToolOptions::default(), vec![]).await.unwrap();
    assert_eq!(
        sorted(all_tool_names(&session)),
        names(&["bash", "edit", "find", "grep", "ls", "powershell", "read", "write"])
    );
    assert_eq!(session.get_active_tool_names(), names(&["grep", "find"]));
    assert!(session.system_prompt().contains("- grep:"));
    assert!(!session.system_prompt().contains("- read:"));
    session.dispose();
}

#[tokio::test]
async fn can_select_powershell_instead_of_bash() {
    let dirs = dirs();
    let session =
        create_session(&dirs, &["read", "powershell", "edit", "write"], ToolOptions::default(), vec![]).await.unwrap();
    assert_eq!(session.get_active_tool_names(), names(&["read", "powershell", "edit", "write"]));
    assert!(session.system_prompt().contains("- powershell: Execute PowerShell commands"));
    assert!(!session.system_prompt().contains("- bash:"));
    session.dispose();
}

#[tokio::test]
async fn activates_an_inactive_extension_tool_with_plus_name() {
    let dirs = dirs();
    let session =
        create_session(&dirs, &["+inactive_tool", "-write"], ToolOptions::default(), vec![inactive_tool()]).await.unwrap();
    assert_eq!(sorted(session.get_active_tool_names()), names(&["bash", "edit", "inactive_tool", "read"]));
    session.dispose();
}

#[tokio::test]
async fn keeps_extension_and_sdk_custom_tools_enabled() {
    let dirs = dirs();
    let extension = extension(|pi| {
        pi.register_tool(tool("static_tool", None))?;
        let api = pi.clone();
        let handler: ExtensionHandler = Arc::new(move |_event, _ctx| {
            let api = api.clone();
            Box::pin(async move {
                api.register_tool(tool("dynamic_tool", None))?;
                Ok(None)
            })
        });
        pi.on("session_start", handler)?;
        Ok(())
    });
    let session = create_session(
        &dirs,
        &["grep"],
        ToolOptions { custom_tools: Some(vec![tool("sdk_tool", None)]), ..Default::default() },
        vec![extension],
    )
    .await
    .unwrap();
    session.bind_extensions(&ExtensionBindings::default()).await.unwrap();
    assert_eq!(
        sorted(session.get_active_tool_names()),
        names(&["dynamic_tool", "grep", "sdk_tool", "static_tool"])
    );
    let all = all_tool_names(&session);
    for name in ["read", "dynamic_tool", "sdk_tool", "static_tool"] {
        assert!(all.contains(&name.to_string()), "{name} missing from {all:?}");
    }
    session.dispose();
}

#[tokio::test]
async fn preserves_explicit_tool_option_precedence() {
    let dirs = dirs();
    let allowlisted = create_session(&dirs, &["grep"], ToolOptions { tools: Some(vec!["read"]), ..Default::default() }, vec![])
        .await
        .unwrap();
    assert_eq!(allowlisted.get_active_tool_names(), names(&["read"]));
    allowlisted.dispose();

    let excluded = create_session(
        &dirs,
        &["read", "grep"],
        ToolOptions { exclude_tools: Some(vec!["read"]), ..Default::default() },
        vec![],
    )
    .await
    .unwrap();
    assert_eq!(excluded.get_active_tool_names(), names(&["grep"]));
    excluded.dispose();

    let tool_less = create_session(
        &dirs,
        &["read"],
        ToolOptions { no_tools: Some(CreateAgentSessionOptionsNoTools::All), ..Default::default() },
        vec![],
    )
    .await
    .unwrap();
    assert!(tool_less.get_all_tools().is_empty());
    assert!(tool_less.get_active_tool_names().is_empty());
    tool_less.dispose();
}

#[tokio::test]
async fn applies_plus_name_and_minus_name_tool_options_to_the_default_selection() {
    let dirs = dirs();
    let session = create_session(
        &dirs,
        &["+grep"],
        ToolOptions { tools: Some(vec!["+inactive_tool", "-write"]), ..Default::default() },
        vec![inactive_and_active_tools()],
    )
    .await
    .unwrap();
    assert_eq!(
        sorted(session.get_active_tool_names()),
        names(&["active_tool", "bash", "edit", "grep", "inactive_tool", "read"])
    );
    session.dispose();

    let tool_less = create_session(
        &dirs,
        &["read"],
        ToolOptions {
            no_tools: Some(CreateAgentSessionOptionsNoTools::All),
            tools: Some(vec!["+inactive_tool"]),
            ..Default::default()
        },
        vec![inactive_and_active_tools()],
    )
    .await
    .unwrap();
    assert_eq!(tool_less.get_active_tool_names(), names(&["inactive_tool"]));
    tool_less.dispose();
}

#[tokio::test]
async fn rejects_invalid_tool_modifier_options() {
    let dirs = dirs();
    let err = create_session(&dirs, &[], ToolOptions { tools: Some(vec!["read", "+grep"]), ..Default::default() }, vec![])
        .await
        .err()
        .expect("mixed tools option rejected");
    assert!(
        err.to_string().contains("Invalid tools option: tool names cannot be mixed with +name or -name entries"),
        "{err}"
    );
    let err = create_session(&dirs, &[], ToolOptions { tools: Some(vec!["-gr*"]), ..Default::default() }, vec![])
        .await
        .err()
        .expect("pattern modifier rejected");
    assert!(
        err.to_string().contains("Invalid tools option: +name and -name entries take exact tool names, not patterns: -gr*"),
        "{err}"
    );
}

mod reload {
    use super::*;

    fn write_settings(dirs: &Dirs, settings: Value) {
        std::fs::write(format!("{}/settings.json", dirs.agent_dir), settings.to_string()).unwrap();
    }

    async fn create_file_session(dirs: &Dirs, options: ToolOptions) -> Arc<AgentSession> {
        let settings = SettingsManager::create(&dirs.temp_dir, Some(&dirs.agent_dir), None);
        create_with(dirs, settings, in_memory_session(dirs), options, vec![inactive_tool()]).await.unwrap()
    }

    // #10245
    #[tokio::test]
    async fn activates_only_tools_newly_added_to_default_tools() {
        let dirs = dirs();
        let session = create_file_session(&dirs, ToolOptions::default()).await;
        assert_eq!(session.get_active_tool_names(), names(&["read", "bash", "edit", "write"]));
        session.set_active_tools_by_name(&names(&["read", "edit", "write"]));

        write_settings(&dirs, json!({ "defaultTools": ["+inactive_tool", "+grep"] }));
        session.reload(None).await.unwrap();
        // bash was disabled during the session and is not newly added, so it stays off.
        assert_eq!(sorted(session.get_active_tool_names()), names(&["edit", "grep", "inactive_tool", "read", "write"]));

        // Removing tools from the setting does not disable them.
        write_settings(&dirs, json!({ "defaultTools": ["-read"] }));
        session.reload(None).await.unwrap();
        assert_eq!(sorted(session.get_active_tool_names()), names(&["edit", "grep", "inactive_tool", "read", "write"]));
        session.dispose();
    }

    #[tokio::test]
    async fn keeps_tools_removed_by_minus_name_tool_options_removed_on_reload() {
        let dirs = dirs();
        write_settings(&dirs, json!({ "defaultTools": ["read"] }));
        let session =
            create_file_session(&dirs, ToolOptions { tools: Some(vec!["-bash", "+grep"]), ..Default::default() }).await;
        assert_eq!(session.get_active_tool_names(), names(&["read", "grep"]));

        write_settings(&dirs, json!({ "defaultTools": ["read", "bash", "inactive_tool"] }));
        session.reload(None).await.unwrap();
        assert_eq!(sorted(session.get_active_tool_names()), names(&["grep", "inactive_tool", "read"]));
        session.dispose();
    }

    #[tokio::test]
    async fn keeps_explicit_tool_options_on_reload() {
        let dirs = dirs();
        let allowlisted = create_file_session(&dirs, ToolOptions { tools: Some(vec!["read"]), ..Default::default() }).await;
        write_settings(&dirs, json!({ "defaultTools": ["+grep"] }));
        allowlisted.reload(None).await.unwrap();
        assert_eq!(allowlisted.get_active_tool_names(), names(&["read"]));
        allowlisted.dispose();

        write_settings(&dirs, json!({}));
        let builtinless = create_file_session(
            &dirs,
            ToolOptions { no_tools: Some(CreateAgentSessionOptionsNoTools::Builtin), ..Default::default() },
        )
        .await;
        write_settings(&dirs, json!({ "defaultTools": ["+grep"] }));
        builtinless.reload(None).await.unwrap();
        assert!(builtinless.get_active_tool_names().is_empty(), "{:?}", builtinless.get_active_tool_names());
        builtinless.dispose();

        write_settings(&dirs, json!({}));
        let excluded =
            create_file_session(&dirs, ToolOptions { exclude_tools: Some(vec!["grep"]), ..Default::default() }).await;
        write_settings(&dirs, json!({ "defaultTools": ["+grep", "+inactive_tool"] }));
        excluded.reload(None).await.unwrap();
        assert_eq!(sorted(excluded.get_active_tool_names()), names(&["bash", "edit", "inactive_tool", "read", "write"]));
        excluded.dispose();
    }
}

// #10785
mod resume {
    use super::*;

    fn now() -> i64 {
        chrono::Utc::now().timestamp_millis()
    }

    fn message(value: Value) -> AgentMessage {
        serde_json::from_value(value).unwrap()
    }

    fn tool_references(names: &[&str]) -> Value {
        Value::Array(
            names
                .iter()
                .map(|name| json!({ "name": name, "description": name, "parameters": { "type": "object", "properties": {} } }))
                .collect(),
        )
    }

    /// Append a system message that adds and removes tools, as a run records loadout changes.
    fn record_loadout_change(session_manager: &SessionManager, added: &[&str], removed: &[&str]) -> String {
        let mut value = json!({ "role": "system", "content": "", "timestamp": now() });
        if !added.is_empty() {
            value["toolsAdded"] = tool_references(added);
        }
        if !removed.is_empty() {
            value["toolsRemoved"] = tool_references(removed);
        }
        session_manager.append_message(message(value)).unwrap()
    }

    fn user(session_manager: &SessionManager, text: &str) -> String {
        session_manager.append_message(message(json!({ "role": "user", "content": text, "timestamp": now() }))).unwrap()
    }

    /// A session whose transcript records the loadout `tool_names`, as a run records it.
    fn create_recorded_session(dirs: &Dirs, tool_names: &[&str]) -> Arc<SessionManager> {
        let session_manager = in_memory_session(dirs);
        record_loadout_change(&session_manager, tool_names, &[]);
        session_manager
    }

    /// A session that recorded `read`, `grep`, and `inactive_tool`, and deactivated `active_tool`.
    fn create_changed_session(dirs: &Dirs) -> Arc<SessionManager> {
        let session_manager = create_recorded_session(dirs, &["read", "grep", "active_tool"]);
        record_loadout_change(&session_manager, &["inactive_tool"], &["active_tool"]);
        session_manager
    }

    async fn resume(dirs: &Dirs, session_manager: Arc<SessionManager>, options: ToolOptions) -> Arc<AgentSession> {
        let settings = SettingsManager::in_memory(None, None);
        create_with(dirs, settings, session_manager, options, vec![inactive_and_active_tools()]).await.unwrap()
    }

    fn no_summary() -> AgentSessionNavigateTreeOptions {
        AgentSessionNavigateTreeOptions { summarize: Some(false), ..Default::default() }
    }

    #[tokio::test]
    async fn restores_the_loadout_the_transcript_records() {
        let dirs = dirs();
        let session = resume(&dirs, create_changed_session(&dirs), ToolOptions::default()).await;
        assert_eq!(sorted(session.get_active_tool_names()), names(&["grep", "inactive_tool", "read"]));
        session.dispose();
    }

    // #11062
    #[tokio::test]
    async fn activates_extension_tools_the_transcript_never_added_or_removed() {
        let dirs = dirs();
        // Built-in tools and tools registered inactive stay inactive.
        let session = resume(&dirs, create_recorded_session(&dirs, &["read", "grep"]), ToolOptions::default()).await;
        assert_eq!(sorted(session.get_active_tool_names()), names(&["active_tool", "grep", "read"]));
        session.dispose();
    }

    #[tokio::test]
    async fn keeps_tools_removed_before_a_compaction_inactive() {
        let dirs = dirs();
        let session_manager = create_changed_session(&dirs);
        let kept = user(&session_manager, "hi");
        session_manager.append_compaction("summary", Some(&kept), 1000, None, None, None).unwrap();
        let session = resume(&dirs, session_manager, ToolOptions::default()).await;
        assert_eq!(sorted(session.get_active_tool_names()), names(&["grep", "inactive_tool", "read"]));
        session.dispose();
    }

    // #11075
    #[tokio::test]
    async fn restores_the_target_branchs_loadout_on_tree_navigation() {
        let dirs = dirs();
        // The target branch never mentions active_tool, so it is active there. It removed grep.
        let session_manager = create_recorded_session(&dirs, &["read", "grep"]);
        record_loadout_change(&session_manager, &[], &["grep"]);
        let target = user(&session_manager, "first");
        record_loadout_change(&session_manager, &["grep"], &["active_tool"]);
        let session = resume(&dirs, session_manager, ToolOptions::default()).await;
        assert_eq!(sorted(session.get_active_tool_names()), names(&["grep", "read"]));

        session.navigate_tree(&target, Some(&no_summary())).await.unwrap();
        assert_eq!(sorted(session.get_active_tool_names()), names(&["active_tool", "read"]));
        session.dispose();
    }

    #[tokio::test]
    async fn keeps_registered_tools_outside_the_tools_allowlist_inactive_on_tree_navigation() {
        let dirs = dirs();
        // An allowlist without mcp__ entries keeps MCP tools registered but inactive.
        let session_manager = create_recorded_session(&dirs, &["read"]);
        let target = user(&session_manager, "first");
        let session = resume(
            &dirs,
            session_manager,
            ToolOptions {
                tools: Some(vec!["read"]),
                custom_tools: Some(vec![tool("mcp__server__lookup", None)]),
                ..Default::default()
            },
        )
        .await;
        assert_eq!(session.get_active_tool_names(), names(&["read"]));

        session.navigate_tree(&target, Some(&no_summary())).await.unwrap();
        assert_eq!(session.get_active_tool_names(), names(&["read"]));
        session.dispose();
    }

    #[tokio::test]
    async fn does_not_restore_excluded_tools() {
        let dirs = dirs();
        let session = resume(
            &dirs,
            create_changed_session(&dirs),
            ToolOptions { exclude_tools: Some(vec!["inactive_tool"]), ..Default::default() },
        )
        .await;
        assert_eq!(sorted(session.get_active_tool_names()), names(&["grep", "read"]));
        session.dispose();
    }

    #[tokio::test]
    async fn uses_explicit_tool_options_instead_of_the_recorded_loadout() {
        let dirs = dirs();
        let allowlisted = resume(
            &dirs,
            create_changed_session(&dirs),
            ToolOptions { tools: Some(vec!["read", "write"]), ..Default::default() },
        )
        .await;
        assert_eq!(allowlisted.get_active_tool_names(), names(&["read", "write"]));
        allowlisted.dispose();

        let builtinless = resume(
            &dirs,
            create_changed_session(&dirs),
            ToolOptions { no_tools: Some(CreateAgentSessionOptionsNoTools::Builtin), ..Default::default() },
        )
        .await;
        assert_eq!(builtinless.get_active_tool_names(), names(&["active_tool"]));
        builtinless.dispose();
    }

    #[tokio::test]
    async fn applies_plus_name_and_minus_name_tool_options_to_the_recorded_loadout() {
        let dirs = dirs();
        let session = resume(
            &dirs,
            create_changed_session(&dirs),
            ToolOptions { tools: Some(vec!["-grep", "+write"]), ..Default::default() },
        )
        .await;
        assert_eq!(sorted(session.get_active_tool_names()), names(&["inactive_tool", "read", "write"]));
        session.dispose();
    }

    #[tokio::test]
    async fn uses_the_default_tools_when_the_transcript_records_no_loadout() {
        let dirs = dirs();
        let session_manager = in_memory_session(&dirs);
        user(&session_manager, "hi");
        let session = resume(&dirs, session_manager, ToolOptions::default()).await;
        assert_eq!(sorted(session.get_active_tool_names()), names(&["active_tool", "bash", "edit", "read", "write"]));
        session.dispose();
    }
}

#[tokio::test]
async fn applies_through_service_based_session_creation() {
    let dirs = dirs();
    let settings_manager =
        SettingsManager::in_memory_json(json!({ "defaultTools": ["ls"] }).as_object().unwrap().clone(), None);
    let services = create_agent_session_services(&CreateAgentSessionServicesOptions {
        cwd: dirs.temp_dir.clone(),
        agent_dir: Some(dirs.agent_dir.clone()),
        settings_manager: Some(settings_manager),
        model_runtime: None,
        model_runtime_signal: None,
        extension_flag_values: None,
        resource_loader_options: None,
        resource_loader_reload_options: None,
    })
    .await
    .unwrap();
    let session = create_agent_session_from_services(&CreateAgentSessionFromServicesOptions {
        services,
        session_manager: in_memory_session(&dirs),
        session_start_event: None,
        model: pi_ai::compat::get_model("anthropic", "claude-sonnet-4-5"),
        thinking_level: None,
        scoped_models: None,
        tools: None,
        exclude_tools: None,
        no_tools: None,
        custom_tools: None,
    })
    .await
    .unwrap()
    .session;
    assert_eq!(
        sorted(all_tool_names(&session)),
        names(&["bash", "edit", "find", "grep", "ls", "powershell", "read", "write"])
    );
    assert_eq!(session.get_active_tool_names(), names(&["ls"]));
    session.dispose();
}
