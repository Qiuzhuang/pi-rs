//! Port of `packages/coding-agent/src/modes/interactive/components/tree-selector.ts`.

use crate::core::session_manager::{SessionEntry, SessionTreeNode};
use crate::core::tools::render_utils::format_read_line_range_ref;
use crate::modes::interactive::components::dynamic_border::DynamicBorder;
use crate::modes::interactive::components::keybinding_hints::{format_key_text, key_hint};
use crate::modes::interactive::theme::theme::{ThemeBg, ThemeColor, theme};
use indexmap::IndexMap;
use parking_lot::Mutex;
use pi_tui::components::input::Input;
use pi_tui::components::spacer::Spacer;
use pi_tui::components::text::Text;
use pi_tui::keybindings::get_keybindings;
use pi_tui::tui::{Component, ComponentRef, Container, MouseEvent, MouseResult};
use pi_tui::utils::{js_trim, js_trim_end, slice_by_column, truncate_to_width, visible_width, wrap_text_with_ansi};
use std::collections::{HashMap, HashSet};
use std::time::Duration;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::sync::Arc;

/// Gutter info: position (displayIndent where connector was) and whether to show │
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GutterInfo {
    pub position: i64,
    pub show: bool,
}

/// Flattened tree node for navigation
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FlatNode {
    pub node: SessionTreeNode,
    /// Indentation level (each level = 3 chars)
    pub indent: i64,
    /// Whether to show connector (├─ or └─) - true if parent has multiple children
    pub show_connector: bool,
    /// If showConnector, true = last sibling (└─), false = not last (├─)
    pub is_last: bool,
    /// Gutter info for each ancestor branch point
    pub gutters: Vec<GutterInfo>,
    /// True if this node is a root under a virtual branching root (multiple roots)
    pub is_virtual_root_child: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HorizontalViewportRow {
    pub gutter: String,
    pub body: String,
    pub anchor_col: i64,
    pub body_width: i64,
    pub is_selected: bool,
}

/// Filter mode for tree display
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum FilterMode {
    #[serde(rename = "default")]
    Default,
    #[serde(rename = "no-tools")]
    NoTools,
    #[serde(rename = "user-only")]
    UserOnly,
    #[serde(rename = "labeled-only")]
    LabeledOnly,
    #[serde(rename = "all")]
    All,
}

impl FilterMode {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Default => "default",
            Self::NoTools => "no-tools",
            Self::UserOnly => "user-only",
            Self::LabeledOnly => "labeled-only",
            Self::All => "all",
        }
    }
}

/// Tool call info for lookup
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolCallInfo {
    pub name: String,
    pub arguments: IndexMap<String, Value>,
}

const TREE_GUTTER_WIDTH: i64 = 2;
const MIN_VISIBLE_ANCHOR_CONTENT_WIDTH: i64 = 4;
const MAX_VISIBLE_ANCHOR_CONTENT_WIDTH: i64 = 20;
const MIN_ANCHOR_CONTEXT_WIDTH: i64 = 2;
const MAX_ANCHOR_CONTEXT_WIDTH: i64 = 12;

/// Render tree rows into a horizontally clipped viewport.
///
/// The tree gutter is always kept visible. The row bodies are shifted left only
/// when the selected row's anchor (the start of its entry text after tree
/// indentation/markers) would otherwise be too far right to see useful content.
fn render_horizontal_viewport(rows: &[HorizontalViewportRow], width: usize) -> Vec<String> {
    let viewport_width = (width as i64 - TREE_GUTTER_WIDTH).max(0);
    let max_body_width = rows.iter().map(|r| r.body_width).max().unwrap_or(0).max(0);
    let max_horizontal_scroll = (max_body_width - viewport_width).max(0);
    let selected_row = rows.iter().find(|r| r.is_selected);

    // Only pan horizontally when needed to keep enough selected-row content visible after its anchor.
    let mut horizontal_scroll = 0;
    if let Some(selected_row) = selected_row {
        if max_horizontal_scroll > 0 {
            let min_visible_anchor_content_width =
                MAX_VISIBLE_ANCHOR_CONTENT_WIDTH.min(MIN_VISIBLE_ANCHOR_CONTENT_WIDTH.max(viewport_width / 3));
            if selected_row.anchor_col > viewport_width - min_visible_anchor_content_width {
                let anchor_context_width =
                    MAX_ANCHOR_CONTEXT_WIDTH.min(MIN_ANCHOR_CONTEXT_WIDTH.max(viewport_width / 4));
                horizontal_scroll = max_horizontal_scroll.min(selected_row.anchor_col - anchor_context_width);
            }
        }
    }

    // Clip only the body; the fixed-width gutter remains visible as navigation context.
    rows.iter()
        .map(|row| {
            let line = if horizontal_scroll > 0 {
                format!(
                    "{}{}\x1b[0m",
                    row.gutter,
                    slice_by_column(&row.body, horizontal_scroll as usize, viewport_width as usize, true)
                )
            } else {
                format!("{}{}", row.gutter, row.body)
            };
            truncate_to_width(&line, width, "", false)
        })
        .collect()
}

// ---------------------------------------------------------------------------------------------
// JS-like helpers over JSON values (messages are inspected dynamically, as in TS)
// ---------------------------------------------------------------------------------------------

fn js_truthy(value: Option<&Value>) -> bool {
    match value {
        None | Some(Value::Null) => false,
        Some(Value::Bool(b)) => *b,
        Some(Value::Number(n)) => n.as_f64().is_some_and(|f| f != 0.0 && !f.is_nan()),
        Some(Value::String(s)) => !s.is_empty(),
        Some(_) => true,
    }
}

/// `String(value)`.
fn js_string(value: &Value) -> String {
    match value {
        Value::Null => "null".to_string(),
        Value::Bool(b) => b.to_string(),
        Value::Number(n) => match n.as_f64() {
            Some(f) if f.fract() == 0.0 && f.abs() < 1e21 => format!("{}", f as i64),
            _ => n.to_string(),
        },
        Value::String(s) => s.clone(),
        Value::Array(items) => items
            .iter()
            .map(|v| if v.is_null() { String::new() } else { js_string(v) })
            .collect::<Vec<_>>()
            .join(","),
        Value::Object(_) => "[object Object]".to_string(),
    }
}

/// `a || b || fallback` over JSON values, stringified.
fn first_truthy(args: &IndexMap<String, Value>, keys: &[&str], fallback: &str) -> String {
    for key in keys {
        let v = args.get(*key);
        if js_truthy(v) {
            return js_string(v.unwrap_or(&Value::Null));
        }
    }
    fallback.to_string()
}

/// `s.slice(0, n)` in UTF-16 code units (never splitting a code point).
fn js_slice(s: &str, n: usize) -> String {
    let mut units = 0;
    let mut out = String::new();
    for c in s.chars() {
        units += c.len_utf16();
        if units > n {
            break;
        }
        out.push(c);
    }
    out
}

fn normalize_text(s: &str) -> String {
    js_trim(&s.replace(['\n', '\t'], " ")).to_string()
}

fn extract_full_content(content: Option<&Value>) -> String {
    match content {
        Some(Value::String(s)) => s.clone(),
        Some(Value::Array(blocks)) => {
            let mut result = String::new();
            for block in blocks {
                if block.get("type").and_then(Value::as_str) == Some("text") {
                    if let Some(text) = block.get("text") {
                        result.push_str(&js_string(text));
                    }
                }
            }
            result
        }
        _ => String::new(),
    }
}

fn extract_content(content: Option<&Value>) -> String {
    js_slice(&extract_full_content(content), 200)
}

fn has_text_content(content: Option<&Value>) -> bool {
    match content {
        Some(Value::String(s)) => !js_trim(s).is_empty(),
        Some(Value::Array(blocks)) => blocks.iter().any(|c| {
            c.get("type").and_then(Value::as_str) == Some("text")
                && c.get("text").and_then(Value::as_str).is_some_and(|t| !js_trim(t).is_empty())
        }),
        _ => false,
    }
}

fn str_field<'a>(value: &'a Value, key: &str) -> Option<&'a str> {
    value.get(key).and_then(Value::as_str)
}

// ---------------------------------------------------------------------------------------------
// TreeList
// ---------------------------------------------------------------------------------------------

/// Internal flattened node: the entry (without its subtree) plus visual properties.
struct TreeEntry {
    entry: SessionEntry,
    /// The entry's message (or custom message content) as JSON, for TS-style dynamic inspection.
    message: Option<Value>,
    label: Option<String>,
    label_timestamp: Option<String>,
    indent: i64,
    show_connector: bool,
    is_last: bool,
    gutters: Vec<GutterInfo>,
    is_virtual_root_child: bool,
}

