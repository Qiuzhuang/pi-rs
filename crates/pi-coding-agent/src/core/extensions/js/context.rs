//! `ctx`, `ctx.ui`, `ctx.sessionManager` and `ctx.modelRegistry` of JavaScript extensions.

use std::sync::Arc;

use pi_jsrt::JsHandle;
use serde_json::{Value, json};

use super::api::{arg, opt, post_call, str_arg};
use super::{Bridge, CtxEntry, component, provider};
use crate::core::extensions::types::*;
use crate::core::session_manager::SessionManager;

fn entry(bridge: &Bridge, payload: &Value) -> anyhow::Result<CtxEntry> {
    let id = payload["ctx"].as_u64().unwrap_or_default();
    bridge
        .contexts
        .get(id)
        .ok_or_else(|| anyhow::anyhow!("This extension ctx is no longer available (it belonged to a finished call)."))
}

fn base(entry: &CtxEntry) -> anyhow::Result<&ExtensionContext> {
    entry.base().ok_or_else(|| anyhow::anyhow!("This property is not available in this context"))
}

/// `ctx.ui` of any context kind.
fn ui_of(entry: &CtxEntry) -> anyhow::Result<Arc<dyn ExtensionUIContext>> {
    match entry {
        CtxEntry::Trust(t) => Ok(t.ui.clone()),
        other => base(other)?.ui(),
    }
}

/// The session manager reachable from a context entry (or a setup callback's manager).
fn session_manager_of(bridge: &Bridge, payload: &Value) -> anyhow::Result<Arc<SessionManager>> {
    let id = payload["ctx"].as_u64().unwrap_or_default();
    if let Some(sm) = bridge.session_managers.get(id) {
        return Ok(sm);
    }
    base(&entry(bridge, payload)?)?.session_manager()
}

pub(crate) fn dispatch(bridge: &Arc<Bridge>, op: &str, payload: Value) -> anyhow::Result<Value> {
    match op {
        "ctx.get" => ctx_get(bridge, &payload),
        "ctx.call" => ctx_call(bridge, &payload),
        "sm" => session_manager_call(bridge, &payload),
        "mr" => model_registry_call(bridge, &payload),
        "ui" => ui_call(bridge, &payload),
        _ => Err(anyhow::anyhow!("unknown host call {op}")),
    }
}

fn ctx_get(bridge: &Arc<Bridge>, payload: &Value) -> anyhow::Result<Value> {
    let entry = entry(bridge, payload)?;
    let prop = str_arg(payload, "prop");
    if let CtxEntry::Trust(t) = &entry {
        return Ok(match prop.as_str() {
            "mode" => json!(t.mode.as_str()),
            "hasUI" => json!(t.has_ui),
            "cwd" => json!(t.cwd),
            _ => Value::Null,
        });
    }
    let ctx = base(&entry)?;
    Ok(match prop.as_str() {
        "assertActive" => {
            ctx.host().assert_active()?;
            Value::Null
        }
        "mode" => json!(ctx.mode()?.as_str()),
        "hasUI" => json!(ctx.has_ui()?),
        "cwd" => json!(ctx.cwd()?),
        "model" => serde_json::to_value(ctx.model()?)?,
        "scopedModels" => serde_json::to_value(ctx.scoped_models()?)?,
        "thinkingLevel" => serde_json::to_value(ctx.thinking_level()?)?,
        "signal" => match ctx.signal()? {
            Some(token) => bridge.detached_signal_for(&token),
            None => Value::Null,
        },
        "tools" => match &entry {
            CtxEntry::Tool(t) => Value::Array(
                t.tools()?
                    .iter()
                    .map(|tool| {
                        json!({
                            "name": tool.name(),
                            "label": tool.label(),
                            "description": tool.description(),
                            "parameters": tool.parameters(),
                        })
                    })
                    .collect(),
            ),
            _ => Value::Null,
        },
        _ => Value::Null,
    })
}

