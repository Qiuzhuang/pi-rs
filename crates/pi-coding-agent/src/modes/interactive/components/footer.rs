//! Port of `packages/coding-agent/src/modes/interactive/components/footer.ts`.

use crate::core::agent_session::{AgentSession, AgentSessionRoutedModel};
use crate::core::experimental::are_experimental_features_enabled;
use crate::core::extensions::types::ContextUsage;
use crate::core::footer_data_provider::ReadonlyFooterDataProvider;
use crate::core::session_manager::SessionEntry;
use crate::core::tools::js_util::{js_to_fixed, locale_compare};
use crate::core::usage_totals::{UsageTotals, add_usage_to_totals, create_usage_totals};
use crate::modes::interactive::theme::theme::{ThemeColor, theme};
use crate::utils::paths::{is_absolute, node_relative, node_resolve};
use parking_lot::Mutex;
use pi_agent::types::{AgentMessage, ThinkingLevel};
use pi_ai::types::Model;
use pi_tui::tui::Component;
use pi_tui::utils::{js_trim, truncate_to_width, visible_width};
use serde_json::Value;
use std::sync::Arc;

/// Sanitize text for display in a single-line status.
/// Removes newlines, tabs, carriage returns, and other control characters.
fn sanitize_status_text(text: &str) -> String {
    // Replace newlines, tabs, carriage returns with space, then collapse multiple spaces
    let replaced: String = text.chars().map(|c| if matches!(c, '\r' | '\n' | '\t') { ' ' } else { c }).collect();
    let mut collapsed = String::with_capacity(replaced.len());
    let mut prev_space = false;
    for c in replaced.chars() {
        if c == ' ' {
            if !prev_space {
                collapsed.push(c);
            }
            prev_space = true;
        } else {
            collapsed.push(c);
            prev_space = false;
        }
    }
    js_trim(&collapsed).to_string()
}

/// `Math.round` (half rounds up).
fn js_round(x: f64) -> f64 {
    (x + 0.5).floor()
}

/// Format token counts for compact footer display.
pub fn format_tokens(count: i64) -> String {
    let c = count as f64;
    if c < 1000.0 {
        return count.to_string();
    }
    if c < 10000.0 {
        return format!("{}k", js_to_fixed(c / 1000.0, 1));
    }
    if c < 1_000_000.0 {
        return format!("{}k", js_round(c / 1000.0));
    }
    if c < 10_000_000.0 {
        return format!("{}M", js_to_fixed(c / 1_000_000.0, 1));
    }
    format!("{}M", js_round(c / 1_000_000.0))
}

pub fn format_cwd_for_footer(cwd: &str, home: Option<&str>) -> String {
    let Some(home) = home.filter(|h| !h.is_empty()) else {
        return cwd.to_string();
    };

    let resolved_cwd = node_resolve(&[cwd]);
    let resolved_home = node_resolve(&[home]);
    let relative_to_home = node_relative(&resolved_home, &resolved_cwd);
    let sep = std::path::MAIN_SEPARATOR;
    let is_inside_home = relative_to_home.is_empty()
        || (relative_to_home != ".."
            && !relative_to_home.starts_with(&format!("..{sep}"))
            && !is_absolute(&relative_to_home));

    if !is_inside_home {
        return cwd.to_string();
    }
    if relative_to_home.is_empty() { "~".to_string() } else { format!("~{sep}{relative_to_home}") }
}

#[derive(Clone)]
pub struct SessionStats {
    pub session: Arc<AgentSession>,
    pub session_id: String,
    pub leaf_id: Option<String>,
    pub entry_count: i64,
    pub limits_model: Value,
    pub usage_totals: UsageTotals,
    pub latest_cache_hit_rate: Option<f64>,
    pub context_usage: Option<ContextUsage>,
}

/// What the footer reads from the session. Implemented by [`AgentSession`]; tests provide fakes.
pub trait FooterSession: Send + Sync {
    fn entries(&self) -> Vec<SessionEntry>;
    fn entry_count(&self) -> i64;
    fn session_id(&self) -> String;
    fn leaf_id(&self) -> Option<String>;
    fn session_name(&self) -> Option<String>;
    fn cwd(&self) -> String;
    fn model(&self) -> Option<Model>;
    fn thinking_level(&self) -> ThinkingLevel;
    fn routed_model(&self) -> Option<AgentSessionRoutedModel>;
    fn context_usage(&self) -> Option<ContextUsage>;
    fn is_using_subscription(&self, provider: &str) -> bool;
}

