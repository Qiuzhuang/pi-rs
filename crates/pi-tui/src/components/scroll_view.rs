//! Scrollable viewport for fullscreen layouts. Port of `components/scroll-view.ts`.

use std::sync::{Arc, Mutex, MutexGuard, Weak};
use std::time::Duration;

use crate::StyleFn;
use crate::event_loop::{self, EventLoop, TimerId};
use crate::layout_node::{LayoutNode, ScrollLayoutNode};
use crate::tui::{Component, ComponentRef, MouseEvent, MouseResult};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ScrollViewScrollbar {
    #[default]
    Hidden,
    Auto,
    Always,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Overscroll {
    #[default]
    Chain,
    Contain,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ScrollFollow {
    #[default]
    None,
    End,
}

/// Options for [`ScrollView`].
#[derive(Clone, Default)]
pub struct ScrollViewOptions {
    pub follow: Option<ScrollFollow>,
    pub primary: Option<bool>,
    pub overscroll: Option<Overscroll>,
    pub scrollbar: Option<ScrollViewScrollbar>,
    pub scrollbar_track_style: Option<StyleFn>,
    pub scrollbar_thumb_style: Option<StyleFn>,
    pub scrollbar_hide_delay_ms: Option<u64>,
    /// Event loop for the scrollbar hide timer (default: [`event_loop::current`]).
    pub event_loop: Option<EventLoop>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ScrollViewScrollToOptions {
    /// Keep follow-end disabled even when the target is the current content end.
    pub disable_follow: bool,
}

type RenderCallback = Arc<dyn Fn() + Send + Sync>;

struct State {
    scrollbar: ScrollViewScrollbar,
    scroll_top: i64,
    content_height: i64,
    viewport_height: i64,
    following_end: bool,
    follow_suppressed_at_end: bool,
    request_render: Option<RenderCallback>,
    transient_scrollbar_visible: bool,
    scrollbar_active: bool,
    hide_timer: Option<TimerId>,
}

/// A vertically scrollable view of exactly one child.
pub struct ScrollView {
    weak_self: Weak<ScrollView>,
    child: ComponentRef,
    pub follow_end: bool,
    pub primary: bool,
    pub overscroll: Overscroll,
    track_style: StyleFn,
    thumb_style: StyleFn,
    hide_delay: Duration,
    event_loop: EventLoop,
    st: Mutex<State>,
}

impl ScrollView {
    pub fn new(component: ComponentRef, options: ScrollViewOptions) -> Arc<Self> {
        let follow_end = options.follow.unwrap_or_default() == ScrollFollow::End;
        Arc::new_cyclic(|weak| ScrollView {
            weak_self: weak.clone(),
            child: component,
            follow_end,
            primary: options.primary.unwrap_or(false),
            overscroll: options.overscroll.unwrap_or_default(),
            track_style: options.scrollbar_track_style.unwrap_or_else(|| Arc::new(|t: &str| format!("\x1b[90m{t}\x1b[39m"))),
            thumb_style: options.scrollbar_thumb_style.unwrap_or_else(|| Arc::new(|t: &str| format!("\x1b[37m{t}\x1b[39m"))),
            hide_delay: Duration::from_millis(options.scrollbar_hide_delay_ms.unwrap_or(1000)),
            event_loop: options.event_loop.unwrap_or_else(event_loop::current),
            st: Mutex::new(State {
                scrollbar: options.scrollbar.unwrap_or_default(),
                scroll_top: 0,
                content_height: 0,
                viewport_height: 0,
                following_end: follow_end,
                follow_suppressed_at_end: false,
                request_render: None,
                transient_scrollbar_visible: false,
                scrollbar_active: false,
                hide_timer: None,
            }),
        })
    }

    fn lock(&self) -> MutexGuard<'_, State> {
        self.st.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn notify(cb: Option<RenderCallback>) {
        if let Some(cb) = cb {
            cb();
        }
    }

    pub fn child(&self) -> &ComponentRef {
        &self.child
    }

    pub fn scroll_top(&self) -> i64 {
        self.lock().scroll_top
    }

    pub fn is_following_end(&self) -> bool {
        self.lock().following_end
    }

    pub fn viewport_height(&self) -> i64 {
        self.lock().viewport_height
    }

    /// Content height measured by the last layout.
    pub fn content_height(&self) -> i64 {
        self.lock().content_height
    }

    pub fn scrollbar(&self) -> ScrollViewScrollbar {
        self.lock().scrollbar
    }

    pub fn scrollbar_track_style(&self, text: &str) -> String {
        (self.track_style)(text)
    }

    pub fn scrollbar_thumb_style(&self, text: &str) -> String {
        (self.thumb_style)(text)
    }

    pub fn is_scrollbar_visible(&self) -> bool {
        let s = self.lock();
        match s.scrollbar {
            ScrollViewScrollbar::Always => s.viewport_height > 0,
            ScrollViewScrollbar::Auto => s.content_height > s.viewport_height && s.transient_scrollbar_visible,
            ScrollViewScrollbar::Hidden => false,
        }
    }

    pub fn is_scrollbar_active(&self) -> bool {
        self.lock().scrollbar_active
    }

    pub fn set_scrollbar(&self, scrollbar: ScrollViewScrollbar) {
        let cb = {
            let mut s = self.lock();
            if scrollbar == s.scrollbar {
                return;
            }
            s.scrollbar = scrollbar;
            if scrollbar != ScrollViewScrollbar::Auto {
                self.hide_transient_scrollbar(&mut s);
            } else if s.scrollbar_active {
                self.mark_scrollbar_activity(&mut s);
            }
            s.request_render.clone()
        };
        Self::notify(cb);
    }

    pub fn get_content_width(&self, width: usize) -> usize {
        if self.lock().scrollbar == ScrollViewScrollbar::Always && width > 1 { width - 1 } else { width }
    }

    fn mark_scrollbar_activity(&self, s: &mut State) {
        if s.scrollbar != ScrollViewScrollbar::Auto || s.content_height <= s.viewport_height {
            return;
        }
        s.transient_scrollbar_visible = true;
        if let Some(t) = s.hide_timer.take() {
            self.event_loop.clear_timer(t);
        }
        if s.scrollbar_active {
            return;
        }
        let weak = self.weak_self.clone();
        s.hide_timer = Some(self.event_loop.set_timeout(self.hide_delay, move || {
            let Some(me) = weak.upgrade() else { return };
            let cb = {
                let mut s = me.lock();
                s.hide_timer = None;
                s.transient_scrollbar_visible = false;
                s.request_render.clone()
            };
            Self::notify(cb);
        }));
    }

    fn hide_transient_scrollbar(&self, s: &mut State) {
        s.transient_scrollbar_visible = false;
        if let Some(t) = s.hide_timer.take() {
            self.event_loop.clear_timer(t);
        }
    }

    pub fn set_scrollbar_active(&self, active: bool) {
        let cb = {
            let mut s = self.lock();
            if active == s.scrollbar_active {
                return;
            }
            s.scrollbar_active = active;
            self.mark_scrollbar_activity(&mut s);
            s.request_render.clone()
        };
        Self::notify(cb);
    }

    pub fn scroll_to(&self, scroll_top: i64, options: ScrollViewScrollToOptions) {
        let cb = {
            let mut s = self.lock();
            let max = (s.content_height - s.viewport_height).max(0);
            let next = scroll_top.clamp(0, max);
            let suppressed = options.disable_follow && next == max;
            let following = !suppressed && self.follow_end && next == max;
            if next == s.scroll_top && following == s.following_end && suppressed == s.follow_suppressed_at_end {
                return;
            }
            let moved = next != s.scroll_top;
            s.scroll_top = next;
            s.following_end = following;
            s.follow_suppressed_at_end = suppressed;
            if moved {
                self.mark_scrollbar_activity(&mut s);
            }
            s.request_render.clone()
        };
        Self::notify(cb);
    }

    /// Scroll by `lines`; returns the part of the request that could not be applied.
    pub fn scroll_by(&self, lines: i64) -> i64 {
        if lines == 0 {
            return 0;
        }
        let (remaining, cb) = {
            let mut s = self.lock();
            let max = (s.content_height - s.viewport_height).max(0);
            let start = if s.following_end { max } else { s.scroll_top };
            let next = (start + lines).clamp(0, max);
            let moved = next - start;
            let was_following = s.following_end;
            s.scroll_top = next;
            s.following_end = self.follow_end && next == max;
            s.follow_suppressed_at_end = false;
            if moved != 0 {
                self.mark_scrollbar_activity(&mut s);
            }
            let cb = if moved != 0 || s.following_end != was_following { s.request_render.clone() } else { None };
            (lines - moved, cb)
        };
        Self::notify(cb);
        remaining
    }

    pub fn scroll_to_start(&self) {
        let cb = {
            let mut s = self.lock();
            let follow = self.follow_end && s.content_height <= s.viewport_height;
            let changed = s.scroll_top != 0 || s.following_end != follow;
            s.scroll_top = 0;
            s.following_end = follow;
            s.follow_suppressed_at_end = false;
            if !changed {
                return;
            }
            self.mark_scrollbar_activity(&mut s);
            s.request_render.clone()
        };
        Self::notify(cb);
    }

    pub fn scroll_to_end(&self) {
        let cb = {
            let mut s = self.lock();
            let next = (s.content_height - s.viewport_height).max(0);
            let changed = s.scroll_top != next || s.following_end != self.follow_end;
            s.scroll_top = next;
            s.following_end = self.follow_end;
            s.follow_suppressed_at_end = false;
            if !changed {
                return;
            }
            self.mark_scrollbar_activity(&mut s);
            s.request_render.clone()
        };
        Self::notify(cb);
    }

    /// Called by the layout engine with measured sizes.
    pub fn update_layout(&self, content_height: i64, viewport_height: i64, request_render: RenderCallback) {
        let mut s = self.lock();
        s.content_height = content_height.max(0);
        s.viewport_height = viewport_height.max(0);
        s.request_render = Some(request_render);
        let max = (s.content_height - s.viewport_height).max(0);
        if s.following_end {
            s.scroll_top = max;
        } else {
            s.scroll_top = s.scroll_top.clamp(0, max);
        }
        if s.scroll_top < max {
            s.follow_suppressed_at_end = false;
        }
        if self.follow_end && s.scroll_top == max && !s.follow_suppressed_at_end {
            s.following_end = true;
        }
        if s.content_height <= s.viewport_height {
            self.hide_transient_scrollbar(&mut s);
        }
    }

    fn self_arc(&self) -> Arc<ScrollView> {
        self.weak_self.upgrade().expect("ScrollView dropped")
    }
}

impl Component for ScrollView {
    fn render(&self, width: usize) -> Vec<String> {
        let content_width = self.get_content_width(width);
        let lines = self.child.render(content_width);
        if content_width == width { lines } else { lines.into_iter().map(|l| format!("{l} ")).collect() }
    }
    fn handle_mouse(&self, event: &MouseEvent) -> Option<MouseResult> {
        // Container mouse dispatch over the single child.
        if event.y < 0 || event.y >= event.height {
            return None;
        }
        let width = event.width.max(0) as usize;
        let h = self.child.render(width).len() as i64;
        if event.y < h {
            let child_event = MouseEvent { height: h, ..*event };
            return crate::tui::dispatch_mouse_event(&self.child, &child_event);
        }
        None
    }
    fn invalidate(&self) {
        self.child.invalidate();
    }
    fn child_components(&self) -> Option<Vec<ComponentRef>> {
        Some(vec![self.child.clone()])
    }
    fn layout_node(&self) -> Option<LayoutNode> {
        Some(LayoutNode::Scroll(ScrollLayoutNode { component: self.child.clone(), state: self.self_arc() }))
    }
    fn uses_container_mouse(&self) -> bool {
        true
    }
}
