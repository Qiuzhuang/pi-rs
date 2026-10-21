//! Load pi's example extensions (from the TypeScript reference checkout) with stub
//! virtual modules. The reference checkout is located via `PI_TS_SOURCE` (path to the
//! cloned `pi` repository); tests are skipped when it is not available.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use parking_lot::Mutex;
use pi_jsrt::{JsHandle, JsRuntime, RuntimeOptions, VirtualModule};
use serde_json::{Value, json};

const DEFAULT_SOURCE: &str = "/tmp/claude-0/-home-user-pi-rs/3420df64-2251-57a4-b9aa-4eb6ba35ed96/scratchpad/src/pi";

fn examples_dir() -> Option<PathBuf> {
    let root = std::env::var("PI_TS_SOURCE").unwrap_or_else(|_| DEFAULT_SOURCE.to_string());
    let dir = Path::new(&root).join("packages/coding-agent/examples/extensions");
    dir.is_dir().then_some(dir)
}

/// Realistic stubs for the host packages used by hello.ts and todo.ts.
fn host_stubs() -> HashMap<String, VirtualModule> {
    let typebox = r#"
const kind = (type, extra = {}) => ({ type, ...extra });
export const Type = {
  Object: (properties, opts = {}) => ({ type: "object", properties, required: Object.keys(properties).filter((k) => !properties[k].optional), ...opts }),
  String: (opts = {}) => kind("string", opts),
  Number: (opts = {}) => kind("number", opts),
  Boolean: (opts = {}) => kind("boolean", opts),
  Optional: (schema) => ({ ...schema, optional: true }),
  Array: (items, opts = {}) => kind("array", { items, ...opts }),
  Union: (anyOf, opts = {}) => ({ anyOf, ...opts }),
  Literal: (value) => ({ const: value }),
};
export default Type;
"#;
    let mut m = HashMap::new();
    m.insert("typebox".to_string(), VirtualModule::Esm(typebox.into()));
    m.insert(
        "@earendil-works/pi-ai".to_string(),
        VirtualModule::Esm(
            r#"
export { Type } from "typebox";
export function StringEnum<T extends readonly string[]>(values: T, opts: object = {}) { return { type: "string", enum: [...values], ...opts }; }
"#
            .into(),
        ),
    );
    m.insert(
        "@earendil-works/pi-coding-agent".to_string(),
        VirtualModule::Esm("export function defineTool<T>(tool: T): T { return tool; }\n".into()),
    );
    m.insert(
        "@earendil-works/pi-tui".to_string(),
        VirtualModule::Esm(
            r#"
export class Text {
  constructor(public text: string, public paddingX = 0, public paddingY = 0) {}
  render(width: number): string[] { return this.text.split("\n").map((l) => l.slice(0, width)); }
  invalidate() {}
}
export function truncateToWidth(s: string, width: number): string { return s.length > width ? s.slice(0, width) : s; }
export function matchesKey(data: string, key: string): boolean { return key === "escape" ? data === "\x1b" : data === key; }
"#
            .into(),
        ),
    );
    m
}

/// JS glue playing the role of pi's extension loader: builds the `pi` API object on
/// top of host functions.
const LOADER_GLUE: &str = r#"
import { ref } from "jsrt:host";
export async function load(path: string) {
  const mod = await import(path);
  const factory = mod.default;
  const pi = {
    registerTool: (tool) => host.register("tool", tool.name, tool),
    registerCommand: (name, options) => host.register("command", name, options),
    on: (event, handler) => host.register("event", event, handler),
  };
  await factory(pi);
}
// Calls a component factory and returns the component as a handle (like ctx.ui.custom).
export function makeComponent(factory, theme) {
  let closed = false;
  const component = factory({}, theme, {}, () => { closed = true; });
  return ref(component);
}
"#;

#[derive(Default)]
struct Registry {
    items: BTreeMap<(String, String), Value>,
}

