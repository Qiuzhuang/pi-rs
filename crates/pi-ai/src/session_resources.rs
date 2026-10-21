//! Session resource cleanup registry (port of `session-resources.ts`).

use std::sync::Arc;

use once_cell::sync::Lazy;
use parking_lot::Mutex;

pub type SessionResourceCleanup = Arc<dyn Fn(Option<&str>) -> anyhow::Result<()> + Send + Sync>;

static CLEANUPS: Lazy<Mutex<Vec<(u64, SessionResourceCleanup)>>> = Lazy::new(|| Mutex::new(Vec::new()));
static NEXT_ID: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);

/// Register a cleanup; returns an unregister function.
pub fn register_session_resource_cleanup(cleanup: SessionResourceCleanup) -> impl FnOnce() + Send + Sync {
    let id = NEXT_ID.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    CLEANUPS.lock().push((id, cleanup));
    move || {
        CLEANUPS.lock().retain(|(entry_id, _)| *entry_id != id);
    }
}

/// Run every registered cleanup; errors are aggregated.
pub fn cleanup_session_resources(session_id: Option<&str>) -> anyhow::Result<()> {
    let cleanups: Vec<SessionResourceCleanup> = CLEANUPS.lock().iter().map(|(_, c)| c.clone()).collect();
    let errors: Vec<String> = cleanups.iter().filter_map(|c| c(session_id).err().map(|e| e.to_string())).collect();
    if !errors.is_empty() {
        anyhow::bail!("Failed to cleanup session resources: {}", errors.join("; "));
    }
    Ok(())
}