fn ctx_call(bridge: &Arc<Bridge>, payload: &Value) -> anyhow::Result<Value> {
    let entry = entry(bridge, payload)?;
    let method = str_arg(payload, "m");
    if method == "getSystemPromptOptions" {
        return match &entry {
            CtxEntry::Command(c) => Ok(serde_json::to_value(c.get_system_prompt_options()?)?),
            CtxEntry::Replaced(c) => Ok(serde_json::to_value(c.get_system_prompt_options()?)?),
            _ => Err(anyhow::anyhow!("getSystemPromptOptions is not available in this context")),
        };
    }
    let ctx = base(&entry)?;
    Ok(match method.as_str() {
        "isIdle" => json!(ctx.is_idle()?),
        "isProjectTrusted" => json!(ctx.is_project_trusted()?),
        "abort" => {
            ctx.abort()?;
            Value::Null
        }
        "hasPendingMessages" => json!(ctx.has_pending_messages()?),
        "shutdown" => {
            ctx.shutdown()?;
            Value::Null
        }
        "getContextUsage" => serde_json::to_value(ctx.get_context_usage()?)?,
        "getSystemPrompt" => json!(ctx.get_system_prompt()?),
        "compact" => {
            let options = arg(payload, 0);
            let on_complete = bridge.handle(&options["onComplete"]);
            let on_error = bridge.handle(&options["onError"]);
            ctx.compact(CompactOptions {
                custom_instructions: options["customInstructions"].as_str().map(str::to_string),
                on_complete: on_complete.map(|h| {
                    Arc::new(move |result: crate::core::compaction::compaction::CompactionResult| {
                        post_call(&h, None, vec![serde_json::to_value(&result).unwrap_or(Value::Null)]);
                    }) as Arc<dyn Fn(_) + Send + Sync>
                }),
                on_error: on_error.map(|h| {
                    Arc::new(move |error: Arc<anyhow::Error>| {
                        post_call(&h, None, vec![json!({ "message": error.to_string() })]);
                    }) as Arc<dyn Fn(_) + Send + Sync>
                }),
            })?;
            Value::Null
        }
        _ => return Err(anyhow::anyhow!("ctx.{method} is not available")),
    })
}

fn with_session_fn(bridge: &Arc<Bridge>, value: &Value) -> Option<WithSessionFn> {
    let handle = bridge.handle(value)?;
    let bridge = bridge.clone();
    Some(Arc::new(move |ctx: ReplacedSessionContext| {
        let bridge = bridge.clone();
        let handle = handle.clone();
        Box::pin(async move {
            let id = bridge.contexts.insert(CtxEntry::Replaced(ctx));
            bridge.glue_call("invokeWithSession", vec![handle.to_value(), json!(id)]).await?;
            Ok(())
        })
    }))
}

fn command_actions(entry: &CtxEntry) -> anyhow::Result<&ExtensionCommandContext> {
    match entry {
        CtxEntry::Command(c) => Ok(c),
        CtxEntry::Replaced(c) => Ok(&c.cmd),
        _ => Err(anyhow::anyhow!("This method is only available in command handlers")),
    }
}

pub(crate) async fn dispatch_async(bridge: Arc<Bridge>, op: &str, payload: Value) -> anyhow::Result<Value> {
    match op {
        "ctx.async" => ctx_async(&bridge, &payload).await,
        "ui.async" => ui_async(&bridge, &payload).await,
        "mr.async" => model_registry_async(&bridge, &payload).await,
        "mr.stream" => {
            let registry = base(&entry(&bridge, &payload)?)?.model_registry()?;
            let method = str_arg(&payload, "m");
            let model = serde_json::from_value(arg(&payload, 0))?;
            let context = serde_json::from_value(arg(&payload, 1))?;
            let options = provider::stream_options_from_json(&bridge, &arg(&payload, 2));
            let stream = if method == "streamSimple" {
                registry.stream_simple(model, context, Some(options.into()))
            } else {
                registry.stream(model, context, Some(options.into()))
            };
            Ok(json!(provider::register_stream(&bridge, stream)))
        }
        _ => Err(anyhow::anyhow!("unknown async host call {op}")),
    }
}