#[tokio::test]
async fn hello_and_todo_extensions() {
    let Some(dir) = examples_dir() else {
        eprintln!("skipping: reference TypeScript sources not found");
        return;
    };
    let rt = JsRuntime::new(RuntimeOptions {
        cwd: Some(dir.clone()),
        virtual_modules: host_stubs(),
        ..Default::default()
    })
    .unwrap();
    let registry = Arc::new(Mutex::new(Registry::default()));
    let handles: Arc<Mutex<Vec<JsHandle>>> = Arc::default();
    let (r2, h2) = (registry.clone(), handles.clone());
    rt.register_function("host.register", move |rt, args| {
        let kind = args[0].as_str().unwrap_or_default().to_string();
        let name = args[1].as_str().unwrap_or_default().to_string();
        let value = args[2].clone();
        // Adopt every function marker so Rust can call it later.
        fn adopt(rt: &JsRuntime, v: &Value, out: &mut Vec<JsHandle>) {
            if let Some(h) = rt.handle(v) {
                out.push(h);
                return;
            }
            match v {
                Value::Object(m) => m.values().for_each(|x| adopt(rt, x, out)),
                Value::Array(a) => a.iter().for_each(|x| adopt(rt, x, out)),
                _ => {}
            }
        }
        adopt(rt, &value, &mut h2.lock());
        r2.lock().items.insert((kind, name), value);
        Ok(Value::Null)
    })
    .await
    .unwrap();
    let glue = rt.eval_module(LOADER_GLUE, "pi-loader.ts").await.unwrap();

    // hello.ts
    glue.call_method("load", vec![json!(dir.join("hello.ts").display().to_string())])
        .await
        .unwrap_or_else(|e| panic!("hello.ts: {e}\n{:?}", e.as_js().and_then(|j| j.stack.clone())));
    let hello = registry.lock().items.get(&("tool".into(), "hello".into())).cloned().expect("hello tool registered");
    assert_eq!(hello["label"], json!("Hello"));
    assert_eq!(hello["parameters"]["properties"]["name"]["type"], json!("string"));
    let execute = rt.handle(&hello["execute"]).unwrap();
    let result = execute.call(vec![json!("call-1"), json!({ "name": "Rust" }), Value::Null, Value::Null, Value::Null]).await.unwrap();
    assert_eq!(result, json!({ "content": [{ "type": "text", "text": "Hello, Rust!" }], "details": { "greeted": "Rust" } }));

    // todo.ts
    glue.call_method("load", vec![json!(dir.join("todo.ts").display().to_string())])
        .await
        .unwrap_or_else(|e| panic!("todo.ts: {e}\n{:?}", e.as_js().and_then(|j| j.stack.clone())));
    let (todo, command, events) = {
        let reg = registry.lock();
        let todo = reg.items.get(&("tool".into(), "todo".into())).cloned().expect("todo tool");
        let command = reg.items.get(&("command".into(), "todos".into())).cloned().expect("todos command");
        let events: Vec<String> = reg.items.keys().filter(|(k, _)| k == "event").map(|(_, n)| n.clone()).collect();
        (todo, command, events)
    };
    assert_eq!(events, vec!["session_start".to_string(), "session_tree".to_string()]);
    assert_eq!(todo["parameters"]["properties"]["action"]["enum"], json!(["list", "add", "toggle", "clear"]));
    assert_eq!(command["description"], json!("Show all todos on the current branch"));

    let execute = rt.handle(&todo["execute"]).unwrap();
    let add = execute.call(vec![json!("1"), json!({ "action": "add", "text": "write tests" })]).await.unwrap();
    assert_eq!(add["content"][0]["text"], json!("Added todo #1: write tests"));
    let toggle = execute.call(vec![json!("2"), json!({ "action": "toggle", "id": 1 })]).await.unwrap();
    assert_eq!(toggle["details"]["todos"], json!([{ "id": 1, "text": "write tests", "done": true }]));

    // renderResult returns a `Text` component instance: keep it as a handle and render it.
    let theme = rt
        .eval_handle("({ fg: (c, s) => `<${c}>${s}`, bg: (c, s) => s, bold: (s) => `*${s}*` })")
        .await
        .unwrap();
    let render_result = rt.handle(&todo["renderResult"]).unwrap();
    let text = render_result
        .call_handle(vec![toggle.clone(), json!({ "expanded": false }), theme.to_value(), Value::Null])
        .await
        .unwrap();
    let lines = text.call_method("render", vec![json!(80)]).await.unwrap();
    assert_eq!(lines, json!(["<success>✓ <muted>Todo #1 completed"]));

    // The /todos command's component is created through ctx.ui.custom; emulate it and
    // render synchronously from a non-async thread like the TUI does.
    let handler = rt.handle(&command["handler"]).unwrap();
    let ctx = rt
        .eval_handle(
            r#"(() => {
                const captured = {};
                return {
                    mode: "tui",
                    captured,
                    ui: {
                        notify() {},
                        custom: (factory) => { captured.factory = factory; return Promise.resolve(); },
                    },
                };
            })()"#,
        )
        .await
        .unwrap();
    handler.call(vec![json!(""), ctx.to_value()]).await.unwrap();
    let factory = ctx.get_handle("captured").await.unwrap().get_handle("factory").await.unwrap();
    let marker = glue.call_method("makeComponent", vec![factory.to_value(), theme.to_value()]).await.unwrap();
    let component = rt.handle(&marker).expect("component handle");
    let c2 = component.clone();
    let rendered = std::thread::spawn(move || c2.call_method_blocking("render", vec![json!(40)])).join().unwrap().unwrap();
    let rendered: Vec<String> = serde_json::from_value(rendered).unwrap();
    assert!(rendered.iter().any(|l| l.contains("1/1 completed")), "{rendered:?}");
    assert!(rendered.iter().any(|l| l.contains("write tests")), "{rendered:?}");
    drop(handles);
}

