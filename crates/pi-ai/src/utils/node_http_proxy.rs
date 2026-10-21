//! HTTP proxy resolution from environment variables (port of `utils/node-http-proxy.ts`).

use url::Url;

use super::provider_env::get_provider_env_value;
use crate::types::ProviderEnv;

pub const UNSUPPORTED_PROXY_PROTOCOL_MESSAGE: &str =
    "Unsupported proxy protocol. SOCKS and PAC proxy URLs are not supported; use an HTTP or HTTPS proxy URL.";

fn default_proxy_port(protocol: &str) -> u16 {
    match protocol {
        "ftp" => 21,
        "gopher" => 70,
        "http" | "ws" => 80,
        "https" | "wss" => 443,
        _ => 0,
    }
}

type Lookup<'a> = &'a dyn Fn(&str) -> Option<String>;

fn get_proxy_env(key: &str, env: Option<&ProviderEnv>, lookup: Lookup) -> String {
    let lower = key.to_lowercase();
    let upper = key.to_uppercase();
    env.and_then(|e| e.get(&lower).filter(|v| !v.is_empty()).cloned())
        .or_else(|| env.and_then(|e| e.get(&upper).filter(|v| !v.is_empty()).cloned()))
        .or_else(|| lookup(&lower).filter(|v| !v.is_empty()))
        .or_else(|| lookup(&upper).filter(|v| !v.is_empty()))
        .unwrap_or_default()
}

fn strip_brackets(host: &str) -> &str {
    if host.starts_with('[') && host.ends_with(']') && host.len() >= 2 { &host[1..host.len() - 1] } else { host }
}

/// `Number.parseInt(text, 10)`.
fn js_parse_int(text: &str) -> Option<u32> {
    let t = text.trim_start();
    let digits: String = t.chars().take_while(|c| c.is_ascii_digit()).collect();
    digits.parse().ok()
}

fn parse_no_proxy_entry(entry: &str) -> Option<(String, u32)> {
    let trimmed = entry.trim().to_lowercase();
    if trimmed.is_empty() {
        return None;
    }
    if trimmed.starts_with('[')
        && let Some(closing) = trimmed.find(']') {
            let host = trimmed[1..closing].to_string();
            let rest = &trimmed[closing + 1..];
            if let Some(port) = rest.strip_prefix(':') {
                return Some((host, js_parse_int(port).unwrap_or(0)));
            }
            return Some((host, 0));
        }
    if trimmed.contains(':') && trimmed.split(':').count() > 2 {
        return Some((trimmed, 0));
    }
    if let Some(colon) = trimmed.rfind(':')
        && Some(colon) == trimmed.find(':') {
            let host = trimmed[..colon].to_string();
            if let Some(port) = js_parse_int(&trimmed[colon + 1..]) {
                return Some((host, port));
            }
        }
    Some((trimmed, 0))
}

fn should_proxy_hostname(hostname: &str, port: u32, env: Option<&ProviderEnv>, lookup: Lookup) -> bool {
    let no_proxy = get_proxy_env("no_proxy", env, lookup).to_lowercase();
    if no_proxy.is_empty() {
        return true;
    }
    if no_proxy == "*" {
        return false;
    }
    let target = hostname.to_lowercase();
    let target = strip_brackets(&target).to_string();
    no_proxy.split(|c: char| c == ',' || c.is_whitespace()).all(|entry| {
        let Some((host, entry_port)) = parse_no_proxy_entry(entry) else { return true };
        if entry_port != 0 && entry_port != port {
            return true;
        }
        let mut domain = strip_brackets(&host).to_string();
        if let Some(rest) = domain.strip_prefix("*.") {
            domain = rest.to_string();
        } else if domain.starts_with('.') || domain.starts_with('*') {
            domain = domain[1..].to_string();
        }
        if domain.is_empty() {
            return true;
        }
        if target == domain || target.ends_with(&format!(".{domain}")) {
            return false;
        }
        true
    })
}

fn get_proxy_for_url(target_url: &str, env: Option<&ProviderEnv>, lookup: Lookup) -> String {
    let Ok(parsed) = Url::parse(target_url) else { return String::new() };
    let Some(host) = parsed.host_str() else { return String::new() };
    let protocol = parsed.scheme().to_string();
    let hostname = strip_brackets(host).to_string();
    let port = parsed.port().map(u32::from).filter(|p| *p != 0).unwrap_or_else(|| default_proxy_port(&protocol) as u32);
    if !should_proxy_hostname(&hostname, port, env, lookup) {
        return String::new();
    }
    let mut proxy = get_proxy_env(&format!("{protocol}_proxy"), env, lookup);
    if proxy.is_empty() {
        proxy = get_proxy_env("all_proxy", env, lookup);
    }
    if !proxy.is_empty() && !proxy.contains("://") {
        proxy = format!("{protocol}://{proxy}");
    }
    proxy
}

