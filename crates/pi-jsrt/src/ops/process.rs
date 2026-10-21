//! `process` / `os` ops and the static configuration object.

use boa_engine::{Context, JsArgs, JsObject, JsResult, JsString, JsValue};

use super::{array, object};
use crate::engine::{OpFn, arg_f64, arg_opt_string, js_str, state};

fn hostname() -> String {
    let mut buf = [0u8; 256];
    // SAFETY: buffer is valid for its length; gethostname NUL-terminates on success.
    let r = unsafe { libc::gethostname(buf.as_mut_ptr().cast(), buf.len()) };
    if r != 0 {
        return "localhost".into();
    }
    let end = buf.iter().position(|b| *b == 0).unwrap_or(buf.len());
    String::from_utf8_lossy(&buf[..end]).into_owned()
}

struct Uname {
    sysname: String,
    release: String,
    version: String,
    machine: String,
}

fn uname() -> Uname {
    // SAFETY: utsname is plain data; uname fills it.
    let mut u: libc::utsname = unsafe { std::mem::zeroed() };
    let ok = unsafe { libc::uname(&mut u) } == 0;
    let f = |s: &[libc::c_char]| {
        let bytes: Vec<u8> = s.iter().take_while(|c| **c != 0).map(|c| *c as u8).collect();
        String::from_utf8_lossy(&bytes).into_owned()
    };
    if !ok {
        return Uname {
            sysname: "Linux".into(),
            release: String::new(),
            version: String::new(),
            machine: std::env::consts::ARCH.into(),
        };
    }
    Uname {
        sysname: f(&u.sysname),
        release: f(&u.release),
        version: f(&u.version),
        machine: f(&u.machine),
    }
}

fn node_platform() -> &'static str {
    match std::env::consts::OS {
        "macos" => "darwin",
        "windows" => "win32",
        other => other,
    }
}

fn node_arch() -> &'static str {
    match std::env::consts::ARCH {
        "x86_64" => "x64",
        "aarch64" => "arm64",
        "x86" => "ia32",
        other => other,
    }
}

/// Static configuration passed to the glue as `ops.config`.
pub(crate) fn config_object(ctx: &mut Context) -> JsResult<JsValue> {
    let st = state(ctx);
    let shared = st.shared.clone();
    let env = JsObject::with_object_proto(ctx.intrinsics());
    let mut keys: Vec<_> = if shared.options.node_compat { shared.env.iter().collect() } else { Vec::new() };
    keys.sort();
    for (k, v) in keys {
        env.set(JsString::from(k.as_str()), js_str(v), true, ctx)?;
    }
    let argv: Vec<JsValue> = shared.options.argv.iter().map(|a| js_str(a)).collect();
    let argv = array(ctx, argv);
    let exec_path = std::env::current_exe().map(|p| p.display().to_string()).unwrap_or_default();
    let home = dirs::home_dir().map(|p| p.display().to_string()).unwrap_or_else(|| "/".into());
    let tmp = std::env::temp_dir().display().to_string();
    let tmp = if tmp.len() > 1 { tmp.trim_end_matches('/').to_string() } else { tmp };
    let u = uname();
    object(
        ctx,
        vec![
            ("nodeCompat", JsValue::from(shared.options.node_compat)),
            ("env", env.into()),
            ("argv", argv),
            ("cwd", js_str(&shared.cwd.display().to_string())),
            ("platform", js_str(node_platform())),
            ("arch", js_str(node_arch())),
            ("pid", JsValue::from(std::process::id())),
            // SAFETY: getppid has no preconditions.
            ("ppid", JsValue::from(unsafe { libc::getppid() })),
            ("execPath", js_str(&exec_path)),
            ("hostname", js_str(&hostname())),
            ("homedir", js_str(&home)),
            ("tmpdir", js_str(&tmp)),
            ("osType", js_str(&u.sysname)),
            ("osRelease", js_str(&u.release)),
            ("osVersion", js_str(&u.version)),
            ("machine", js_str(&u.machine)),
            ("version", js_str(env!("CARGO_PKG_VERSION"))),
            ("hasExitHandler", JsValue::from(shared.options.on_exit.is_some())),
        ],
    )
}

fn op_exit(_: &JsValue, args: &[JsValue], ctx: &mut Context) -> JsResult<JsValue> {
    let code = args.get_or_undefined(0).to_number(ctx).unwrap_or(0.0) as i32;
    let st = state(ctx);
    if let Some(h) = &st.shared.options.on_exit {
        h(code);
    }
    Ok(JsValue::undefined())
}

fn op_kill(_: &JsValue, args: &[JsValue], ctx: &mut Context) -> JsResult<JsValue> {
    let pid = arg_f64(args, 0, ctx)? as i32;
    let sig = args.get_or_undefined(1).to_number(ctx).unwrap_or(15.0) as i32;
    // SAFETY: plain syscall.
    let r = unsafe { libc::kill(pid, sig) };
    if r != 0 {
        let e = std::io::Error::last_os_error();
        return Err(super::throw_io(ctx, &e, "kill", None));
    }
    Ok(JsValue::from(true))
}

fn op_isatty(_: &JsValue, args: &[JsValue], ctx: &mut Context) -> JsResult<JsValue> {
    let fd = arg_f64(args, 0, ctx)? as i32;
    // SAFETY: plain syscall.
    Ok(JsValue::from(unsafe { libc::isatty(fd) } == 1))
}