async fn ctx_async(bridge: &Arc<Bridge>, payload: &Value) -> anyhow::Result<Value> {
    let entry = entry(bridge, payload)?;
    let method = str_arg(payload, "m");
    match method.as_str() {
        "executeTool" => {
            let CtxEntry::Tool(tool_ctx) = &entry else {
                anyhow::bail!("executeTool is only available in tool contexts");
            };
            let name = arg(payload, 0).as_str().unwrap_or_default().to_string();
            let args = arg(payload, 1);
            let options = arg(payload, 2);
            let on_update = bridge.handle(&options["onUpdate"]).map(|h| {
                Arc::new(move |partial: pi_agent::types::AgentToolResult| {
                    post_call(&h, None, vec![serde_json::to_value(&partial).unwrap_or(Value::Null)]);
                }) as pi_agent::types::AgentToolUpdateCallback
            });
            let outcome = tool_ctx
                .execute_tool(&name, args, ExecuteToolOptions { signal: bridge.token(&options["signal"]), on_update })
                .await?;
            let mut out = json!({
                "toolCall": serde_json::to_value(&outcome.tool_call)?,
                "result": serde_json::to_value(&outcome.result)?,
                "isError": outcome.is_error,
            });
            if let Some(d) = outcome.duration_ms {
                out["durationMs"] = json!(d);
            }
            Ok(out)
        }
        "waitForIdle" => {
            command_actions(&entry)?.wait_for_idle().await?;
            Ok(Value::Null)
        }
        "newSession" => {
            let options = arg(payload, 0);
            let setup = bridge.handle(&options["setup"]).map(|h| {
                let bridge = bridge.clone();
                Arc::new(move |sm: Arc<SessionManager>| {
                    let bridge = bridge.clone();
                    let h = h.clone();
                    Box::pin(async move {
                        let id = super::next_id();
                        bridge.session_managers.insert_with_id(id, sm);
                        let r = bridge.glue_call("invokeSetup", vec![h.to_value(), json!(id)]).await;
                        bridge.session_managers.remove(id);
                        r.map(|_| ())
                    }) as futures::future::BoxFuture<'static, anyhow::Result<()>>
                }) as Arc<dyn Fn(Arc<SessionManager>) -> futures::future::BoxFuture<'static, anyhow::Result<()>> + Send + Sync>
            });
            let outcome = command_actions(&entry)?
                .new_session(NewSessionOptions {
                    parent_session: options["parentSession"].as_str().map(str::to_string),
                    setup,
                    with_session: with_session_fn(bridge, &options["withSession"]),
                })
                .await?;
            Ok(serde_json::to_value(outcome)?)
        }
        "fork" => {
            let entry_id = arg(payload, 0).as_str().unwrap_or_default().to_string();
            let options = arg(payload, 1);
            let outcome = command_actions(&entry)?
                .fork(
                    &entry_id,
                    ForkOptions {
                        position: opt(&options["position"])?,
                        with_session: with_session_fn(bridge, &options["withSession"]),
                    },
                )
                .await?;
            Ok(serde_json::to_value(outcome)?)
        }
        "navigateTree" => {
            let target_id = arg(payload, 0).as_str().unwrap_or_default().to_string();
            let options: NavigateTreeOptions = opt(&arg(payload, 1))?.unwrap_or_default();
            Ok(serde_json::to_value(command_actions(&entry)?.navigate_tree(&target_id, options).await?)?)
        }
        "switchSession" => {
            let path = arg(payload, 0).as_str().unwrap_or_default().to_string();
            let options = arg(payload, 1);
            let outcome = command_actions(&entry)?
                .switch_session(&path, SwitchSessionOptions { with_session: with_session_fn(bridge, &options["withSession"]) })
                .await?;
            Ok(serde_json::to_value(outcome)?)
        }
        "reload" => {
            command_actions(&entry)?.reload().await?;
            Ok(Value::Null)
        }
        "sendMessage" | "sendUserMessage" => {
            let CtxEntry::Replaced(replaced) = &entry else {
                anyhow::bail!("{method} is only available in withSession contexts");
            };
            if method == "sendMessage" {
                let message: CustomMessageInput = serde_json::from_value(arg(payload, 0))?;
                let options: SendMessageOptions = opt(&arg(payload, 1))?.unwrap_or_default();
                replaced.send_message(message, options).await?;
            } else {
                let content = serde_json::from_value(arg(payload, 0))?;
                let options: SendUserMessageOptions = opt(&arg(payload, 1))?.unwrap_or_default();
                replaced.send_user_message(content, options).await?;
            }
            Ok(Value::Null)
        }
        _ => Err(anyhow::anyhow!("ctx.{method} is not available")),
    }
}

