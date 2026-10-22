//! Port of `packages/coding-agent/src/utils/wsl.ts`.

use std::collections::HashMap;

/// Environment snapshot used by helpers that accept an injected `process.env`.
pub type EnvMap = HashMap<String, String>;

/// Windows Subsystem for Linux, where Windows executables are reachable through interop.
///
/// `env` defaults to the process environment.
pub fn is_wsl(env: Option<&EnvMap>) -> bool {
    let get = |key: &str| -> Option<String> {
        match env {
            Some(env) => env.get(key).cloned(),
            None => std::env::var(key).ok(),
        }
    };
    if get("WSL_DISTRO_NAME").is_some_and(|v| !v.is_empty()) || get("WSLENV").is_some_and(|v| !v.is_empty()) {
        return true;
    }
    match std::fs::read_to_string("/proc/version") {
        Ok(release) => {
            let lower = release.to_lowercase();
            lower.contains("microsoft") || lower.contains("wsl")
        }
        Err(_) => false,
    }
}
