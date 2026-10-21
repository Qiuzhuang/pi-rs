//! Native ops exposed to the JS glue (`js/`), and runtime bootstrap.
//!
//! Glue files are evaluated as `(function (ops, internal) { ... })`: `ops` holds the
//! Rust functions defined here, `internal` is a private object shared between glue
//! files (builtin registry, conversion helpers).

use boa_engine::object::builtins::{JsArrayBuffer, JsTypedArray, JsUint8Array};
use boa_engine::{Context, JsObject, JsResult, JsValue, js_string};

use crate::engine::{self, define_ops, js_str, run_glue, state};

pub(crate) mod child_process;
pub(crate) mod crypto;
pub(crate) mod encoding;
pub(crate) mod fetch;
pub(crate) mod fs;
pub(crate) mod process;

/// Glue scripts installed in every runtime.
const WEB_GLUE: &[(&str, &str)] = &[
    ("prelude.js", include_str!("../../js/prelude.js")),
    ("inspect.js", include_str!("../../js/inspect.js")),
    ("web/encoding.js", include_str!("../../js/web/encoding.js")),
    ("web/url.js", include_str!("../../js/web/url.js")),
    ("web/events.js", include_str!("../../js/web/events.js")),
    ("web/globals.js", include_str!("../../js/web/globals.js")),
];

/// Glue scripts installed when Node compatibility is enabled.
const NODE_GLUE: &[(&str, &str)] = &[
    ("node/events.js", include_str!("../../js/node/events.js")),
    ("node/path.js", include_str!("../../js/node/path.js")),
    ("node/buffer.js", include_str!("../../js/node/buffer.js")),
    ("node/util.js", include_str!("../../js/node/util.js")),
    ("node/process.js", include_str!("../../js/node/process.js")),
    ("node/stream.js", include_str!("../../js/node/stream.js")),
    ("node/fs.js", include_str!("../../js/node/fs.js")),
    ("node/os.js", include_str!("../../js/node/os.js")),
    ("node/child_process.js", include_str!("../../js/node/child_process.js")),
    ("node/crypto.js", include_str!("../../js/node/crypto.js")),
    ("node/misc.js", include_str!("../../js/node/misc.js")),
    ("web/fetch.js", include_str!("../../js/web/fetch.js")),
];

/// Create the ops object, run the glue, and store `internal` in the state.
pub(crate) fn bootstrap(ctx: &mut Context) -> JsResult<()> {
    let st = state(ctx);
    let node = st.shared.options.node_compat;
    let ops = JsObject::with_object_proto(ctx.intrinsics());
    define_ops(ctx, &ops, engine::CORE_OPS)?;
    define_ops(ctx, &ops, crate::loader::LOADER_OPS)?;
    define_ops(ctx, &ops, encoding::OPS)?;
    define_ops(ctx, &ops, crypto::WEB_OPS)?;
    if node {
        define_ops(ctx, &ops, fs::OPS)?;
        define_ops(ctx, &ops, process::OPS)?;
        define_ops(ctx, &ops, child_process::OPS)?;
        define_ops(ctx, &ops, crypto::OPS)?;
        define_ops(ctx, &ops, fetch::OPS)?;
    }
    let config = process::config_object(ctx)?;
    ops.set(js_string!("config"), config, true, ctx)?;

    let internal = JsObject::with_object_proto(ctx.intrinsics());
    *st.internal.borrow_mut() = Some(internal.clone());
    for (name, src) in WEB_GLUE {
        run_glue(ctx, name, src, &ops, &internal)?;
        engine::run_microtasks(ctx);
    }
    if node {
        for (name, src) in NODE_GLUE {
            run_glue(ctx, name, src, &ops, &internal)?;
            engine::run_microtasks(ctx);
        }
    }
    Ok(())
}

/// Release OS resources owned by ops (child processes, open files).
pub(crate) fn teardown(ctx: &mut Context) {
    child_process::teardown(ctx);
    fs::teardown(ctx);
}

/// Read bytes from a string (UTF-8), ArrayBuffer, typed array or DataView.
pub(crate) fn bytes_arg(v: &JsValue, ctx: &mut Context) -> JsResult<Vec<u8>> {
    if let Some(s) = v.as_string() {
        return Ok(s.to_std_string_lossy().into_bytes());
    }
    if let Some(obj) = v.as_object() {
        if let Ok(ta) = JsTypedArray::from_object(obj.clone()) {
            let buffer = ta.buffer(ctx)?;
            let offset = ta.byte_offset(ctx)?;
            let len = ta.byte_length(ctx)?;
            if let Some(buf) = buffer.as_object()
                && let Ok(ab) = JsArrayBuffer::from_object(buf)
            {
                let data = ab.data().ok_or_else(|| engine::type_error("detached ArrayBuffer"))?;
                return Ok(data[offset..offset + len].to_vec());
            }
            return Err(engine::type_error("unsupported typed array buffer"));
        }
        if let Ok(ab) = JsArrayBuffer::from_object(obj.clone()) {
            return Ok(ab.data().map(|d| d.to_vec()).unwrap_or_default());
        }
        if let Ok(dv) = boa_engine::object::builtins::JsDataView::from_object(obj.clone()) {
            let buffer = dv.buffer(ctx)?;
            let offset = dv.byte_offset(ctx)? as usize;
            let len = dv.byte_length(ctx)? as usize;
            if let Some(buf) = buffer.as_object()
                && let Ok(ab) = JsArrayBuffer::from_object(buf)
            {
                let data = ab.data().ok_or_else(|| engine::type_error("detached ArrayBuffer"))?;
                return Ok(data[offset..offset + len].to_vec());
            }
        }
    }
    if v.is_null_or_undefined() {
        return Ok(Vec::new());
    }
    Ok(v.to_string(ctx)?.to_std_string_lossy().into_bytes())
}

