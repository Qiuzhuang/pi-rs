//! `node:vm`: every context is a new Boa realm (its own intrinsics: `Object`, `Array`, ...).
//!
//! A contextified object behaves like V8's: the realm's global object is a Proxy (built by
//! `js/node/vm.js` in the outer realm) that resolves names on the contextified object first and
//! on the realm's builtins next, and sends assignments and declarations to the contextified
//! object. The Proxy becomes the realm's global object through `Hooks::create_global_object`.

use std::path::PathBuf;

use boa_engine::realm::Realm;
use boa_engine::{Context, Finalize, JsArgs, JsData, JsObject, JsResult, JsValue, Script, Source, Trace};

use crate::engine::{self, OpFn, arg_opt_string, arg_string, state};

/// A realm owned by a JS object (traced, so the realm lives as long as the object).
struct VmRealm {
    realm: Realm,
}

impl Finalize for VmRealm {}
// SAFETY: the only GC-managed field is traced.
unsafe impl Trace for VmRealm {
    boa_engine::gc::custom_trace!(this, mark, {
        mark(&this.realm);
    });
}
impl JsData for VmRealm {}

const REALM_GLUE: &str = r#"
// Per-realm setup of a vm context: Node-style Error.stack, Error.captureStackTrace and
// ArrayBuffer.prototype.transfer (as in the main realm's prelude).
const desc = Object.getOwnPropertyDescriptor(Error.prototype, "stack");
if (desc && typeof desc.get === "function") {
  const originalGet = desc.get;
  const header = (err) => {
    let name, message;
    try {
      name = err.name === undefined ? "Error" : String(err.name);
      message = err.message === undefined ? "" : String(err.message);
    } catch {
      name = "Error";
      message = "";
    }
    if (!name) return message;
    if (!message) return name;
    return `${name}: ${message}`;
  };
  Object.defineProperty(Error.prototype, "stack", {
    configurable: true,
    enumerable: false,
    get() {
      let frames = "";
      try {
        frames = originalGet.call(this) || "";
      } catch {}
      frames = frames.replace(/\n?\s*at\s+\(native\)\s*$/g, "").replace(/\s+$/, "");
      if (frames) frames = ops.remapStack(frames);
      return frames ? `${header(this)}\n${frames}` : header(this);
    },
    set(value) {
      Object.defineProperty(this, "stack", { value, writable: true, configurable: true, enumerable: false });
    },
  });
}
Error.stackTraceLimit = 10;
Error.captureStackTrace = function captureStackTrace(target) {
  if (target === null || (typeof target !== "object" && typeof target !== "function")) throw new TypeError("Invalid argument");
  const frames = new Error().stack.split("\n").slice(2).join("\n");
  let head;
  try {
    head = target.message ? `${target.name ?? "Error"}: ${target.message}` : String(target.name ?? "Error");
  } catch {
    head = "Error";
  }
  Object.defineProperty(target, "stack", { value: frames ? `${head}\n${frames}` : head, writable: true, configurable: true, enumerable: false });
};
if (typeof ArrayBuffer.prototype.transfer !== "function") {
  for (const [name, fixed] of [["transfer", false], ["transferToFixedLength", true]]) {
    Object.defineProperty(ArrayBuffer.prototype, name, {
      value: { [name](n) { return ops.arrayBufferTransfer(this, n, !fixed && this.resizable ? this.maxByteLength : undefined); } }[name],
      writable: true, configurable: true, enumerable: false,
    });
  }
  Object.defineProperty(ArrayBuffer.prototype, "detached", { get: function () { return ops.arrayBufferDetached(this); }, configurable: true, enumerable: false });
}
"#;

fn realm_of(v: &JsValue) -> Option<Realm> {
    let obj = v.as_object()?;
    let data = obj.downcast_ref::<VmRealm>()?;
    Some(data.realm.clone())
}

/// Run `f` with `realm` as the current realm.
fn in_realm<R>(ctx: &mut Context, realm: &Realm, f: impl FnOnce(&mut Context) -> R) -> R {
    let previous = ctx.enter_realm(realm.clone());
    let r = f(ctx);
    ctx.enter_realm(previous);
    r
}

