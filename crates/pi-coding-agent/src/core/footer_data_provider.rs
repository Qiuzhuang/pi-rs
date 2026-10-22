//! Port of `packages/coding-agent/src/core/footer-data-provider.ts`.
//!
//! Git HEAD (and reftable) changes are watched like TS: event-driven watchers
//! ([`crate::utils::fs_watch`], `fs.watch`) on the directory holding HEAD, the reftable directory
//! and `tables.list`, plus `fs.watchFile` polling where TS polls (HEAD on WSL-mounted Windows
//! repos, `tables.list`). Changes refresh the branch after the TS 500 ms debounce; watcher errors
//! retry after [`FS_WATCH_RETRY_DELAY_MS`].

use crate::utils::fs_watch::{
    FS_WATCH_RETRY_DELAY_MS, FileWatcher, FsWatcher, WatchFileStats, close_watcher, watch_file,
    watch_with_error_handler,
};
use crate::utils::paths::{node_dirname, node_join, node_resolve};
use indexmap::IndexMap;
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Weak};
use std::time::Duration;

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

fn is_wsl_environment() -> bool {
    cfg!(target_os = "linux")
        && (std::env::var("WSL_DISTRO_NAME").is_ok_and(|v| !v.is_empty())
            || std::env::var("WSL_INTEROP").is_ok_and(|v| !v.is_empty()))
}

/// `/^\/mnt\/[a-z](?:\/|$)/i`
fn is_windows_mounted_repo_path(repo_dir: &str) -> bool {
    let Some(rest) = repo_dir.strip_prefix("/mnt/") else {
        return false;
    };
    let mut chars = rest.chars();
    matches!(chars.next(), Some(c) if c.is_ascii_alphabetic()) && matches!(chars.next(), None | Some('/'))
}

fn should_poll_git_head(repo_dir: &str) -> bool {
    is_wsl_environment() && is_windows_mounted_repo_path(repo_dir)
}

/// Debounce between a detected git change and the branch refresh (`WATCH_DEBOUNCE_MS`).
const WATCH_DEBOUNCE: Duration = Duration::from_millis(500);

type Callback = Arc<dyn Fn() + Send + Sync>;

/// A pending `setTimeout`; it fires only while it is still the one stored in its slot.
struct Timer {
    id: u64,
}

static NEXT_TIMER_ID: AtomicU64 = AtomicU64::new(1);

/// Start a timer thread that runs `fire(id)` after `delay`; `fire` checks the slot holds `id`.
fn spawn_timer(delay: Duration, fire: impl FnOnce(u64) + Send + 'static) -> Timer {
    let id = NEXT_TIMER_ID.fetch_add(1, Ordering::SeqCst);
    let _ = std::thread::Builder::new().name("pi-footer-timer".into()).spawn(move || {
        std::thread::sleep(delay);
        fire(id);
    });
    Timer { id }
}

#[derive(Default)]
struct GitWatchers {
    head_watcher: Option<FsWatcher>,
    head_watch_file: Option<FileWatcher>,
    reftable_watcher: Option<FsWatcher>,
    reftable_tables_list_watcher: Option<FsWatcher>,
    reftable_tables_list_poller: Option<FileWatcher>,
}

impl GitWatchers {
    fn close(&mut self) {
        close_watcher(self.head_watcher.take().as_ref());
        if let Some(poller) = self.head_watch_file.take() {
            poller.close();
        }
        close_watcher(self.reftable_watcher.take().as_ref());
        close_watcher(self.reftable_tables_list_watcher.take().as_ref());
        if let Some(poller) = self.reftable_tables_list_poller.take() {
            poller.close();
        }
    }
}

struct Inner {
    cwd: String,
    extension_statuses: IndexMap<String, String>,
    /// `None` = not resolved yet (`undefined` in TS).
    cached_branch: Option<Option<String>>,
    git_paths: Option<GitPaths>,
    branch_change_callbacks: Vec<(u64, Callback)>,
    available_provider_count: i64,
    watchers: GitWatchers,
    refresh_timer: Option<Timer>,
    git_watcher_retry_timer: Option<Timer>,
    refresh_in_flight: bool,
    refresh_pending: bool,
    disposed: bool,
    /// Number of debounced branch refreshes started (observability for tests).
    refresh_count: u64,
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
                watchers: GitWatchers::default(),
                refresh_timer: None,
                git_watcher_retry_timer: None,
                refresh_in_flight: false,
                refresh_pending: false,
                disposed: false,
                refresh_count: 0,
            })),
            next_callback_id: Arc::new(AtomicU64::new(1)),
        };
        setup_git_watcher(&provider.inner);
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
            inner.refresh_timer = None;
            clear_git_watchers(&mut inner);
            inner.cached_branch = None;
            inner.git_paths = find_git_paths(cwd);
        }
        setup_git_watcher(&self.inner);
        notify_branch_change(&self.inner);
    }

    /// Internal: cleanup
    pub fn dispose(&self) {
        let mut inner = self.inner.lock();
        inner.disposed = true;
        inner.refresh_timer = None;
        clear_git_watchers(&mut inner);
        inner.branch_change_callbacks.clear();
    }
}