impl TreeEntry {
    fn id(&self) -> &str {
        self.entry.id()
    }

    fn parent_id(&self) -> Option<&str> {
        self.entry.parent_id()
    }

    fn role(&self) -> Option<&str> {
        match &self.entry {
            SessionEntry::Message(_) => self.message.as_ref().and_then(|m| str_field(m, "role")),
            _ => None,
        }
    }
}

type LabelEditFn = Arc<dyn Fn(String, Option<String>) + Send + Sync>;

#[derive(Default)]
struct TreeCallbacks {
    on_select: Option<Arc<dyn Fn(String) + Send + Sync>>,
    on_cancel: Option<Arc<dyn Fn() + Send + Sync>>,
    on_copy: Option<Arc<dyn Fn(Option<String>) + Send + Sync>>,
    on_label_edit: Option<LabelEditFn>,
}

struct TreeState {
    flat_nodes: Vec<TreeEntry>,
    /// Indices into `flat_nodes`.
    filtered_nodes: Vec<usize>,
    selected_index: usize,
    current_leaf_id: Option<String>,
    max_visible_lines: usize,
    filter_mode: FilterMode,
    search_query: String,
    tool_call_map: HashMap<String, ToolCallInfo>,
    multiple_roots: bool,
    show_label_timestamps: bool,
    active_path_ids: HashSet<String>,
    visible_parent_map: HashMap<String, Option<String>>,
    visible_children_map: HashMap<Option<String>, Vec<String>>,
    last_selected_id: Option<String>,
    folded_nodes: HashSet<String>,
}

const FILTER_MODES: [FilterMode; 5] =
    [FilterMode::Default, FilterMode::NoTools, FilterMode::UserOnly, FilterMode::LabeledOnly, FilterMode::All];

impl TreeState {
    fn selected_id(&self) -> Option<String> {
        self.filtered_nodes.get(self.selected_index).map(|&i| self.flat_nodes[i].id().to_string())
    }

    fn index_by_id(&self) -> HashMap<&str, usize> {
        self.flat_nodes.iter().enumerate().map(|(i, n)| (n.id(), i)).collect()
    }

    /// Find the index of the nearest visible entry, walking up the parent chain if needed.
    /// Returns the index in filteredNodes, or the last index as fallback.
    fn find_nearest_visible_index(&self, entry_id: Option<&str>) -> usize {
        if self.filtered_nodes.is_empty() {
            return 0;
        }
        let entry_map = self.index_by_id();
        let visible_id_to_index: HashMap<&str, usize> =
            self.filtered_nodes.iter().enumerate().map(|(pos, &i)| (self.flat_nodes[i].id(), pos)).collect();

        let mut current_id = entry_id.map(str::to_string);
        let mut guard = 0;
        while let Some(id) = current_id {
            if let Some(&index) = visible_id_to_index.get(id.as_str()) {
                return index;
            }
            let Some(&node) = entry_map.get(id.as_str()) else { break };
            current_id = self.flat_nodes[node].parent_id().map(str::to_string);
            guard += 1;
            if guard > self.flat_nodes.len() {
                break;
            }
        }

        // Fallback: last visible entry
        self.filtered_nodes.len() - 1
    }

    /// Build the set of entry IDs on the path from root to current leaf
    fn build_active_path(&mut self) {
        self.active_path_ids.clear();
        let Some(leaf) = self.current_leaf_id.clone().filter(|l| !l.is_empty()) else { return };
        let entry_map: HashMap<String, Option<String>> = self
            .flat_nodes
            .iter()
            .map(|n| (n.id().to_string(), n.parent_id().map(str::to_string)))
            .collect();
        let mut current_id = Some(leaf);
        while let Some(id) = current_id.filter(|id| !id.is_empty()) {
            if !self.active_path_ids.insert(id.clone()) {
                break;
            }
            let Some(parent) = entry_map.get(&id) else { break };
            current_id = parent.clone();
        }
    }

    fn flatten_tree(&mut self, roots: &[SessionTreeNode]) {
        let mut result: Vec<TreeEntry> = Vec::new();
        self.tool_call_map.clear();

        // Determine which subtrees contain the active leaf (to sort current branch first)
        // Use iterative post-order traversal to avoid stack overflow
        let mut contains_active: HashMap<*const SessionTreeNode, bool> = HashMap::new();
        let leaf_id = self.current_leaf_id.clone();
        {
            let mut all_nodes: Vec<&SessionTreeNode> = Vec::new();
            let mut pre_order_stack: Vec<&SessionTreeNode> = roots.iter().collect();
            while let Some(node) = pre_order_stack.pop() {
                all_nodes.push(node);
                for child in node.children.iter().rev() {
                    pre_order_stack.push(child);
                }
            }
            for node in all_nodes.iter().rev() {
                let mut has = leaf_id.as_deref().is_some_and(|l| node.entry.id() == l);
                for child in &node.children {
                    if contains_active.get(&(child as *const _)).copied().unwrap_or(false) {
                        has = true;
                    }
                }
                contains_active.insert(*node as *const _, has);
            }
        }
        let is_active = |n: &SessionTreeNode| contains_active.get(&(n as *const _)).copied().unwrap_or(false);

        // Stack items: [node, indent, justBranched, showConnector, isLast, gutters, isVirtualRootChild]
        type StackItem<'a> = (&'a SessionTreeNode, i64, bool, bool, bool, Vec<GutterInfo>, bool);
        let mut stack: Vec<StackItem> = Vec::new();

        // Add roots in reverse order, prioritizing the one containing the active leaf
        // If multiple roots, treat them as children of a virtual root that branches
        let multiple_roots = roots.len() > 1;
        let mut ordered_roots: Vec<&SessionTreeNode> = roots.iter().collect();
        ordered_roots.sort_by_key(|r| std::cmp::Reverse(is_active(r) as u8));
        for (i, root) in ordered_roots.iter().enumerate().rev() {
            let is_last = i == ordered_roots.len() - 1;
            stack.push((
                root,
                if multiple_roots { 1 } else { 0 },
                multiple_roots,
                multiple_roots,
                is_last,
                Vec::new(),
                multiple_roots,
            ));
        }

        while let Some((node, indent, just_branched, show_connector, is_last, gutters, is_virtual_root_child)) =
            stack.pop()
        {
            // Extract tool calls from assistant messages for later lookup
            let message = match &node.entry {
                SessionEntry::Message(m) => serde_json::to_value(&m.message).ok(),
                SessionEntry::CustomMessage(m) => serde_json::to_value(&m.content).ok(),
                _ => None,
            };
            if let (SessionEntry::Message(_), Some(msg)) = (&node.entry, &message) {
                if str_field(msg, "role") == Some("assistant") {
                    if let Some(Value::Array(content)) = msg.get("content") {
                        for block in content {
                            if str_field(block, "type") == Some("toolCall") {
                                let id = str_field(block, "id").unwrap_or_default().to_string();
                                let name = str_field(block, "name").unwrap_or_default().to_string();
                                let arguments = match block.get("arguments") {
                                    Some(Value::Object(map)) => map.iter().map(|(k, v)| (k.clone(), v.clone())).collect(),
                                    _ => IndexMap::new(),
                                };
                                self.tool_call_map.insert(id, ToolCallInfo { name, arguments });
                            }
                        }
                    }
                }
            }

            result.push(TreeEntry {
                entry: node.entry.clone(),
                message,
                label: node.label.clone(),
                label_timestamp: node.label_timestamp.clone(),
                indent,
                show_connector,
                is_last,
                gutters: gutters.clone(),
                is_virtual_root_child,
            });

            let children = &node.children;
            let multiple_children = children.len() > 1;

            // Order children so the branch containing the active leaf comes first
            let mut ordered_children: Vec<&SessionTreeNode> = children.iter().filter(|c| is_active(c)).collect();
            ordered_children.extend(children.iter().filter(|c| !is_active(c)));

            // Calculate child indent
            let child_indent = if multiple_children || (just_branched && indent > 0) { indent + 1 } else { indent };

            // Build gutters for children
            let connector_displayed = show_connector && !is_virtual_root_child;
            let current_display_indent = if self.multiple_roots { (indent - 1).max(0) } else { indent };
            let connector_position = (current_display_indent - 1).max(0);
            let child_gutters = if connector_displayed {
                let mut g = gutters.clone();
                g.push(GutterInfo { position: connector_position, show: !is_last });
                g
            } else {
                gutters
            };

            // Add children in reverse order
            for (i, child) in ordered_children.iter().enumerate().rev() {
                let child_is_last = i == ordered_children.len() - 1;
                stack.push((
                    child,
                    child_indent,
                    multiple_children,
                    multiple_children,
                    child_is_last,
                    child_gutters.clone(),
                    false,
                ));
            }
        }