impl FooterSession for AgentSession {
    fn entries(&self) -> Vec<SessionEntry> {
        self.session_manager().get_entries()
    }
    fn entry_count(&self) -> i64 {
        self.session_manager().get_entry_count()
    }
    fn session_id(&self) -> String {
        self.session_manager().get_session_id()
    }
    fn leaf_id(&self) -> Option<String> {
        self.session_manager().get_leaf_id()
    }
    fn session_name(&self) -> Option<String> {
        self.session_manager().get_session_name()
    }
    fn cwd(&self) -> String {
        self.session_manager().get_cwd()
    }
    fn model(&self) -> Option<Model> {
        AgentSession::model(self)
    }
    fn thinking_level(&self) -> ThinkingLevel {
        AgentSession::thinking_level(self)
    }
    fn routed_model(&self) -> Option<AgentSessionRoutedModel> {
        AgentSession::routed_model(self)
    }
    fn context_usage(&self) -> Option<ContextUsage> {
        self.get_context_usage()
    }
    fn is_using_subscription(&self, provider: &str) -> bool {
        self.model_runtime().is_using_subscription(provider)
    }
}

/// Cache key and value of `getSessionStats()`.
struct CachedStats {
    session_ptr: *const (),
    session_id: String,
    leaf_id: Option<String>,
    entry_count: i64,
    limits_model: Option<(String, String, u64)>,
    usage_totals: UsageTotals,
    latest_cache_hit_rate: Option<f64>,
    context_usage: Option<ContextUsage>,
}

// The raw pointer is only compared for identity, never dereferenced.
unsafe impl Send for CachedStats {}

struct State {
    auto_compact_enabled: bool,
    session: Arc<dyn FooterSession>,
    session_stats: Option<CachedStats>,
}

/// Footer component that shows pwd, token stats, and context usage.
/// Computes token/context stats from session, gets git branch and extension statuses from provider.
/// TS: `implements Component`
pub struct FooterComponent {
    footer_data: ReadonlyFooterDataProvider,
    st: Mutex<State>,
}

fn model_key(model: &Model) -> (String, String, u64) {
    (model.provider.clone(), model.id.clone(), model.context_window)
}

impl FooterComponent {
    pub fn new(session: Arc<AgentSession>, footer_data: &ReadonlyFooterDataProvider) -> Self {
        Self::with_session(session, footer_data)
    }

    /// A footer over any [`FooterSession`].
    pub fn with_session(session: Arc<dyn FooterSession>, footer_data: &ReadonlyFooterDataProvider) -> Self {
        Self {
            footer_data: footer_data.clone(),
            st: Mutex::new(State { auto_compact_enabled: true, session, session_stats: None }),
        }
    }

    pub fn set_session(&self, session: Arc<AgentSession>) {
        self.st.lock().session = session;
    }

    pub fn set_auto_compact_enabled(&self, enabled: bool) {
        self.st.lock().auto_compact_enabled = enabled;
    }

    /// Clean up resources.
    /// Git watcher cleanup now handled by provider.
    pub fn dispose(&self) {
        // Git watcher cleanup handled by provider
    }

    /// Usage totals and context usage scan the whole session, and the footer renders on every frame.
    /// Entries are append-only and every append moves the leaf, so the results only change with the
    /// session, leaf, entry count, or the model whose context window applies.
    fn session_stats(&self, session: &Arc<dyn FooterSession>) -> (UsageTotals, Option<f64>, Option<ContextUsage>) {
        let entry_count = session.entry_count();
        let session_id = session.session_id();
        let leaf_id = session.leaf_id();
        let limits_model = session.routed_model().map(|r| r.model).or_else(|| session.model()).map(|m| model_key(&m));
        let session_ptr = Arc::as_ptr(session) as *const ();
        {
            let st = self.st.lock();
            if let Some(cached) = &st.session_stats
                && cached.session_ptr == session_ptr
                && cached.session_id == session_id
                && cached.leaf_id == leaf_id
                && cached.entry_count == entry_count
                && cached.limits_model == limits_model
            {
                return (cached.usage_totals.clone(), cached.latest_cache_hit_rate, cached.context_usage.clone());
            }
        }

        // Calculate cumulative usage from ALL session entries (not just post-compaction messages)
        let mut usage_totals = create_usage_totals();
        let mut latest_cache_hit_rate: Option<f64> = None;

        for entry in session.entries() {
            match &entry {
                SessionEntry::Usage(e) => add_usage_to_totals(&mut usage_totals, &e.usage),
                SessionEntry::Message(e) => match &e.message {
                    AgentMessage::Assistant(m) => {
                        add_usage_to_totals(&mut usage_totals, &m.usage);
                        let latest_prompt_tokens = (m.usage.input + m.usage.cache_read + m.usage.cache_write) as f64;
                        latest_cache_hit_rate = (latest_prompt_tokens > 0.0)
                            .then(|| (m.usage.cache_read as f64 / latest_prompt_tokens) * 100.0);
                    }
                    AgentMessage::ToolResult(m) => {
                        if let Some(usage) = &m.usage {
                            add_usage_to_totals(&mut usage_totals, usage);
                        }
                    }
                    _ => {}
                },
                SessionEntry::BranchSummary(e) => {
                    if let Some(usage) = &e.usage {
                        add_usage_to_totals(&mut usage_totals, usage);
                    }
                }
                SessionEntry::Compaction(e) => {
                    if let Some(usage) = &e.usage {
                        add_usage_to_totals(&mut usage_totals, usage);
                    }
                }
                _ => {}
            }
        }

        // Calculate context usage from session (handles compaction correctly).
        // After compaction, tokens are unknown until the next LLM response.
        let context_usage = session.context_usage();
        self.st.lock().session_stats = Some(CachedStats {
            session_ptr,
            session_id,
            leaf_id,
            entry_count,
            limits_model,
            usage_totals: usage_totals.clone(),
            latest_cache_hit_rate,
            context_usage: context_usage.clone(),
        });
        (usage_totals, latest_cache_hit_rate, context_usage)
    }
}