fn session_manager_call(bridge: &Arc<Bridge>, payload: &Value) -> anyhow::Result<Value> {
    let sm = session_manager_of(bridge, payload)?;
    let method = str_arg(payload, "m");
    let s = |i: usize| arg(payload, i).as_str().map(str::to_string);
    Ok(match method.as_str() {
        "getCwd" => json!(sm.get_cwd()),
        "getSessionDir" => json!(sm.get_session_dir()),
        "getSessionId" => json!(sm.get_session_id()),
        "getSessionFile" => json!(sm.get_session_file()),
        "getLeafId" => json!(sm.get_leaf_id()),
        "getLeafEntry" => serde_json::to_value(sm.get_leaf_entry())?,
        "getEntry" => serde_json::to_value(sm.get_entry(&s(0).unwrap_or_default()))?,
        "getChildren" => serde_json::to_value(sm.get_children(&s(0).unwrap_or_default()))?,
        "getLabel" => json!(sm.get_label(&s(0).unwrap_or_default())),
        "getBranch" => serde_json::to_value(sm.get_branch(s(0).as_deref()))?,
        "buildContextEntries" => serde_json::to_value(sm.build_context_entries())?,
        "buildSessionProjection" => serde_json::to_value(sm.build_session_projection())?,
        "buildSessionContext" => serde_json::to_value(sm.build_session_context())?,
        "getHeader" => serde_json::to_value(sm.get_header())?,
        "getEntries" => serde_json::to_value(sm.get_entries())?,
        "getEntryCount" => json!(sm.get_entry_count()),
        "getTree" => serde_json::to_value(sm.get_tree())?,
        "getSessionName" => json!(sm.get_session_name()),
        "isPersisted" => json!(sm.is_persisted()),
        "appendCustomEntry" => json!(sm.append_custom_entry(&s(0).unwrap_or_default(), Some(arg(payload, 1)).filter(|v| !v.is_null()))?),
        _ => return Err(anyhow::anyhow!("sessionManager.{method} is not available to extensions in pi-rs")),
    })
}