/// `ops.vmCreateRealm(global?, target?) -> realm handle`: a new realm whose global object is
/// `global` (a Proxy whose target is `target`), or an ordinary global object.
fn op_vm_create_realm(_: &JsValue, args: &[JsValue], ctx: &mut Context) -> JsResult<JsValue> {
    if let (Some(global), Some(target)) = (args.get_or_undefined(0).as_object(), args.get_or_undefined(1).as_object()) {
        engine::NEXT_REALM_GLOBAL.with(|g| *g.borrow_mut() = Some((global, target)));
    }
    let realm = ctx.create_realm();
    engine::NEXT_REALM_GLOBAL.with(|g| g.borrow_mut().take());
    let realm = realm?;
    let ops = state(ctx).ops.borrow().clone();
    in_realm(ctx, &realm, |ctx| -> JsResult<()> {
        engine::serialize_symbol_for(ctx)?;
        let wrapped = format!("(function (ops) {{\"use strict\";\n{REALM_GLUE}\n}})");
        let f = ctx.eval(Source::from_bytes(wrapped.as_bytes()).with_path(&PathBuf::from("pi-jsrt:vm-realm.js")))?;
        if let (Some(f), Some(ops)) = (f.as_function(), ops) {
            f.call(&JsValue::undefined(), &[ops.into()], ctx)?;
        }
        Ok(())
    })?;
    let proto = ctx.intrinsics().constructors().object().prototype();
    Ok(JsObject::from_proto_and_data(Some(proto), VmRealm { realm }).into())
}

/// `ops.vmGlobal(realm) -> globalThis of the realm`
fn op_vm_global(_: &JsValue, args: &[JsValue], ctx: &mut Context) -> JsResult<JsValue> {
    let realm = realm_of(args.get_or_undefined(0)).ok_or_else(|| engine::type_error("invalid vm context"))?;
    Ok(in_realm(ctx, &realm, |ctx| ctx.global_object()).into())
}

/// Shift positions like V8's `lineOffset`/`columnOffset` (Boa sources start at 1:1).
fn with_offsets(code: &str, line_offset: i64, column_offset: i64) -> String {
    let mut out = String::new();
    for _ in 0..line_offset.max(0) {
        out.push('\n');
    }
    for _ in 0..column_offset.max(0) {
        out.push(' ');
    }
    out.push_str(code);
    out
}

/// `ops.vmCompile(code, filename)`: parse only (throws the SyntaxError), like `new vm.Script`.
fn op_vm_compile(_: &JsValue, args: &[JsValue], ctx: &mut Context) -> JsResult<JsValue> {
    let code = arg_string(args, 0, ctx)?;
    let filename = arg_opt_string(args, 1, ctx)?.unwrap_or_else(|| "evalmachine.<anonymous>".into());
    Script::parse(Source::from_bytes(code.as_bytes()).with_path(&PathBuf::from(filename)), None, ctx)?;
    Ok(JsValue::undefined())
}

/// `ops.vmRun(realm | undefined, code, filename, lineOffset, columnOffset)`: evaluate a script in
/// the realm (the current one when `undefined`) and return its completion value.
fn op_vm_run(_: &JsValue, args: &[JsValue], ctx: &mut Context) -> JsResult<JsValue> {
    let realm = realm_of(args.get_or_undefined(0));
    let code = arg_string(args, 1, ctx)?;
    let filename = arg_opt_string(args, 2, ctx)?.unwrap_or_else(|| "evalmachine.<anonymous>".into());
    let line = args.get_or_undefined(3).to_number(ctx).unwrap_or(0.0) as i64;
    let column = args.get_or_undefined(4).to_number(ctx).unwrap_or(0.0) as i64;
    let code = with_offsets(&code, line, column);
    let path = PathBuf::from(filename);
    let run = |ctx: &mut Context| -> JsResult<JsValue> {
        let realm = ctx.realm().clone();
        let script = Script::parse(Source::from_bytes(code.as_bytes()).with_path(&path), Some(realm), ctx)?;
        script.evaluate(ctx)
    };
    match realm {
        Some(realm) => in_realm(ctx, &realm, run),
        None => run(ctx),
    }
}

pub(crate) const OPS: &[(&str, usize, OpFn)] = &[
    ("vmCreateRealm", 2, op_vm_create_realm),
    ("vmGlobal", 1, op_vm_global),
    ("vmCompile", 2, op_vm_compile),
    ("vmRun", 5, op_vm_run),
];
