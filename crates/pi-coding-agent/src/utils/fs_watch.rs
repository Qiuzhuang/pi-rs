//! Port of `packages/coding-agent/src/utils/fs-watch.ts`.
//!
//! Node's `fs.watch` is replaced by a small polling watcher (a background thread that
//! compares modification times and sizes every [`FS_WATCH_POLL_INTERVAL_MS`]). Watching a
//! directory reports changes of its direct entries with their file names; watching a file
//! reports changes of the file itself. Event types follow Node: `"rename"` when an entry
//! appears or disappears, `"change"` when it is modified.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, SystemTime};

pub const FS_WATCH_RETRY_DELAY_MS: i64 = 5000;

/// Poll interval of the watcher thread.
pub const FS_WATCH_POLL_INTERVAL_MS: u64 = 100;

/// Node `WatchEventType`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WatchEventType {
    Rename,
    Change,
}

impl WatchEventType {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Rename => "rename",
            Self::Change => "change",
        }
    }
}

/// `WatchListener<string>`: `(eventType, filename)`.
pub type WatchListener = Arc<dyn Fn(WatchEventType, Option<String>) + Send + Sync>;

/// A running watcher (`FSWatcher`). Cloning shares the same watcher.
#[derive(Clone)]
pub struct FsWatcher {
    closed: Arc<AtomicBool>,
}

impl FsWatcher {
    /// Stop watching. Idempotent.
    pub fn close(&self) {
        self.closed.store(true, Ordering::SeqCst);
    }

    pub fn is_closed(&self) -> bool {
        self.closed.load(Ordering::SeqCst)
    }
}

impl std::fmt::Debug for FsWatcher {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FsWatcher").field("closed", &self.is_closed()).finish()
    }
}

pub fn close_watcher(watcher: Option<&FsWatcher>) {
    if let Some(watcher) = watcher {
        watcher.close();
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
struct Stamp {
    modified: Option<SystemTime>,
    len: u64,
}

fn stamp(path: &Path) -> Option<Stamp> {
    let meta = std::fs::metadata(path).ok()?;
    Some(Stamp { modified: meta.modified().ok(), len: meta.len() })
}

enum Snapshot {
    File(Option<Stamp>),
    Dir(HashMap<String, Stamp>),
}

fn snapshot_dir(path: &Path) -> Option<HashMap<String, Stamp>> {
    let entries = std::fs::read_dir(path).ok()?;
    let mut map = HashMap::new();
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        if let Some(s) = stamp(&entry.path()) {
            map.insert(name, s);
        } else {
            map.insert(name, Stamp { modified: None, len: 0 });
        }
    }
    Some(map)
}

/// Start watching `path`. When the path cannot be watched (for example it does not
/// exist), `on_error` runs and `None` is returned. Errors after creation (the watched
/// directory disappearing) also call `on_error`, after which the watcher stops.
pub fn watch_with_error_handler(
    path: &str,
    listener: WatchListener,
    on_error: Arc<dyn Fn() + Send + Sync>,
) -> Option<FsWatcher> {
    let root = PathBuf::from(path);
    let meta = match std::fs::metadata(&root) {
        Ok(meta) => meta,
        Err(_) => {
            on_error();
            return None;
        }
    };
    let initial = if meta.is_dir() {
        match snapshot_dir(&root) {
            Some(map) => Snapshot::Dir(map),
            None => {
                on_error();
                return None;
            }
        }
    } else {
        Snapshot::File(stamp(&root))
    };
    let file_name = root.file_name().map(|n| n.to_string_lossy().into_owned());

    let watcher = FsWatcher { closed: Arc::new(AtomicBool::new(false)) };
    let closed = watcher.closed.clone();
    let spawned = std::thread::Builder::new().name("pi-fs-watch".into()).spawn(move || {
        let mut snapshot = initial;
        loop {
            std::thread::sleep(Duration::from_millis(FS_WATCH_POLL_INTERVAL_MS));
            if closed.load(Ordering::SeqCst) {
                return;
            }
            match &mut snapshot {
                Snapshot::File(previous) => {
                    let current = stamp(&root);
                    if current != *previous {
                        let kind = if current.is_some() && previous.is_some() {
                            WatchEventType::Change
                        } else {
                            WatchEventType::Rename
                        };
                        *previous = current;
                        if !closed.load(Ordering::SeqCst) {
                            listener(kind, file_name.clone());
                        }
                    }
                }
                Snapshot::Dir(previous) => {
                    let Some(current) = snapshot_dir(&root) else {
                        closed.store(true, Ordering::SeqCst);
                        on_error();
                        return;
                    };
                    let mut events: Vec<(WatchEventType, String)> = Vec::new();
                    for (name, s) in &current {
                        match previous.get(name) {
                            None => events.push((WatchEventType::Rename, name.clone())),
                            Some(old) if old != s => events.push((WatchEventType::Change, name.clone())),
                            _ => {}
                        }
                    }
                    for name in previous.keys() {
                        if !current.contains_key(name) {
                            events.push((WatchEventType::Rename, name.clone()));
                        }
                    }
                    *previous = current;
                    for (kind, name) in events {
                        if closed.load(Ordering::SeqCst) {
                            return;
                        }
                        listener(kind, Some(name));
                    }
                }
            }
        }
    });
    if spawned.is_err() {
        watcher.close();
        return None;
    }
    Some(watcher)
}

#[cfg(test)]
mod tests {
    use super::*;
    use parking_lot::Mutex;

    #[test]
    fn missing_path_calls_on_error() {
        let errors = Arc::new(Mutex::new(0));
        let e = errors.clone();
        let watcher = watch_with_error_handler(
            "/definitely/not/here/pi-watch",
            Arc::new(|_, _| {}),
            Arc::new(move || *e.lock() += 1),
        );
        assert!(watcher.is_none());
        assert_eq!(*errors.lock(), 1);
        close_watcher(None);
    }

    #[test]
    fn reports_directory_entry_changes() {
        let dir = std::env::temp_dir().join(format!("pi-fswatch-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let events: Arc<Mutex<Vec<(WatchEventType, Option<String>)>>> = Arc::default();
        let ev = events.clone();
        let watcher = watch_with_error_handler(
            dir.to_str().unwrap(),
            Arc::new(move |kind, name| ev.lock().push((kind, name))),
            Arc::new(|| {}),
        )
        .unwrap();
        std::fs::write(dir.join("a.json"), "1").unwrap();
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        while events.lock().is_empty() && std::time::Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(20));
        }
        assert_eq!(events.lock()[0], (WatchEventType::Rename, Some("a.json".to_string())));
        watcher.close();
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