fn process_env(name: &str) -> Option<String> {
    get_provider_env_value(name, None)
}

/// The proxy URL for a target from `*_proxy`/`no_proxy` env vars (scoped env first), any scheme.
pub fn get_proxy_url_for_target(target_url: &str, env: Option<&ProviderEnv>) -> Option<String> {
    Some(get_proxy_for_url(target_url, env, &process_env)).filter(|p| !p.is_empty())
}

/// Resolve the HTTP(S) proxy URL for a target, rejecting SOCKS/PAC proxies.
pub fn resolve_http_proxy_url_for_target(target_url: &str, env: Option<&ProviderEnv>) -> anyhow::Result<Option<Url>> {
    resolve_with_lookup(target_url, env, &process_env)
}

fn resolve_with_lookup(target_url: &str, env: Option<&ProviderEnv>, lookup: Lookup) -> anyhow::Result<Option<Url>> {
    let proxy = get_proxy_for_url(target_url, env, lookup);
    if proxy.is_empty() {
        return Ok(None);
    }
    let proxy_url = Url::parse(&proxy)
        .map_err(|e| anyhow::anyhow!("Invalid proxy URL {}: {e}", serde_json::to_string(&proxy).unwrap_or_default()))?;
    if proxy_url.scheme() != "http" && proxy_url.scheme() != "https" {
        anyhow::bail!("{UNSUPPORTED_PROXY_PROTOCOL_MESSAGE} Got {}:", proxy_url.scheme());
    }
    Ok(Some(proxy_url))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn with_env(vars: &[(&str, &str)]) -> impl Fn(&str) -> Option<String> {
        let map: HashMap<String, String> = vars.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect();
        move |name| map.get(name).cloned()
    }

    fn resolve(target: &str, env: Option<&ProviderEnv>, lookup: &dyn Fn(&str) -> Option<String>) -> Option<String> {
        resolve_with_lookup(target, env, lookup).unwrap().map(|u| u.to_string())
    }

    #[test]
    fn respects_no_proxy() {
        let lookup = with_env(&[("HTTPS_PROXY", "http://proxy.example:8080"), ("NO_PROXY", "bedrock-runtime.us-east-1.amazonaws.com")]);
        assert_eq!(resolve("https://bedrock-runtime.us-east-1.amazonaws.com", None, &lookup), None);
    }

    #[test]
    fn resolves_proxy_and_prefers_scoped_env() {
        let lookup = with_env(&[("HTTPS_PROXY", "http://proxy.example:8080")]);
        assert_eq!(resolve("https://bedrock-runtime.us-east-1.amazonaws.com", None, &lookup).as_deref(), Some("http://proxy.example:8080/"));
        let lookup = with_env(&[("https_proxy", "http://process-proxy.example:8080")]);
        let mut scoped = ProviderEnv::new();
        scoped.insert("HTTPS_PROXY".into(), "http://scoped-proxy.example:8080".into());
        assert_eq!(resolve("https://x.amazonaws.com", Some(&scoped), &lookup).as_deref(), Some("http://scoped-proxy.example:8080/"));
    }

    #[test]
    fn rejects_socks() {
        let lookup = with_env(&[("HTTPS_PROXY", "socks5://proxy.example:1080")]);
        let err = resolve_with_lookup("https://x.amazonaws.com", None, &lookup).unwrap_err();
        assert!(err.to_string().contains(UNSUPPORTED_PROXY_PROTOCOL_MESSAGE));
    }

    #[test]
    fn handles_wildcards_ipv6_and_ports() {
        let lookup = with_env(&[
            ("HTTPS_PROXY", "http://proxy.example:8080"),
            ("NO_PROXY", "example.com, .wildcard.org, *.star.net, ::1, [2001:db8::1], 127.0.0.1:8080"),
        ]);
        for target in [
            "https://example.com",
            "https://api.example.com",
            "https://wildcard.org",
            "https://api.wildcard.org",
            "https://star.net",
            "https://api.star.net",
            "https://[::1]:80",
            "https://[2001:db8::1]",
            "https://127.0.0.1:8080",
        ] {
            assert_eq!(resolve(target, None, &lookup), None, "{target}");
        }
        assert_eq!(resolve("https://notexample.com", None, &lookup).as_deref(), Some("http://proxy.example:8080/"));
        assert_eq!(resolve("https://127.0.0.1:3000", None, &lookup).as_deref(), Some("http://proxy.example:8080/"));
    }
}
