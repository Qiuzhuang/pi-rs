//! Port of `test/suite/agent-session-compaction-model-overrides.test.ts` (regression #8133).

use std::sync::Arc;

use parking_lot::Mutex;
use pi_agent::types::AgentMessage;
use pi_ai::AnyModel;
use pi_ai::auth::types::{ApiKeyAuth, ApiKeyResolveInput, AuthResult, ModelAuth, ProviderAuth};
use pi_ai::models::{CreateProviderOptions, ProviderApi, ProviderStreams, create_provider};
use pi_ai::providers::faux::{FauxModelDefinition, FauxResponseStep};
use pi_coding_agent::core::compaction::CompactionResult;
use pi_coding_agent::core::extensions::types::{
    SessionBeforeCompactEvent, SessionBeforeCompactResult, on_event,
};
use pi_coding_agent::core::settings_manager::SettingsManagerGetCompactionSettingsModel;
use serde_json::json;

use crate::common::*;
use crate::retry_events::error_response;
use crate::support::*;

fn seed_history(harness: &Harness, total_tokens: u64) -> String {
    let model = harness.session.model().unwrap();
    let mut recent_user_id = String::new();
    for label in ["old", "recent"] {
        recent_user_id = harness
            .session_manager
            .append_message(user_blocks(&format!("{label:x<400}"), now_ms() - 2000))
            .unwrap();
        let mut assistant = faux_text_message(format!("{label:y<400}"));
        assistant.timestamp = now_ms() - 1000;
        assistant.api = model.api.clone();
        assistant.provider = model.provider.clone();
        assistant.model = model.id.clone();
        assistant.usage.input = total_tokens;
        assistant.usage.total_tokens = total_tokens;
        harness
            .session_manager
            .append_message(AgentMessage::Assistant(assistant))
            .unwrap();
    }
    harness
        .session
        .agent()
        .set_messages(harness.session_manager.build_session_context().messages);
    recent_user_id
}

fn recording_compactor(
    summary: &'static str,
    preparations: Arc<Mutex<Vec<SessionBeforeCompactEvent>>>,
) -> TestExtension {
    ext(move |pi| {
        let preparations = preparations.clone();
        on_event::<SessionBeforeCompactEvent, _, _>(&*pi, move |event, _ctx| {
            let preparations = preparations.clone();
            async move {
                let result = SessionBeforeCompactResult {
                    cancel: None,
                    compaction: Some(CompactionResult {
                        summary: summary.into(),
                        first_kept_entry_id: event.preparation.first_kept_entry_id.clone(),
                        tokens_before: event.preparation.tokens_before,
                        estimated_tokens_after: None,
                        usage: None,
                        details: None,
                    }),
                };
                preparations.lock().push(event);
                Ok(Some(result))
            }
        })?;
        Ok(())
    })
}

#[derive(Clone, Copy, PartialEq)]
enum Path {
    Manual,
    PrePrompt,
    PostRun,
    Overflow,
}

