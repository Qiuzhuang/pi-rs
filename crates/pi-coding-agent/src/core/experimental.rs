//! Port of `packages/coding-agent/src/core/experimental.ts`.

pub fn are_experimental_features_enabled() -> bool {
    std::env::var("PI_EXPERIMENTAL").is_ok_and(|v| v == "1")
}
