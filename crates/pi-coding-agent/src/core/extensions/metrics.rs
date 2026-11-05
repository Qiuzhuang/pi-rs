//! Per-extension bridge latency: counts, slow warnings, and the notification timeout.
//!
//! Every JS extension handler call is timed. Slow handlers are logged once per
//! (extension, event) pair and re-warned at most once a minute. A stalled
//! notification handler is abandoned after a budget so the core never waits on
//! it; a late JS reply is dropped with the call. Set `PI_EXT_SLOW_MS` to change
//! the warn threshold and `PI_EXT_TIMEOUT_MS` to change or disable (0) the
//! notification budget.

use std::collections::HashMap;
use std::sync::Mutex;
use std::sync::OnceLock;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// Re-warn interval for a still-slow (extension, event) pair.
const REWARN_MS: u64 = 60_000;

#[derive(Default)]
struct Stats {
    count: u64,
    total_ms: u64,
    max_ms: u64,
    timeouts: u64,
    last_warn_wall_ms: u64,
}

static STATS: OnceLock<Mutex<HashMap<(String, String), Stats>>> = OnceLock::new();

fn env_ms(name: &str, default: u64) -> u64 {
    std::env::var(name)
        .ok()
        .and_then(|v| v.trim().parse::<u64>().ok())
        .unwrap_or(default)
}

fn slow_threshold_ms() -> u64 {
    env_ms("PI_EXT_SLOW_MS", 1_000)
}

fn timeout_ms() -> u64 {
    env_ms("PI_EXT_TIMEOUT_MS", 15_000)
}

fn wall_ms() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0)
}

/// Events whose handler result the core consumes, or that have their own budget.
/// These are never abandoned: the core needs the reply (or already caps the wait).
fn is_exempt(event: &str) -> bool {
    matches!(
        event,
        "project_trust"
            | "resources_discover"
            | "session_before_switch"
            | "session_before_fork"
            | "session_before_compact"
            | "session_before_tree"
            | "session_shutdown"
            | "context"
            | "context_with_system"
            | "cache_warming_decision"
            | "before_provider_request"
            | "before_provider_headers"
            | "before_agent_start"
            | "agent_before_settle"
            | "turn_end"
            | "message_end"
            | "user_bash"
            | "input"
            | "tool_call"
            | "tool_result"
    )
}

/// The wait budget for a notification event, or `None` when the event must be
/// awaited fully (mutating events, or when `PI_EXT_TIMEOUT_MS=0`).
pub(crate) fn notify_timeout(event: &str) -> Option<Duration> {
    if is_exempt(event) {
        return None;
    }
    match timeout_ms() {
        0 => None,
        ms => Some(Duration::from_millis(ms)),
    }
}

/// Record one completed handler call. Warn when it crossed the slow threshold.
pub(crate) fn record(extension: &str, event: &str, elapsed: Duration, timed_out: bool) {
    let ms = elapsed.as_millis() as u64;
    let map = STATS.get_or_init(|| Mutex::new(HashMap::new()));
    let mut stats = map.lock().unwrap_or_else(|e| e.into_inner());
    let entry = stats.entry((extension.to_string(), event.to_string())).or_default();
    entry.count += 1;
    entry.total_ms += ms;
    entry.max_ms = entry.max_ms.max(ms);
    if timed_out {
        entry.timeouts += 1;
    }
    let over_budget = timed_out || ms >= slow_threshold_ms();
    if !over_budget {
        return;
    }
    let now = wall_ms();
    if entry.last_warn_wall_ms != 0 && now.saturating_sub(entry.last_warn_wall_ms) < REWARN_MS {
        return;
    }
    entry.last_warn_wall_ms = now;
    if timed_out {
        tracing::warn!(
            "extension {extension} handler for {event} exceeded the {timeout_ms} ms notification budget; \
             the reply will be ignored (raise PI_EXT_TIMEOUT_MS or fix the handler)",
            timeout_ms = timeout_ms()
        );
    } else {
        tracing::warn!("extension {extension} handler for {event} took {ms} ms (slow threshold {slow} ms)", slow = slow_threshold_ms());
    }
}

/// One line per (extension, event), slowest total first. Empty when nothing was measured.
pub(crate) fn summary_lines() -> Vec<String> {
    let Some(map) = STATS.get() else { return Vec::new() };
    let stats = map.lock().unwrap_or_else(|e| e.into_inner());
    let mut rows: Vec<(&String, &String, &Stats)> = stats.iter().map(|((e, v), s)| (e, v, s)).collect();
    rows.sort_by_key(|(_, _, s)| std::cmp::Reverse(s.total_ms));
    rows.truncate(25);
    rows.into_iter()
        .map(|(ext, event, s)| {
            let timeouts = if s.timeouts > 0 { format!(" timeouts={}", s.timeouts) } else { String::new() };
            format!("{ext} · {event}: n={} total={}ms max={}ms{timeouts}", s.count, s.total_ms, s.max_ms)
        })
        .collect()
}

/// Reset all counters (tests).
#[cfg(test)]
pub(crate) fn reset_for_test() {
    if let Some(map) = STATS.get() {
        map.lock().unwrap_or_else(|e| e.into_inner()).clear();
    }
}

/// Measured total for one pair (tests).
#[cfg(test)]
pub(crate) fn stats_for(extension: &str, event: &str) -> Option<(u64, u64)> {
    STATS.get()?.lock().unwrap_or_else(|e| e.into_inner()).get(&(extension.to_string(), event.to_string())).map(|s| (s.count, s.total_ms))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mutating_events_are_exempt_and_notifications_are_bounded() {
        assert!(notify_timeout("turn_end").is_none(), "turn_end results drive the loop; never abandoned");
        assert!(notify_timeout("tool_call").is_none(), "tool approval may wait on the user");
        assert!(notify_timeout("before_agent_start").is_none());
        assert!(notify_timeout("agent_end").is_some());
        assert!(notify_timeout("message_update").is_some());
        assert!(notify_timeout("agent_start").is_some());
    }

    #[test]
    fn record_tracks_counts_and_survives_timeouts() {
        reset_for_test();
        record("ext-a", "agent_end", Duration::from_millis(5), false);
        record("ext-a", "agent_end", Duration::from_millis(40), false);
        record("ext-a", "agent_end", Duration::from_secs(15), true);
        let (count, total) = stats_for("ext-a", "agent_end").expect("recorded");
        assert_eq!(count, 3);
        assert_eq!(total, 15_045);
        let lines = summary_lines();
        assert!(lines[0].contains("ext-a · agent_end"));
        assert!(lines[0].contains("timeouts=1"));
    }
}
