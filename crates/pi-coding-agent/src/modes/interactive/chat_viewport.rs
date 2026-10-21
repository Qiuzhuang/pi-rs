//! Port of `packages/coding-agent/src/modes/interactive/chat-viewport.ts`.

use pi_tui::components::scroll_view::ScrollView;
use pi_tui::components::scroll_view::ScrollViewScrollbar;
use pi_tui::tui::ComponentRef;
use std::sync::Arc;

#[derive(Clone)]
pub struct ChatViewportOptions {
    pub document: ComponentRef,
    pub pending_messages: ComponentRef,
    pub status: ComponentRef,
    pub editor: ComponentRef,
    pub footer: ComponentRef,
    pub widgets_above: Option<ComponentRef>,
    pub widgets_below: Option<ComponentRef>,
    pub scrollbar: Option<ScrollViewScrollbar>,
    pub scrollbar_track_style: Option<Arc<dyn Fn(String) -> String + Send + Sync>>,
    pub scrollbar_thumb_style: Option<Arc<dyn Fn(String) -> String + Send + Sync>>,
}

#[derive(Clone)]
pub struct ChatViewport {
    pub root: ComponentRef,
    pub transcript: Arc<ScrollView>,
}

/// Shared fullscreen transcript and fixed input-dock layout.
pub fn create_chat_viewport(options: &ChatViewportOptions) -> ChatViewport {
    todo!("port modes/interactive/chat-viewport.ts:createChatViewport")
}
