//! Port of `packages/coding-agent/src/utils/pi-user-agent.ts`.

use pi_ai::utils::identity::{ClientIdentity, UserAgentMode, UserAgentNamed, format_user_agent};

/// pi's `User-Agent` for its own network requests; the same detailed form pi-ai sends to providers.
pub fn get_pi_user_agent(version: &str) -> String {
    format_user_agent(Some(&ClientIdentity {
        user_agent: Some(UserAgentMode::Named(UserAgentNamed::Detailed)),
        attribution: None,
        version: Some(version.to_string()),
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_the_user_agent_expected_by_pi_dev() {
        let user_agent = get_pi_user_agent("1.2.3");
        assert!(user_agent.starts_with("pi/1.2.3 ("), "{user_agent}");
        let re = regex::Regex::new(r"^pi/[^\s()]+ \([^;()]+;\s*[^;()]+;\s*[^()]+\)$").unwrap();
        assert!(re.is_match(&user_agent), "{user_agent}");
    }
}