fn op_term_size(_: &JsValue, args: &[JsValue], ctx: &mut Context) -> JsResult<JsValue> {
    let fd = arg_f64(args, 0, ctx)? as i32;
    // SAFETY: winsize is plain data; ioctl fills it.
    let mut ws: libc::winsize = unsafe { std::mem::zeroed() };
    let r = unsafe { libc::ioctl(fd, libc::TIOCGWINSZ, &mut ws) };
    if r != 0 || ws.ws_col == 0 {
        return Ok(JsValue::null());
    }
    let cols = JsValue::from(ws.ws_col as u32);
    let rows = JsValue::from(ws.ws_row as u32);
    Ok(array(ctx, vec![cols, rows]))
}

fn op_memory(_: &JsValue, _: &[JsValue], ctx: &mut Context) -> JsResult<JsValue> {
    let rss = std::fs::read_to_string("/proc/self/statm")
        .ok()
        .and_then(|s| s.split_whitespace().nth(1).and_then(|v| v.parse::<f64>().ok()))
        .map(|pages| pages * 4096.0)
        .unwrap_or(0.0);
    object(
        ctx,
        vec![
            ("rss", JsValue::from(rss)),
            ("heapTotal", JsValue::from(0)),
            ("heapUsed", JsValue::from(0)),
            ("external", JsValue::from(0)),
            ("arrayBuffers", JsValue::from(0)),
        ],
    )
}

fn meminfo(key: &str) -> f64 {
    std::fs::read_to_string("/proc/meminfo")
        .ok()
        .and_then(|s| {
            s.lines()
                .find(|l| l.starts_with(key))
                .and_then(|l| l.split_whitespace().nth(1).and_then(|v| v.parse::<f64>().ok()))
        })
        .map(|kb| kb * 1024.0)
        .unwrap_or(0.0)
}

fn op_os_info(_: &JsValue, _: &[JsValue], ctx: &mut Context) -> JsResult<JsValue> {
    let cpus = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(1);
    let model = std::fs::read_to_string("/proc/cpuinfo")
        .ok()
        .and_then(|s| {
            s.lines()
                .find(|l| l.starts_with("model name"))
                .and_then(|l| l.split(':').nth(1).map(|v| v.trim().to_string()))
        })
        .unwrap_or_else(|| "unknown".into());
    let uptime = std::fs::read_to_string("/proc/uptime")
        .ok()
        .and_then(|s| s.split_whitespace().next().and_then(|v| v.parse::<f64>().ok()))
        .unwrap_or(0.0);
    let load = std::fs::read_to_string("/proc/loadavg")
        .ok()
        .map(|s| s.split_whitespace().take(3).filter_map(|v| v.parse::<f64>().ok()).collect::<Vec<_>>())
        .unwrap_or_default();
    let load: Vec<JsValue> = (0..3).map(|i| JsValue::from(load.get(i).copied().unwrap_or(0.0))).collect();
    let load = array(ctx, load);
    // SAFETY: plain syscalls.
    let (uid, gid) = unsafe { (libc::getuid(), libc::getgid()) };
    let user = std::env::var("USER").or_else(|_| std::env::var("LOGNAME")).unwrap_or_default();
    let shell = std::env::var("SHELL").unwrap_or_default();
    object(
        ctx,
        vec![
            ("cpus", JsValue::from(cpus as u32)),
            ("cpuModel", js_str(&model)),
            ("uptime", JsValue::from(uptime)),
            ("loadavg", load),
            ("totalmem", JsValue::from(meminfo("MemTotal:"))),
            ("freemem", JsValue::from(meminfo("MemAvailable:"))),
            ("uid", JsValue::from(uid)),
            ("gid", JsValue::from(gid)),
            ("username", js_str(&user)),
            ("shell", js_str(&shell)),
        ],
    )
}

fn op_cpu_usage(_: &JsValue, _: &[JsValue], ctx: &mut Context) -> JsResult<JsValue> {
    // SAFETY: rusage is plain data.
    let mut ru: libc::rusage = unsafe { std::mem::zeroed() };
    unsafe { libc::getrusage(libc::RUSAGE_SELF, &mut ru) };
    let user = ru.ru_utime.tv_sec as f64 * 1e6 + ru.ru_utime.tv_usec as f64;
    let system = ru.ru_stime.tv_sec as f64 * 1e6 + ru.ru_stime.tv_usec as f64;
    object(ctx, vec![("user", JsValue::from(user)), ("system", JsValue::from(system))])
}

fn op_set_env(_: &JsValue, args: &[JsValue], ctx: &mut Context) -> JsResult<JsValue> {
    // process.env is a plain object; this hook exists so embedders can observe changes later.
    let _ = arg_opt_string(args, 0, ctx)?;
    Ok(JsValue::undefined())
}

pub(crate) const OPS: &[(&str, usize, OpFn)] = &[
    ("exit", 1, op_exit),
    ("kill", 2, op_kill),
    ("isatty", 1, op_isatty),
    ("termSize", 1, op_term_size),
    ("memoryUsage", 0, op_memory),
    ("osInfo", 0, op_os_info),
    ("cpuUsage", 0, op_cpu_usage),
    ("setEnv", 2, op_set_env),
];
