//! Layout metadata for fullscreen layout. Port of `packages/tui/src/layout-node.ts`.

use std::sync::Arc;

use crate::components::scroll_view::ScrollView;
use crate::tui::ComponentRef;

/// Viewport passed to stack entry visibility predicates.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LayoutViewport {
    pub width: usize,
    pub height: usize,
}

/// Flex basis of a stack entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum StackBasis {
    #[default]
    Auto,
    Size(usize),
}

/// Visibility predicate for stack entries.
pub type StackVisibleFn = Arc<dyn Fn(LayoutViewport) -> bool + Send + Sync>;

/// One child of a vertical or horizontal stack.
#[derive(Clone)]
pub struct StackLayoutEntry {
    pub component: ComponentRef,
    pub basis: Option<StackBasis>,
    pub grow: Option<usize>,
    pub shrink: Option<usize>,
    pub min_size: Option<usize>,
    pub max_size: Option<usize>,
    pub visible: Option<StackVisibleFn>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StackType {
    VStack,
    HStack,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum StackAlign {
    #[default]
    Stretch,
    Start,
    Center,
    End,
}

#[derive(Clone)]
pub struct StackLayoutNode {
    pub kind: StackType,
    pub entries: Vec<StackLayoutEntry>,
    pub gap: usize,
    pub align: StackAlign,
}

#[derive(Clone)]
pub struct ScrollLayoutNode {
    pub component: ComponentRef,
    pub state: Arc<ScrollView>,
}

/// Layout node of a component.
#[derive(Clone)]
pub enum LayoutNode {
    Stack(StackLayoutNode),
    Scroll(ScrollLayoutNode),
}

/// The layout node of a component, if it participates in fullscreen layout.
pub fn get_layout_node(component: &ComponentRef) -> Option<LayoutNode> {
    component.layout_node()
}