fn model_registry_call(bridge: &Arc<Bridge>, payload: &Value) -> anyhow::Result<Value> {
    let registry = base(&entry(bridge, payload)?)?.model_registry()?;
    let method = str_arg(payload, "m");
    let s = |i: usize| arg(payload, i).as_str().unwrap_or_default().to_string();
    Ok(match method.as_str() {
        "find" => serde_json::to_value(registry.find(&s(0), &s(1)))?,
        "findOfType" => serde_json::to_value(registry.find_of_type(serde_json::from_value(arg(payload, 0))?, &s(1), &s(2)))?,
        "getModelOfType" => {
            serde_json::to_value(registry.get_model_of_type(serde_json::from_value(arg(payload, 0))?, &s(1), &s(2)))?
        }
        "getModelsOfType" => serde_json::to_value(
            registry.get_models_of_type(serde_json::from_value(arg(payload, 0))?, arg(payload, 1).as_str()),
        )?,
        "getAll" => serde_json::to_value(registry.get_all())?,
        "getAvailable" => serde_json::to_value(registry.get_available())?,
        "getError" => json!(registry.get_error()),
        "hasConfiguredAuth" => json!(registry.has_configured_auth(&serde_json::from_value(arg(payload, 0))?)),
        "isUsingOAuth" => json!(registry.is_using_o_auth(&serde_json::from_value(arg(payload, 0))?)),
        "getProviderAuthStatus" => serde_json::to_value(registry.get_provider_auth_status(&s(0)))?,
        "getProviderDisplayName" => json!(registry.get_provider_display_name(&s(0))),
        "getRegisteredProviderIds" => json!(registry.get_registered_provider_ids()),
        _ => return Err(anyhow::anyhow!("modelRegistry.{method} is not available to extensions in pi-rs")),
    })
}

async fn model_registry_async(bridge: &Arc<Bridge>, payload: &Value) -> anyhow::Result<Value> {
    let registry = base(&entry(bridge, payload)?)?.model_registry()?;
    let method = str_arg(payload, "m");
    Ok(match method.as_str() {
        "refresh" => {
            registry.refresh(None).await?;
            Value::Null
        }
        "getApiKeyAndHeaders" => {
            let model = serde_json::from_value(arg(payload, 0))?;
            serde_json::to_value(registry.get_api_key_and_headers(&model).await)?
        }
        "complete" => {
            let model = serde_json::from_value(arg(payload, 0))?;
            let context = serde_json::from_value(arg(payload, 1))?;
            let options = provider::stream_options_from_json(bridge, &arg(payload, 2));
            serde_json::to_value(registry.complete(model, context, Some(options.into())).await)?
        }
        "getApiKeyForProvider" => json!(registry.get_api_key_for_provider(arg(payload, 0).as_str().unwrap_or_default()).await),
        "getProviderAuth" => match registry.get_provider_auth(arg(payload, 0).as_str().unwrap_or_default()).await? {
            Some(auth) => json!({
                "auth": {
                    "apiKey": auth.auth.api_key,
                    "headers": serde_json::to_value(&auth.auth.headers)?,
                    "baseUrl": auth.auth.base_url,
                },
                "env": serde_json::to_value(&auth.env)?,
                "source": auth.source,
            }),
            None => Value::Null,
        },
        "getAvailableOfType" => serde_json::to_value(
            registry.get_available_of_type(serde_json::from_value(arg(payload, 0))?, arg(payload, 1).as_str(), None).await?,
        )?,
        "classify" => {
            let model = serde_json::from_value(arg(payload, 0))?;
            let context = serde_json::from_value(arg(payload, 1))?;
            let options = provider::stream_options_from_json(bridge, &arg(payload, 2));
            serde_json::to_value(registry.classify(model, context, Some(options.into())).await)?
        }
        "generateImages" => {
            let model = serde_json::from_value(arg(payload, 0))?;
            let context = serde_json::from_value(arg(payload, 1))?;
            let options = provider::stream_options_from_json(bridge, &arg(payload, 2));
            serde_json::to_value(registry.generate_images(model, context, Some(options.into())).await)?
        }
        _ => return Err(anyhow::anyhow!("modelRegistry.{method} is not available to extensions in pi-rs")),
    })
}

fn dialog_options(bridge: &Bridge, value: &Value) -> ExtensionUIDialogOptions {
    ExtensionUIDialogOptions { signal: bridge.token(&value["signal"]), timeout: value["timeout"].as_u64() }
}