        self.flat_nodes = result;
    }

    fn passes_filters(&self, flat: &TreeEntry, search_tokens: &[String]) -> bool {
        let entry = &flat.entry;
        if matches!(entry, SessionEntry::Usage(_)) {
            return false;
        }
        let is_current_leaf = self.current_leaf_id.as_deref() == Some(flat.id());
        let role = flat.role();

        // Skip assistant messages with only tool calls (no text) unless error/aborted
        // Always show current leaf so active position is visible
        if role == Some("assistant") && !is_current_leaf {
            let msg = flat.message.as_ref();
            let has_text = has_text_content(msg.and_then(|m| m.get("content")));
            let stop_reason = msg.and_then(|m| str_field(m, "stopReason")).unwrap_or("");
            let is_error_or_aborted = !stop_reason.is_empty() && stop_reason != "stop" && stop_reason != "toolUse";
            // Only hide if no text AND not an error/aborted message
            if !has_text && !is_error_or_aborted {
                return false;
            }
        }

        // Entry types hidden in default view (settings/bookkeeping)
        let is_settings_entry = matches!(
            entry,
            SessionEntry::Label(_)
                | SessionEntry::ContextEdit(_)
                | SessionEntry::Custom(_)
                | SessionEntry::ModelChange(_)
                | SessionEntry::ThinkingLevelChange(_)
                | SessionEntry::SessionInfo(_)
        );

        let passes_filter = match self.filter_mode {
            // Just user messages
            FilterMode::UserOnly => role == Some("user"),
            // Default minus tool results
            FilterMode::NoTools => !is_settings_entry && role != Some("toolResult"),
            // Just labeled entries
            FilterMode::LabeledOnly => flat.label.is_some(),
            // Show everything
            FilterMode::All => true,
            // Default mode: hide settings/bookkeeping entries
            FilterMode::Default => !is_settings_entry,
        };
        if !passes_filter {
            return false;
        }

        // Apply search filter
        if !search_tokens.is_empty() {
            let node_text = get_searchable_text(flat).to_lowercase();
            return search_tokens.iter().all(|token| node_text.contains(token.as_str()));
        }
        true
    }

    fn apply_filter(&mut self) {
        // Update lastSelectedId only when we have a valid selection (non-empty list)
        // This preserves the selection when switching through empty filter results
        if !self.filtered_nodes.is_empty() {
            if let Some(id) = self.selected_id() {
                self.last_selected_id = Some(id);
            }
        }

        let lower = self.search_query.to_lowercase();
        let search_tokens: Vec<String> =
            lower.split(char::is_whitespace).filter(|t| !t.is_empty()).map(str::to_string).collect();

        let mut filtered: Vec<usize> = (0..self.flat_nodes.len())
            .filter(|&i| self.passes_filters(&self.flat_nodes[i], &search_tokens))
            .collect();

        // Filter out descendants of folded nodes.
        if !self.folded_nodes.is_empty() {
            let mut skip_set: HashSet<String> = HashSet::new();
            for flat in &self.flat_nodes {
                if let Some(parent_id) = flat.parent_id() {
                    if self.folded_nodes.contains(parent_id) || skip_set.contains(parent_id) {
                        skip_set.insert(flat.id().to_string());
                    }
                }
            }
            filtered.retain(|&i| !skip_set.contains(self.flat_nodes[i].id()));
        }
        self.filtered_nodes = filtered;

        // Recalculate visual structure (indent, connectors, gutters) based on visible tree
        self.recalculate_visual_structure();

        // Try to preserve cursor on the same node, or find nearest visible ancestor
        if let Some(last) = self.last_selected_id.clone() {
            self.selected_index = self.find_nearest_visible_index(Some(&last));
        } else if self.selected_index >= self.filtered_nodes.len() {
            // Clamp index if out of bounds
            self.selected_index = self.filtered_nodes.len().saturating_sub(1);
        }

        // Update lastSelectedId to the actual selection (may have changed due to parent walk)
        if !self.filtered_nodes.is_empty() {
            if let Some(id) = self.selected_id() {
                self.last_selected_id = Some(id);
            }
        }
    }

    /// Recompute indentation/connectors for the filtered view
    ///
    /// Filtering can hide intermediate entries; descendants attach to the nearest visible ancestor.
    /// Keep indentation semantics aligned with flattenTree() so single-child chains don't drift right.
    fn recalculate_visual_structure(&mut self) {
        if self.filtered_nodes.is_empty() {
            return;
        }

        let visible_ids: HashSet<String> =
            self.filtered_nodes.iter().map(|&i| self.flat_nodes[i].id().to_string()).collect();
        let parent_of: HashMap<String, Option<String>> = self
            .flat_nodes
            .iter()
            .map(|n| (n.id().to_string(), n.parent_id().map(str::to_string)))
            .collect();

        // Find nearest visible ancestor for a node
        let find_visible_ancestor = |node_id: &str| -> Option<String> {
            let mut current_id = parent_of.get(node_id).cloned().flatten();
            let mut guard = 0;
            while let Some(id) = current_id {
                if visible_ids.contains(&id) {
                    return Some(id);
                }
                current_id = parent_of.get(&id).cloned().flatten();
                guard += 1;
                if guard > parent_of.len() {
                    break;
                }
            }
            None
        };

        let mut visible_parent: HashMap<String, Option<String>> = HashMap::new();
        let mut visible_children: HashMap<Option<String>, Vec<String>> = HashMap::new();
        visible_children.insert(None, Vec::new()); // root-level nodes

        for &i in &self.filtered_nodes {
            let node_id = self.flat_nodes[i].id().to_string();
            let ancestor_id = find_visible_ancestor(&node_id);
            visible_parent.insert(node_id.clone(), ancestor_id.clone());
            visible_children.entry(ancestor_id).or_default().push(node_id);
        }

        // Update multipleRoots based on visible roots
        let visible_root_ids = visible_children.get(&None).cloned().unwrap_or_default();
        self.multiple_roots = visible_root_ids.len() > 1;

        let filtered_node_map: HashMap<String, usize> =
            self.filtered_nodes.iter().map(|&i| (self.flat_nodes[i].id().to_string(), i)).collect();

        type StackItem = (String, i64, bool, bool, bool, Vec<GutterInfo>, bool);
        let mut stack: Vec<StackItem> = Vec::new();
        let multiple_roots = self.multiple_roots;
        for (i, id) in visible_root_ids.iter().enumerate().rev() {
            let is_last = i == visible_root_ids.len() - 1;
            stack.push((
                id.clone(),
                if multiple_roots { 1 } else { 0 },
                multiple_roots,
                multiple_roots,
                is_last,
                Vec::new(),
                multiple_roots,
            ));
        }

        while let Some((node_id, indent, just_branched, show_connector, is_last, gutters, is_virtual_root_child)) =
            stack.pop()
        {
            let Some(&flat_index) = filtered_node_map.get(&node_id) else { continue };
            {
                let flat = &mut self.flat_nodes[flat_index];
                flat.indent = indent;
                flat.show_connector = show_connector;
                flat.is_last = is_last;
                flat.gutters = gutters.clone();
                flat.is_virtual_root_child = is_virtual_root_child;
            }

            let children = visible_children.get(&Some(node_id.clone())).cloned().unwrap_or_default();
            let multiple_children = children.len() > 1;
            let child_indent = if multiple_children || (just_branched && indent > 0) { indent + 1 } else { indent };

            let connector_displayed = show_connector && !is_virtual_root_child;
            let current_display_indent = if multiple_roots { (indent - 1).max(0) } else { indent };
            let connector_position = (current_display_indent - 1).max(0);
            let child_gutters = if connector_displayed {
                let mut g = gutters.clone();
                g.push(GutterInfo { position: connector_position, show: !is_last });
                g
            } else {
                gutters
            };

            for (i, child) in children.iter().enumerate().rev() {
                let child_is_last = i == children.len() - 1;
                stack.push((
                    child.clone(),
                    child_indent,
                    multiple_children,
                    multiple_children,
                    child_is_last,
                    child_gutters.clone(),
                    false,
                ));
            }
        }

        // Store visible tree maps for ancestor/descendant lookups in navigation
        self.visible_parent_map = visible_parent;
        self.visible_children_map = visible_children;
    }

    fn get_status_labels(&self) -> String {
        let mut labels = String::new();
        match self.filter_mode {
            FilterMode::NoTools => labels.push_str(" [no-tools]"),
            FilterMode::UserOnly => labels.push_str(" [user]"),
            FilterMode::LabeledOnly => labels.push_str(" [labeled]"),
            FilterMode::All => labels.push_str(" [all]"),
            FilterMode::Default => {}
        }
        if self.show_label_timestamps {
            labels.push_str(" [+label time]");
        }
        labels
    }

    /// Whether a node can be folded. A node is foldable if it has visible children
    /// and is either a root (no visible parent) or a segment start (visible parent
    /// has multiple visible children).
    fn is_foldable(&self, entry_id: &str) -> bool {
        let Some(children) = self.visible_children_map.get(&Some(entry_id.to_string())) else { return false };
        if children.is_empty() {
            return false;
        }
        match self.visible_parent_map.get(entry_id) {
            None | Some(None) => true,
            Some(Some(parent_id)) => {
                self.visible_children_map.get(&Some(parent_id.clone())).is_some_and(|siblings| siblings.len() > 1)
            }
        }
    }

    /// Find the index of the next branch segment start in the given direction.
    /// A segment start is the first child of a branch point.
    ///
    /// "up" walks the visible parent chain; "down" walks visible children
    /// (always following the first child).
    fn find_branch_segment_start(&self, up: bool) -> usize {
        let Some(selected_id) = self.selected_id() else { return self.selected_index };
        let index_by_entry_id: HashMap<&str, usize> =
            self.filtered_nodes.iter().enumerate().map(|(pos, &i)| (self.flat_nodes[i].id(), pos)).collect();
        let lookup = |id: &str| index_by_entry_id.get(id).copied().unwrap_or(self.selected_index);
        let mut current_id = selected_id;
        let mut guard = 0;
        if !up {
            loop {
                let children = self.visible_children_map.get(&Some(current_id.clone())).cloned().unwrap_or_default();
                if children.is_empty() {
                    return lookup(&current_id);
                }
                if children.len() > 1 {
                    return lookup(&children[0]);
                }
                current_id = children[0].clone();
                guard += 1;
                if guard > self.flat_nodes.len() {
                    return self.selected_index;
                }
            }
        }

        loop {
            let parent_id = self.visible_parent_map.get(&current_id).cloned().flatten();
            let Some(parent_id) = parent_id else { return lookup(&current_id) };
            let children = self.visible_children_map.get(&Some(parent_id.clone())).cloned().unwrap_or_default();
            if children.len() > 1 {
                let segment_start = lookup(&current_id);
                if segment_start < self.selected_index {
                    return segment_start;
                }
            }
            current_id = parent_id;
            guard += 1;
            if guard > self.flat_nodes.len() {
                return self.selected_index;
            }
        }
    }

    fn get_entry_display_text(&self, flat: &TreeEntry, is_selected: bool) -> String {
        let t = theme();
        let result = match &flat.entry {
            SessionEntry::Message(_) => {
                let empty = Value::Null;
                let msg = flat.message.as_ref().unwrap_or(&empty);
                let role = str_field(msg, "role").unwrap_or("");
                match role {
                    "user" => {
                        let content = normalize_text(&extract_content(msg.get("content")));
                        t.fg(&ThemeColor::Accent, "user: ") + &content
                    }
                    "assistant" => {
                        let text_content = normalize_text(&extract_content(msg.get("content")));
                        let prefix = t.fg(&ThemeColor::Success, "assistant: ");
                        if !text_content.is_empty() {
                            prefix + &text_content
                        } else if str_field(msg, "stopReason") == Some("aborted") {
                            prefix + &t.fg(&ThemeColor::Muted, "(aborted)")
                        } else if let Some(err) = str_field(msg, "errorMessage").filter(|e| !e.is_empty()) {
                            let err_msg = js_slice(&normalize_text(err), 80);
                            prefix + &t.fg(&ThemeColor::Error, &err_msg)
                        } else {
                            prefix + &t.fg(&ThemeColor::Muted, "(no content)")
                        }
                    }
                    "toolResult" => {
                        let tool_call = str_field(msg, "toolCallId")
                            .filter(|id| !id.is_empty())
                            .and_then(|id| self.tool_call_map.get(id));
                        match tool_call {
                            Some(tc) => t.fg(&ThemeColor::Muted, &format_tool_call(&tc.name, &tc.arguments)),
                            None => t.fg(
                                &ThemeColor::Muted,
                                &format!("[{}]", str_field(msg, "toolName").unwrap_or("tool")),
                            ),
                        }
                    }
                    "bashExecution" => t.fg(
                        &ThemeColor::Dim,
                        &format!("[bash]: {}", normalize_text(str_field(msg, "command").unwrap_or(""))),
                    ),
                    _ => t.fg(&ThemeColor::Dim, &format!("[{role}]")),
                }
            }
            SessionEntry::CustomMessage(e) => {
                let content = match &flat.message {
                    Some(Value::String(s)) => s.clone(),
                    Some(Value::Array(blocks)) => blocks
                        .iter()
                        .filter(|c| str_field(c, "type") == Some("text"))
                        .map(|c| str_field(c, "text").unwrap_or("").to_string())
                        .collect::<String>(),
                    _ => String::new(),
                };
                t.fg(&ThemeColor::CustomMessageLabel, &format!("[{}]: ", e.custom_type)) + &normalize_text(&content)
            }
            SessionEntry::Compaction(e) => {
                let tokens = (e.tokens_before as f64 / 1000.0).round() as i64;
                t.fg(&ThemeColor::BorderAccent, &format!("[compaction: {tokens}k tokens]"))
            }
            SessionEntry::BranchSummary(e) => {
                t.fg(&ThemeColor::Warning, "[branch summary]: ") + &normalize_text(&e.summary)
            }
            SessionEntry::ModelChange(e) => t.fg(&ThemeColor::Dim, &format!("[model: {}]", e.model_id)),
            SessionEntry::ThinkingLevelChange(e) => {
                t.fg(&ThemeColor::Dim, &format!("[thinking: {}]", e.thinking_level))
            }
            SessionEntry::Custom(e) => t.fg(&ThemeColor::Dim, &format!("[custom: {}]", e.custom_type)),
            SessionEntry::ContextEdit(e) => t.fg(
                &ThemeColor::Dim,
                &format!(
                    "[context {}: {}]",
                    if e.replacement.is_none() { "omit" } else { "replace" },
                    e.target_id
                ),
            ),
            SessionEntry::Label(e) => {
                t.fg(&ThemeColor::Dim, &format!("[label: {}]", e.label.as_deref().unwrap_or("(cleared)")))
            }
            SessionEntry::SessionInfo(e) => match e.name.as_deref().filter(|n| !n.is_empty()) {
                Some(name) => [
                    t.fg(&ThemeColor::Dim, "[title: "),
                    t.fg(&ThemeColor::Dim, name),
                    t.fg(&ThemeColor::Dim, "]"),
                ]
                .join(""),
                None => [
                    t.fg(&ThemeColor::Dim, "[title: "),
                    t.italic(&t.fg(&ThemeColor::Dim, "empty")),
                    t.fg(&ThemeColor::Dim, "]"),
                ]
                .join(""),
            },
            SessionEntry::Usage(_) | SessionEntry::Unknown(_) => String::new(),
        };
        if is_selected { t.bold(&result) } else { result }
    }
}

