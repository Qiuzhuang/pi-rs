//! Port of `packages/coding-agent/test/file-mutation-queue.test.ts` (the symlink alias case and the
//! built-in edit/write tool cases; the plain queue cases live in `src/core/tools/file_mutation_queue.rs`).

use pi_agent::types::{AgentToolRef, AgentToolResult};
use pi_coding_agent::core::tools::edit::{EditOperations, EditToolOptions, create_edit_tool};
use pi_coding_agent::core::tools::with_file_mutation_queue;
use pi_coding_agent::core::tools::write::{WriteOperations, WriteToolOptions, create_write_tool};
use serde_json::{Value, json};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;
use tokio::sync::Notify;
use tokio_util::sync::CancellationToken;

fn delay(ms: u64) -> tokio::time::Sleep {
    tokio::time::sleep(Duration::from_millis(ms))
}

fn p(path: &std::path::Path) -> String {
    path.to_string_lossy().into_owned()
}

async fn execute(tool: &AgentToolRef, id: &str, params: Value, signal: CancellationToken) -> anyhow::Result<AgentToolResult> {
    tool.execute(id, params, signal, Arc::new(|_| {})).await
}

/// A one-shot latch like the TS `createDeferred()`.
#[derive(Clone, Default)]
struct Deferred {
    done: Arc<AtomicBool>,
    notify: Arc<Notify>,
}

impl Deferred {
    fn resolve(&self) {
        self.done.store(true, Ordering::SeqCst);
        self.notify.notify_waiters();
    }

    async fn wait(&self) {
        loop {
            let notified = self.notify.notified();
            if self.done.load(Ordering::SeqCst) {
                return;
            }
            notified.await;
        }
    }

    async fn resolves_within(&self, ms: u64) -> bool {
        tokio::time::timeout(Duration::from_millis(ms), self.wait()).await.is_ok()
    }
}

fn slow_edit_operations() -> EditOperations {
    EditOperations {
        access: Arc::new(|path| Box::pin(async move { Ok(std::fs::metadata(path).map(|_| ())?) })),
        read_file: Arc::new(|path| {
            Box::pin(async move {
                let buffer = tokio::fs::read(path).await?;
                delay(30).await;
                Ok(buffer)
            })
        }),
        write_file: Arc::new(|path, content| {
            Box::pin(async move {
                delay(30).await;
                Ok(tokio::fs::write(path, content).await?)
            })
        }),
    }
}

#[cfg(unix)]
#[tokio::test]
async fn uses_the_same_queue_for_symlink_aliases() {
    let dir = tempfile::tempdir().unwrap();
    let target_path = dir.path().join("target.txt");
    let symlink_path = dir.path().join("alias.txt");
    std::fs::write(&target_path, "hello\n").unwrap();
    std::os::unix::fs::symlink(&target_path, &symlink_path).unwrap();

    let order = Arc::new(parking_lot::Mutex::new(Vec::<&str>::new()));
    let (target, alias) = (p(&target_path), p(&symlink_path));
    let first = {
        let order = order.clone();
        with_file_mutation_queue(&target, move || async move {
            order.lock().push("target:start");
            delay(30).await;
            order.lock().push("target:end");
            Ok(())
        })
    };
    let second = {
        let order = order.clone();
        with_file_mutation_queue(&alias, move || async move {
            order.lock().push("alias:start");
            order.lock().push("alias:end");
            Ok(())
        })
    };
    let (a, b) = tokio::join!(first, second);
    a.unwrap();
    b.unwrap();
    assert_eq!(*order.lock(), vec!["target:start", "target:end", "alias:start", "alias:end"]);
}