async fn uses_model_token_settings(path: Path) {
    let preparations = Arc::new(Mutex::new(Vec::new()));
    let harness = create_harness(HarnessOptions {
        models: vec![FauxModelDefinition {
            id: "faux-1".into(),
            context_window: Some(4000),
            ..Default::default()
        }],
        tools: Some(Vec::new()),
        settings: Some(json!({"compaction": {
            "enabled": path != Path::Manual,
            "reserveTokens": 10,
            "keepRecentTokens": 20000,
            "modelOverrides": {"faux/faux-1": {"reserveTokens": 2000, "keepRecentTokens": 150}}
        }})),
        extension_factories: vec![recording_compactor("compacted history", preparations.clone())],
        ..Default::default()
    })
    .await
    .unwrap();
    let recent_user_id = seed_history(&harness, if path == Path::PrePrompt { 2500 } else { 650 });

    if path == Path::Manual {
        harness.session.compact(None).await.unwrap();
    } else {
        harness.set_responses(match path {
            Path::Overflow => vec![error_response("prompt is too long"), text("recovered")],
            Path::PostRun => vec![text(&"z".repeat(8000))],
            _ => vec![text("done")],
        });
        harness.session.prompt("continue", None).await.unwrap();
    }

    let preparations = preparations.lock();
    assert_eq!(preparations.len(), 1);
    assert_eq!(
        serde_json::to_value(&preparations[0].preparation.settings).unwrap(),
        json!({"enabled": path != Path::Manual, "reserveTokens": 2000, "keepRecentTokens": 150})
    );
    let expected_reason = match path {
        Path::Manual => "manual",
        Path::Overflow => "overflow",
        _ => "threshold",
    };
    assert_eq!(serde_json::to_value(preparations[0].reason).unwrap(), json!(expected_reason));
    if matches!(path, Path::Manual | Path::PrePrompt) {
        assert_eq!(preparations[0].preparation.first_kept_entry_id, recent_user_id);
    }
    let ends = harness.events_of_type("compaction_end");
    assert_eq!(ends.len(), 1);
    assert_eq!(ends[0]["aborted"], json!(false));
    assert_eq!(ends[0]["willRetry"], json!(path == Path::Overflow));
    assert_eq!(ends[0]["result"]["summary"], json!("compacted history"));
    assert_eq!(harness.get_pending_response_count(), 0);
    harness.cleanup();
}

#[tokio::test]
async fn uses_model_token_settings_for_manual_compaction() {
    uses_model_token_settings(Path::Manual).await;
}

#[tokio::test]
async fn uses_model_token_settings_for_pre_prompt_compaction() {
    uses_model_token_settings(Path::PrePrompt).await;
}

#[tokio::test]
async fn uses_model_token_settings_for_post_run_compaction() {
    uses_model_token_settings(Path::PostRun).await;
}

#[tokio::test]
async fn uses_model_token_settings_for_overflow_compaction() {
    uses_model_token_settings(Path::Overflow).await;
}

#[tokio::test]
async fn uses_the_newly_selected_model_without_changing_ordinary_settings() {
    let preparations = Arc::new(Mutex::new(Vec::new()));
    let model = |id: &str, window: u64| FauxModelDefinition {
        id: id.into(),
        context_window: Some(window),
        ..Default::default()
    };
    let harness = create_harness(HarnessOptions {
        models: vec![model("small", 4000), model("big", 10000)],
        tools: Some(Vec::new()),
        settings: Some(json!({"compaction": {
            "reserveTokens": 10,
            "modelOverrides": {"faux/big": {"reserveTokens": 8000, "keepRecentTokens": 150}}
        }})),
        extension_factories: vec![recording_compactor("big model summary", preparations)],
        ..Default::default()
    })
    .await
    .unwrap();
    seed_history(&harness, 2500);
    harness.set_responses(vec![text("small response"), text("big response")]);
    harness.session.prompt("continue on small", None).await.unwrap();
    assert!(harness.events_of_type("compaction_start").is_empty());
    // Retain usage from the small model: the next check must use the active big model's policy.
    seed_history(&harness, 2500);
    harness
        .session
        .set_model(harness.get_model_by_id("big").unwrap(), None)
        .await
        .unwrap();
    harness.session.prompt("continue on big", None).await.unwrap();
    let ends = harness.events_of_type("compaction_end");
    assert_eq!(ends.len(), 1);
    assert_eq!(ends[0]["result"]["summary"], json!("big model summary"));
    assert_eq!(harness.settings_manager.get_compaction_reserve_tokens(None).unwrap(), 10);
    harness
        .session
        .set_model(harness.get_model_by_id("small").unwrap(), None)
        .await
        .unwrap();
    let current = harness.session.model().unwrap();
    assert_eq!(
        harness
            .settings_manager
            .get_compaction_reserve_tokens(Some(&SettingsManagerGetCompactionSettingsModel::new(
                current.provider,
                current.id
            )))
            .unwrap(),
        10
    );
    harness.cleanup();
}

