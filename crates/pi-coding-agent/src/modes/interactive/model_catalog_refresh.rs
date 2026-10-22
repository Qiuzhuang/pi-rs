//! Port of `packages/coding-agent/src/modes/interactive/model-catalog-refresh.ts`.

use crate::core::model_runtime::ModelRuntime;
use crate::utils::abort::race_with_abort_signal;
use futures::FutureExt;
use futures::future::{BoxFuture, Shared};
use parking_lot::Mutex;
use pi_ai::models::{ModelsRefreshOptions, ModelsRefreshResult};
use std::collections::HashMap;
use std::sync::{Arc, LazyLock};
use tokio_util::sync::CancellationToken;

/// TS: `Pick<ModelRuntime, "refresh">`.
pub trait ModelCatalogRefresh: Send + Sync + 'static {
    fn refresh_catalogs(self: Arc<Self>, options: ModelsRefreshOptions) -> BoxFuture<'static, anyhow::Result<ModelsRefreshResult>>;
}

impl ModelCatalogRefresh for ModelRuntime {
    fn refresh_catalogs(self: Arc<Self>, options: ModelsRefreshOptions) -> BoxFuture<'static, anyhow::Result<ModelsRefreshResult>> {
        async move { self.refresh(Some(options)).await }.boxed()
    }
}

/// TS: Pick<ModelRuntime, "refresh">
pub type ModelCatalogRuntime = Arc<ModelRuntime>;

type SharedRefresh = Shared<BoxFuture<'static, Result<(bool, HashMap<String, String>), Arc<anyhow::Error>>>>;

#[derive(Clone)]
pub struct ActiveModelCatalogRefresh {
    pub controller: CancellationToken,
    pub promise: SharedRefresh,
    pub waiters: usize,
    id: u64,
}

/// Shares concurrent refreshes per runtime (keyed by the runtime's address, TS: a WeakMap).
pub struct ModelCatalogRefreshCoordinator {
    active_by_runtime: Arc<Mutex<HashMap<usize, ActiveModelCatalogRefresh>>>,
    next_id: std::sync::atomic::AtomicU64,
}

impl Default for ModelCatalogRefreshCoordinator {
    fn default() -> Self {
        Self::new()
    }
}

fn to_result(value: Result<(bool, HashMap<String, String>), Arc<anyhow::Error>>) -> anyhow::Result<ModelsRefreshResult> {
    match value {
        Ok((aborted, errors)) => Ok(ModelsRefreshResult { aborted, errors }),
        Err(error) => {
            if error.downcast_ref::<crate::utils::abort::OperationAbortedError>().is_some() {
                return Err(crate::utils::abort::OperationAbortedError.into());
            }
            Err(anyhow::anyhow!("{error:#}"))
        }
    }
}

impl ModelCatalogRefreshCoordinator {
    pub fn new() -> Self {
        Self { active_by_runtime: Arc::new(Mutex::new(HashMap::new())), next_id: std::sync::atomic::AtomicU64::new(1) }
    }

    pub async fn refresh<R: ModelCatalogRefresh>(
        &self,
        model_runtime: &Arc<R>,
        signal: CancellationToken,
    ) -> anyhow::Result<ModelsRefreshResult> {
        if signal.is_cancelled() {
            return Err(crate::utils::abort::OperationAbortedError.into());
        }
        let key = Arc::as_ptr(model_runtime) as *const () as usize;
        let (promise, id) = {
            let mut map = self.active_by_runtime.lock();
            if !map.contains_key(&key) {
                let controller = CancellationToken::new();
                let id = self.next_id.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                let operation = model_runtime
                    .clone()
                    .refresh_catalogs(ModelsRefreshOptions { signal: Some(controller.clone()), ..Default::default() });
                let active_map = self.active_by_runtime.clone();
                let raced_signal = controller.clone();
                let promise: SharedRefresh = async move {
                    let result = race_with_abort_signal(operation, Some(raced_signal)).await;
                    {
                        let mut map = active_map.lock();
                        if map.get(&key).is_some_and(|a| a.id == id) {
                            map.remove(&key);
                        }
                    }
                    result.map(|r| (r.aborted, r.errors)).map_err(Arc::new)
                }
                .boxed()
                .shared();
                map.insert(key, ActiveModelCatalogRefresh { controller, promise, waiters: 0, id });
            }
            let active = map.get_mut(&key).expect("inserted above");
            active.waiters += 1;
            (active.promise.clone(), active.id)
        };

        let result = race_with_abort_signal(async move { to_result(promise.await) }, Some(signal)).await;

        let mut map = self.active_by_runtime.lock();
        if let Some(active) = map.get_mut(&key)
            && active.id == id
        {
            active.waiters -= 1;
            if active.waiters == 0 {
                active.controller.cancel();
                // TS: the abandoned promise settles as aborted and its `finally` removes the entry. Rust
                // futures only run while polled and nobody awaits it any more, so remove it here.
                map.remove(&key);
            }
        }
        result
    }
}