/// Get searchable text content from a node
fn get_searchable_text(flat: &TreeEntry) -> String {
    let mut parts: Vec<String> = Vec::new();
    if let Some(label) = flat.label.as_deref().filter(|l| !l.is_empty()) {
        parts.push(label.to_string());
    }
    match &flat.entry {
        SessionEntry::Message(_) => {
            if let Some(msg) = &flat.message {
                let role = str_field(msg, "role").unwrap_or("");
                parts.push(role.to_string());
                if js_truthy(msg.get("content")) {
                    parts.push(extract_content(msg.get("content")));
                }
                if role == "bashExecution" {
                    if let Some(command) = str_field(msg, "command").filter(|c| !c.is_empty()) {
                        parts.push(command.to_string());
                    }
                }
            }
        }
        SessionEntry::CustomMessage(e) => {
            parts.push(e.custom_type.clone());
            match &flat.message {
                Some(Value::String(s)) => parts.push(s.clone()),
                other => parts.push(extract_content(other.as_ref())),
            }
        }
        SessionEntry::Compaction(_) => parts.push("compaction".into()),
        SessionEntry::BranchSummary(e) => {
            parts.push("branch summary".into());
            parts.push(e.summary.clone());
        }
        SessionEntry::SessionInfo(e) => {
            parts.push("title".into());
            if let Some(name) = e.name.as_deref().filter(|n| !n.is_empty()) {
                parts.push(name.to_string());
            }
        }
        SessionEntry::ModelChange(e) => {
            parts.push("model".into());
            parts.push(e.model_id.clone());
        }
        SessionEntry::ThinkingLevelChange(e) => {
            parts.push("thinking".into());
            parts.push(e.thinking_level.clone());
        }
        SessionEntry::Custom(e) => {
            parts.push("custom".into());
            parts.push(e.custom_type.clone());
        }
        SessionEntry::ContextEdit(e) => {
            parts.push("context edit".into());
            parts.push(if e.replacement.is_none() { "omit" } else { "replace" }.into());
            parts.push(e.target_id.clone());
        }
        SessionEntry::Label(e) => {
            parts.push("label".into());
            parts.push(e.label.clone().unwrap_or_default());
        }
        SessionEntry::Usage(_) | SessionEntry::Unknown(_) => {}
    }
    parts.join(" ")
}

