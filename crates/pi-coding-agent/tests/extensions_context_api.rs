//! `ctx.modelRegistry` and `ctx.sessionManager` of JavaScript extensions behave like the TS
//! objects extensions receive: every public `ModelRegistry` / `SessionManager` method, the same
//! return shapes (undefined vs null), and the same object identities (`getProvider()`,
//! `getRegisteredProviderConfig()` and `getRegisteredNativeProvider()` return the same object
//! until a registration changes it, the extension's own config functions and provider objects
//! come back unchanged). The expected values were recorded with the TS pi.

mod extensions_support;
mod support;

use std::path::PathBuf;
use std::sync::Arc;

use pi_coding_agent::core::resource_loader::{DefaultResourceLoader, DefaultResourceLoaderOptions, ResourceLoader};
use serde_json::{Value, json};
use support::*;

/// Load `source` (which writes its findings to `globalThis.RESULT_PATH`) into a fresh session and
/// return what it wrote once `session_start` ran.
async fn run_probe(source: &str) -> (Value, Harness, PathBuf) {
    let dir = std::env::temp_dir().join(format!("pi-ext-ctx-api-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    let result = dir.join("result.json");
    let path = dir.join("extension.ts");
    let source = format!("const RESULT_PATH = {};\n{source}", serde_json::to_string(&result.display().to_string()).unwrap());
    std::fs::write(&path, source).unwrap();
    let loader = Arc::new(DefaultResourceLoader::new(&DefaultResourceLoaderOptions {
        cwd: dir.display().to_string(),
        agent_dir: dir.display().to_string(),
        additional_extension_paths: Some(vec![path.display().to_string()]),
        no_skills: Some(true),
        no_prompt_templates: Some(true),
        no_context_files: Some(true),
        ..Default::default()
    }));
    loader.reload(None).await.unwrap();
    assert!(loader.get_extensions().errors.is_empty(), "{:#?}", loader.get_extensions().errors);
    let harness = create_harness(HarnessOptions { resource_loader: Some(loader), ..Default::default() }).await.unwrap();
    harness.session.bind_extensions(&Default::default()).await.unwrap();
    let done = dir.join("result.json.done");
    for _ in 0..600 {
        if done.exists() {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }
    let text = std::fs::read_to_string(&result).unwrap_or_else(|_| "null".into());
    (serde_json::from_str(&text).unwrap(), harness, dir)
}

const PROBE_PRELUDE: &str = r#"
import { writeFileSync } from "node:fs";
const out = {};
const t = async (k, f) => {
    try { out[k] = await f(); } catch (e) { out[k] = "THROW " + e.message; }
    writeFileSync(RESULT_PATH, JSON.stringify(out));
};
const finish = () => {
    writeFileSync(RESULT_PATH, JSON.stringify(out));
    writeFileSync(RESULT_PATH + ".done", "");
};
"#;

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn model_registry_matches_ts() {
    let source = format!(
        "{PROBE_PRELUDE}{}",
        r#"
export default function (pi) {
    pi.on("session_start", async (_event, ctx) => {
        const r = ctx.modelRegistry;
        await t("sameRegistry", () => r === ctx.modelRegistry);
        await t("methods", () => Object.getOwnPropertyNames(Object.getPrototypeOf(r)).sort());
        await t("unknownProp", () => typeof r.notAThing);
        const p1 = r.getProvider("anthropic");
        await t("provider.same", () => p1 === r.getProvider("anthropic"));
        await t("provider.keys", () => Object.keys(p1));
        await t("provider.missing", () => r.getProvider("nope") === undefined);
        await t("cfg.none", () => r.getRegisteredProviderConfig("anthropic") === undefined);
        await t("native.none", () => r.getRegisteredNativeProvider("anthropic") === undefined);
        const streamSimple = () => { throw new Error("x"); };
        r.registerProvider("anthropic", { api: "anthropic-messages", streamSimple, headers: { a: "b" } });
        const c1 = r.getRegisteredProviderConfig("anthropic");
        await t("cfg1", () => ({ keys: Object.keys(c1).sort(), same: c1 === r.getRegisteredProviderConfig("anthropic"), fn: c1.streamSimple === streamSimple, headers: c1.headers }));
        await t("provider.changed", () => r.getProvider("anthropic") !== p1);
        const p2 = r.getProvider("anthropic");
        await t("provider.stable", () => p2 === r.getProvider("anthropic"));
        r.registerProvider("anthropic", { name: "Renamed" });
        const c2 = r.getRegisteredProviderConfig("anthropic");
        await t("cfg2", () => ({ keys: Object.keys(c2).sort(), newObject: c2 !== c1, fn: c2.streamSimple === streamSimple, name: c2.name }));
        await t("ids.registered", () => r.getRegisteredProviderIds().includes("anthropic"));
        await t("displayName", () => r.getProviderDisplayName("anthropic"));
        r.unregisterProvider("anthropic");
        await t("cfg.after", () => r.getRegisteredProviderConfig("anthropic") === undefined);
        await t("ids.unregistered", () => r.getRegisteredProviderIds().includes("anthropic"));
        const native = { id: "probe-native", name: "Probe Native", auth: {}, getModels: () => [], stream: () => { throw new Error("no"); }, streamSimple: () => { throw new Error("no"); } };
        r.registerProvider(native);
        await t("native.same", () => r.getRegisteredNativeProvider("probe-native") === native);
        await t("native.effective", () => r.getProvider("probe-native") === native);
        r.registerProvider(p1);
        await t("builtin.reregistered", () => ({ native: r.getRegisteredNativeProvider("anthropic") === p1, effective: r.getProvider("anthropic") === p1 }));
        r.unregisterProvider("anthropic");
        r.unregisterProvider("probe-native");
        await t("registerWithoutConfig", () => r.registerProvider("x"));
        const model = r.getAll()[0];
        r.registerVirtualModel({ provider: model.provider, id: "virt", name: "Virtual", route: () => ({ model }) });
        await t("virtual", () => { const m = r.find(model.provider, "virt"); return m && { id: m.id, name: m.name }; });
        r.unregisterVirtualModel(model.provider, "virt");
        await t("virtual.after", () => r.find(model.provider, "virt") === undefined);
        await t("refresh", async () => { const res = await r.refresh({ allowNetwork: false }); return { aborted: res.aborted, isMap: res.errors instanceof Map, size: res.errors.size }; });
        await t("find.none", () => r.find("nope", "zzz") === undefined);
        await t("auth.none", async () => (await r.getProviderAuth("nope")) === undefined);
        finish();
    });
}
"#
    );
    let (result, harness, dir) = run_probe(&source).await;
    let expected = json!({
        "sameRegistry": true,
        "methods": [
            "classify", "complete", "constructor", "find", "findOfType", "generateImages", "getAll",
            "getApiKeyAndHeaders", "getApiKeyForProvider", "getAvailable", "getAvailableOfType", "getError",
            "getModelOfType", "getModelsOfType", "getProvider", "getProviderAuth", "getProviderAuthStatus",
            "getProviderDisplayName", "getRegisteredNativeProvider", "getRegisteredProviderConfig",
            "getRegisteredProviderIds", "hasConfiguredAuth", "isUsingOAuth", "refresh", "registerProvider",
            "registerVirtualModel", "stream", "streamSimple", "unregisterProvider", "unregisterVirtualModel"
        ],
        "unknownProp": "undefined",
        "provider.same": true,
        "provider.keys": [
            "id", "name", "baseUrl", "headers", "auth", "getModels", "getAllModels", "refreshModels",
            "filterModels", "filterAllModels", "stream", "streamSimple"
        ],
        "provider.missing": true,
        "cfg.none": true,
        "native.none": true,
        "cfg1": { "keys": ["api", "headers", "streamSimple"], "same": true, "fn": true, "headers": { "a": "b" } },
        "provider.changed": true,
        "provider.stable": true,
        "cfg2": { "keys": ["api", "headers", "name", "streamSimple"], "newObject": true, "fn": true, "name": "Renamed" },
        "ids.registered": true,
        "displayName": "Renamed",
        "cfg.after": true,
        "ids.unregistered": false,
        "native.same": true,
        "native.effective": true,
        "builtin.reregistered": { "native": true, "effective": true },
        "registerWithoutConfig": "THROW Provider config is required when registering by name",
        "virtual": { "id": "virt", "name": "Virtual" },
        "virtual.after": true,
        "refresh": { "aborted": false, "isMap": true, "size": 0 },
        "find.none": true,
        "auth.none": true,
    });
    assert_eq!(result, expected);
    harness.cleanup();
    let _ = std::fs::remove_dir_all(dir);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn session_manager_matches_ts() {
    let source = format!(
        "{PROBE_PRELUDE}{}",
        r#"
import { mkdtempSync } from "node:fs";
import { join } from "node:path";
import { tmpdir } from "node:os";
import { SessionManager } from "@earendil-works/pi-coding-agent";
const usage = { input: 1, output: 2, cacheRead: 0, cacheWrite: 0, totalTokens: 3, cost: { input: 0, output: 0, cacheRead: 0, cacheWrite: 0, total: 0 } };
const assistant = (text, timestamp) => ({ role: "assistant", content: [{ type: "text", text }], api: "x", provider: "p", model: "m", usage, stopReason: "stop", timestamp });
export default function (pi) {
    pi.on("session_start", async (_event, ctx) => {
        const sm = ctx.sessionManager;
        await t("same", () => sm === ctx.sessionManager);
        await t("instanceof", () => sm instanceof SessionManager);
        await t("header.type", () => sm.getHeader().type);
        await t("label.missing", () => sm.getLabel("zz") === undefined);
        await t("unknownProp", () => typeof sm.notAThing);
        const m = SessionManager.inMemory("/tmp");
        await t("mem.leaf", () => m.getLeafId());
        const id1 = m.appendMessage({ role: "user", content: "hello", timestamp: 1 });
        const id2 = m.appendMessage(assistant("hi", 2));
        m.appendThinkingLevelChange("high");
        m.appendModelChange("p", "m");
        m.appendCustomEntry("x", { a: 1 });
        m.appendSessionInfo("My Session");
        await t("mem.name", () => m.getSessionName());
        m.appendCustomMessageEntry("ct", "text", true, { d: 1 });
        await t("mem.label", () => { m.appendLabelChange(id1, "L1"); return m.getLabel(id1); });
        await t("mem.editBad", () => m.appendContextEdit(id1, { nope: 1 }));
        m.appendContextEdit(id1, { content: "edited" });
        m.appendCompaction("sum", id2, 123, { x: 1 }, true);
        await t("mem.usage", () => { const e = m.appendUsage("compaction", "p", "m", usage, "note"); return [e.type, e.kind, e.note, e.usage.totalTokens]; });
        await t("mem.types", () => m.getEntries().map((e) => e.type));
        await t("mem.context", () => m.buildSessionContext().messages.map((msg) => msg.role));
        await t("mem.branch", () => { m.branch(id1); return m.getLeafId() === id1; });
        await t("mem.summary", () => { m.branchWithSummary(id1, "summary", { d: 2 }, false); return m.getLeafEntry().type; });
        await t("mem.reset", () => { m.resetLeaf(); return m.getLeafId(); });
        await t("mem.branchMissing", () => m.branch("nope"));
        const dir = mkdtempSync(join(tmpdir(), "sm-probe-"));
        const c = SessionManager.create("/tmp/sm-probe-cwd", dir);
        c.appendMessage({ role: "user", content: "first message", timestamp: 1 });
        c.appendMessage(assistant("ok", 2));
        await t("create.persisted", () => [typeof c.getSessionFile(), c.isPersisted()]);
        const list = await SessionManager.list("/tmp/sm-probe-cwd", dir);
        await t("list", () => list.map((s) => [s.firstMessage, s.messageCount, s.created instanceof Date, s.modified instanceof Date]));
        const opened = SessionManager.open(c.getSessionFile(), dir);
        await t("open.count", () => opened.getEntryCount());
        await t("findById", () => SessionManager.findById("/tmp/sm-probe-cwd", c.getSessionId(), dir) === c.getSessionFile());
        await t("listAll", async () => (await SessionManager.listAll(dir)).length);
        await t("constructor", () => { const x = new SessionManager("/tmp", "", undefined, false); return [x.getCwd(), x.isPersisted(), x.getLeafId()]; });
        finish();
    });
}
"#
    );
    let (result, harness, dir) = run_probe(&source).await;
    let expected = json!({
        "same": true,
        "instanceof": true,
        "header.type": "session",
        "label.missing": true,
        "unknownProp": "undefined",
        "mem.leaf": null,
        "mem.name": "My Session",
        "mem.label": "L1",
        "mem.editBad": "THROW Context edit replacement must be null or contain string/array content",
        "mem.usage": ["usage", "compaction", "note", 3],
        "mem.types": [
            "message", "message", "thinking_level_change", "model_change", "custom", "session_info",
            "custom_message", "label", "context_edit", "compaction", "usage"
        ],
        "mem.context": ["compactionSummary", "assistant", "custom"],
        "mem.branch": true,
        "mem.summary": "branch_summary",
        "mem.reset": null,
        "mem.branchMissing": "THROW Entry nope not found",
        "create.persisted": ["string", true],
        "list": [["first message", 2, true, true]],
        "open.count": 2,
        "findById": true,
        "listAll": 1,
        "constructor": ["/tmp", false, null],
    });
    assert_eq!(result, expected);
    harness.cleanup();
    let _ = std::fs::remove_dir_all(dir);
}