/// Create a `Uint8Array` holding `bytes`.
pub(crate) fn bytes_to_js(bytes: &[u8], ctx: &mut Context) -> JsResult<JsValue> {
    let ab = JsArrayBuffer::new(bytes.len(), ctx)?;
    if let Some(mut data) = ab.data_mut() {
        data.copy_from_slice(bytes);
    }
    let arr = JsUint8Array::from_array_buffer(ab, ctx)?;
    Ok(arr.into())
}

/// Node-style system error (`err.code`, `err.syscall`, `err.path`, `err.errno`).
pub(crate) fn io_error(e: &std::io::Error, syscall: &str, path: Option<&str>) -> crate::Error {
    let code = io_code(e);
    let desc = io_desc(e, code);
    let message = match path {
        Some(p) => format!("{code}: {desc}, {syscall} '{p}'"),
        None => format!("{code}: {desc}, {syscall}"),
    };
    crate::Error::Js(crate::error::JsException {
        name: "Error".into(),
        message,
        stack: None,
        code: Some(code.to_string()),
    })
}

pub(crate) fn io_code(e: &std::io::Error) -> &'static str {
    use std::io::ErrorKind::*;
    if let Some(raw) = e.raw_os_error() {
        match raw {
            libc::ENOENT => return "ENOENT",
            libc::EEXIST => return "EEXIST",
            libc::EACCES => return "EACCES",
            libc::EPERM => return "EPERM",
            libc::ENOTDIR => return "ENOTDIR",
            libc::EISDIR => return "EISDIR",
            libc::ENOTEMPTY => return "ENOTEMPTY",
            libc::EBADF => return "EBADF",
            libc::EINVAL => return "EINVAL",
            libc::EMFILE => return "EMFILE",
            libc::ELOOP => return "ELOOP",
            libc::EXDEV => return "EXDEV",
            libc::EPIPE => return "EPIPE",
            libc::ENOSPC => return "ENOSPC",
            libc::EROFS => return "EROFS",
            libc::EBUSY => return "EBUSY",
            libc::ENAMETOOLONG => return "ENAMETOOLONG",
            libc::ECHILD => return "ECHILD",
            libc::ESRCH => return "ESRCH",
            libc::EAGAIN => return "EAGAIN",
            _ => {}
        }
    }
    match e.kind() {
        NotFound => "ENOENT",
        PermissionDenied => "EACCES",
        AlreadyExists => "EEXIST",
        InvalidInput => "EINVAL",
        BrokenPipe => "EPIPE",
        TimedOut => "ETIMEDOUT",
        ConnectionRefused => "ECONNREFUSED",
        ConnectionReset => "ECONNRESET",
        _ => "EIO",
    }
}

fn io_desc(e: &std::io::Error, code: &str) -> String {
    match code {
        "ENOENT" => "no such file or directory".into(),
        "EEXIST" => "file already exists".into(),
        "EACCES" => "permission denied".into(),
        "EPERM" => "operation not permitted".into(),
        "ENOTDIR" => "not a directory".into(),
        "EISDIR" => "illegal operation on a directory".into(),
        "ENOTEMPTY" => "directory not empty".into(),
        "EBADF" => "bad file descriptor".into(),
        "EINVAL" => "invalid argument".into(),
        _ => e.to_string(),
    }
}

/// Throw a Node-style system error.
pub(crate) fn throw_io(ctx: &mut Context, e: &std::io::Error, syscall: &str, path: Option<&str>) -> boa_engine::JsError {
    crate::error::to_boa(&io_error(e, syscall, path), ctx)
}

/// Build a plain object from key/value pairs.
pub(crate) fn object(ctx: &mut Context, entries: Vec<(&str, JsValue)>) -> JsResult<JsValue> {
    let obj = JsObject::with_object_proto(ctx.intrinsics());
    for (k, v) in entries {
        obj.set(js_str(k).as_string().unwrap(), v, true, ctx)?;
    }
    Ok(obj.into())
}

/// Build a JS array from values.
pub(crate) fn array(ctx: &mut Context, items: Vec<JsValue>) -> JsValue {
    boa_engine::object::builtins::JsArray::from_iter(items, ctx).into()
}