fn get_entry_copy_text(flat: &TreeEntry) -> Option<String> {
    let text: Option<String> = match &flat.entry {
        SessionEntry::Message(_) => {
            let msg = flat.message.as_ref()?;
            let role = str_field(msg, "role").unwrap_or("");
            if role == "bashExecution" {
                str_field(msg, "command").map(str::to_string)
            } else if msg.get("content").is_some() {
                let mut text = extract_full_content(msg.get("content"));
                if text.is_empty() && role == "assistant" {
                    text = str_field(msg, "errorMessage").unwrap_or("").to_string();
                }
                Some(text)
            } else {
                None
            }
        }
        SessionEntry::CustomMessage(_) => Some(extract_full_content(flat.message.as_ref())),
        SessionEntry::Compaction(e) => Some(e.summary.clone()),
        SessionEntry::BranchSummary(e) => Some(e.summary.clone()),
        _ => None,
    };
    text.filter(|t| !js_trim(t).is_empty())
}

fn format_label_timestamp(timestamp: &str) -> String {
    use chrono::{Datelike, Local, Timelike};
    let Ok(parsed) = chrono::DateTime::parse_from_rfc3339(timestamp) else {
        return "NaN:NaN".to_string();
    };
    let date = parsed.with_timezone(&Local);
    let now = Local::now();
    let time = format!("{:02}:{:02}", date.hour(), date.minute());
    if date.year() == now.year() && date.month() == now.month() && date.day() == now.day() {
        return time;
    }
    let month = date.month();
    let day = date.day();
    if date.year() == now.year() {
        return format!("{month}/{day} {time}");
    }
    let year = date.year().to_string();
    let year = &year[year.len().saturating_sub(2)..];
    format!("{year}/{month}/{day} {time}")
}

fn format_tool_call(name: &str, args: &IndexMap<String, Value>) -> String {
    let shorten_path = |p: String| -> String {
        let home = std::env::var("HOME")
            .ok()
            .filter(|h| !h.is_empty())
            .or_else(|| std::env::var("USERPROFILE").ok())
            .unwrap_or_default();
        if !home.is_empty() && p.starts_with(&home) {
            return format!("~{}", &p[home.len()..]);
        }
        p
    };

    match name {
        "read" => {
            let path = shorten_path(first_truthy(args, &["path", "file_path"], ""));
            format!("[read: {path}{}]", format_read_line_range_ref(args.get("offset"), args.get("limit")))
        }
        "write" => format!("[write: {}]", shorten_path(first_truthy(args, &["path", "file_path"], ""))),
        "edit" => format!("[edit: {}]", shorten_path(first_truthy(args, &["path", "file_path"], ""))),
        "bash" => {
            let raw_cmd = first_truthy(args, &["command"], "");
            let cmd = js_slice(&normalize_text(&raw_cmd), 50);
            let ellipsis = if raw_cmd.encode_utf16().count() > 50 { "..." } else { "" };
            format!("[bash: {cmd}{ellipsis}]")
        }
        "grep" => {
            let pattern = first_truthy(args, &["pattern"], "");
            let path = shorten_path(first_truthy(args, &["path"], "."));
            format!("[grep: /{pattern}/ in {path}]")
        }
        "find" => {
            let pattern = first_truthy(args, &["pattern"], "");
            let path = shorten_path(first_truthy(args, &["path"], "."));
            format!("[find: {pattern} in {path}]")
        }
        "ls" => format!("[ls: {}]", shorten_path(first_truthy(args, &["path"], "."))),
        _ => {
            // Custom tool - show name and truncated JSON args
            let json = serde_json::to_string(args).unwrap_or_default();
            let args_str = js_slice(&json, 40);
            let ellipsis = if json.encode_utf16().count() > 40 { "..." } else { "" };
            format!("[{name}: {args_str}{ellipsis}]")
        }
    }
}

/// Tree list component with selection and ASCII art visualization
/// TS: `implements Component`
pub struct TreeList {
    state: Mutex<TreeState>,
    callbacks: Mutex<TreeCallbacks>,
}

impl TreeList {
    pub fn new(
        tree: &[SessionTreeNode],
        current_leaf_id: Option<&str>,
        max_visible_lines: i64,
        initial_selected_id: Option<&str>,
        initial_filter_mode: Option<&FilterMode>,
    ) -> Self {
        let mut state = TreeState {
            flat_nodes: Vec::new(),
            filtered_nodes: Vec::new(),
            selected_index: 0,
            current_leaf_id: current_leaf_id.map(str::to_string),
            max_visible_lines: max_visible_lines.max(0) as usize,
            filter_mode: initial_filter_mode.copied().unwrap_or(FilterMode::Default),
            search_query: String::new(),
            tool_call_map: HashMap::new(),
            multiple_roots: tree.len() > 1,
            show_label_timestamps: false,
            active_path_ids: HashSet::new(),
            visible_parent_map: HashMap::new(),
            visible_children_map: HashMap::new(),
            last_selected_id: None,
            folded_nodes: HashSet::new(),
        };
        state.flatten_tree(tree);
        state.build_active_path();
        state.apply_filter();

        // Start with initialSelectedId if provided, otherwise current leaf
        let target_id = initial_selected_id.or(current_leaf_id);
        state.selected_index = state.find_nearest_visible_index(target_id);
        state.last_selected_id = state.selected_id();

        Self { state: Mutex::new(state), callbacks: Mutex::new(TreeCallbacks::default()) }
    }

    pub fn on_select(&self) -> Option<Arc<dyn Fn(String) + Send + Sync>> {
        self.callbacks.lock().on_select.clone()
    }

    pub fn set_on_select(&self, value: Option<Arc<dyn Fn(String) + Send + Sync>>) {
        self.callbacks.lock().on_select = value;
    }

    pub fn on_cancel(&self) -> Option<Arc<dyn Fn() + Send + Sync>> {
        self.callbacks.lock().on_cancel.clone()
    }

    pub fn set_on_cancel(&self, value: Option<Arc<dyn Fn() + Send + Sync>>) {
        self.callbacks.lock().on_cancel = value;
    }

    pub fn on_copy(&self) -> Option<Arc<dyn Fn(Option<String>) + Send + Sync>> {
        self.callbacks.lock().on_copy.clone()
    }

    pub fn set_on_copy(&self, value: Option<Arc<dyn Fn(Option<String>) + Send + Sync>>) {
        self.callbacks.lock().on_copy = value;
    }

    pub fn on_label_edit(&self) -> Option<Arc<dyn Fn(String, Option<String>) + Send + Sync>> {
        self.callbacks.lock().on_label_edit.clone()
    }

    pub fn set_on_label_edit(&self, value: Option<Arc<dyn Fn(String, Option<String>) + Send + Sync>>) {
        self.callbacks.lock().on_label_edit = value;
    }

    pub fn get_search_query(&self) -> String {
        self.state.lock().search_query.clone()
    }

    /// The selected node. Rust: the returned node carries the entry and its label, without the
    /// subtree (`children` is empty).
    pub fn get_selected_node(&self) -> Option<SessionTreeNode> {
        let s = self.state.lock();
        let &i = s.filtered_nodes.get(s.selected_index)?;
        let flat = &s.flat_nodes[i];
        Some(SessionTreeNode {
            entry: flat.entry.clone(),
            children: Vec::new(),
            label: flat.label.clone(),
            label_timestamp: flat.label_timestamp.clone(),
        })
    }

    pub fn copy_selected(&self) {
        let text = {
            let s = self.state.lock();
            s.filtered_nodes.get(s.selected_index).and_then(|&i| get_entry_copy_text(&s.flat_nodes[i]))
        };
        let cb = self.callbacks.lock().on_copy.clone();
        if let Some(cb) = cb {
            cb(text);
        }
    }

