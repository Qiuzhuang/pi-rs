//! Port of `packages/coding-agent/src/core/footer-data-provider.ts`.
//!
//! Git HEAD (and reftable) changes are detected by a background polling thread instead of
//! `fs.watch`: it samples the metadata of the watched files every [`POLL_INTERVAL`] and refreshes
//! the branch after the same 500 ms debounce the TS watcher uses.

use crate::utils::paths::{node_dirname, node_join, node_resolve};
use indexmap::IndexMap;
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Weak};
use std::time::{Duration, Instant, SystemTime};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GitPaths {
    pub repo_dir: String,
    pub common_git_dir: String,
    pub head_path: String,
}

/// Find git metadata paths by walking up from cwd.
/// Handles both regular git repos (.git is a directory) and worktrees (.git is a file).
pub fn find_git_paths(cwd: &str) -> Option<GitPaths> {
    let mut dir = cwd.to_string();
    loop {
        let git_path = node_join(&[&dir, ".git"]);
        if Path::new(&git_path).exists() {
            let stat = std::fs::metadata(&git_path).ok()?;
            if stat.is_file() {
                let content = std::fs::read_to_string(&git_path).ok()?;
                let content = content.trim();
                if let Some(rest) = content.strip_prefix("gitdir: ") {
                    let git_dir = node_resolve(&[&dir, rest.trim()]);
                    let head_path = node_join(&[&git_dir, "HEAD"]);
                    if !Path::new(&head_path).exists() {
                        return None;
                    }
                    let common_dir_path = node_join(&[&git_dir, "commondir"]);
                    let common_git_dir = if Path::new(&common_dir_path).exists() {
                        let common = std::fs::read_to_string(&common_dir_path).ok()?;
                        node_resolve(&[&git_dir, common.trim()])
                    } else {
                        git_dir
                    };
                    return Some(GitPaths {
                        repo_dir: dir,
                        common_git_dir,
                        head_path,
                    });
                }
            } else if stat.is_dir() {
                let head_path = node_join(&[&git_path, "HEAD"]);
                if !Path::new(&head_path).exists() {
                    return None;
                }
                return Some(GitPaths {
                    repo_dir: dir,
                    common_git_dir: git_path,
                    head_path,
                });
            }
        }
        let parent = node_dirname(&dir);
        if parent == dir {
            return None;
        }
        dir = parent;
    }
}