#[tokio::test]
async fn preserves_both_parallel_edits_on_the_same_file() {
    let dir = tempfile::tempdir().unwrap();
    let file_path = dir.path().join("parallel-edit.txt");
    std::fs::write(&file_path, "alpha\nbeta\ngamma\n").unwrap();
    let edit_tool = create_edit_tool(
        &p(dir.path()),
        Some(&EditToolOptions { operations: Some(slow_edit_operations()) }),
    );
    let path = p(&file_path);
    let (a, b) = tokio::join!(
        execute(&edit_tool, "call-1", json!({ "path": path, "edits": [{ "oldText": "alpha", "newText": "ALPHA" }] }), CancellationToken::new()),
        execute(&edit_tool, "call-2", json!({ "path": path, "edits": [{ "oldText": "beta", "newText": "BETA" }] }), CancellationToken::new()),
    );
    a.unwrap();
    b.unwrap();
    assert_eq!(std::fs::read_to_string(&file_path).unwrap(), "ALPHA\nBETA\ngamma\n");
}

#[tokio::test]
async fn shares_the_queue_between_edit_and_write() {
    let dir = tempfile::tempdir().unwrap();
    let file_path = dir.path().join("mixed.txt");
    std::fs::write(&file_path, "original\n").unwrap();
    let edit_tool = create_edit_tool(
        &p(dir.path()),
        Some(&EditToolOptions { operations: Some(slow_edit_operations()) }),
    );
    let write_tool = create_write_tool(
        &p(dir.path()),
        Some(&WriteToolOptions {
            operations: Some(WriteOperations {
                mkdir: Arc::new(|_| Box::pin(async { Ok(()) })),
                write_file: Arc::new(|path, content| {
                    Box::pin(async move {
                        delay(10).await;
                        Ok(tokio::fs::write(path, content).await?)
                    })
                }),
            }),
        }),
    );
    let path = p(&file_path);
    let edit = execute(
        &edit_tool,
        "call-1",
        json!({ "path": path, "edits": [{ "oldText": "original", "newText": "edited" }] }),
        CancellationToken::new(),
    );
    let write = async {
        delay(5).await;
        execute(&write_tool, "call-2", json!({ "path": path, "content": "replacement\n" }), CancellationToken::new()).await
    };
    let (a, b) = tokio::join!(edit, write);
    a.unwrap();
    b.unwrap();
    assert_eq!(std::fs::read_to_string(&file_path).unwrap(), "replacement\n");
}

#[tokio::test]
async fn keeps_write_queue_locked_while_an_aborted_write_is_still_in_flight() {
    let dir = tempfile::tempdir().unwrap();
    let file_path = dir.path().join("abort-write.txt");
    let first_write_started = Deferred::default();
    let finish_first_write = Deferred::default();
    let second_write_started = Deferred::default();
    let first_write_settled = Arc::new(AtomicBool::new(false));

    let write_file: Arc<dyn Fn(String, String) -> futures::future::BoxFuture<'static, anyhow::Result<()>> + Send + Sync> = {
        let (started, finish, second, settled) = (
            first_write_started.clone(),
            finish_first_write.clone(),
            second_write_started.clone(),
            first_write_settled.clone(),
        );
        Arc::new(move |path, content| {
            let (started, finish, second, settled) = (started.clone(), finish.clone(), second.clone(), settled.clone());
            Box::pin(async move {
                if content == "first\n" {
                    started.resolve();
                    finish.wait().await;
                    tokio::fs::write(&path, &content).await?;
                    settled.store(true, Ordering::SeqCst);
                    return Ok(());
                }
                if content == "second\n" {
                    assert!(settled.load(Ordering::SeqCst));
                    second.resolve();
                }
                Ok(tokio::fs::write(&path, &content).await?)
            })
        })
    };
    let write_tool = Arc::new(create_write_tool(
        &p(dir.path()),
        Some(&WriteToolOptions {
            operations: Some(WriteOperations { mkdir: Arc::new(|_| Box::pin(async { Ok(()) })), write_file }),
        }),
    ));

    let path = p(&file_path);
    let controller = CancellationToken::new();
    let first_write = tokio::spawn({
        let (tool, path, signal) = (write_tool.clone(), path.clone(), controller.clone());
        async move { execute(&tool, "call-1", json!({ "path": path, "content": "first\n" }), signal).await }
    });
    first_write_started.wait().await;
    controller.cancel();

    let second_write = tokio::spawn({
        let (tool, path) = (write_tool.clone(), path.clone());
        async move { execute(&tool, "call-2", json!({ "path": path, "content": "second\n" }), CancellationToken::new()).await }
    });
    assert!(!second_write_started.resolves_within(20).await);

    finish_first_write.resolve();
    let err = first_write.await.unwrap().unwrap_err();
    assert!(err.to_string().contains("Operation aborted"), "{err}");
    second_write.await.unwrap().unwrap();
    assert_eq!(std::fs::read_to_string(&file_path).unwrap(), "second\n");
}

