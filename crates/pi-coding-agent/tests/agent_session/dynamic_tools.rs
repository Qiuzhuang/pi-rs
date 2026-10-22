//! Port of `test/agent-session-dynamic-tools.test.ts`.

use std::sync::Arc;

use indexmap::IndexMap;
use parking_lot::Mutex;
use pi_agent::types::{AgentToolResult, ThinkingLevel};
use pi_coding_agent::core::agent_session::{AgentSession, ExtensionBindings};
use pi_coding_agent::core::extensions::types::{
    ExtensionAPIRef, ExtensionFactory, InlineExtension, SessionStartEvent, ToolDefinition, on_event,
};
use pi_coding_agent::core::resource_loader::{
    DefaultResourceLoader, DefaultResourceLoaderOptions, ResourceLoader,
};
use pi_coding_agent::core::sdk::{CreateAgentSessionOptions, create_agent_session};
use pi_coding_agent::core::session_manager::{NewSessionOptions, SessionManager};
use pi_coding_agent::core::settings_manager::SettingsManager;
use pi_coding_agent::core::tools::bash::{BashToolOptions, create_bash_tool_definition};
use serde_json::{Value, json};

struct Dirs {
    temp_dir: String,
    agent_dir: String,
}

impl Drop for Dirs {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.temp_dir);
    }
}

fn dirs() -> Dirs {
    let temp_dir = std::env::temp_dir()
        .join(format!("pi-dynamic-tool-test-{}", uuid::Uuid::new_v4()))
        .to_string_lossy()
        .into_owned();
    let agent_dir = format!("{temp_dir}/agent");
    std::fs::create_dir_all(&agent_dir).unwrap();
    Dirs { temp_dir, agent_dir }
}

fn factory(setup: impl Fn(ExtensionAPIRef) -> anyhow::Result<()> + Send + Sync + 'static) -> InlineExtension {
    let setup = Arc::new(setup);
    let factory: ExtensionFactory = Arc::new(move |pi| {
        let setup = setup.clone();
        Box::pin(async move { setup(pi) })
    });
    InlineExtension::Factory(factory)
}

async fn create(
    dirs: &Dirs,
    extensions: Vec<InlineExtension>,
    session_manager: Option<Arc<SessionManager>>,
    thinking_level: Option<ThinkingLevel>,
    custom_tools: Option<Vec<ToolDefinition>>,
) -> Arc<AgentSession> {
    let settings_manager = SettingsManager::create(&dirs.temp_dir, Some(&dirs.agent_dir), None);
    let resource_loader = Arc::new(DefaultResourceLoader::new(&DefaultResourceLoaderOptions {
        cwd: dirs.temp_dir.clone(),
        agent_dir: dirs.agent_dir.clone(),
        settings_manager: Some(settings_manager.clone()),
        extension_factories: Some(extensions),
        ..Default::default()
    }));
    resource_loader.reload(None).await.unwrap();
    create_agent_session(Some(&CreateAgentSessionOptions {
        cwd: Some(dirs.temp_dir.clone()),
        agent_dir: Some(dirs.agent_dir.clone()),
        model: pi_ai::compat::get_model("anthropic", "claude-sonnet-4-5"),
        thinking_level,
        settings_manager: Some(settings_manager),
        session_manager: Some(
            session_manager.unwrap_or_else(|| SessionManager::in_memory(Some(&dirs.temp_dir), None, None).unwrap()),
        ),
        resource_loader: Some(resource_loader as Arc<dyn ResourceLoader>),
        custom_tools,
        ..Default::default()
    }))
    .await
    .unwrap()
    .session
}

fn simple_tool(name: &str, label: &str, description: &str) -> ToolDefinition {
    ToolDefinition::new(
        name,
        label,
        description,
        json!({"type": "object", "properties": {}}),
        |_id, _params, _signal, _update, _ctx| async { Ok(AgentToolResult::text("ok", Some(json!({})))) },
    )
}

async fn run_tool(session: &AgentSession, name: &str, id: &str) {
    let tool = session
        .agent()
        .tools()
        .into_iter()
        .find(|t| t.name() == name)
        .unwrap_or_else(|| panic!("missing tool {name}"));
    tool.execute(
        id,
        json!({"command": "printf ok"}),
        tokio_util::sync::CancellationToken::new(),
        Arc::new(|_| {}),
    )
    .await
    .unwrap();
}

