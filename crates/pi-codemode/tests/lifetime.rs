//! Port of the meaningful parts of `packages/codemode/test/worker-lifetime.test.ts`: admission
//! control and exit accounting of VM threads. (The Node worker constructor, wasm loading, and
//! Node/Bun process exit tests have no Rust counterpart.)

mod common;

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use common::*;
use parking_lot::Mutex;
use pi_codemode::*;
use serde_json::json;
use tokio_util::sync::CancellationToken;

fn counting_acquire(acquired: Arc<AtomicUsize>, released: Arc<AtomicUsize>) -> AcquireWorker {
    Arc::new(move || {
        acquired.fetch_add(1, Ordering::SeqCst);
        let released = released.clone();
        Ok(Box::new(move || {
            released.fetch_add(1, Ordering::SeqCst);
        }) as WorkerRelease)
    })
}

// #10748: reject admission before creating a thread, not by stopping an excess thread.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn does_not_start_a_thread_when_admission_is_denied() {
    let attempts = Arc::new(AtomicUsize::new(0));
    let started = Arc::new(AtomicUsize::new(0));
    let (attempts_, started_) = (attempts.clone(), started.clone());
    let sandbox = CodemodeSandbox::new(CodemodeSandboxOptions {
        acquire_worker: Some(Arc::new(move || {
            attempts_.fetch_add(1, Ordering::SeqCst);
            Err("worker capacity exhausted".to_string())
        })),
        on_worker_started: Some(Arc::new(move |_| {
            started_.fetch_add(1, Ordering::SeqCst);
            Ok(())
        })),
        ..Default::default()
    })
    .unwrap();
    assert_matches(
        &run(&sandbox, "while (true) {}").await,
        json!({ "ok": false, "error": { "kind": "sandbox", "message": "worker capacity exhausted" } }),
    );
    assert_eq!(attempts.load(Ordering::SeqCst), 1);
    assert_eq!(started.load(Ordering::SeqCst), 0);
    sandbox.close().await;
}

/// Abort, timeout, and close settle the result right away, cancel the pending tool call, and the
/// thread then exits on its own.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn settles_abort_timeout_and_close_without_waiting_for_thread_exit() {
    for how in ["abort", "timeout", "close"] {
        let exited: Arc<Mutex<Option<WorkerExited>>> = Arc::default();
        let tool_signal: Arc<Mutex<Option<CancellationToken>>> = Arc::default();
        let called = Arc::new(tokio::sync::Notify::new());
        let (exited_, signal_, called_) = (exited.clone(), tool_signal.clone(), called.clone());
        let sandbox = CodemodeSandbox::new(CodemodeSandboxOptions {
            // The deadline includes VM startup, which takes a few hundred ms in debug builds.
            timeout_ms: Some(if how == "timeout" { 1_500.0 } else { 10_000.0 }),
            on_worker_started: Some(Arc::new(move |promise| {
                *exited_.lock() = Some(promise);
                Ok(())
            })),
            tools: vec![CodemodeTool::new("hang", move |_, context| {
                *signal_.lock() = Some(context.signal.clone());
                called_.notify_one();
                std::future::pending()
            })],
            ..Default::default()
        })
        .unwrap();
        let controller = AbortController::new();
        let signal = controller.signal();
        let pending = {
            let sandbox = sandbox.clone();
            tokio::spawn(async move {
                run_with(&sandbox, "text(\"partial\"); await tools.hang()", CodemodeExecuteOptions { signal: Some(signal), ..Default::default() })
                    .await
            })
        };
        tokio::time::timeout(Duration::from_secs(10), called.notified()).await.expect("tool called");
        match how {
            "abort" => controller.abort_with_reason("cancelled"),
            "close" => sandbox.close().await,
            _ => {}
        }
        let result = tokio::time::timeout(Duration::from_secs(5), pending).await.expect("settled").unwrap();
        assert_matches(
            &result,
            json!({
                "ok": false,
                "error": { "kind": if how == "timeout" { "timeout" } else { "aborted" } },
                "output": [{ "type": "text", "text": "partial" }],
                "calls": [{ "name": "hang", "status": "cancelled" }],
            }),
        );
        assert!(tool_signal.lock().as_ref().unwrap().is_cancelled());
        sandbox.close().await;
        let exited = exited.lock().clone().expect("thread started");
        tokio::time::timeout(Duration::from_secs(5), exited).await.expect("thread exited");
    }
}

