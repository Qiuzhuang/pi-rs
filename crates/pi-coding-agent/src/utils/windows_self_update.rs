//! Port of `packages/coding-agent/src/utils/windows-self-update.ts`.
//!
//! On Windows, an npm self-update cannot replace native addons that the running process has
//! loaded, so they are moved into a quarantine directory first. The Rust binary loads no
//! native addons from the package directory, so there is nothing to quarantine
//! ([`quarantine_windows_native_dependencies`] finds no loaded shared objects); cleanup of a
//! quarantine left by an earlier Node-based install is still performed.

use std::path::{Path, PathBuf};

const QUARANTINE_DIR_NAME: &str = ".pi-native-quarantine";

fn resolve(path: &str) -> PathBuf {
    let path = Path::new(path);
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir().map(|cwd| cwd.join(path)).unwrap_or_else(|_| path.to_path_buf())
    }
}

fn get_quarantine_root(package_dir: &str) -> Option<PathBuf> {
    let mut current = resolve(package_dir);
    loop {
        if current
            .file_name()
            .is_some_and(|name| name.to_string_lossy().eq_ignore_ascii_case("node_modules"))
        {
            return Some(current.join(QUARANTINE_DIR_NAME));
        }
        let parent = current.parent()?.to_path_buf();
        if parent == current {
            return None;
        }
        current = parent;
    }
}

/// Shared objects loaded from `package_dir`. The Rust binary loads none (Node's
/// `process.report.getReport().sharedObjects` has no counterpart).
fn get_loaded_shared_objects_in_package_dir(_package_dir: &Path) -> Vec<PathBuf> {
    Vec::new()
}

pub fn cleanup_windows_self_update_quarantine(package_dir: &str) {
    let Some(quarantine_root) = get_quarantine_root(package_dir) else {
        return;
    };
    // A previous pi process may still be exiting and holding a native addon.
    let _ = std::fs::remove_dir_all(quarantine_root);
}

pub fn quarantine_windows_native_dependencies(package_dir: &str) {
    let resolved_package_dir = resolve(package_dir);
    let Some(quarantine_root) = get_quarantine_root(&resolved_package_dir.to_string_lossy()) else {
        return;
    };
    let loaded_files = get_loaded_shared_objects_in_package_dir(&resolved_package_dir);
    if loaded_files.is_empty() {
        return;
    }
    let millis = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or_default();
    let run_dir = quarantine_root.join(format!("{millis}-{}-{}", std::process::id(), uuid::Uuid::new_v4()));
    for loaded_file in loaded_files {
        if !loaded_file.exists() {
            continue;
        }
        let Ok(relative) = loaded_file.strip_prefix(&resolved_package_dir) else { continue };
        let quarantine_path = run_dir.join(relative);
        if let Some(parent) = quarantine_path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        if std::fs::rename(&loaded_file, &quarantine_path).is_ok() {
            let _ = std::fs::copy(&quarantine_path, &loaded_file);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn removes_the_quarantine_next_to_node_modules() {
        let root = std::env::temp_dir().join(format!("pi-wsu-{}", uuid::Uuid::new_v4()));
        let package = root.join("node_modules").join("@earendil-works").join("pi");
        let quarantine = root.join("node_modules").join(QUARANTINE_DIR_NAME).join("x");
        std::fs::create_dir_all(&package).unwrap();
        std::fs::create_dir_all(&quarantine).unwrap();
        cleanup_windows_self_update_quarantine(package.to_str().unwrap());
        assert!(!root.join("node_modules").join(QUARANTINE_DIR_NAME).exists());
        quarantine_windows_native_dependencies(package.to_str().unwrap());
        std::fs::remove_dir_all(root).unwrap();
    }
}