impl Drop for FooterDataProvider {
    fn drop(&mut self) {
        // The watchers hold only weak references; stop their threads with the provider.
        let mut inner = self.inner.lock();
        inner.refresh_timer = None;
        clear_git_watchers(&mut inner);
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

fn schedule_refresh(inner: &Arc<Mutex<Inner>>) {
    let mut state = inner.lock();
    if state.disposed || state.refresh_timer.is_some() {
        return;
    }
    if state.refresh_in_flight {
        state.refresh_pending = true;
        return;
    }
    let weak = Arc::downgrade(inner);
    state.refresh_timer = Some(spawn_timer(WATCH_DEBOUNCE, move |id| {
        let Some(inner) = weak.upgrade() else { return };
        {
            let mut state = inner.lock();
            if state.refresh_timer.as_ref().map(|t| t.id) != Some(id) {
                return;
            }
            state.refresh_timer = None;
        }
        refresh_git_branch_async(&inner);
    }));
}

/// `refreshGitBranchAsync`: runs on the timer thread, so the git subprocess does not block the UI.
fn refresh_git_branch_async(inner: &Arc<Mutex<Inner>>) {
    let git_paths = {
        let mut state = inner.lock();
        if state.disposed {
            return;
        }
        if state.refresh_in_flight {
            state.refresh_pending = true;
            return;
        }
        state.refresh_in_flight = true;
        state.refresh_count += 1;
        state.git_paths.clone()
    };
    let next_branch = resolve_git_branch(git_paths.as_ref());
    let (changed, reschedule) = {
        let mut state = inner.lock();
        let mut changed = false;
        if !state.disposed {
            changed = matches!(&state.cached_branch, Some(previous) if *previous != next_branch);
            state.cached_branch = Some(next_branch);
        }
        state.refresh_in_flight = false;
        let reschedule = state.refresh_pending && !state.disposed;
        if reschedule {
            state.refresh_pending = false;
        }
        (changed, reschedule)
    };
    if changed {
        notify_branch_change(inner);
    }
    if reschedule {
        schedule_refresh(inner);
    }
}

fn clear_git_watchers(state: &mut Inner) {
    state.watchers.close();
    state.git_watcher_retry_timer = None;
}

fn schedule_git_watcher_retry(inner: &Arc<Mutex<Inner>>) {
    let mut state = inner.lock();
    if state.disposed || state.git_watcher_retry_timer.is_some() {
        return;
    }
    let weak = Arc::downgrade(inner);
    state.git_watcher_retry_timer = Some(spawn_timer(
        Duration::from_millis(FS_WATCH_RETRY_DELAY_MS as u64),
        move |id| {
            let Some(inner) = weak.upgrade() else { return };
            {
                let mut state = inner.lock();
                if state.git_watcher_retry_timer.as_ref().map(|t| t.id) != Some(id) {
                    return;
                }
                state.git_watcher_retry_timer = None;
            }
            setup_git_watcher(&inner);
        },
    ));
}

fn handle_git_watcher_error(inner: &Arc<Mutex<Inner>>) {
    clear_git_watchers(&mut inner.lock());
    schedule_git_watcher_retry(inner);
}

/// `fs.watchFile` listener body: refresh when mtime, ctime or size changed.
fn stats_changed(current: &WatchFileStats, previous: &WatchFileStats) -> bool {
    current.mtime_ms != previous.mtime_ms || current.ctime_ms != previous.ctime_ms || current.size != previous.size
}

fn setup_git_watcher(inner: &Arc<Mutex<Inner>>) {
    let git_paths = {
        let mut state = inner.lock();
        clear_git_watchers(&mut state);
        if state.disposed {
            return;
        }
        match state.git_paths.clone() {
            Some(paths) => paths,
            None => return,
        }
    };
    let weak = Arc::downgrade(inner);
    let refresh: Arc<dyn Fn() + Send + Sync> = {
        let weak = weak.clone();
        Arc::new(move || {
            if let Some(inner) = weak.upgrade() {
                schedule_refresh(&inner);
            }
        })
    };
    let on_error: Arc<dyn Fn() + Send + Sync> = {
        let weak = weak.clone();
        Arc::new(move || {
            if let Some(inner) = weak.upgrade() {
                handle_git_watcher_error(&inner);
            }
        })
    };
    let poll_git_head = should_poll_git_head(&git_paths.repo_dir);

    // Watch the directory containing HEAD, not HEAD itself.
    // Git uses atomic writes (write temp, rename over HEAD), which changes the inode.
    // fs.watch on a file stops working after the inode changes.
    let head_refresh = refresh.clone();
    let head_watcher = watch_with_error_handler(
        &node_dirname(&git_paths.head_path),
        Arc::new(move |_event, filename| {
            if filename.as_deref().is_none_or(|name| name == "HEAD") {
                head_refresh();
            }
        }),
        on_error.clone(),
    );
    let has_head_watcher = head_watcher.is_some();
    let store = |set: &mut dyn FnMut(&mut GitWatchers)| -> bool {
        let mut state = inner.lock();
        if state.disposed {
            drop(state);
            return false;
        }
        set(&mut state.watchers);
        true
    };
    if !store(&mut |w| w.head_watcher = head_watcher.clone()) {
        close_watcher(head_watcher.as_ref());
        return;
    }
    if poll_git_head {
        let poll_refresh = refresh.clone();
        let poller = watch_file(
            &git_paths.head_path,
            1000,
            Arc::new(move |current, previous| {
                if stats_changed(current, previous) {
                    poll_refresh();
                }
            }),
        );
        if !store(&mut |w| w.head_watch_file = Some(poller.clone())) {
            poller.close();
            return;
        }
    }
    if !has_head_watcher && !poll_git_head {
        return;
    }

    // In reftable repos, branch switches update files in the reftable directory
    // instead of HEAD. Watch it separately so the footer picks up those changes.
    let reftable_dir = node_join(&[&git_paths.common_git_dir, "reftable"]);
    if !Path::new(&reftable_dir).exists() {
        return;
    }
    let reftable_refresh = refresh.clone();
    let reftable_watcher = watch_with_error_handler(
        &reftable_dir,
        Arc::new(move |_event, _filename| reftable_refresh()),
        on_error.clone(),
    );
    if !store(&mut |w| w.reftable_watcher = reftable_watcher.clone()) {
        close_watcher(reftable_watcher.as_ref());
        return;
    }
    if reftable_watcher.is_none() {
        return;
    }

    let tables_list_path = node_join(&[&reftable_dir, "tables.list"]);
    if !Path::new(&tables_list_path).exists() {
        return;
    }
    let tables_refresh = refresh.clone();
    let tables_list_watcher = watch_with_error_handler(
        &tables_list_path,
        Arc::new(move |_event, _filename| tables_refresh()),
        on_error,
    );
    if !store(&mut |w| w.reftable_tables_list_watcher = tables_list_watcher.clone()) {
        close_watcher(tables_list_watcher.as_ref());
        return;
    }
    if tables_list_watcher.is_none() {
        return;
    }
    let poll_refresh = refresh;
    let poller = watch_file(
        &tables_list_path,
        250,
        Arc::new(move |current, previous| {
            if stats_changed(current, previous) {
                poll_refresh();
            }
        }),
    );
    if !store(&mut |w| w.reftable_tables_list_poller = Some(poller.clone())) {
        poller.close();
    }
}

/// Read-only view for extensions - excludes setExtensionStatus, setAvailableProviderCount and dispose
/// TS: Pick< FooterDataProvider, "getGitBranch" | "getExtensionStatuses" | "getAvailableProviderCount" | "onBranchChange" >
pub type ReadonlyFooterDataProvider = Arc<FooterDataProvider>;

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicUsize;
    use std::time::Instant;

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

    /// A worktree whose common git dir has a reftable directory with `tables.list`
    /// (HEAD names the branch directly so no git binary is needed).
    fn create_reftable_worktree(temp: &Path) -> (std::path::PathBuf, std::path::PathBuf, std::path::PathBuf) {
        let common = temp.join("repo").join(".git");
        let git_dir = common.join("worktrees").join("src");
        let worktree = temp.join("worktree");
        let reftable = common.join("reftable");
        std::fs::create_dir_all(&git_dir).unwrap();
        std::fs::create_dir_all(&reftable).unwrap();
        std::fs::create_dir_all(&worktree).unwrap();
        std::fs::write(worktree.join(".git"), format!("gitdir: {}\n", git_dir.display())).unwrap();
        std::fs::write(git_dir.join("HEAD"), "ref: refs/heads/main\n").unwrap();
        std::fs::write(git_dir.join("commondir"), "../..\n").unwrap();
        std::fs::write(reftable.join("tables.list"), "0\n").unwrap();
        (worktree, reftable, git_dir)
    }

    fn watchers(provider: &FooterDataProvider) -> (Option<FsWatcher>, Option<FsWatcher>, Option<FsWatcher>) {
        let inner = provider.inner.lock();
        (
            inner.watchers.head_watcher.clone(),
            inner.watchers.reftable_watcher.clone(),
            inner.watchers.reftable_tables_list_watcher.clone(),
        )
    }

    #[test]
    fn watches_head_dir_reftable_dir_and_tables_list() {
        let temp = tempfile::tempdir().unwrap();
        let (worktree, _, _) = create_reftable_worktree(temp.path());
        let provider = FooterDataProvider::new(worktree.to_str().unwrap());
        let (head, reftable, tables) = watchers(&provider);
        assert!(head.is_some() && reftable.is_some() && tables.is_some());
        assert!(provider.inner.lock().watchers.reftable_tables_list_poller.is_some());
        provider.dispose();
        assert!(head.unwrap().is_closed() && reftable.unwrap().is_closed() && tables.unwrap().is_closed());
    }

    #[test]
    fn debounces_rapid_reftable_updates_into_a_single_refresh() {
        let temp = tempfile::tempdir().unwrap();
        let (worktree, _, _) = create_reftable_worktree(temp.path());
        let provider = FooterDataProvider::new(worktree.to_str().unwrap());
        assert_eq!(provider.get_git_branch().as_deref(), Some("main"));
        let calls = Arc::new(AtomicUsize::new(0));
        let calls_clone = calls.clone();
        provider.on_branch_change(Arc::new(move || {
            calls_clone.fetch_add(1, Ordering::SeqCst);
        }));
        let (_, reftable, _) = watchers(&provider);
        let reftable = reftable.unwrap();
        let started = Instant::now();
        for _ in 0..3 {
            reftable.emit_event(crate::utils::fs_watch::WatchEventType::Change, Some("tables.list"));
        }
        std::thread::sleep(Duration::from_millis(300));
        assert_eq!(provider.inner.lock().refresh_count, 0, "refresh waits for the 500 ms debounce");
        wait_for(|| provider.inner.lock().refresh_count == 1);
        assert!(started.elapsed() >= WATCH_DEBOUNCE);
        std::thread::sleep(Duration::from_millis(650));
        assert_eq!(provider.inner.lock().refresh_count, 1);
        // Same branch: no notification.
        assert_eq!(calls.load(Ordering::SeqCst), 0);
        provider.dispose();
    }

    #[test]
    fn updates_the_cached_branch_when_reftable_files_change() {
        let temp = tempfile::tempdir().unwrap();
        let (worktree, reftable, git_dir) = create_reftable_worktree(temp.path());
        let provider = FooterDataProvider::new(worktree.to_str().unwrap());
        assert_eq!(provider.get_git_branch().as_deref(), Some("main"));
        let calls = Arc::new(AtomicUsize::new(0));
        let calls_clone = calls.clone();
        provider.on_branch_change(Arc::new(move || {
            calls_clone.fetch_add(1, Ordering::SeqCst);
        }));
        // Change what HEAD says without an event in the HEAD directory: close that watcher so
        // only the reftable watchers can trigger the refresh.
        close_watcher(provider.inner.lock().watchers.head_watcher.as_ref());
        std::fs::write(git_dir.join("HEAD"), "ref: refs/heads/foo\n").unwrap();
        std::fs::write(reftable.join("tables.list"), "1\n").unwrap();
        wait_for(|| provider.get_git_branch().as_deref() == Some("foo"));
        wait_for(|| calls.load(Ordering::SeqCst) == 1);
        provider.dispose();
    }

    #[test]
    fn retries_git_watchers_after_an_async_watch_error() {
        let temp = tempfile::tempdir().unwrap();
        let repo = create_plain_repo(temp.path());
        let provider = FooterDataProvider::new(repo.to_str().unwrap());
        let (original, _, _) = watchers(&provider);
        let original = original.expect("head watcher");
        original.emit_error();
        assert!(original.is_closed());
        assert!(watchers(&provider).0.is_none());
        std::thread::sleep(Duration::from_millis(FS_WATCH_RETRY_DELAY_MS as u64 - 500));
        assert!(watchers(&provider).0.is_none(), "no retry before 5 s");
        let start = Instant::now();
        while watchers(&provider).0.is_none() {
            assert!(start.elapsed() < Duration::from_secs(5), "watcher was not recreated");
            std::thread::sleep(Duration::from_millis(10));
        }
        let recreated = watchers(&provider).0.unwrap();
        assert!(!recreated.ptr_eq(&original));
        provider.dispose();
    }

    #[test]
    fn wsl_windows_mount_detection() {
        assert!(is_windows_mounted_repo_path("/mnt/c"));
        assert!(is_windows_mounted_repo_path("/mnt/D/projects"));
        assert!(!is_windows_mounted_repo_path("/mnt/cd/projects"));
        assert!(!is_windows_mounted_repo_path("/home/user"));
    }
}
