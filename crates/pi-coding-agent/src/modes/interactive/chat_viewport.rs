//! Port of `packages/coding-agent/src/modes/interactive/chat-viewport.ts`.

use pi_tui::components::scroll_view::{
    Overscroll, ScrollFollow, ScrollView, ScrollViewOptions, ScrollViewScrollbar,
};
use pi_tui::components::stack::{StackChild, StackEntryOptions, StackOptions, VStack};
use pi_tui::layout_node::StackBasis;
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

fn dock_entry(component: ComponentRef, min_size: usize) -> StackChild {
    StackChild {
        component,
        options: StackEntryOptions { shrink: Some(1), min_size: Some(min_size), ..Default::default() },
    }
}

type StyleFn = Arc<dyn Fn(&str) -> String + Send + Sync>;

fn style_fn(style: &Option<Arc<dyn Fn(String) -> String + Send + Sync>>) -> Option<StyleFn> {
    style.as_ref().map(|f| {
        let f = f.clone();
        Arc::new(move |text: &str| f(text.to_string())) as StyleFn
    })
}

/// Shared fullscreen transcript and fixed input-dock layout.
pub fn create_chat_viewport(options: &ChatViewportOptions) -> ChatViewport {
    let transcript = ScrollView::new(
        options.document.clone(),
        ScrollViewOptions {
            follow: Some(ScrollFollow::End),
            primary: Some(true),
            overscroll: Some(Overscroll::Chain),
            scrollbar: Some(options.scrollbar.unwrap_or(ScrollViewScrollbar::Auto)),
            scrollbar_track_style: style_fn(&options.scrollbar_track_style),
            scrollbar_thumb_style: style_fn(&options.scrollbar_thumb_style),
            ..Default::default()
        },
    );
    let mut dock_children = vec![
        dock_entry(options.pending_messages.clone(), 0),
        dock_entry(options.status.clone(), 0),
    ];
    if let Some(widgets_above) = &options.widgets_above {
        dock_children.push(dock_entry(widgets_above.clone(), 0));
    }
    dock_children.push(dock_entry(options.editor.clone(), 3));
    if let Some(widgets_below) = &options.widgets_below {
        dock_children.push(dock_entry(widgets_below.clone(), 0));
    }
    dock_children.push(dock_entry(options.footer.clone(), 0));
    let dock: ComponentRef = Arc::new(VStack::new(dock_children, StackOptions::default()));
    let transcript_ref: ComponentRef = transcript.clone();
    let root = VStack::new(
        vec![
            StackChild {
                component: transcript_ref,
                options: StackEntryOptions {
                    basis: Some(StackBasis::Size(0)),
                    grow: Some(1),
                    shrink: Some(1),
                    min_size: Some(1),
                    ..Default::default()
                },
            },
            StackChild {
                component: dock,
                options: StackEntryOptions {
                    basis: Some(StackBasis::Auto),
                    grow: Some(0),
                    shrink: Some(1),
                    min_size: Some(1),
                    ..Default::default()
                },
            },
        ],
        StackOptions::default(),
    );
    ChatViewport { root: Arc::new(root), transcript }
}