    pub fn update_node_label(&self, entry_id: &str, label: Option<&str>, label_timestamp: Option<&str>) {
        let mut s = self.state.lock();
        if let Some(flat) = s.flat_nodes.iter_mut().find(|f| f.id() == entry_id) {
            flat.label = label.map(str::to_string);
            flat.label_timestamp = if label.is_some_and(|l| !l.is_empty()) {
                Some(label_timestamp.map(str::to_string).unwrap_or_else(crate::core::session_manager::now_iso))
            } else {
                None
            };
        }
    }
}

impl Component for TreeList {
    fn render(&self, width: usize) -> Vec<String> {
        let t = theme();
        let s = self.state.lock();
        let mut lines = Vec::new();

        if s.filtered_nodes.is_empty() {
            lines.push(truncate_to_width(&t.fg(&ThemeColor::Muted, "  No entries found"), width, "...", false));
            lines.push(truncate_to_width(
                &t.fg(&ThemeColor::Muted, &format!("  (0/0){}", s.get_status_labels())),
                width,
                "...",
                false,
            ));
            return lines;
        }

        let len = s.filtered_nodes.len() as i64;
        let max_visible = s.max_visible_lines as i64;
        let selected = s.selected_index as i64;
        let start_index = (selected - max_visible / 2).min(len - max_visible).max(0);
        let end_index = (start_index + max_visible).min(len);

        let mut rendered_rows: Vec<HorizontalViewportRow> = Vec::new();
        for i in start_index..end_index {
            let flat = &s.flat_nodes[s.filtered_nodes[i as usize]];
            let entry_id = flat.id();
            let is_selected = i == selected;

            // Build line: cursor + prefix + path marker + label + content
            let cursor = if is_selected { t.fg(&ThemeColor::Accent, "› ") } else { "  ".to_string() };

            // If multiple roots, shift display (roots at 0, not 1)
            let display_indent = if s.multiple_roots { (flat.indent - 1).max(0) } else { flat.indent };

            // Build prefix with gutters at their correct positions
            let has_connector = flat.show_connector && !flat.is_virtual_root_child;
            let connector_position = if has_connector { display_indent - 1 } else { -1 };

            let total_chars = display_indent * 3;
            let is_folded = s.folded_nodes.contains(entry_id);
            let mut prefix = String::new();
            for c in 0..total_chars {
                let level = c / 3;
                let pos_in_level = c % 3;
                if let Some(gutter) = flat.gutters.iter().find(|g| g.position == level) {
                    if pos_in_level == 0 {
                        prefix.push(if gutter.show { '│' } else { ' ' });
                    } else {
                        prefix.push(' ');
                    }
                } else if has_connector && level == connector_position {
                    // Connector at this level, with fold indicator
                    if pos_in_level == 0 {
                        prefix.push(if flat.is_last { '└' } else { '├' });
                    } else if pos_in_level == 1 {
                        let foldable = s.is_foldable(entry_id);
                        prefix.push(if is_folded {
                            '⊞'
                        } else if foldable {
                            '⊟'
                        } else {
                            '─'
                        });
                    } else {
                        prefix.push(' ');
                    }
                } else {
                    prefix.push(' ');
                }
            }

            // Fold marker for nodes without connectors (roots)
            let fold_marker =
                if is_folded && !has_connector { t.fg(&ThemeColor::Accent, "⊞ ") } else { String::new() };

            // Active path marker - shown right before the entry text
            let path_marker =
                if s.active_path_ids.contains(entry_id) { t.fg(&ThemeColor::Accent, "• ") } else { String::new() };

            let label = match flat.label.as_deref().filter(|l| !l.is_empty()) {
                Some(l) => t.fg(&ThemeColor::Warning, &format!("[{l}] ")),
                None => String::new(),
            };
            let label_timestamp = match (s.show_label_timestamps, flat.label.as_deref(), flat.label_timestamp.as_deref())
            {
                (true, Some(l), Some(ts)) if !l.is_empty() && !ts.is_empty() => {
                    t.fg(&ThemeColor::Muted, &format!("{} ", format_label_timestamp(ts)))
                }
                _ => String::new(),
            };
            let content = s.get_entry_display_text(flat, is_selected);
            let prefix_part = format!("{}{fold_marker}{path_marker}", t.fg(&ThemeColor::Dim, &prefix));
            let anchor_col = visible_width(&prefix_part) as i64;
            let mut gutter = cursor;
            let mut body = format!("{prefix_part}{label}{label_timestamp}{content}");
            if is_selected {
                gutter = t.bg(&ThemeBg::SelectedBg, &gutter);
                body = t.bg(&ThemeBg::SelectedBg, &body);
            }
            let body_width = visible_width(&body) as i64;
            rendered_rows.push(HorizontalViewportRow { gutter, body, anchor_col, body_width, is_selected });
        }

        lines.extend(render_horizontal_viewport(&rendered_rows, width));
        lines.push(truncate_to_width(
            &t.fg(&ThemeColor::Muted, &format!("  ({}/{}){}", selected + 1, len, s.get_status_labels())),
            width,
            "...",
            false,
        ));
        lines
    }

    fn handle_input(&self, data: &str) {
        let kb = get_keybindings();
        let mut s = self.state.lock();
        let len = s.filtered_nodes.len();
        if kb.matches(data, "tui.select.up") {
            s.selected_index = if s.selected_index == 0 { len.saturating_sub(1) } else { s.selected_index - 1 };
        } else if kb.matches(data, "tui.select.down") {
            s.selected_index = if s.selected_index + 1 >= len { 0 } else { s.selected_index + 1 };
        } else if kb.matches(data, "app.tree.foldOrUp") {
            let current_id = s.selected_id();
            match current_id {
                Some(id) if s.is_foldable(&id) && !s.folded_nodes.contains(&id) => {
                    s.folded_nodes.insert(id);
                    s.apply_filter();
                }
                _ => s.selected_index = s.find_branch_segment_start(true),
            }
        } else if kb.matches(data, "app.tree.unfoldOrDown") {
            let current_id = s.selected_id();
            match current_id {
                Some(id) if s.folded_nodes.contains(&id) => {
                    s.folded_nodes.remove(&id);
                    s.apply_filter();
                }
                _ => s.selected_index = s.find_branch_segment_start(false),
            }
        } else if kb.matches(data, "tui.editor.cursorLeft") || kb.matches(data, "tui.select.pageUp") {
            // Page up
            s.selected_index = s.selected_index.saturating_sub(s.max_visible_lines);
        } else if kb.matches(data, "tui.editor.cursorRight") || kb.matches(data, "tui.select.pageDown") {
            // Page down
            s.selected_index = (s.selected_index + s.max_visible_lines).min(len.saturating_sub(1));
        } else if kb.matches(data, "tui.select.confirm") {
            let selected = s.selected_id();
            drop(s);
            let cb = self.callbacks.lock().on_select.clone();
            if let (Some(id), Some(cb)) = (selected, cb) {
                cb(id);
            }
        } else if kb.matches(data, "app.message.copy") {
            drop(s);
            self.copy_selected();
        } else if kb.matches(data, "tui.select.cancel") {
            if !s.search_query.is_empty() {
                s.search_query.clear();
                s.folded_nodes.clear();
                s.apply_filter();
            } else {
                drop(s);
                let cb = self.callbacks.lock().on_cancel.clone();
                if let Some(cb) = cb {
                    cb();
                }
            }
        } else if kb.matches(data, "app.tree.filter.default") {
            // Direct filter: default
            s.filter_mode = FilterMode::Default;
            s.folded_nodes.clear();
            s.apply_filter();
        } else if kb.matches(data, "app.tree.filter.noTools") {
            // Toggle filter: no-tools ↔ default
            s.filter_mode = if s.filter_mode == FilterMode::NoTools { FilterMode::Default } else { FilterMode::NoTools };
            s.folded_nodes.clear();
            s.apply_filter();
        } else if kb.matches(data, "app.tree.filter.userOnly") {
            s.filter_mode =
                if s.filter_mode == FilterMode::UserOnly { FilterMode::Default } else { FilterMode::UserOnly };
            s.folded_nodes.clear();
            s.apply_filter();
        } else if kb.matches(data, "app.tree.filter.labeledOnly") {
            s.filter_mode =
                if s.filter_mode == FilterMode::LabeledOnly { FilterMode::Default } else { FilterMode::LabeledOnly };
            s.folded_nodes.clear();
            s.apply_filter();
        } else if kb.matches(data, "app.tree.filter.all") {
            s.filter_mode = if s.filter_mode == FilterMode::All { FilterMode::Default } else { FilterMode::All };
            s.folded_nodes.clear();
            s.apply_filter();
        } else if kb.matches(data, "app.tree.filter.cycleBackward") {
            // Cycle filter backwards
            let current = FILTER_MODES.iter().position(|m| *m == s.filter_mode).unwrap_or(0);
            s.filter_mode = FILTER_MODES[(current + FILTER_MODES.len() - 1) % FILTER_MODES.len()];
            s.folded_nodes.clear();
            s.apply_filter();
        } else if kb.matches(data, "app.tree.filter.cycleForward") {
            // Cycle filter forwards: default → no-tools → user-only → labeled-only → all → default
            let current = FILTER_MODES.iter().position(|m| *m == s.filter_mode).unwrap_or(0);
            s.filter_mode = FILTER_MODES[(current + 1) % FILTER_MODES.len()];
            s.folded_nodes.clear();
            s.apply_filter();
        } else if kb.matches(data, "tui.editor.deleteCharBackward") {
            if !s.search_query.is_empty() {
                // JS `slice(0, -1)` drops the last UTF-16 code unit; drop the last char.
                s.search_query.pop();
                s.folded_nodes.clear();
                s.apply_filter();
            }
        } else if kb.matches(data, "app.tree.editLabel") {
            let selected = s.filtered_nodes.get(s.selected_index).map(|&i| {
                let flat = &s.flat_nodes[i];
                (flat.id().to_string(), flat.label.clone())
            });
            drop(s);
            let cb = self.callbacks.lock().on_label_edit.clone();
            if let (Some((id, label)), Some(cb)) = (selected, cb) {
                cb(id, label);
            }
        } else if kb.matches(data, "app.tree.toggleLabelTimestamp") {
            s.show_label_timestamps = !s.show_label_timestamps;
        } else {
            let has_control_chars = data.encode_utf16().any(|code| code < 32 || code == 0x7f || (0x80..=0x9f).contains(&code));
            if !has_control_chars && !data.is_empty() {
                s.search_query.push_str(data);
                s.folded_nodes.clear();
                s.apply_filter();
            }
        }
    }

