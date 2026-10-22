//! Regression tests for engine bugs fixed by the vendored Boa patches (`vendor/*/PATCH.md`).

use std::path::Path;

use pi_jsrt::{JsRuntime, RuntimeOptions};
use serde_json::json;

fn write(root: &Path, rel: &str, content: &str) {
    let p = root.join(rel);
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    std::fs::write(p, content).unwrap();
}

/// boa_ast's scope analyzer skipped the computed key of class methods (including
/// accessors, static and async/generator methods), so functions created there could not
/// resolve module imports or top-level bindings (`ReferenceError`). `@anthropic-ai/sdk`
/// uses `[(_x = new WeakMap(), Symbol.asyncIterator)]()` style keys after down-leveling.
#[tokio::test]
async fn class_computed_method_key_closures_see_module_scope() {
    let dir = tempfile::tempdir().unwrap();
    write(dir.path(), "t.mjs", "export const g = 7;\n");
    write(
        dir.path(),
        "main.mjs",
        r#"
import { g } from "./t.mjs";
const top = "top";
let f1, f2, f3, f4, f5, f6, f7, f8, f9;
class C {
  [(f1 = () => typeof g, "m")]() { return 1; }
  static [(f2 = () => g + 1, "s")]() { return 2; }
  get [(f3 = () => top, "acc")]() { return 3; }
  set [(f4 = () => g * 2, "acc")](v) {}
  async [(f5 = () => `${top}${g}`, "am")]() { return 5; }
  *[(f6 = () => [g, top], "gen")]() { yield 6; }
  [(f7 = () => typeof g, "fld")] = (f8 = () => g, 8);
  static { f9 = () => g - 1; }
  #p() { return g; }
  priv() { return this.#p(); }
}
const D = class Named {
  [(f10 = () => [typeof Named, g], "x")]() {}
};
var f10;
const o = { [(f11 = () => g, "k")]() {} };
var f11;
const c = new C();
export default {
  f1: f1(), f2: f2(), f3: f3(), f4: f4(), f5: f5(), f6: f6(), f7: f7(), f8: f8(), f9: f9(),
  f10: f10(), f11: f11(),
  m: c.m(), s: C.s(), acc: c.acc, am: await c.am(), gen: [...c.gen()], fld: c.fld, priv: c.priv(),
  d: typeof D.prototype.x,
};
"#,
    );
    let rt = JsRuntime::new(RuntimeOptions {
        cwd: Some(dir.path().to_path_buf()),
        ..Default::default()
    })
    .unwrap();
    let ns = rt
        .import(dir.path().join("main.mjs").to_str().unwrap())
        .await
        .unwrap_or_else(|e| panic!("{e}\n{:?}", e.as_js().and_then(|j| j.stack.clone())));
    let v = ns.get("default").await.unwrap();
    assert_eq!(
        v,
        json!({
            "f1": "number", "f2": 8, "f3": "top", "f4": 14, "f5": "top7", "f6": [7, "top"],
            "f7": "number", "f8": 7, "f9": 6, "f10": ["function", 7], "f11": 7,
            "m": 1, "s": 2, "acc": 3, "am": 5, "gen": [6], "fld": 8, "priv": 7, "d": "function"
        })
    );
}

/// The same pattern in a script (non-module) and inside a function scope.
#[tokio::test]
async fn class_computed_method_key_closures_in_function_scope() {
    let rt = JsRuntime::new(RuntimeOptions::default()).unwrap();
    let v = rt
        .eval(
            r#"(() => {
                const local = 41;
                let f;
                class K { [(f = () => local + 1, "m")]() {} }
                return f();
            })()"#,
        )
        .await
        .unwrap();
    assert_eq!(v, json!(42));
}