/// Resolves the faux key after switching the agent to `second` (TS spies on `getAuth`).
struct SwitchingAuth {
    harness_agent: Arc<Mutex<Option<(pi_agent::Agent, pi_ai::types::Model)>>>,
}

#[async_trait::async_trait]
impl ApiKeyAuth for SwitchingAuth {
    fn name(&self) -> &str {
        "Switching key"
    }
    async fn resolve(&self, _input: ApiKeyResolveInput) -> anyhow::Result<Option<AuthResult>> {
        if let Some((agent, model)) = self.harness_agent.lock().clone() {
            agent.set_model(model);
        }
        Ok(Some(AuthResult {
            auth: ModelAuth {
                api_key: Some("faux-key".into()),
                headers: None,
                base_url: None,
            },
            env: None,
            source: None,
        }))
    }
}

struct FauxStreams;

impl ProviderStreams for FauxStreams {
    fn stream(
        &self,
        model: &pi_ai::types::Model,
        context: &pi_ai::types::TranscriptContext,
        options: pi_ai::types::StreamOptions,
    ) -> pi_ai::AssistantMessageEventStream {
        pi_ai::compat::stream(model, &to_context(context), options)
    }
    fn stream_simple(
        &self,
        model: &pi_ai::types::Model,
        context: &pi_ai::types::TranscriptContext,
        options: pi_ai::types::StreamOptions,
    ) -> pi_ai::AssistantMessageEventStream {
        pi_ai::compat::stream_simple(model, &to_context(context), options)
    }
}

fn to_context(context: &pi_ai::types::TranscriptContext) -> pi_ai::types::Context {
    pi_ai::types::Context {
        system_prompt: None,
        messages: context.messages.clone(),
        tools: None,
    }
}

#[tokio::test]
async fn captures_model_identity_before_awaiting_summarization_auth() {
    let harness = create_harness(HarnessOptions {
        models: vec![
            FauxModelDefinition {
                id: "first".into(),
                ..Default::default()
            },
            FauxModelDefinition {
                id: "second".into(),
                ..Default::default()
            },
        ],
        settings: Some(json!({"compaction": {"modelOverrides": {
            "faux/first": {"reserveTokens": 2000, "keepRecentTokens": 150},
            "faux/second": {"reserveTokens": 4000, "keepRecentTokens": 20000}
        }}})),
        ..Default::default()
    })
    .await
    .unwrap();
    seed_history(&harness, 650);
    let slot = Arc::new(Mutex::new(Some((
        harness.session.agent(),
        harness.get_model_by_id("second").unwrap(),
    ))));
    let provider = create_provider(CreateProviderOptions {
        id: harness.get_model().provider.clone(),
        auth: ProviderAuth {
            api_key: Some(Arc::new(SwitchingAuth {
                harness_agent: slot.clone(),
            })),
            oauth: None,
        },
        models: harness.models.iter().cloned().map(AnyModel::Chat).collect(),
        api: Some(ProviderApi::Single(Arc::new(FauxStreams))),
        ..Default::default()
    })
    .unwrap();
    harness.model_runtime.register_native_provider(provider).unwrap();
    let requests = Arc::new(Mutex::new(Vec::<String>::new()));
    let r = requests.clone();
    harness.set_responses(vec![FauxResponseStep::factory(move |_c, _o, _s, model| {
        r.lock().push(model.id.clone());
        faux_text_message("summary")
    })]);

    harness.session.compact(None).await.unwrap();

    assert_eq!(*requests.lock(), vec!["first".to_string()]);
    // The auth lookup switched the selection while the summary was prepared.
    assert_eq!(harness.session.model().unwrap().id, "second");
    slot.lock().take();
    harness.cleanup();
}
