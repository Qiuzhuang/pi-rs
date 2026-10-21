//! Port of `packages/coding-agent/src/modes/interactive/components/tree-selector.ts`.

use crate::core::session_manager::SessionTreeNode;
use indexmap::IndexMap;
use pi_tui::tui::Component;
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
    #[serde(with = "pi_ai::serde_util::js_f64")]
    pub indent: f64,
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

/// Component that renders a session tree selector for navigation
/// TS: `extends Container implements Focusable`
pub struct TreeSelectorComponent {
    // TODO(port): private state (interior mutability; methods take &self)
}

impl TreeSelectorComponent {
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
        todo!(
            "port modes/interactive/components/tree-selector.ts:TreeSelectorComponent.constructor"
        )
    }

    pub fn on_copy(&self) -> Option<Arc<dyn Fn(Option<String>) + Send + Sync>> {
        todo!("port modes/interactive/components/tree-selector.ts:TreeSelectorComponent.onCopy")
    }

    pub fn set_on_copy(&self, value: Option<Arc<dyn Fn(Option<String>) + Send + Sync>>) {
        todo!("port modes/interactive/components/tree-selector.ts:TreeSelectorComponent.onCopy")
    }

    pub fn get_tree_list(&self) -> Value /* TS: TreeList */ {
        todo!(
            "port modes/interactive/components/tree-selector.ts:TreeSelectorComponent.getTreeList"
        )
    }
}

// TODO(port): impl pi-tui::Container for TreeSelectorComponent

impl Component for TreeSelectorComponent {
    fn render(&self, width: usize) -> Vec<String> {
        todo!("port modes/interactive/components/tree-selector.ts:TreeSelectorComponent.render")
    }

    fn handle_input(&self, data: &str) {
        todo!(
            "port modes/interactive/components/tree-selector.ts:TreeSelectorComponent.handleInput"
        )
    }

    fn handles_input(&self) -> bool {
        true
    }
}
