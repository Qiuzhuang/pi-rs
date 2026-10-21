//! Shared helpers: every behavior test runs against each place a VM can run. The remote backend
//! serves executions in process and round-trips every message through JSON, as an RPC transport
//! would.

#![allow(dead_code)]

use std::sync::Arc;

use futures::future::BoxFuture;
use pi_codemode::*;
use serde::Serialize;
use serde::de::DeserializeOwned;
use serde_json::{Map, Value};

/// Interrupt budget of the remote backend: enough for the output-limit tests (100001 calls).
pub const INTERRUPT_BUDGET: u64 = 1_000_000;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Backend {
    Thread,
    Remote,
}

pub fn roundtrip<T: Serialize + DeserializeOwned>(value: &T) -> T {
    serde_json::from_str(&serde_json::to_string(value).expect("serialize")).expect("deserialize")
}

/// A remote whose requests and exchanges cross a JSON boundary.
pub struct WireRemote {
    pub options: CodemodeServeOptions,
}

impl CodemodeRemote for WireRemote {
    fn execute(&self, request: CodemodeRemoteRequest, exchange: CodemodeRemoteExchange) -> BoxFuture<'static, Result<(), String>> {
        let request = roundtrip(&request);
        let wrapped: CodemodeRemoteExchange = Arc::new(move |messages: Vec<CodemodeRemoteMessage>| {
            let reply = exchange(roundtrip(&messages));
            Box::pin(async move { reply.await.map(|reply| roundtrip(&reply)) })
        });
        let options = self.options.clone();
        Box::pin(async move {
            serve_codemode_remote(request, wrapped, options).await;
            Ok(())
        })
    }
}

pub fn wire_remote(options: CodemodeServeOptions) -> CodemodeRemoteFactory {
    Arc::new(move || Ok(Arc::new(WireRemote { options: options.clone() }) as Arc<dyn CodemodeRemote>))
}

pub fn new_sandbox(backend: Backend, options: CodemodeSandboxBaseOptions) -> Result<CodemodeSandbox, CodemodeSandboxError> {
    match backend {
        Backend::Thread => CodemodeSandbox::new(CodemodeSandboxOptions {
            tools: options.tools,
            globals: options.globals,
            timeout_ms: options.timeout_ms,
            memory_limit_bytes: options.memory_limit_bytes,
            ..Default::default()
        }),
        Backend::Remote => CodemodeSandbox::remote(RemoteCodemodeSandboxOptions {
            tools: options.tools,
            globals: options.globals,
            timeout_ms: options.timeout_ms,
            memory_limit_bytes: options.memory_limit_bytes,
            remote: wire_remote(CodemodeServeOptions { max_interrupt_budget: Some(INTERRUPT_BUDGET), ..Default::default() }),
            interrupt_budget: None,
        }),
    }
}

pub fn create_sandbox(backend: Backend, tools: Vec<CodemodeTool>) -> CodemodeSandbox {
    new_sandbox(backend, CodemodeSandboxBaseOptions { tools, timeout_ms: Some(10_000.0), ..Default::default() }).expect("sandbox")
}

pub fn echo() -> CodemodeTool {
    CodemodeTool::new_sync("echo", |args, _| Ok(args))
}

pub async fn run(sandbox: &CodemodeSandbox, code: &str) -> CodemodeResult {
    sandbox.execute(code, CodemodeExecuteOptions::default()).await.expect("execute")
}

pub async fn run_with(sandbox: &CodemodeSandbox, code: &str, options: CodemodeExecuteOptions) -> CodemodeResult {
    sandbox.execute(code, options).await.expect("execute")
}

/// The result as JSON, in the TS shape.
pub fn json(result: &CodemodeResult) -> Value {
    serde_json::to_value(result).expect("result json")
}

/// `toMatchObject`: every key of `expected` is in `actual` with a matching value; arrays match
/// element by element and must have the same length.
pub fn matches(actual: &Value, expected: &Value) -> bool {
    match (actual, expected) {
        (Value::Object(actual), Value::Object(expected)) => {
            expected.iter().all(|(key, value)| actual.get(key).is_some_and(|actual| matches(actual, value)))
        }
        (Value::Array(actual), Value::Array(expected)) => {
            actual.len() == expected.len() && actual.iter().zip(expected).all(|(a, e)| matches(a, e))
        }
        (Value::Number(a), Value::Number(e)) => a.as_f64() == e.as_f64(),
        _ => actual == expected,
    }
}

#[track_caller]
pub fn assert_matches(result: &CodemodeResult, expected: Value) {
    let actual = json(result);
    assert!(matches(&actual, &expected), "expected {expected:#}\n     got {actual:#}");
}

pub fn store(value: Value) -> Option<Map<String, Value>> {
    match value {
        Value::Object(map) => Some(map),
        _ => None,
    }
}

/// Wait until `check` holds (5 s at most).
pub async fn wait_for(check: impl Fn() -> bool) {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    while !check() {
        assert!(std::time::Instant::now() < deadline, "Condition was not reached");
        tokio::time::sleep(std::time::Duration::from_millis(5)).await;
    }
}

/// Runs each listed async test fn once per backend.
#[macro_export]
macro_rules! backend_tests {
    ($($name:ident),* $(,)?) => {
        mod thread_backend {
            $(
                #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
                async fn $name() {
                    super::$name($crate::common::Backend::Thread).await;
                }
            )*
        }
        mod remote_backend {
            $(
                #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
                async fn $name() {
                    super::$name($crate::common::Backend::Remote).await;
                }
            )*
        }
    };
}
