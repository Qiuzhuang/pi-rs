//! Node-like process mode: run a script the way `node script.js args...` does.
//!
//! There is no `node` binary at runtime, so `child_process.fork()`, `cluster.fork()` and
//! worker-like helpers start another runtime by re-executing the current program with
//! [`NODE_MARKER`] as its first argument (see [`crate::RuntimeOptions::node_exec`]). An
//! embedding binary calls [`main_if_requested`] first thing in `main`; the `pi` binary does.
//!
//! The process behaves like Node's: the main module (ES module or CommonJS, `.ts` included)
//! runs, then the event loop runs until nothing keeps it alive (referenced timers, sockets,
//! servers, child processes, the IPC channel with listeners...), `beforeExit` and `exit` are
//! emitted and the process exits with `process.exitCode`. An uncaught exception or unhandled
//! rejection without listeners prints the error like Node and exits with code 1. When started
//! with an IPC channel (`NODE_CHANNEL_FD`), `process.send()`, `process.connected`,
//! `process.disconnect()` and the `message`/`disconnect` events work as in a forked Node child.

use std::collections::HashMap;
use std::sync::Arc;

use serde_json::{Value, json};

use crate::runtime::{JsRuntime, RuntimeOptions};

/// First argument that makes [`main_if_requested`] run the process in Node mode.
pub const NODE_MARKER: &str = "__pi_jsrt_node__";

/// Environment variables Node consumes at startup (and removes from `process.env`).
const STARTUP_ENV: &[&str] = &["NODE_CHANNEL_FD", "NODE_CHANNEL_SERIALIZATION_MODE", "NODE_UNIQUE_ID"];

/// If the process was started as `<exe> __pi_jsrt_node__ [node options] <script> [args]`, run it
/// in Node mode and return the exit code (the caller exits with it). Otherwise `None`.
pub fn main_if_requested() -> Option<i32> {
    let mut args = std::env::args_os();
    args.next();
    if args.next().as_deref() != Some(std::ffi::OsStr::new(NODE_MARKER)) {
        return None;
    }
    Some(run(args.map(|a| a.to_string_lossy().into_owned()).collect()))
}

struct Parsed {
    exec_argv: Vec<String>,
    eval: Option<String>,
    print: bool,
    script: Option<String>,
    args: Vec<String>,
}

fn parse(args: Vec<String>) -> Parsed {
    let mut p = Parsed { exec_argv: Vec::new(), eval: None, print: false, script: None, args: Vec::new() };
    let mut i = 0;
    while i < args.len() {
        let a = args[i].as_str();
        match a {
            "-e" | "--eval" | "-p" | "--print" => {
                p.print |= a == "-p" || a == "--print";
                p.exec_argv.push(a.to_string());
                if let Some(code) = args.get(i + 1) {
                    p.exec_argv.push(code.clone());
                    p.eval = Some(code.clone());
                }
                i += 2;
            }
            "--" => {
                i += 1;
                break;
            }
            "-r" | "--require" | "--import" | "--loader" | "--experimental-loader" | "--conditions" | "-C" => {
                p.exec_argv.push(a.to_string());
                if let Some(v) = args.get(i + 1) {
                    p.exec_argv.push(v.clone());
                }
                i += 2;
            }
            _ if a.starts_with('-') && a.len() > 1 => {
                p.exec_argv.push(a.to_string());
                i += 1;
            }
            _ => break,
        }
    }
    let mut rest = args.into_iter().skip(i);
    if p.eval.is_none() {
        p.script = rest.next();
    }
    p.args = rest.collect();
    p
}

/// Run in Node mode with `args` (everything after the program name and the marker) and return
/// the exit code. Normally the process exits from inside (`process.exit`, uncaught errors).
pub fn run(args: Vec<String>) -> i32 {
    let parsed = parse(args);
    let tokio = match tokio::runtime::Builder::new_multi_thread().worker_threads(2).thread_name("pi-jsrt-io").enable_all().build() {
        Ok(t) => t,
        Err(e) => {
            eprintln!("failed to start tokio runtime: {e}");
            return 1;
        }
    };
    let exe = std::env::current_exe().map(|p| p.display().to_string()).unwrap_or_else(|_| "node".into());
    let cwd = std::env::current_dir().unwrap_or_else(|_| "/".into());
    let mut env: HashMap<String, String> = std::env::vars().collect();
    let startup: HashMap<&str, String> = STARTUP_ENV.iter().filter_map(|k| env.remove(*k).map(|v| (*k, v))).collect();
    let script = parsed.script.as_ref().map(|s| {
        let p = std::path::Path::new(s);
        if p.is_absolute() { p.to_path_buf() } else { cwd.join(p) }
    });
    let mut argv = vec![exe];
    if let Some(s) = &script {
        argv.push(s.display().to_string());
    }
    argv.extend(parsed.args.iter().cloned());
    let options = RuntimeOptions {
        cwd: Some(cwd),
        env: Some(env),
        argv,
        exec_argv: parsed.exec_argv.clone(),
        tokio: Some(tokio.handle().clone()),
        on_exit: Some(Arc::new(|code| {
            use std::io::Write;
            let _ = std::io::stdout().flush();
            let _ = std::io::stderr().flush();
            std::process::exit(code);
        })),
        ..Default::default()
    };
    tokio.block_on(async move {
        let rt = match JsRuntime::new(options) {
            Ok(rt) => rt,
            Err(e) => {
                eprintln!("{e}");
                return 1;
            }
        };
        let main = json!({
            "file": script.map(|s| s.display().to_string()),
            "eval": parsed.eval,
            "print": parsed.print,
            "ipcFd": startup.get("NODE_CHANNEL_FD").and_then(|v| v.parse::<i64>().ok()),
            "serialization": startup.get("NODE_CHANNEL_SERIALIZATION_MODE").cloned().unwrap_or_else(|| "json".into()),
            "uniqueId": startup.get("NODE_UNIQUE_ID"),
        });
        if call_internal(&rt, "nodeMain", main).await.is_err() {
            return 1;
        }
        loop {
            if rt.wait_idle().await.is_err() {
                return 1;
            }
            match call_internal(&rt, "nodeBeforeExit", Value::Null).await {
                Ok(Value::Bool(true)) => continue,
                Ok(_) => break,
                Err(_) => return 1,
            }
        }
        match call_internal(&rt, "nodeExit", Value::Null).await {
            Ok(v) => v.as_i64().unwrap_or(0) as i32,
            Err(_) => 1,
        }
    })
}

async fn call_internal(rt: &JsRuntime, name: &'static str, arg: Value) -> crate::Result<Value> {
    rt.call_internal(name, arg).await
}