impl Component for FooterComponent {
    fn render(&self, width: usize) -> Vec<String> {
        let t = theme();
        let (session, auto_compact_enabled) = {
            let st = self.st.lock();
            (st.session.clone(), st.auto_compact_enabled)
        };
        let model = session.model();
        let (usage_totals, latest_cache_hit_rate, context_usage) = self.session_stats(&session);
        let context_window = context_usage
            .as_ref()
            .map(|c| c.context_window)
            .or_else(|| model.as_ref().map(|m| m.context_window))
            .unwrap_or(0) as i64;
        let context_percent_value = context_usage.as_ref().and_then(|c| c.percent).unwrap_or(0.0);
        // TS: `contextUsage?.percent !== null` - undefined usage still shows a number.
        let context_percent = match &context_usage {
            Some(c) if c.percent.is_none() => "?".to_string(),
            _ => js_to_fixed(context_percent_value, 1),
        };

        // Replace home directory with ~
        let home = std::env::var("HOME").ok().filter(|h| !h.is_empty()).or_else(|| std::env::var("USERPROFILE").ok());
        let mut pwd = format_cwd_for_footer(&session.cwd(), home.as_deref());

        // Add git branch if available
        if let Some(branch) = self.footer_data.get_git_branch().filter(|b| !b.is_empty()) {
            pwd = format!("{pwd} ({branch})");
        }

        // Add session name if set
        if let Some(session_name) = session.session_name().filter(|n| !n.is_empty()) {
            pwd = format!("{pwd} • {session_name}");
        }

        // Build stats line
        let mut stats_parts: Vec<String> = Vec::new();
        if usage_totals.input != 0.0 {
            stats_parts.push(format!("↑{}", format_tokens(usage_totals.input as i64)));
        }
        if usage_totals.output != 0.0 {
            stats_parts.push(format!("↓{}", format_tokens(usage_totals.output as i64)));
        }
        if usage_totals.cache_read != 0.0 {
            stats_parts.push(format!("R{}", format_tokens(usage_totals.cache_read as i64)));
        }
        if usage_totals.cache_write != 0.0 {
            stats_parts.push(format!("W{}", format_tokens(usage_totals.cache_write as i64)));
        }
        if (usage_totals.cache_read > 0.0 || usage_totals.cache_write > 0.0)
            && let Some(rate) = latest_cache_hit_rate
        {
            stats_parts.push(format!("CH{}%", js_to_fixed(rate, 1)));
        }

        // Kimi Coding is subscription-backed despite using API-key authentication.
        let using_subscription = match &model {
            Some(m) => m.provider == "kimi-coding" || session.is_using_subscription(&m.provider),
            None => false,
        };
        if usage_totals.cost != 0.0 || using_subscription {
            let cost_str =
                format!("${}{}", js_to_fixed(usage_totals.cost, 3), if using_subscription { " (sub)" } else { "" });
            stats_parts.push(cost_str);
        }

        // Colorize context percentage based on usage
        let auto_indicator = if auto_compact_enabled { " (auto)" } else { "" };
        let context_percent_display = if context_percent == "?" {
            format!("?/{}{auto_indicator}", format_tokens(context_window))
        } else {
            format!("{context_percent}%/{}{auto_indicator}", format_tokens(context_window))
        };
        let context_percent_str = if context_percent_value > 90.0 {
            t.fg(&ThemeColor::Error, &context_percent_display)
        } else if context_percent_value > 70.0 {
            t.fg(&ThemeColor::Warning, &context_percent_display)
        } else {
            context_percent_display
        };
        stats_parts.push(context_percent_str);
        if are_experimental_features_enabled() {
            stats_parts.push(format!("{} {}", t.fg(&ThemeColor::Dim, "•"), t.bold(&t.fg(&ThemeColor::Warning, "xp"))));
        }

        let mut stats_left = stats_parts.join(" ");

        // Add model name on the right side, plus thinking level if model supports it
        let model_name = model.as_ref().map(|m| m.id.clone()).filter(|id| !id.is_empty()).unwrap_or_else(|| "no-model".to_string());

        let mut stats_left_width = visible_width(&stats_left);

        // If statsLeft is too wide, truncate it
        if stats_left_width > width {
            stats_left = truncate_to_width(&stats_left, width, "...", false);
            stats_left_width = visible_width(&stats_left);
        }

        // Calculate available space for padding (minimum 2 spaces between stats and model)
        let min_padding = 2usize;

        // Add thinking level indicator if model supports reasoning
        let mut right_side_without_provider = model_name.clone();
        if model.as_ref().is_some_and(|m| m.reasoning) {
            let thinking_level = session.thinking_level();
            right_side_without_provider = if thinking_level == ThinkingLevel::Off {
                format!("{model_name} • thinking off")
            } else {
                format!("{model_name} • {}", thinking_level.as_str())
            };
        }
        // A virtual model routes each request; show where the latest response went.
        if let Some(routed) = session.routed_model() {
            let level = routed.thinking_level.map(|l| format!(" • {}", l.as_str())).unwrap_or_default();
            right_side_without_provider.push_str(&format!(" → {}{level}", routed.model.id));
        }

        // Prepend the provider in parentheses if there are multiple providers and there's enough room
        let mut right_side = right_side_without_provider.clone();
        if self.footer_data.get_available_provider_count() > 1
            && let Some(m) = &model
        {
            right_side = format!("({}) {right_side_without_provider}", m.provider);
            if stats_left_width + min_padding + visible_width(&right_side) > width {
                // Too wide, fall back
                right_side = right_side_without_provider;
            }
        }

        let right_side_width = visible_width(&right_side);
        let total_needed = stats_left_width + min_padding + right_side_width;

        let stats_line = if total_needed <= width {
            // Both fit - add padding to right-align model
            let padding = " ".repeat(width - stats_left_width - right_side_width);
            format!("{stats_left}{padding}{right_side}")
        } else {
            // Need to truncate right side
            let available_for_right = width as i64 - stats_left_width as i64 - min_padding as i64;
            if available_for_right > 0 {
                let truncated_right = truncate_to_width(&right_side, available_for_right as usize, "", false);
                let truncated_right_width = visible_width(&truncated_right);
                let padding = " ".repeat(width.saturating_sub(stats_left_width + truncated_right_width));
                format!("{stats_left}{padding}{truncated_right}")
            } else {
                // Not enough space for right side at all
                stats_left.clone()
            }
        };

        // Apply dim to each part separately. statsLeft may contain color codes (for context %)
        // that end with a reset, which would clear an outer dim wrapper. So we dim the parts
        // before and after the colored section independently.
        let dim_stats_left = t.fg(&ThemeColor::Dim, &stats_left);
        let remainder = &stats_line[stats_left.len()..]; // padding + rightSide
        let dim_remainder = t.fg(&ThemeColor::Dim, remainder);

        let pwd_line = truncate_to_width(&t.fg(&ThemeColor::Dim, &pwd), width, &t.fg(&ThemeColor::Dim, "..."), false);
        let mut lines = vec![pwd_line, format!("{dim_stats_left}{dim_remainder}")];

        // Add extension statuses on a single line, sorted by key alphabetically
        let extension_statuses = self.footer_data.get_extension_statuses();
        if !extension_statuses.is_empty() {
            let mut sorted: Vec<(String, String)> = extension_statuses.into_iter().collect();
            sorted.sort_by(|(a, _), (b, _)| locale_compare(a, b));
            let status_line = sorted.iter().map(|(_, text)| sanitize_status_text(text)).collect::<Vec<_>>().join(" ");
            // Truncate to terminal width with dim ellipsis for consistency with footer style
            lines.push(truncate_to_width(&status_line, width, &t.fg(&ThemeColor::Dim, "..."), false));
        }

        lines
    }

    /// No-op: git branch caching now handled by provider.
    /// Kept for compatibility with existing call sites in interactive-mode.
    fn invalidate(&self) {
        // No-op: git branch is cached/invalidated by provider
    }
}