#[tokio::test]
async fn exposes_session_state_before_custom_bash_spawn_hooks_and_supports_opting_out() {
    let d = dirs();
    let session_env = Arc::new(Mutex::new(None::<IndexMap<String, String>>));
    let opted_out_env = Arc::new(Mutex::new(None::<IndexMap<String, String>>));
    let (se, oe, cwd) = (session_env.clone(), opted_out_env.clone(), d.temp_dir.clone());
    let extension = factory(move |pi| {
        let se = se.clone();
        pi.register_tool(create_bash_tool_definition(
            &cwd,
            Some(&BashToolOptions {
                spawn_hook: Some(Arc::new(move |ctx| {
                    *se.lock() = Some(ctx.env.clone());
                    ctx
                })),
                ..Default::default()
            }),
        ))?;
        let oe = oe.clone();
        let mut opted_out = create_bash_tool_definition(
            &cwd,
            Some(&BashToolOptions {
                expose_session_environment: Some(false),
                spawn_hook: Some(Arc::new(move |ctx| {
                    *oe.lock() = Some(ctx.env.clone());
                    ctx
                })),
                ..Default::default()
            }),
        );
        opted_out.name = "bash_without_session_env".into();
        opted_out.label = "bash without session env".into();
        pi.register_tool(opted_out)
    });
    let session_manager = SessionManager::create(
        &d.temp_dir,
        Some(&format!("{}/sessions", d.agent_dir)),
        Some(&NewSessionOptions {
            id: Some("bash-env-test".into()),
            parent_session: None,
        }),
    )
    .unwrap();
    let session = create(&d, vec![extension], Some(session_manager), Some(ThinkingLevel::High), None).await;

    assert!(
        session
            .system_prompt()
            .contains("You can inspect PI_* environment variables for current model and session details.")
    );
    run_tool(&session, "bash", "bash-env").await;
    let env = session_env.lock().clone().expect("spawn hook ran");
    let model = session.model().unwrap();
    assert_eq!(env.get("PI_SESSION_ID"), Some(&session.session_id()));
    assert_eq!(env.get("PI_SESSION_FILE").cloned(), session.session_file());
    assert_eq!(env.get("PI_PROVIDER"), Some(&model.provider));
    assert_eq!(env.get("PI_MODEL"), Some(&model.id));
    assert_eq!(
        env.get("PI_REASONING_LEVEL").map(String::as_str),
        Some(session.thinking_level().as_str())
    );

    run_tool(&session, "bash_without_session_env", "bash-no-env").await;
    let env = opted_out_env.lock().clone().expect("spawn hook ran");
    for key in ["PI_SESSION_ID", "PI_SESSION_FILE", "PI_PROVIDER", "PI_MODEL", "PI_REASONING_LEVEL"] {
        assert!(!env.contains_key(key), "{key} should not be exposed");
    }
    session.dispose();
}

fn source_info(session: &AgentSession, name: &str) -> Value {
    let tool = session.get_all_tools().into_iter().find(|t| t.name == name).unwrap();
    serde_json::to_value(&tool.source_info).unwrap()
}

#[tokio::test]
async fn refreshes_tool_registry_when_tools_are_registered_after_initialization() {
    let d = dirs();
    let extension = factory(|pi| {
        let api = pi.clone();
        on_event::<SessionStartEvent, _, _>(&*pi, move |_event, _ctx| {
            let api = api.clone();
            async move {
                let mut tool = simple_tool("dynamic_tool", "Dynamic Tool", "Tool registered from session_start");
                tool.prompt_snippet = Some("Run dynamic test behavior".into());
                tool.prompt_guidelines =
                    Some(vec!["Use dynamic_tool when the user asks for dynamic behavior tests.".into()]);
                api.register_tool(tool)?;
                Ok(None)
            }
        })?;
        Ok(())
    });
    let session = create(&d, vec![extension], None, None, None).await;

    assert!(!session.get_all_tools().iter().any(|t| t.name == "dynamic_tool"));

    session.bind_extensions(&ExtensionBindings::default()).await.unwrap();

    let all_tools = session.get_all_tools();
    let dynamic = all_tools.iter().find(|t| t.name == "dynamic_tool").expect("dynamic_tool");
    assert_eq!(
        dynamic.prompt_guidelines,
        Some(vec!["Use dynamic_tool when the user asks for dynamic behavior tests.".to_string()])
    );
    let dynamic_source = source_info(&session, "dynamic_tool");
    assert_eq!(dynamic_source["path"], json!("<inline:1>"));
    assert_eq!(dynamic_source["source"], json!("inline"));
    assert_eq!(dynamic_source["scope"], json!("temporary"));
    assert_eq!(dynamic_source["origin"], json!("top-level"));
    let read_source = source_info(&session, "read");
    assert_eq!(read_source["path"], json!("builtin:read"));
    assert_eq!(read_source["source"], json!("builtin"));
    assert_eq!(read_source["scope"], json!("temporary"));
    assert_eq!(read_source["origin"], json!("top-level"));
    assert!(session.get_active_tool_names().contains(&"dynamic_tool".to_string()));
    let prompt = session.system_prompt();
    assert!(prompt.contains("- dynamic_tool: Run dynamic test behavior"), "{prompt}");
    assert!(prompt.contains("- Use dynamic_tool when the user asks for dynamic behavior tests."));
    session.dispose();
}

#[tokio::test]
async fn returns_source_metadata_for_sdk_custom_tools() {
    let d = dirs();
    let session = create(
        &d,
        Vec::new(),
        None,
        None,
        Some(vec![simple_tool("sdk_tool", "SDK Tool", "Tool registered through createAgentSession")]),
    )
    .await;

    let source = source_info(&session, "sdk_tool");
    assert_eq!(source["path"], json!("<sdk:sdk_tool>"));
    assert_eq!(source["source"], json!("sdk"));
    assert_eq!(source["scope"], json!("temporary"));
    assert_eq!(source["origin"], json!("top-level"));
    assert!(session.get_active_tool_names().contains(&"sdk_tool".to_string()));
    session.dispose();
}

#[tokio::test]
async fn keeps_custom_tools_active_but_omits_them_from_available_tools_when_prompt_snippet_is_not_provided() {
    let d = dirs();
    let extension = factory(|pi| {
        let api = pi.clone();
        on_event::<SessionStartEvent, _, _>(&*pi, move |_event, _ctx| {
            let api = api.clone();
            async move {
                api.register_tool(simple_tool(
                    "hidden_tool",
                    "Hidden Tool",
                    "Description should not appear in available tools",
                ))?;
                Ok(None)
            }
        })?;
        Ok(())
    });
    let session = create(&d, vec![extension], None, None, None).await;

    session.bind_extensions(&ExtensionBindings::default()).await.unwrap();

    assert!(session.get_all_tools().iter().any(|t| t.name == "hidden_tool"));
    assert!(session.get_active_tool_names().contains(&"hidden_tool".to_string()));
    let prompt = session.system_prompt();
    assert!(!prompt.contains("hidden_tool"));
    assert!(!prompt.contains("Description should not appear in available tools"));
    session.dispose();
}