async fn ui_async(bridge: &Arc<Bridge>, payload: &Value) -> anyhow::Result<Value> {
    let entry = entry(bridge, payload)?;
    let ui = ui_of(&entry)?;
    let method = str_arg(payload, "m");
    let s = |i: usize| arg(payload, i).as_str().map(str::to_string);
    Ok(match method.as_str() {
        "select" => {
            let options: Vec<String> = opt(&arg(payload, 1))?.unwrap_or_default();
            json!(ui.select(&s(0).unwrap_or_default(), options, dialog_options(bridge, &arg(payload, 2))).await)
        }
        "confirm" => json!(
            ui.confirm(&s(0).unwrap_or_default(), &s(1).unwrap_or_default(), dialog_options(bridge, &arg(payload, 2)))
                .await
        ),
        "input" => {
            json!(ui.input(&s(0).unwrap_or_default(), s(1).as_deref(), dialog_options(bridge, &arg(payload, 2))).await)
        }
        "editor" => json!(ui.editor(&s(0).unwrap_or_default(), s(1).as_deref()).await),
        "custom" => {
            let factory = bridge.handle(&arg(payload, 0)).ok_or_else(|| anyhow::anyhow!("ui.custom() expects a factory"))?;
            let options = arg(payload, 1);
            let custom_options = component::custom_ui_options(bridge, &options);
            let factory = component::js_custom_factory(bridge.clone(), factory);
            let result = ui.custom(factory, custom_options).await?;
            match result.downcast::<component::JsUiValue>() {
                Ok(v) => v.0,
                Err(_) => Value::Null,
            }
        }
        _ => return Err(anyhow::anyhow!("ui.{method} is not available")),
    })
}

