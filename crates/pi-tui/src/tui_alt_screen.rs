//! Fullscreen (alternate screen) renderer. Port of `packages/tui/src/tui-alt-screen.ts`.
// Temporary scaffold; replaced by the full port.

use crate::tui::{ComponentRef, Tui, TuiStopOptions};

pub(crate) struct AltScreen;

impl AltScreen {
    pub(crate) fn mounted_roots(&self, tui: &Tui) -> Vec<ComponentRef> {
        tui.container.children()
    }
    pub(crate) fn before_terminal_start(&self, _tui: &Tui) {}
    pub(crate) fn before_terminal_stop(&self, _tui: &Tui, _o: TuiStopOptions) {}
    pub(crate) fn after_terminal_stop(&self, _tui: &Tui, _o: TuiStopOptions) {}
    pub(crate) fn exit_restore_prefix(&self, base: String) -> String {
        base
    }
    pub(crate) fn exit_restore_suffix(&self) -> String {
        String::new()
    }
    pub(crate) fn reset_render_state(&self) {}
    pub(crate) fn do_render(&self, _tui: &Tui) {}
    pub(crate) fn render_root(&self, tui: &Tui, width: usize) -> Vec<String> {
        tui.container.render_children(width)
    }
}