static MODEL_CATALOG_REFRESH_COORDINATOR: LazyLock<ModelCatalogRefreshCoordinator> =
    LazyLock::new(ModelCatalogRefreshCoordinator::new);

/// Share concurrent interactive all-catalog refreshes while keeping each caller's cancellation independent.
pub async fn refresh_model_catalogs<R: ModelCatalogRefresh>(
    model_runtime: &Arc<R>,
    signal: CancellationToken,
) -> anyhow::Result<ModelsRefreshResult> {
    MODEL_CATALOG_REFRESH_COORDINATOR.refresh(model_runtime, signal).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use tokio::sync::watch;

    struct FakeRuntime {
        calls: AtomicUsize,
        signals: Mutex<Vec<CancellationToken>>,
        release: watch::Receiver<bool>,
        never: bool,
    }

    impl ModelCatalogRefresh for FakeRuntime {
        fn refresh_catalogs(self: Arc<Self>, options: ModelsRefreshOptions) -> BoxFuture<'static, anyhow::Result<ModelsRefreshResult>> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            if let Some(signal) = options.signal {
                self.signals.lock().push(signal);
            }
            let mut release = self.release.clone();
            let never = self.never;
            async move {
                if never {
                    futures::future::pending::<()>().await;
                }
                while !*release.borrow() {
                    if release.changed().await.is_err() {
                        break;
                    }
                }
                Ok(ModelsRefreshResult::default())
            }
            .boxed()
        }
    }

    fn runtime(never: bool) -> (Arc<FakeRuntime>, watch::Sender<bool>) {
        let (tx, rx) = watch::channel(false);
        (Arc::new(FakeRuntime { calls: AtomicUsize::new(0), signals: Mutex::new(Vec::new()), release: rx, never }), tx)
    }

    fn is_abort(error: &anyhow::Error) -> bool {
        error.downcast_ref::<crate::utils::abort::OperationAbortedError>().is_some()
    }

    #[tokio::test]
    async fn shares_one_runtime_refresh_between_concurrent_callers() {
        let coordinator = ModelCatalogRefreshCoordinator::new();
        let (rt, tx) = runtime(false);
        let first = coordinator.refresh(&rt, CancellationToken::new());
        let second = coordinator.refresh(&rt, CancellationToken::new());
        let release = async {
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
            assert_eq!(rt.calls.load(Ordering::SeqCst), 1);
            tx.send(true).unwrap();
        };
        let (a, b, _) = tokio::join!(first, second, release);
        assert!(!a.unwrap().aborted);
        assert!(!b.unwrap().aborted);
        assert_eq!(rt.calls.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn keeps_the_shared_refresh_alive_when_one_caller_stops_waiting() {
        let coordinator = ModelCatalogRefreshCoordinator::new();
        let (rt, tx) = runtime(false);
        let first_signal = CancellationToken::new();
        let first = coordinator.refresh(&rt, first_signal.clone());
        let second = coordinator.refresh(&rt, CancellationToken::new());
        let driver = async {
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
            first_signal.cancel();
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
            assert!(!rt.signals.lock()[0].is_cancelled());
            tx.send(true).unwrap();
        };
        let (a, b, _) = tokio::join!(first, second, driver);
        assert!(is_abort(&a.unwrap_err()));
        assert!(!b.unwrap().aborted);
    }

    #[tokio::test]
    async fn aborts_an_abandoned_refresh_and_allows_a_later_refresh() {
        let coordinator = ModelCatalogRefreshCoordinator::new();
        let (rt, _tx) = runtime(true);
        let first_signal = CancellationToken::new();
        let first = coordinator.refresh(&rt, first_signal.clone());
        let cancel = async {
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
            first_signal.cancel();
        };
        let (a, _) = tokio::join!(first, cancel);
        assert!(is_abort(&a.unwrap_err()));
        for _ in 0..100 {
            if rt.signals.lock()[0].is_cancelled() {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(5)).await;
        }
        assert!(rt.signals.lock()[0].is_cancelled());

        let second_signal = CancellationToken::new();
        let second = coordinator.refresh(&rt, second_signal.clone());
        let cancel = async {
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
            assert_eq!(rt.calls.load(Ordering::SeqCst), 2);
            second_signal.cancel();
        };
        let (b, _) = tokio::join!(second, cancel);
        assert!(is_abort(&b.unwrap_err()));
    }
}
