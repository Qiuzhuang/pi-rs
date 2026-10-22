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
pub(crate) mod net;
pub(crate) mod process;
pub(crate) mod zlib;

/// Glue scripts installed in every runtime.
const WEB_GLUE: &[(&str, &str)] = &[
    ("prelude.js", include_str!("../../js/prelude.js")),
    ("inspect.js", include_str!("../../js/inspect.js")),
    ("web/encoding.js", include_str!("../../js/web/encoding.js")),
    ("web/url.js", include_str!("../../js/web/url.js")),
    ("web/events.js", include_str!("../../js/web/events.js")),
    ("web/globals.js", include_str!("../../js/web/globals.js")),
];

/// Glue scripts installed eagerly when Node compatibility is enabled.
const NODE_GLUE: &[(&str, &str)] = &[
    ("node/events.js", include_str!("../../js/node/events.js")),
    ("node/buffer.js", include_str!("../../js/node/buffer.js")),
    ("node/process.js", include_str!("../../js/node/process.js")),
];

/// A glue file compiled on first use: the builtins and globals it provides.
struct LazyGlue {
    file: &'static str,
    src: &'static str,
    builtins: &'static [&'static str],
    globals: &'static [&'static str],
}

/// Glue loaded on demand (Node compatibility only), to keep startup fast.
const LAZY_GLUE: &[LazyGlue] = &[
    LazyGlue {
        file: "node/path.js",
        src: include_str!("../../js/node/path.js"),
        builtins: &["path", "path/posix", "path/win32"],
        globals: &[],
    },
    LazyGlue {
        file: "node/util.js",
        src: include_str!("../../js/node/util.js"),
        builtins: &["util", "util/types"],
        globals: &[],
    },
    LazyGlue {
        file: "node/stream.js",
        src: include_str!("../../js/node/stream.js"),
        builtins: &["stream", "stream/promises", "stream/web"],
        globals: &[],
    },
    LazyGlue {
        file: "node/fs.js",
        src: include_str!("../../js/node/fs.js"),
        builtins: &["fs", "fs/promises"],
        globals: &[],
    },
    LazyGlue {
        file: "node/os.js",
        src: include_str!("../../js/node/os.js"),
        builtins: &["os"],
        globals: &[],
    },
    LazyGlue {
        file: "node/child_process.js",
        src: include_str!("../../js/node/child_process.js"),
        builtins: &["child_process"],
        globals: &[],
    },
    LazyGlue {
        file: "node/crypto.js",
        src: include_str!("../../js/node/crypto.js"),
        builtins: &["crypto"],
        globals: &[],
    },
    LazyGlue {
        file: "node/misc.js",
        src: include_str!("../../js/node/misc.js"),
        builtins: &[
            "url",
            "querystring",
            "string_decoder",
            "readline",
            "readline/promises",
            "assert",
            "assert/strict",
            "module",
            "timers",
            "timers/promises",
            "tty",
            "perf_hooks",
            "async_hooks",
            "worker_threads",
            "console",
            "tls",
            "dns",
            "dgram",
            "cluster",
            "vm",
            "v8",
            "inspector",
            "http2",
            "diagnostics_channel",
            "constants",
        ],
        globals: &[],
    },
    LazyGlue {
        file: "node/zlib.js",
        src: include_str!("../../js/node/zlib.js"),
        builtins: &["zlib"],
        globals: &[],
    },
    LazyGlue {
        file: "node/net.js",
        src: include_str!("../../js/node/net.js"),
        builtins: &["net"],
        globals: &[],
    },
    LazyGlue {
        file: "node/http.js",
        src: include_str!("../../js/node/http.js"),
        builtins: &["http", "https"],
        globals: &[],
    },
    LazyGlue {
        file: "web/fetch.js",
        src: include_str!("../../js/web/fetch.js"),
        builtins: &[],
        globals: &[
            "fetch",
            "Headers",
            "Request",
            "Response",
            "FormData",
            "ReadableStream",
            "ReadableStreamDefaultReader",
            "ReadableStreamDefaultController",
            "WritableStream",
            "TransformStream",
            "TextDecoderStream",
            "TextEncoderStream",
            "CountQueuingStrategy",
            "ByteLengthQueuingStrategy",
        ],
    },
];

fn trace_glue(name: &str, t: std::time::Instant) {
    tracing::trace!(target: "pi_jsrt", "glue {name}: {:?}", t.elapsed());
    if std::env::var_os("PI_JSRT_TRACE_BOOT").is_some() {
        eprintln!("glue {name}: {:?}", t.elapsed());
    }
}

/// `ops.loadGlue(file)`: compile and run a lazily loaded glue file.
fn op_load_glue(_: &JsValue, args: &[JsValue], ctx: &mut Context) -> JsResult<JsValue> {
    let file = engine::arg_string(args, 0, ctx)?;
    let Some(glue) = LAZY_GLUE.iter().find(|g| g.file == file) else {
        return Err(engine::type_error(format!("unknown glue file {file}")));
    };
    let st = state(ctx);
    let ops = st.ops.borrow().clone().ok_or_else(|| engine::type_error("ops missing"))?;
    let internal = engine::internal(ctx);
    let t = std::time::Instant::now();
    run_glue(ctx, glue.file, glue.src, &ops, &internal)?;
    trace_glue(glue.file, t);
    Ok(JsValue::undefined())
}