    fn handles_input(&self) -> bool {
        true
    }

    fn invalidate(&self) {}
}

/// Component that displays the current search query
/// TS: `implements Component`
pub struct SearchLine {
    tree_list: Arc<TreeList>,
}

impl SearchLine {
    pub fn new(tree_list: Arc<TreeList>) -> Self {
        Self { tree_list }
    }
}

impl Component for SearchLine {
    fn render(&self, width: usize) -> Vec<String> {
        let t = theme();
        let query = self.tree_list.get_search_query();
        if !query.is_empty() {
            return vec![truncate_to_width(
                &format!("  {} {}", t.fg(&ThemeColor::Muted, "Type to search:"), t.fg(&ThemeColor::Accent, &query)),
                width,
                "...",
                false,
            )];
        }
        vec![truncate_to_width(&format!("  {}", t.fg(&ThemeColor::Muted, "Type to search:")), width, "...", false)]
    }

    fn handle_input(&self, _data: &str) {}

    fn handles_input(&self) -> bool {
        true
    }

    fn invalidate(&self) {}
}

struct TreeHelpItem {
    keys: &'static [&'static str],
    label: &'static str,
    label_first: bool,
}

const TREE_HELP_ITEMS: &[TreeHelpItem] = &[
    TreeHelpItem { keys: &["tui.select.up", "tui.select.down"], label: "move", label_first: false },
    TreeHelpItem { keys: &["tui.editor.cursorLeft", "tui.editor.cursorRight"], label: "page", label_first: false },
    TreeHelpItem { keys: &["app.tree.foldOrUp", "app.tree.unfoldOrDown"], label: "branch", label_first: false },
    TreeHelpItem { keys: &["app.message.copy"], label: "copy", label_first: false },
    TreeHelpItem { keys: &["app.tree.editLabel"], label: "label", label_first: false },
    TreeHelpItem { keys: &["app.tree.toggleLabelTimestamp"], label: "label time", label_first: false },
    TreeHelpItem {
        keys: &[
            "app.tree.filter.default",
            "app.tree.filter.noTools",
            "app.tree.filter.userOnly",
            "app.tree.filter.labeledOnly",
            "app.tree.filter.all",
        ],
        label: "filters",
        label_first: true,
    },
    TreeHelpItem {
        keys: &["app.tree.filter.cycleForward", "app.tree.filter.cycleBackward"],
        label: "cycle",
        label_first: true,
    },
];

fn replace_word(text: &str, word: &str, replacement: &str) -> String {
    let re = regex::Regex::new(&format!(r"\b{}\b", regex::escape(word))).expect("valid regex");
    re.replace_all(text, replacement).into_owned()
}

fn format_help_keys(keybindings: &[&str]) -> String {
    let kb = get_keybindings();
    let keys: Vec<String> = keybindings.iter().filter_map(|k| kb.get_keys(k).into_iter().next()).collect();
    if keys.is_empty() {
        return String::new();
    }
    let mut text = format_key_text(&compact_raw_keys(&keys), None);
    for (word, replacement) in
        [("pageUp", "pgup"), ("pageDown", "pgdn"), ("up", "↑"), ("down", "↓"), ("left", "←"), ("right", "→")]
    {
        text = replace_word(&text, word, replacement);
    }
    text
}

fn compact_raw_keys(keys: &[String]) -> String {
    if keys.len() == 1 {
        return keys[0].clone();
    }
    let parts: Vec<(String, String)> = keys
        .iter()
        .map(|key| match key.rfind('+') {
            None => (String::new(), key.clone()),
            Some(i) => (key[..=i].to_string(), key[i + 1..].to_string()),
        })
        .collect();
    let prefix = &parts[0].0;
    if !prefix.is_empty() && parts.iter().all(|p| &p.0 == prefix) {
        format!("{prefix}{}", parts.iter().map(|p| p.1.as_str()).collect::<Vec<_>>().join("/"))
    } else {
        keys.join("/")
    }
}

/// Component that renders tree help as semantic rows with chunk-aware wrapping
/// TS: `implements Component`
#[derive(Default)]
pub struct TreeHelp {}

impl TreeHelp {
    pub fn new() -> Self {
        Self {}
    }
}

impl Component for TreeHelp {
    fn render(&self, width: usize) -> Vec<String> {
        let items: Vec<String> = TREE_HELP_ITEMS
            .iter()
            .map(|item| {
                let text = format_help_keys(item.keys);
                if text.is_empty() {
                    item.label.to_string()
                } else if item.label_first {
                    format!("{} {text}", item.label)
                } else {
                    format!("{text} {}", item.label)
                }
            })
            .collect();

        let available_width = width.max(1);
        let indent = "  ";
        let separator = " · ";
        let mut lines: Vec<String> = Vec::new();
        let mut current_line = String::new();

        for item in &items {
            let indented = format!("{indent}{item}");
            let candidate = if !current_line.is_empty() {
                format!("{current_line}{separator}{item}")
            } else if visible_width(&indented) <= available_width {
                indented.clone()
            } else {
                item.clone()
            };
            if current_line.is_empty() || visible_width(&candidate) <= available_width {
                current_line = candidate;
                continue;
            }
            lines.extend(wrap_text_with_ansi(js_trim_end(&current_line), available_width));
            current_line = if visible_width(&indented) <= available_width { indented } else { item.clone() };
        }

        if !current_line.is_empty() {
            lines.extend(wrap_text_with_ansi(js_trim_end(&current_line), available_width));
        }

        let t = theme();
        lines.into_iter().map(|line| t.fg(&ThemeColor::Muted, &line)).collect()
    }

    fn invalidate(&self) {}
}

type LabelSubmitFn = Arc<dyn Fn(String, Option<String>) + Send + Sync>;

/// Label input component shown when editing a label
/// TS: `implements Component, Focusable`
pub struct LabelInput {
    input: Input,
    entry_id: String,
    on_submit: Mutex<Option<LabelSubmitFn>>,
    on_cancel: Mutex<Option<Arc<dyn Fn() + Send + Sync>>>,
    focused: Mutex<bool>,
}

impl LabelInput {
    pub fn new(entry_id: &str, current_label: Option<&str>) -> Self {
        let input = Input::default();
        if let Some(label) = current_label.filter(|l| !l.is_empty()) {
            input.set_value(label);
        }
        Self {
            input,
            entry_id: entry_id.to_string(),
            on_submit: Mutex::new(None),
            on_cancel: Mutex::new(None),
            focused: Mutex::new(false),
        }
    }

    pub fn on_submit(&self) -> Option<Arc<dyn Fn(String, Option<String>) + Send + Sync>> {
        self.on_submit.lock().clone()
    }