// #10748: exit accounting applies to successful executions and host-controlled runs too.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn reports_the_actual_exit_of_execute_and_start_threads() {
    let exits: Arc<Mutex<Vec<WorkerExited>>> = Arc::default();
    let (acquired, released) = (Arc::new(AtomicUsize::new(0)), Arc::new(AtomicUsize::new(0)));
    let exits_ = exits.clone();
    let sandbox = CodemodeSandbox::new(CodemodeSandboxOptions {
        acquire_worker: Some(counting_acquire(acquired.clone(), released.clone())),
        on_worker_started: Some(Arc::new(move |exited| {
            exits_.lock().push(exited);
            Ok(())
        })),
        ..Default::default()
    })
    .unwrap();
    assert_matches(&run(&sandbox, "return 1").await, json!({ "ok": true, "value": 1 }));
    let run = sandbox.start("return 2", CodemodeRunOptions::new(0.0, 0, |_, _| {})).unwrap();
    let handle = run.handle();
    assert_matches(&run.result().await, json!({ "ok": true, "value": 2 }));
    assert!(handle.finished());
    let exits: Vec<WorkerExited> = exits.lock().clone();
    assert_eq!(exits.len(), 2);
    tokio::time::timeout(Duration::from_secs(5), futures::future::join_all(exits)).await.expect("exited");
    assert_eq!(acquired.load(Ordering::SeqCst), 2);
    assert_eq!(released.load(Ordering::SeqCst), 2);
    sandbox.close().await;
}

// #10748: a failing lifetime hook must not leak the thread it was handed.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn stops_the_thread_when_on_worker_started_fails() {
    let exited: Arc<Mutex<Option<WorkerExited>>> = Arc::default();
    let (acquired, released) = (Arc::new(AtomicUsize::new(0)), Arc::new(AtomicUsize::new(0)));
    let exited_ = exited.clone();
    let sandbox = CodemodeSandbox::new(CodemodeSandboxOptions {
        acquire_worker: Some(counting_acquire(acquired.clone(), released.clone())),
        on_worker_started: Some(Arc::new(move |promise| {
            *exited_.lock() = Some(promise);
            Err("lifetime hook failed".to_string())
        })),
        ..Default::default()
    })
    .unwrap();
    assert_matches(
        &run(&sandbox, "while (true) {}").await,
        json!({ "ok": false, "error": { "kind": "sandbox", "message": "lifetime hook failed" } }),
    );
    let exited = exited.lock().clone().expect("hook called");
    tokio::time::timeout(Duration::from_secs(5), exited).await.expect("exited");
    assert_eq!(released.load(Ordering::SeqCst), 1);
    sandbox.close().await;
}

/// Dropping the `execute()` future aborts the execution and stops its thread.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn dropping_the_execute_future_stops_the_thread() {
    let exited: Arc<Mutex<Option<WorkerExited>>> = Arc::default();
    let exited_ = exited.clone();
    let sandbox = CodemodeSandbox::new(CodemodeSandboxOptions {
        on_worker_started: Some(Arc::new(move |promise| {
            *exited_.lock() = Some(promise);
            Ok(())
        })),
        ..Default::default()
    })
    .unwrap();
    let outcome = tokio::time::timeout(Duration::from_millis(300), run(&sandbox, "while (true) {}")).await;
    assert!(outcome.is_err());
    let exited = exited.lock().clone().expect("thread started");
    tokio::time::timeout(Duration::from_secs(5), exited).await.expect("exited");
}