/// Ask git for the current branch. Returns None on detached HEAD or if git is unavailable.
fn resolve_branch_with_git_sync(repo_dir: &str) -> Option<String> {
    let output = std::process::Command::new("git")
        .args([
            "--no-optional-locks",
            "symbolic-ref",
            "--quiet",
            "--short",
            "HEAD",
        ])
        .current_dir(repo_dir)
        .stdin(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let branch = String::from_utf8_lossy(&output.stdout).trim().to_string();
    (!branch.is_empty()).then_some(branch)
}

fn resolve_git_branch(git_paths: Option<&GitPaths>) -> Option<String> {
    let git_paths = git_paths?;
    let content = std::fs::read_to_string(&git_paths.head_path).ok()?;
    let content = content.trim();
    if let Some(branch) = content.strip_prefix("ref: refs/heads/") {
        return Some(if branch == ".invalid" {
            resolve_branch_with_git_sync(&git_paths.repo_dir).unwrap_or_else(|| "detached".into())
        } else {
            branch.to_string()
        });
    }
    Some("detached".to_string())
}

/// Debounce between a detected git change and the branch refresh.
const WATCH_DEBOUNCE: Duration = Duration::from_millis(500);
/// How often the watcher samples the git metadata files.
const POLL_INTERVAL: Duration = Duration::from_millis(100);

type Callback = Arc<dyn Fn() + Send + Sync>;

struct Inner {
    cwd: String,
    extension_statuses: IndexMap<String, String>,
    /// `None` = not resolved yet (`undefined` in TS).
    cached_branch: Option<Option<String>>,
    git_paths: Option<GitPaths>,
    branch_change_callbacks: Vec<(u64, Callback)>,
    available_provider_count: i64,
    disposed: bool,
    /// Incremented whenever the watcher must stop (cwd change, dispose).
    watcher_generation: u64,
}

/// Provides git branch and extension statuses - data not otherwise accessible to extensions.
/// Context usage on ctx.getContextUsage(), token stats on ctx.sessionManager.getEntries(), model info on ctx.model.
pub struct FooterDataProvider {
    inner: Arc<Mutex<Inner>>,
    next_callback_id: Arc<AtomicU64>,
}

impl FooterDataProvider {
    pub fn new(cwd: &str) -> Self {
        let provider = Self {
            inner: Arc::new(Mutex::new(Inner {
                cwd: cwd.to_string(),
                extension_statuses: IndexMap::new(),
                cached_branch: None,
                git_paths: find_git_paths(cwd),
                branch_change_callbacks: Vec::new(),
                available_provider_count: 0,
                disposed: false,
                watcher_generation: 0,
            })),
            next_callback_id: Arc::new(AtomicU64::new(1)),
        };
        provider.setup_git_watcher();
        provider
    }

    /// Current git branch, null if not in repo, "detached" if detached HEAD
    pub fn get_git_branch(&self) -> Option<String> {
        let git_paths = {
            let inner = self.inner.lock();
            if let Some(cached) = &inner.cached_branch {
                return cached.clone();
            }
            inner.git_paths.clone()
        };
        let branch = resolve_git_branch(git_paths.as_ref());
        let mut inner = self.inner.lock();
        if inner.cached_branch.is_none() {
            inner.cached_branch = Some(branch.clone());
        }
        inner.cached_branch.clone().unwrap_or(branch)
    }

    /// Extension status texts set via ctx.ui.setStatus()
    pub fn get_extension_statuses(&self) -> IndexMap<String, String> {
        self.inner.lock().extension_statuses.clone()
    }

    /// Subscribe to git branch changes. Returns unsubscribe function.
    pub fn on_branch_change(
        &self,
        callback: Arc<dyn Fn() + Send + Sync>,
    ) -> Arc<dyn Fn() + Send + Sync> {
        let id = self.next_callback_id.fetch_add(1, Ordering::SeqCst);
        self.inner.lock().branch_change_callbacks.push((id, callback));
        let weak: Weak<Mutex<Inner>> = Arc::downgrade(&self.inner);
        Arc::new(move || {
            if let Some(inner) = weak.upgrade() {
                inner
                    .lock()
                    .branch_change_callbacks
                    .retain(|(cb_id, _)| *cb_id != id);
            }
        })
    }

    /// Internal: set extension status
    pub fn set_extension_status(&self, key: &str, text: Option<&str>) {
        let mut inner = self.inner.lock();
        match text {
            None => {
                inner.extension_statuses.shift_remove(key);
            }
            Some(text) => {
                inner
                    .extension_statuses
                    .insert(key.to_string(), text.to_string());
            }
        }
    }

    /// Internal: clear extension statuses
    pub fn clear_extension_statuses(&self) {
        self.inner.lock().extension_statuses.clear();
    }

    /// Number of unique providers with available models (for footer display)
    pub fn get_available_provider_count(&self) -> i64 {
        self.inner.lock().available_provider_count
    }

    /// Internal: update available provider count
    pub fn set_available_provider_count(&self, count: i64) {
        self.inner.lock().available_provider_count = count;
    }

    pub fn set_cwd(&self, cwd: &str) {
        {
            let mut inner = self.inner.lock();
            if inner.cwd == cwd {
                return;
            }
            inner.cwd = cwd.to_string();
            inner.watcher_generation += 1;
            inner.cached_branch = None;
            inner.git_paths = find_git_paths(cwd);
        }
        self.setup_git_watcher();
        notify_branch_change(&self.inner);
    }

    /// Internal: cleanup
    pub fn dispose(&self) {
        let mut inner = self.inner.lock();
        inner.disposed = true;
        inner.watcher_generation += 1;
        inner.branch_change_callbacks.clear();
    }

    fn setup_git_watcher(&self) {
        let (generation, git_paths) = {
            let inner = self.inner.lock();
            if inner.disposed {
                return;
            }
            (inner.watcher_generation, inner.git_paths.clone())
        };
        let Some(git_paths) = git_paths else {
            return;
        };
        let weak = Arc::downgrade(&self.inner);
        let _ = std::thread::Builder::new()
            .name("pi-git-head-watch".to_string())
            .spawn(move || watch_git(weak, generation, git_paths));
    }
}

impl Drop for FooterDataProvider {
    fn drop(&mut self) {
        let mut inner = self.inner.lock();
        inner.watcher_generation += 1;
    }
}

fn notify_branch_change(inner: &Arc<Mutex<Inner>>) {
    let callbacks: Vec<Callback> = inner
        .lock()
        .branch_change_callbacks
        .iter()
        .map(|(_, cb)| cb.clone())
        .collect();
    for callback in callbacks {
        callback();
    }
}

type FileSignature = Option<(Option<SystemTime>, u64, u64)>;

fn file_signature(path: &str) -> FileSignature {
    let meta = std::fs::metadata(path).ok()?;
    #[cfg(unix)]
    let ino = {
        use std::os::unix::fs::MetadataExt;
        meta.ino() ^ (meta.ctime_nsec() as u64) ^ ((meta.ctime() as u64) << 20)
    };
    #[cfg(not(unix))]
    let ino = 0;
    Some((meta.modified().ok(), meta.len(), ino))
}

fn dir_signature(path: &str) -> Vec<(String, FileSignature)> {
    let mut entries: Vec<(String, FileSignature)> = std::fs::read_dir(path)
        .map(|rd| {
            rd.filter_map(Result::ok)
                .map(|e| {
                    let name = e.file_name().to_string_lossy().into_owned();
                    let sig = file_signature(&node_join(&[path, &name]));
                    (name, sig)
                })
                .collect()
        })
        .unwrap_or_default();
    entries.sort_by(|a, b| a.0.cmp(&b.0));
    entries
}

/// Snapshot of everything the TS watchers observe: the directory holding HEAD (only HEAD changes
/// matter), and the reftable directory when present.
fn git_snapshot(git_paths: &GitPaths, reftable_dir: Option<&str>) -> Vec<(String, FileSignature)> {
    let mut snapshot = vec![("HEAD".to_string(), file_signature(&git_paths.head_path))];
    if let Some(reftable_dir) = reftable_dir {
        snapshot.extend(
            dir_signature(reftable_dir)
                .into_iter()
                .map(|(name, sig)| (format!("reftable/{name}"), sig)),
        );
    }
    snapshot
}

fn watch_git(inner: Weak<Mutex<Inner>>, generation: u64, git_paths: GitPaths) {
    let reftable_dir = node_join(&[&git_paths.common_git_dir, "reftable"]);
    let reftable_dir = Path::new(&reftable_dir)
        .exists()
        .then_some(reftable_dir);
    let mut last = git_snapshot(&git_paths, reftable_dir.as_deref());
    let mut pending_since: Option<Instant> = None;

    loop {
        std::thread::sleep(POLL_INTERVAL);
        let Some(strong) = inner.upgrade() else {
            return;
        };
        {
            let state = strong.lock();
            if state.disposed || state.watcher_generation != generation {
                return;
            }
        }

        let current = git_snapshot(&git_paths, reftable_dir.as_deref());
        if current != last {
            last = current;
            if pending_since.is_none() {
                pending_since = Some(Instant::now());
            }
        }

        if let Some(since) = pending_since
            && since.elapsed() >= WATCH_DEBOUNCE
        {
            pending_since = None;
            let next_branch = resolve_git_branch(Some(&git_paths));
            let changed = {
                let mut state = strong.lock();
                if state.disposed || state.watcher_generation != generation {
                    return;
                }
                let changed = matches!(&state.cached_branch, Some(prev) if *prev != next_branch);
                state.cached_branch = Some(next_branch);
                changed
            };
            if changed {
                notify_branch_change(&strong);
            }
        }
    }
}

/// Read-only view for extensions - excludes setExtensionStatus, setAvailableProviderCount and dispose
/// TS: Pick< FooterDataProvider, "getGitBranch" | "getExtensionStatuses" | "getAvailableProviderCount" | "onBranchChange" >
pub type ReadonlyFooterDataProvider = Arc<FooterDataProvider>;

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicUsize;

    fn create_plain_repo(temp: &Path) -> std::path::PathBuf {
        let repo = temp.join("repo");
        std::fs::create_dir_all(repo.join(".git")).unwrap();
        std::fs::write(repo.join(".git").join("HEAD"), "ref: refs/heads/main\n").unwrap();
        repo
    }

    fn wait_for(condition: impl Fn() -> bool) {
        let start = Instant::now();
        while !condition() {
            assert!(
                start.elapsed() < Duration::from_secs(5),
                "timed out waiting for condition"
            );
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    #[test]
    fn uses_head_directly_from_nested_directory() {
        let temp = tempfile::tempdir().unwrap();
        let repo = create_plain_repo(temp.path());
        let nested = repo.join("src").join("nested");
        std::fs::create_dir_all(&nested).unwrap();
        let provider = FooterDataProvider::new(nested.to_str().unwrap());
        assert_eq!(provider.get_git_branch().as_deref(), Some("main"));
        provider.dispose();
    }

    #[test]
    fn reports_detached_and_no_repo() {
        let temp = tempfile::tempdir().unwrap();
        let repo = create_plain_repo(temp.path());
        std::fs::write(repo.join(".git").join("HEAD"), "0123456789abcdef\n").unwrap();
        let provider = FooterDataProvider::new(repo.to_str().unwrap());
        assert_eq!(provider.get_git_branch().as_deref(), Some("detached"));

        let outside = temp.path().join("outside");
        std::fs::create_dir_all(&outside).unwrap();
        if find_git_paths(outside.to_str().unwrap()).is_none() {
            let provider = FooterDataProvider::new(outside.to_str().unwrap());
            assert_eq!(provider.get_git_branch(), None);
        }
    }

    #[test]
    fn finds_worktree_git_paths() {
        let temp = tempfile::tempdir().unwrap();
        let common = temp.path().join("repo").join(".git");
        let git_dir = common.join("worktrees").join("src");
        let worktree = temp.path().join("worktree");
        std::fs::create_dir_all(&git_dir).unwrap();
        std::fs::create_dir_all(&worktree).unwrap();
        std::fs::write(
            worktree.join(".git"),
            format!("gitdir: {}\n", git_dir.display()),
        )
        .unwrap();
        std::fs::write(git_dir.join("HEAD"), "ref: refs/heads/feature\n").unwrap();
        std::fs::write(git_dir.join("commondir"), "../..\n").unwrap();

        let paths = find_git_paths(worktree.to_str().unwrap()).unwrap();
        assert_eq!(paths.repo_dir, worktree.to_str().unwrap());
        assert_eq!(paths.common_git_dir, common.to_str().unwrap());
        assert_eq!(
            paths.head_path,
            git_dir.join("HEAD").to_str().unwrap().to_string()
        );
        let provider = FooterDataProvider::new(worktree.to_str().unwrap());
        assert_eq!(provider.get_git_branch().as_deref(), Some("feature"));
    }

    #[test]
    fn notifies_when_head_changes() {
        let temp = tempfile::tempdir().unwrap();
        let repo = create_plain_repo(temp.path());
        let provider = FooterDataProvider::new(repo.to_str().unwrap());
        assert_eq!(provider.get_git_branch().as_deref(), Some("main"));
        let calls = Arc::new(AtomicUsize::new(0));
        let calls_clone = calls.clone();
        let unsubscribe = provider.on_branch_change(Arc::new(move || {
            calls_clone.fetch_add(1, Ordering::SeqCst);
        }));

        std::thread::sleep(Duration::from_millis(50));
        std::fs::write(repo.join(".git").join("HEAD"), "ref: refs/heads/feature-x\n").unwrap();
        wait_for(|| calls.load(Ordering::SeqCst) == 1);
        assert_eq!(provider.get_git_branch().as_deref(), Some("feature-x"));

        unsubscribe();
        std::fs::write(repo.join(".git").join("HEAD"), "ref: refs/heads/other\n").unwrap();
        wait_for(|| provider.get_git_branch().as_deref() == Some("other"));
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        provider.dispose();
    }

    #[test]
    fn manages_extension_statuses_and_provider_count() {
        let provider = FooterDataProvider::new("/");
        provider.set_extension_status("b", Some("two"));
        provider.set_extension_status("a", Some("one"));
        assert_eq!(
            provider
                .get_extension_statuses()
                .into_iter()
                .collect::<Vec<_>>(),
            vec![("b".into(), "two".into()), ("a".into(), "one".into())]
        );
        provider.set_extension_status("b", None);
        assert_eq!(provider.get_extension_statuses().len(), 1);
        provider.clear_extension_statuses();
        assert!(provider.get_extension_statuses().is_empty());
        provider.set_available_provider_count(3);
        assert_eq!(provider.get_available_provider_count(), 3);
    }

    #[test]
    fn set_cwd_notifies_and_resets_branch() {
        let temp = tempfile::tempdir().unwrap();
        let repo = create_plain_repo(temp.path());
        let other = temp.path().join("other");
        std::fs::create_dir_all(other.join(".git")).unwrap();
        std::fs::write(other.join(".git").join("HEAD"), "ref: refs/heads/dev\n").unwrap();
        let provider = FooterDataProvider::new(repo.to_str().unwrap());
        assert_eq!(provider.get_git_branch().as_deref(), Some("main"));
        let calls = Arc::new(AtomicUsize::new(0));
        let calls_clone = calls.clone();
        let _unsub = provider.on_branch_change(Arc::new(move || {
            calls_clone.fetch_add(1, Ordering::SeqCst);
        }));
        provider.set_cwd(other.to_str().unwrap());
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        assert_eq!(provider.get_git_branch().as_deref(), Some("dev"));
    }
}