#[tokio::test]
async fn keeps_edit_queue_locked_while_an_aborted_edit_write_is_still_in_flight() {
    let dir = tempfile::tempdir().unwrap();
    let file_path = dir.path().join("abort-edit.txt");
    std::fs::write(&file_path, "alpha\nbeta\n").unwrap();
    let first_write_started = Deferred::default();
    let finish_first_write = Deferred::default();
    let second_write_started = Deferred::default();
    let first_write_settled = Arc::new(AtomicBool::new(false));

    let write_file: Arc<dyn Fn(String, String) -> futures::future::BoxFuture<'static, anyhow::Result<()>> + Send + Sync> = {
        let (started, finish, second, settled) = (
            first_write_started.clone(),
            finish_first_write.clone(),
            second_write_started.clone(),
            first_write_settled.clone(),
        );
        Arc::new(move |path, content| {
            let (started, finish, second, settled) = (started.clone(), finish.clone(), second.clone(), settled.clone());
            Box::pin(async move {
                if content == "ALPHA\nbeta\n" {
                    started.resolve();
                    finish.wait().await;
                    tokio::fs::write(&path, &content).await?;
                    settled.store(true, Ordering::SeqCst);
                    return Ok(());
                }
                if content == "ALPHA\nBETA\n" || content == "alpha\nBETA\n" {
                    assert!(settled.load(Ordering::SeqCst));
                    second.resolve();
                }
                Ok(tokio::fs::write(&path, &content).await?)
            })
        })
    };
    let edit_tool = Arc::new(create_edit_tool(
        &p(dir.path()),
        Some(&EditToolOptions {
            operations: Some(EditOperations {
                access: Arc::new(|path| Box::pin(async move { Ok(std::fs::metadata(path).map(|_| ())?) })),
                read_file: Arc::new(|path| Box::pin(async move { Ok(tokio::fs::read(path).await?) })),
                write_file,
            }),
        }),
    ));

    let path = p(&file_path);
    let controller = CancellationToken::new();
    let first_edit = tokio::spawn({
        let (tool, path, signal) = (edit_tool.clone(), path.clone(), controller.clone());
        async move {
            execute(&tool, "call-1", json!({ "path": path, "edits": [{ "oldText": "alpha", "newText": "ALPHA" }] }), signal).await
        }
    });
    first_write_started.wait().await;
    controller.cancel();

    let second_edit = tokio::spawn({
        let (tool, path) = (edit_tool.clone(), path.clone());
        async move {
            execute(
                &tool,
                "call-2",
                json!({ "path": path, "edits": [{ "oldText": "beta", "newText": "BETA" }] }),
                CancellationToken::new(),
            )
            .await
        }
    });
    assert!(!second_write_started.resolves_within(20).await);

    finish_first_write.resolve();
    let err = first_edit.await.unwrap().unwrap_err();
    assert!(err.to_string().contains("Operation aborted"), "{err}");
    second_edit.await.unwrap().unwrap();
    assert_eq!(std::fs::read_to_string(&file_path).unwrap(), "ALPHA\nBETA\n");
}
