//! Provider env lookup (port of `utils/provider-env.ts`).

use crate::types::ProviderEnv;

/// Resolve a provider env value from scoped overrides, then the process environment.
/// Empty values count as unset.
pub fn get_provider_env_value(name: &str, env: Option<&ProviderEnv>) -> Option<String> {
    if let Some(value) = env.and_then(|e| e.get(name)).filter(|v| !v.is_empty()) {
        return Some(value.clone());
    }
    std::env::var(name).ok().filter(|v| !v.is_empty())
}