fn collect_imports(dir: &Path) -> BTreeMap<String, BTreeSet<String>> {
    let named = regex::Regex::new(r#"(?s)(?:import|export)\s+(?:type\s+)?(?:([A-Za-z_$][\w$]*)\s*,\s*)?\{([^}]*)\}\s*from\s*["']([^"']+)["']"#).unwrap();
    let mut out: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        for entry in std::fs::read_dir(&d).unwrap().flatten() {
            let p = entry.path();
            if p.is_dir() {
                if p.file_name().is_some_and(|n| n != "node_modules") {
                    stack.push(p);
                }
                continue;
            }
            if p.extension().is_none_or(|e| e != "ts") {
                continue;
            }
            let text = std::fs::read_to_string(&p).unwrap();
            for c in named.captures_iter(&text) {
                let spec = c[3].to_string();
                let set = out.entry(spec).or_default();
                for part in c[2].split(',') {
                    let part = part.trim().trim_start_matches("type ").trim();
                    let name = part.split(" as ").next().unwrap_or("").trim();
                    if !name.is_empty() && name.chars().all(|ch| ch.is_alphanumeric() || ch == '_' || ch == '$') {
                        set.insert(name.to_string());
                    }
                }
            }
        }
    }
    out
}

/// Every example extension parses, transpiles and evaluates with generic stubs, and
/// its factory runs against a permissive `pi` API stub.
#[tokio::test]
async fn all_examples_load() {
    let Some(dir) = examples_dir() else {
        eprintln!("skipping: reference TypeScript sources not found");
        return;
    };
    let imports = collect_imports(&dir);
    let stub_prelude = r#"
const makeStub = (name) => {
  const f = function (...args) { return /(Dir|Path)$/.test(name) ? "/tmp/pi-stub" : makeStub(`${name}()`); };
  return new Proxy(f, {
    get(target, prop) {
      if (prop === Symbol.toPrimitive) return () => name;
      if (prop === "then") return undefined;
      if (prop === Symbol.iterator) return function* () {};
      if (prop === "prototype") return target.prototype;
      if (typeof prop === "symbol") return undefined;
      return makeStub(`${name}.${prop}`);
    },
    construct() { return makeStub(`new ${name}`); },
  });
};
"#;
    let mut virtual_modules = HashMap::new();
    for (spec, names) in &imports {
        let host = spec.starts_with("@earendil-works/") || spec.starts_with("@mariozechner/") || spec == "typebox" || spec.starts_with("typebox/");
        if !host {
            continue;
        }
        let mut src = stub_prelude.to_string();
        for n in names {
            if n == "default" {
                continue;
            }
            src.push_str(&format!("export const {n} = makeStub({n:?});\n"));
        }
        src.push_str("export default makeStub(\"default\");\n");
        virtual_modules.insert(spec.clone(), VirtualModule::Esm(src));
    }
    let mut entries: Vec<PathBuf> = Vec::new();
    for entry in std::fs::read_dir(&dir).unwrap().flatten() {
        let p = entry.path();
        if p.is_file() && p.extension().is_some_and(|e| e == "ts") {
            entries.push(p);
        } else if p.is_dir() && p.join("index.ts").is_file() {
            entries.push(p.join("index.ts"));
        }
    }
    entries.sort();
    let rt = JsRuntime::new(RuntimeOptions {
        cwd: Some(dir.clone()),
        virtual_modules,
        stdout: Some(Arc::new(|_: &[u8]| {})),
        stderr: Some(Arc::new(|_: &[u8]| {})),
        on_uncaught: Some(Arc::new(|_: &str, _: &pi_jsrt::Error| {})),
        ..Default::default()
    })
    .unwrap();
    let driver = rt
        .eval_module(
            &format!(
                "{stub_prelude}\n{}",
                r#"
export async function run(path) {
  const calls = [];
  const pi = new Proxy({}, {
    get(_, prop) {
      if (prop === "then") return undefined;
      return (...args) => { calls.push(String(prop)); return makeStub(`pi.${String(prop)}()`); };
    },
  });
  let mod;
  try { mod = await import(path); } catch (e) { return { stage: "load", error: String(e && e.stack || e) }; }
  if (typeof mod.default !== "function") return { stage: "load", error: "no default export function" };
  try { await mod.default(pi); } catch (e) { return { stage: "factory", error: String(e && e.stack || e), calls }; }
  return { stage: "ok", calls };
}
"#
            ),
            "driver.mjs",
        )
        .await
        .unwrap();
    // Packages that need npm dependencies which are not installed in the reference checkout.
    let needs_npm = |p: &Path| {
        let text = std::fs::read_to_string(p).unwrap_or_default();
        imports_unresolvable(&text, p.parent().unwrap())
    };
    let mut loaded = 0;
    let mut failures = Vec::new();
    let mut factory_failures = Vec::new();
    let mut skipped = Vec::new();
    for p in &entries {
        if needs_npm(p) {
            skipped.push(p.display().to_string());
            continue;
        }
        let r = driver.call_method("run", vec![json!(p.display().to_string())]).await.unwrap();
        match r["stage"].as_str() {
            Some("ok") => loaded += 1,
            Some("factory") => {
                loaded += 1;
                factory_failures.push(format!("{}: {}", p.display(), r["error"]));
            }
            _ => failures.push(format!("{}: {}", p.display(), r["error"])),
        }
    }
    eprintln!("loaded {loaded}/{} example extensions (skipped {} needing npm deps: {skipped:?})", entries.len(), skipped.len());
    for f in &factory_failures {
        eprintln!("factory (with generic stubs) failed: {f}");
    }
    assert!(failures.is_empty(), "extensions failed to load:\n{}", failures.join("\n"));
    assert!(loaded >= 60, "only {loaded} extensions loaded");
}

/// True if the file (or its local imports) imports a bare npm package that is not
/// installed and is not one of pi's host packages.
fn imports_unresolvable(text: &str, dir: &Path) -> bool {
    let re = regex::Regex::new(r#"(?m)^\s*(?:import|export)\b[^;]*?\bfrom\s+["']([^"'./][^"']*)["']"#).unwrap();
    for c in re.captures_iter(text) {
        let spec = &c[1];
        let host = spec.starts_with("@earendil-works/pi-") || spec.starts_with("@mariozechner/") || spec == "typebox" || spec.starts_with("typebox/") || spec.starts_with("node:");
        let builtin = ["fs", "path", "os", "child_process", "fs/promises", "url", "util", "events", "crypto", "readline"].contains(&spec);
        if host || builtin {
            continue;
        }
        let pkg = if spec.starts_with('@') { spec.splitn(3, '/').take(2).collect::<Vec<_>>().join("/") } else { spec.split('/').next().unwrap().to_string() };
        let mut d = Some(dir);
        let mut found = false;
        while let Some(x) = d {
            if x.join("node_modules").join(&pkg).is_dir() {
                found = true;
                break;
            }
            d = x.parent();
        }
        if !found {
            return true;
        }
    }
    false
}