fn lazy_tables(ctx: &mut Context) -> JsResult<(JsValue, JsValue)> {
    let builtins = JsObject::with_null_proto();
    let globals = JsObject::with_null_proto();
    for g in LAZY_GLUE {
        for b in g.builtins {
            builtins.set(js_str(b).as_string().unwrap(), js_str(g.file), true, ctx)?;
        }
        for name in g.globals {
            globals.set(js_str(name).as_string().unwrap(), js_str(g.file), true, ctx)?;
        }
    }
    Ok((builtins.into(), globals.into()))
}

/// Create the ops object, run the glue, and store `internal` in the state.
pub(crate) fn bootstrap(ctx: &mut Context) -> JsResult<()> {
    let st = state(ctx);
    let node = st.shared.options.node_compat;
    let ops = JsObject::with_object_proto(ctx.intrinsics());
    define_ops(ctx, &ops, engine::CORE_OPS)?;
    define_ops(ctx, &ops, crate::loader::LOADER_OPS)?;
    define_ops(ctx, &ops, encoding::OPS)?;
    define_ops(ctx, &ops, crypto::WEB_OPS)?;
    define_ops(ctx, &ops, &[("loadGlue", 1, op_load_glue)])?;
    if node {
        define_ops(ctx, &ops, fs::OPS)?;
        define_ops(ctx, &ops, process::OPS)?;
        define_ops(ctx, &ops, child_process::OPS)?;
        define_ops(ctx, &ops, crypto::OPS)?;
        define_ops(ctx, &ops, fetch::OPS)?;
        define_ops(ctx, &ops, net::OPS)?;
        define_ops(ctx, &ops, zlib::OPS)?;
    }
    let config = process::config_object(ctx)?;
    if node && let Some(cfg) = config.as_object() {
        let (builtins, globals) = lazy_tables(ctx)?;
        cfg.set(js_string!("lazyBuiltins"), builtins, true, ctx)?;
        cfg.set(js_string!("lazyGlobals"), globals, true, ctx)?;
    }
    ops.set(js_string!("config"), config, true, ctx)?;
    *st.ops.borrow_mut() = Some(ops.clone());

    let internal = JsObject::with_object_proto(ctx.intrinsics());
    *st.internal.borrow_mut() = Some(internal.clone());
    let glue = WEB_GLUE.iter().chain(if node { NODE_GLUE } else { &[] });
    for (name, src) in glue {
        let t = std::time::Instant::now();
        run_glue(ctx, name, src, &ops, &internal)?;
        engine::run_microtasks(ctx);
        trace_glue(name, t);
    }
    Ok(())
}

/// Release OS resources owned by ops (child processes, open files).
pub(crate) fn teardown(ctx: &mut Context) {
    child_process::teardown(ctx);
    net::teardown(ctx);
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

/// Rust state owned by a JS object (dropped when the object is collected).
pub(crate) struct NativeCell<T: 'static>(pub std::cell::RefCell<T>);

impl<T: 'static> boa_engine::gc::Finalize for NativeCell<T> {}
// SAFETY: the cell holds no GC-managed values.
unsafe impl<T: 'static> boa_engine::gc::Trace for NativeCell<T> {
    boa_engine::gc::empty_trace!();
}
impl<T: 'static> boa_engine::JsData for NativeCell<T> {}

/// Wrap Rust state in an opaque JS object.
pub(crate) fn native_object<T: 'static>(value: T) -> JsValue {
    JsObject::from_proto_and_data(None, NativeCell(std::cell::RefCell::new(value))).into()
}

/// Borrow the Rust state of an object made by [`native_object`].
pub(crate) fn with_native<T: 'static, R>(v: &JsValue, f: impl FnOnce(&mut T) -> R) -> JsResult<R> {
    let obj = v.as_object().ok_or_else(|| engine::type_error("invalid native handle"))?;
    let cell = obj.downcast_ref::<NativeCell<T>>().ok_or_else(|| engine::type_error("invalid native handle"))?;
    let mut inner = cell.0.try_borrow_mut().map_err(|_| engine::type_error("native handle is busy"))?;
    Ok(f(&mut inner))
}

/// An `Error` (or `TypeError`/`RangeError` with `kind`) carrying extra properties such as
/// `code` and `errno`.
pub(crate) fn coded_error(ctx: &mut Context, kind: &str, message: &str, props: &[(&str, JsValue)]) -> boa_engine::JsError {
    let native = match kind {
        "TypeError" => boa_engine::JsNativeError::typ(),
        "RangeError" => boa_engine::JsNativeError::range(),
        "SyntaxError" => boa_engine::JsNativeError::syntax(),
        _ => boa_engine::JsNativeError::error(),
    }
    .with_message(message.to_string());
    let value = boa_engine::JsError::from(native).into_opaque(ctx).unwrap_or_default();
    if let Some(obj) = value.as_object() {
        for (k, v) in props {
            let _ = obj.set(js_str(k).as_string().unwrap(), v.clone(), true, ctx);
        }
    }
    boa_engine::JsError::from_opaque(value)
}