    pub fn set_on_submit(&self, value: Option<Arc<dyn Fn(String, Option<String>) + Send + Sync>>) {
        *self.on_submit.lock() = value;
    }

    pub fn on_cancel(&self) -> Option<Arc<dyn Fn() + Send + Sync>> {
        self.on_cancel.lock().clone()
    }

    pub fn set_on_cancel(&self, value: Option<Arc<dyn Fn() + Send + Sync>>) {
        *self.on_cancel.lock() = value;
    }
}

impl Component for LabelInput {
    fn render(&self, width: usize) -> Vec<String> {
        let t = theme();
        let indent = "  ";
        let available_width = width.saturating_sub(indent.len());
        let mut lines = vec![truncate_to_width(
            &format!("{indent}{}", t.fg(&ThemeColor::Muted, "Label (empty to remove):")),
            width,
            "...",
            false,
        )];
        lines.extend(
            self.input
                .render(available_width)
                .into_iter()
                .map(|line| truncate_to_width(&format!("{indent}{line}"), width, "...", false)),
        );
        lines.push(truncate_to_width(
            &format!(
                "{indent}{}  {}",
                key_hint("tui.select.confirm".into(), "save"),
                key_hint("tui.select.cancel".into(), "cancel")
            ),
            width,
            "...",
            false,
        ));
        lines
    }

    fn handle_input(&self, data: &str) {
        let kb = get_keybindings();
        if kb.matches(data, "tui.select.confirm") {
            let value = js_trim(&self.input.get_value()).to_string();
            let cb = self.on_submit.lock().clone();
            if let Some(cb) = cb {
                cb(self.entry_id.clone(), if value.is_empty() { None } else { Some(value) });
            }
        } else if kb.matches(data, "tui.select.cancel") {
            let cb = self.on_cancel.lock().clone();
            if let Some(cb) = cb {
                cb();
            }
        } else {
            self.input.handle_input(data);
        }
    }

    fn handles_input(&self) -> bool {
        true
    }

    fn invalidate(&self) {}

    fn is_focusable(&self) -> bool {
        true
    }

    fn set_focused(&self, focused: bool) {
        *self.focused.lock() = focused;
        self.input.set_focused(focused);
    }

    fn focused(&self) -> bool {
        *self.focused.lock()
    }
}

struct TreeSelectorInner {
    container: Container,
    tree_list: Arc<TreeList>,
    label_input: Mutex<Option<Arc<LabelInput>>>,
    label_input_container: Arc<Container>,
    tree_container: Arc<Container>,
    on_label_change_callback: Option<Arc<dyn Fn(String, Option<String>) + Send + Sync>>,
    on_copy: Mutex<Option<Arc<dyn Fn(Option<String>) + Send + Sync>>>,
    focused: Mutex<bool>,
}

impl TreeSelectorInner {
    fn show_label_input(self: &Arc<Self>, entry_id: String, current_label: Option<String>) {
        let label_input = Arc::new(LabelInput::new(&entry_id, current_label.as_deref()));
        let weak = Arc::downgrade(self);
        label_input.set_on_submit(Some(Arc::new(move |id: String, label: Option<String>| {
            let Some(this) = weak.upgrade() else { return };
            this.tree_list.update_node_label(&id, label.as_deref(), None);
            if let Some(cb) = &this.on_label_change_callback {
                cb(id, label);
            }
            this.hide_label_input();
        })));
        let weak = Arc::downgrade(self);
        label_input.set_on_cancel(Some(Arc::new(move || {
            if let Some(this) = weak.upgrade() {
                this.hide_label_input();
            }
        })));

        // Propagate current focused state to the new labelInput
        label_input.set_focused(*self.focused.lock());

        *self.label_input.lock() = Some(label_input.clone());
        self.tree_container.clear();
        self.label_input_container.clear();
        self.label_input_container.add_child(label_input);
    }

    fn hide_label_input(&self) {
        *self.label_input.lock() = None;
        self.label_input_container.clear();
        self.tree_container.clear();
        self.tree_container.add_child(self.tree_list.clone());
    }
}

/// Component that renders a session tree selector for navigation
/// TS: `extends Container implements Focusable`
pub struct TreeSelectorComponent {
    inner: Arc<TreeSelectorInner>,
}

impl TreeSelectorComponent {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        tree: &[SessionTreeNode],
        current_leaf_id: Option<&str>,
        terminal_height: i64,
        on_select: Arc<dyn Fn(String) + Send + Sync>,
        on_cancel: Arc<dyn Fn() + Send + Sync>,
        on_label_change: Option<Arc<dyn Fn(String, Option<String>) + Send + Sync>>,
        initial_selected_id: Option<&str>,
        initial_filter_mode: Option<&FilterMode>,
    ) -> Self {
        let max_visible_lines = (terminal_height / 2).max(5);
        let tree_list = Arc::new(TreeList::new(
            tree,
            current_leaf_id,
            max_visible_lines,
            initial_selected_id,
            initial_filter_mode,
        ));
        tree_list.set_on_select(Some(on_select));
        tree_list.set_on_cancel(Some(on_cancel.clone()));

        let tree_container = Arc::new(Container::new());
        tree_container.add_child(tree_list.clone());
        let label_input_container = Arc::new(Container::new());

        let container = Container::new();
        container.add_child(Arc::new(Spacer::new(1)));
        container.add_child(Arc::new(DynamicBorder::new(None)));
        container.add_child(Arc::new(Text::new(theme().bold("  Session Tree"), 1, 0, None)));
        container.add_child(Arc::new(TreeHelp::new()));
        container.add_child(Arc::new(SearchLine::new(tree_list.clone())));
        container.add_child(Arc::new(DynamicBorder::new(None)));
        container.add_child(Arc::new(Spacer::new(1)));
        container.add_child(tree_container.clone());
        container.add_child(label_input_container.clone());
        container.add_child(Arc::new(Spacer::new(1)));
        container.add_child(Arc::new(DynamicBorder::new(None)));

        let inner = Arc::new(TreeSelectorInner {
            container,
            tree_list: tree_list.clone(),
            label_input: Mutex::new(None),
            label_input_container,
            tree_container,
            on_label_change_callback: on_label_change,
            on_copy: Mutex::new(None),
            focused: Mutex::new(false),
        });

        let weak = Arc::downgrade(&inner);
        tree_list.set_on_copy(Some(Arc::new(move |text: Option<String>| {
            let Some(this) = weak.upgrade() else { return };
            let cb = this.on_copy.lock().clone();
            if let Some(cb) = cb {
                cb(text);
            }
        })));
        let weak = Arc::downgrade(&inner);
        tree_list.set_on_label_edit(Some(Arc::new(move |entry_id: String, current_label: Option<String>| {
            if let Some(this) = weak.upgrade() {
                this.show_label_input(entry_id, current_label);
            }
        })));

        if tree.is_empty() {
            std::thread::spawn(move || {
                std::thread::sleep(Duration::from_millis(100));
                on_cancel();
            });
        }

        Self { inner }
    }

    pub fn on_copy(&self) -> Option<Arc<dyn Fn(Option<String>) + Send + Sync>> {
        self.inner.on_copy.lock().clone()
    }

    pub fn set_on_copy(&self, value: Option<Arc<dyn Fn(Option<String>) + Send + Sync>>) {
        *self.inner.on_copy.lock() = value;
    }

    pub fn get_tree_list(&self) -> Arc<TreeList> {
        self.inner.tree_list.clone()
    }
}

impl Component for TreeSelectorComponent {
    fn render(&self, width: usize) -> Vec<String> {
        self.inner.container.render(width)
    }

    fn handle_input(&self, data: &str) {
        let label_input = self.inner.label_input.lock().clone();
        match label_input {
            Some(label_input) => label_input.handle_input(data),
            None => self.inner.tree_list.handle_input(data),
        }
    }

    fn handles_input(&self) -> bool {
        true
    }

    fn invalidate(&self) {
        self.inner.container.invalidate();
    }

    fn handle_mouse(&self, event: &MouseEvent) -> Option<MouseResult> {
        self.inner.container.handle_mouse(event)
    }

    fn child_components(&self) -> Option<Vec<ComponentRef>> {
        self.inner.container.child_components()
    }

    fn is_focusable(&self) -> bool {
        true
    }

    fn set_focused(&self, focused: bool) {
        *self.inner.focused.lock() = focused;
        // Propagate to labelInput when it's active
        let label_input = self.inner.label_input.lock().clone();
        if let Some(label_input) = label_input {
            label_input.set_focused(focused);
        }
    }

    fn focused(&self) -> bool {
        *self.inner.focused.lock()
    }
}
