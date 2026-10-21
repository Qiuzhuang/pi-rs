//! Lock directories compatible with the `proper-lockfile` npm package.
//!
//! The TS code locks files with `lockfile.lockSync(path, { realpath: false })`, which creates the
//! directory `<path>.lock` (or `lockfilePath`) with `mkdir`. A lock whose directory mtime is older
//! than the stale timeout (10 s) is considered abandoned and taken over. Using the same directory
//! keeps Rust and TS processes mutually exclusive.

use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

/// Default `stale` option of proper-lockfile, in milliseconds.
pub const DEFAULT_STALE_MS: u64 = 10_000;

#[derive(Debug, thiserror::Error)]
pub enum LockError {
    /// `ELOCKED`: another process holds the lock.
    #[error("Lock file is already being held")]
    Locked,
    #[error(transparent)]
    Io(#[from] std::io::Error),
}

impl LockError {
    /// The proper-lockfile error code (`ELOCKED` or the I/O error kind).
    pub fn code(&self) -> String {
        match self {
            LockError::Locked => "ELOCKED".to_string(),
            LockError::Io(e) => format!("{:?}", e.kind()),
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct LockOptions {
    /// Lock directory path; defaults to `<file>.lock`.
    pub lockfile_path: Option<PathBuf>,
    /// Stale timeout in milliseconds; defaults to [`DEFAULT_STALE_MS`].
    pub stale_ms: Option<u64>,
}

/// A held lock. Dropping it releases the lock (removes the lock directory).
#[derive(Debug)]
pub struct LockGuard {
    path: PathBuf,
    released: bool,
}

impl LockGuard {
    /// Release the lock explicitly.
    pub fn release(mut self) {
        self.release_inner();
    }

    fn release_inner(&mut self) {
        if !self.released {
            self.released = true;
            let _ = std::fs::remove_dir(&self.path);
        }
    }

    /// Path of the lock directory.
    pub fn lock_path(&self) -> &Path {
        &self.path
    }
}

impl Drop for LockGuard {
    fn drop(&mut self) {
        self.release_inner();
    }
}

fn is_stale(lock_path: &Path, stale_ms: u64) -> bool {
    let Ok(meta) = std::fs::metadata(lock_path) else {
        return false;
    };
    let Ok(mtime) = meta.modified() else {
        return false;
    };
    SystemTime::now()
        .duration_since(mtime)
        .map(|age| age > Duration::from_millis(stale_ms))
        .unwrap_or(false)
}

/// Acquire the lock once (proper-lockfile `lockSync` with `retries: 0`).
pub fn lock_sync(file: impl AsRef<Path>, options: &LockOptions) -> Result<LockGuard, LockError> {
    let lock_path = options.lockfile_path.clone().unwrap_or_else(|| {
        let mut p = file.as_ref().as_os_str().to_owned();
        p.push(".lock");
        PathBuf::from(p)
    });
    let stale_ms = options.stale_ms.unwrap_or(DEFAULT_STALE_MS);

    for attempt in 0..2 {
        match std::fs::create_dir(&lock_path) {
            Ok(()) => {
                return Ok(LockGuard {
                    path: lock_path,
                    released: false,
                });
            }
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
                if attempt == 0 && is_stale(&lock_path, stale_ms) {
                    // Abandoned lock: remove it and try again.
                    let _ = std::fs::remove_dir(&lock_path);
                    continue;
                }
                return Err(LockError::Locked);
            }
            Err(e) => return Err(LockError::Io(e)),
        }
    }
    Err(LockError::Locked)
}

/// Acquire the lock, retrying `ELOCKED` up to `max_attempts` times with a synchronous `delay`
/// between attempts (the retry loop used by the settings, trust and auth stores).
pub fn lock_sync_with_retry(
    file: impl AsRef<Path>,
    options: &LockOptions,
    max_attempts: u32,
    delay: Duration,
) -> Result<LockGuard, LockError> {
    let mut attempt = 1;
    loop {
        match lock_sync(file.as_ref(), options) {
            Ok(guard) => return Ok(guard),
            Err(LockError::Locked) if attempt < max_attempts => {
                attempt += 1;
                std::thread::sleep(delay);
            }
            Err(e) => return Err(e),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn creates_and_releases_lock_directory() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("settings.json");
        let guard = lock_sync(&file, &LockOptions::default()).unwrap();
        assert!(dir.path().join("settings.json.lock").is_dir());
        assert!(matches!(
            lock_sync(&file, &LockOptions::default()),
            Err(LockError::Locked)
        ));
        guard.release();
        assert!(!dir.path().join("settings.json.lock").exists());
        drop(lock_sync(&file, &LockOptions::default()).unwrap());
    }

    #[test]
    fn takes_over_stale_locks() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("trust.json");
        std::fs::create_dir(dir.path().join("trust.json.lock")).unwrap();
        let options = LockOptions {
            stale_ms: Some(0),
            ..Default::default()
        };
        std::thread::sleep(Duration::from_millis(5));
        assert!(lock_sync(&file, &options).is_ok());
    }
}