fn ui_call(bridge: &Arc<Bridge>, payload: &Value) -> anyhow::Result<Value> {
    let entry = entry(bridge, payload)?;
    let ui = ui_of(&entry)?;
    let method = str_arg(payload, "m");
    let s = |i: usize| arg(payload, i).as_str().map(str::to_string);
    match method.as_str() {
        "notify" => {
            let message = s(0).unwrap_or_default();
            let kind: NotifyType = opt(&arg(payload, 1))?.unwrap_or_default();
            bridge.ui_dispatch(move || ui.notify(&message, kind));
        }
        "setStatus" => {
            let (key, text) = (s(0).unwrap_or_default(), s(1));
            bridge.ui_dispatch(move || ui.set_status(&key, text.as_deref()));
        }
        "setWorkingMessage" => {
            let message = s(0);
            bridge.ui_dispatch(move || ui.set_working_message(message.as_deref()));
        }
        "setWorkingVisible" => {
            let visible = arg(payload, 0).as_bool().unwrap_or(false);
            bridge.ui_dispatch(move || ui.set_working_visible(visible));
        }
        "setWorkingIndicator" => {
            let options: Option<WorkingIndicatorOptions> = opt(&arg(payload, 0))?;
            bridge.ui_dispatch(move || ui.set_working_indicator(options));
        }
        "setHiddenThinkingLabel" => {
            let label = s(0);
            bridge.ui_dispatch(move || ui.set_hidden_thinking_label(label.as_deref()));
        }
        "setTitle" => {
            let title = s(0).unwrap_or_default();
            bridge.ui_dispatch(move || ui.set_title(&title));
        }
        "pasteToEditor" => {
            let text = s(0).unwrap_or_default();
            bridge.ui_dispatch(move || ui.paste_to_editor(&text));
        }
        "setEditorText" => {
            let text = s(0).unwrap_or_default();
            bridge.ui_dispatch(move || ui.set_editor_text(&text));
        }
        "getEditorText" => return Ok(json!(bridge.ui_dispatch_wait(move || ui.get_editor_text())?)),
        "getToolsExpanded" => return Ok(json!(bridge.ui_dispatch_wait(move || ui.get_tools_expanded())?)),
        "setToolsExpanded" => {
            let expanded = arg(payload, 0).as_bool().unwrap_or(false);
            bridge.ui_dispatch(move || ui.set_tools_expanded(expanded));
        }
        "setWidget" => {
            let key = s(0).unwrap_or_default();
            let content = match arg(payload, 1) {
                Value::Null => None,
                Value::Array(lines) => {
                    Some(WidgetContent::Lines(lines.iter().map(|l| l.as_str().unwrap_or_default().to_string()).collect()))
                }
                other => match bridge.handle(&other) {
                    Some(factory) => Some(WidgetContent::Component(component::js_component_factory(bridge.clone(), factory))),
                    None => Some(WidgetContent::Lines(vec![other.as_str().unwrap_or_default().to_string()])),
                },
            };
            let options: ExtensionWidgetOptions = opt(&arg(payload, 2))?.unwrap_or_default();
            bridge.ui_dispatch(move || ui.set_widget(&key, content, options));
        }
        "setFooter" => {
            let factory = bridge.handle(&arg(payload, 0)).map(|f| component::js_footer_factory(bridge.clone(), f));
            bridge.ui_dispatch(move || ui.set_footer(factory));
        }
        "setHeader" => {
            let factory = bridge.handle(&arg(payload, 0)).map(|f| component::js_component_factory(bridge.clone(), f));
            bridge.ui_dispatch(move || ui.set_header(factory));
        }
        "setEditorComponent" => {
            let factory = bridge.handle(&arg(payload, 0)).map(|f| component::js_editor_factory(bridge.clone(), f));
            bridge.ui_dispatch(move || ui.set_editor_component(factory));
        }
        "getEditorComponent" => return Ok(json!(bridge.ui_dispatch_wait(move || ui.get_editor_component().is_some())?)),
        "addAutocompleteProvider" => {
            let factory = bridge
                .handle(&arg(payload, 0))
                .ok_or_else(|| anyhow::anyhow!("addAutocompleteProvider() expects a factory"))?;
            let factory = component::js_autocomplete_factory(bridge.clone(), factory);
            bridge.ui_dispatch(move || ui.add_autocomplete_provider(factory));
        }
        "onTerminalInput" => {
            let handler: JsHandle =
                bridge.handle(&arg(payload, 0)).ok_or_else(|| anyhow::anyhow!("onTerminalInput() expects a handler"))?;
            let unsubscribe = ui.on_terminal_input(Arc::new(move |data: &str| {
                let value = handler.call_blocking(vec![json!(data)]).ok()?;
                opt::<TerminalInputHandlerResult>(&value).ok().flatten()
            }));
            return Ok(json!(bridge.unsubscribes.insert(unsubscribe)));
        }
        "theme" => {
            let theme = bridge.ui_dispatch_wait(move || ui.theme())?;
            return Ok(json!(component::theme_id(bridge, &theme)));
        }
        "getAllThemes" => return Ok(serde_json::to_value(bridge.ui_dispatch_wait(move || ui.get_all_themes())?)?),
        "getTheme" => {
            let name = s(0).unwrap_or_default();
            let theme = bridge.ui_dispatch_wait(move || ui.get_theme(&name))?;
            return Ok(match theme {
                Some(t) => json!(component::theme_id(bridge, &t)),
                None => Value::Null,
            });
        }
        "setTheme" => {
            let selection = match arg(payload, 0) {
                Value::String(name) => ThemeSelection::Name(name),
                other => {
                    let id = other["$themeId"].as_u64().unwrap_or_default();
                    let theme = bridge
                        .components
                        .themes
                        .get(id)
                        .ok_or_else(|| anyhow::anyhow!("setTheme() expects a theme name or a theme from ctx.ui"))?;
                    ThemeSelection::Theme(theme)
                }
            };
            return Ok(serde_json::to_value(bridge.ui_dispatch_wait(move || ui.set_theme(selection))?)?);
        }
        _ => return Err(anyhow::anyhow!("ui.{method} is not available")),
    }
    Ok(Value::Null)
}
