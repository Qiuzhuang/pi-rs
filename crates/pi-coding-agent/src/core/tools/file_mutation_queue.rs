//! Port of `packages/coding-agent/src/core/tools/file-mutation-queue.ts`.

use std::collections::HashMap;
use std::future::Future;
use std::sync::{Arc, LazyLock};

/// Per-file FIFO locks, keyed by the real path of the target file. Entries are removed when the
/// last queued operation for a file finishes (TS `fileMutationQueues`).
static FILE_MUTATION_QUEUES: LazyLock<parking_lot::Mutex<HashMap<String, Arc<tokio::sync::Mutex<()>>>>> =
    LazyLock::new(|| parking_lot::Mutex::new(HashMap::new()));

fn get_mutation_queue_key(file_path: &str) -> anyhow::Result<String> {
    let resolved_path = crate::core::tools::path_utils::node_path_resolve(&[file_path]);
    match std::fs::canonicalize(&resolved_path) {
        Ok(real) => Ok(real.to_string_lossy().into_owned()),
        Err(err) => {
            let (code, _) = crate::core::tools::node_fs::io_error_code(&err);
            if code == "ENOENT" || code == "ENOTDIR" {
                Ok(resolved_path)
            } else {
                Err(crate::core::tools::node_fs::node_fs_error(&err, "realpath", Some(&resolved_path)).into())
            }
        }
    }
}

/// Serialize file mutation operations targeting the same file.
/// Operations for different files still run in parallel.
pub async fn with_file_mutation_queue<T, F, Fut>(file_path: &str, f: F) -> anyhow::Result<T>
where
    F: FnOnce() -> Fut,
    Fut: Future<Output = anyhow::Result<T>>,
{
    let key = get_mutation_queue_key(file_path)?;
    let queue = {
        let mut queues = FILE_MUTATION_QUEUES.lock();
        queues
            .entry(key.clone())
            .or_insert_with(|| Arc::new(tokio::sync::Mutex::new(())))
            .clone()
    };

    struct Release {
        key: String,
        queue: Option<Arc<tokio::sync::Mutex<()>>>,
    }
    impl Drop for Release {
        fn drop(&mut self) {
            let queue = self.queue.take();
            let mut queues = FILE_MUTATION_QUEUES.lock();
            drop(queue);
            if let Some(current) = queues.get(&self.key)
                && Arc::strong_count(current) == 1
            {
                queues.remove(&self.key);
            }
        }
    }

    let release = Release {
        key,
        queue: Some(queue.clone()),
    };
    let result = {
        let _guard = queue.lock().await;
        f().await
    };
    drop(queue);
    drop(release);
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[tokio::test]
    async fn serializes_operations_for_the_same_file() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("a.txt").to_string_lossy().into_owned();
        let log = Arc::new(parking_lot::Mutex::new(Vec::<String>::new()));

        let mut handles = Vec::new();
        for i in 0..3 {
            let file = file.clone();
            let log = log.clone();
            handles.push(tokio::spawn(async move {
                with_file_mutation_queue(&file, || async {
                    log.lock().push(format!("start {i}"));
                    tokio::time::sleep(Duration::from_millis(20)).await;
                    log.lock().push(format!("end {i}"));
                    Ok(())
                })
                .await
            }));
            tokio::time::sleep(Duration::from_millis(2)).await;
        }
        for handle in handles {
            handle.await.unwrap().unwrap();
        }
        assert_eq!(
            *log.lock(),
            vec!["start 0", "end 0", "start 1", "end 1", "start 2", "end 2"]
        );
        assert!(!FILE_MUTATION_QUEUES.lock().contains_key(&file));
    }

    #[tokio::test]
    async fn different_files_run_in_parallel() {
        let dir = tempfile::tempdir().unwrap();
        let a = dir.path().join("a.txt").to_string_lossy().into_owned();
        let b = dir.path().join("b.txt").to_string_lossy().into_owned();
        let log = Arc::new(parking_lot::Mutex::new(Vec::<String>::new()));
        let la = log.clone();
        let lb = log.clone();
        let fa = with_file_mutation_queue(&a, || async move {
            la.lock().push("start a".into());
            tokio::time::sleep(Duration::from_millis(30)).await;
            la.lock().push("end a".into());
            Ok(())
        });
        let fb = with_file_mutation_queue(&b, || async move {
            lb.lock().push("start b".into());
            tokio::time::sleep(Duration::from_millis(5)).await;
            lb.lock().push("end b".into());
            Ok(())
        });
        let (ra, rb) = tokio::join!(fa, fb);
        ra.unwrap();
        rb.unwrap();
        assert_eq!(*log.lock(), vec!["start a", "start b", "end b", "end a"]);
    }

    #[tokio::test]
    async fn releases_the_queue_after_errors() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("err.txt").to_string_lossy().into_owned();
        let result: anyhow::Result<()> =
            with_file_mutation_queue(&file, || async { anyhow::bail!("boom") }).await;
        assert_eq!(result.unwrap_err().to_string(), "boom");
        let ok = with_file_mutation_queue(&file, || async { Ok(42) }).await.unwrap();
        assert_eq!(ok, 42);
    }
}
