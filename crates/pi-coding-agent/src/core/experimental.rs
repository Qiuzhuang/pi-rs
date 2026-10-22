//! Port of `packages/coding-agent/src/core/experimental.ts`.

pub fn are_experimental_features_enabled() -> bool {
    experimental_flag_enabled(std::env::var("PI_EXPERIMENTAL").ok().as_deref())
}

/// `PI_EXPERIMENTAL` is on only when set to exactly `1`.
fn experimental_flag_enabled(value: Option<&str>) -> bool {
    value == Some("1")
}

#[cfg(test)]
mod tests {
    use super::experimental_flag_enabled;

    // Ported from experimental.test.ts. The env lookup is split out so the cases do not race
    // other tests in this binary that set PI_EXPERIMENTAL.
    #[test]
    fn returns_false_when_unset() {
        assert!(!experimental_flag_enabled(None));
    }

    #[test]
    fn returns_false_when_empty() {
        assert!(!experimental_flag_enabled(Some("")));
    }

    #[test]
    fn returns_true_when_set_to_1() {
        assert!(experimental_flag_enabled(Some("1")));
    }

    #[test]
    fn returns_false_when_set_to_0() {
        assert!(!experimental_flag_enabled(Some("0")));
    }

    #[test]
    fn returns_false_when_set_to_a_non_1_value() {
        assert!(!experimental_flag_enabled(Some("true")));
    }
}
