//! Shared implementation of the collapsible message boxes (`BranchSummaryMessageComponent`,
//! `CompactionSummaryMessageComponent`, `SkillInvocationMessageComponent`), which extend `Box` in TS.

use crate::modes::interactive::theme::theme::{ThemeBg, theme};
use pi_tui::components::box_component::BoxComponent;
use pi_tui::components::mouse_region::MouseRegion;
use pi_tui::tui::{Component, ComponentRef, Container, MouseButton, MouseEvent, MouseEventType, MouseResult};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Weak};

/// Fills the content container for the given expanded state.
pub(crate) type BuildContent = Box<dyn Fn(&Container, bool) + Send + Sync>;

pub(crate) struct CollapsibleBox {
    weak_self: Weak<CollapsibleBox>,
    box_component: BoxComponent,
    expanded: AtomicBool,
    build: BuildContent,
}

impl CollapsibleBox {
    pub(crate) fn new(output_pad: Option<i64>, build: BuildContent) -> Arc<Self> {
        let output_pad = output_pad.unwrap_or(1).max(0) as usize;
        let this = Arc::new_cyclic(|weak| CollapsibleBox {
            weak_self: weak.clone(),
            box_component: BoxComponent::new(
                output_pad,
                1,
                Some(Arc::new(|t: &str| theme().bg(&ThemeBg::CustomMessageBg, t))),
            ),
            expanded: AtomicBool::new(false),
            build,
        });
        this.update_display();
        this
    }

    pub(crate) fn set_expanded(&self, expanded: bool) {
        self.expanded.store(expanded, Ordering::SeqCst);
        self.update_display();
    }

    pub(crate) fn set_output_pad(&self, output_pad: i64) {
        self.box_component.set_padding_x(output_pad.max(0) as usize);
    }

    fn update_display(&self) {
        self.box_component.clear();
        let content = Arc::new(Container::new());
        (self.build)(&content, self.expanded.load(Ordering::SeqCst));
        let weak = self.weak_self.clone();
        self.box_component.add_child(Arc::new(MouseRegion::new(
            content as ComponentRef,
            Arc::new(move |event: &MouseEvent| {
                if event.kind != MouseEventType::Click || event.button != MouseButton::Left {
                    return None;
                }
                let this = weak.upgrade()?;
                let expanded = this.expanded.load(Ordering::SeqCst);
                this.set_expanded(!expanded);
                Some(MouseResult::handled())
            }),
        )));
    }

    pub(crate) fn render(&self, width: usize) -> Vec<String> {
        self.box_component.render(width)
    }

    pub(crate) fn invalidate(&self) {
        self.box_component.invalidate();
        self.update_display();
    }

    pub(crate) fn handle_mouse(&self, event: &MouseEvent) -> Option<MouseResult> {
        self.box_component.handle_mouse(event)
    }
}

/// JavaScript `Number.prototype.toLocaleString()` (en-US) for integers.
pub(crate) fn to_locale_string(n: u64) -> String {
    let digits = n.to_string();
    let mut out = String::new();
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i) % 3 == 0 {
            out.push(',');
        }
        out.push(c);
    }
    out
}

/// Implements `Component` for a wrapper holding `inner: Arc<CollapsibleBox>`.
macro_rules! collapsible_component {
    ($ty:ty) => {
        impl pi_tui::tui::Component for $ty {
            fn render(&self, width: usize) -> Vec<String> {
                self.inner.render(width)
            }

            fn invalidate(&self) {
                self.inner.invalidate();
            }

            fn handle_mouse(&self, event: &pi_tui::tui::MouseEvent) -> Option<pi_tui::tui::MouseResult> {
                self.inner.handle_mouse(event)
            }
        }
    };
}
pub(crate) use collapsible_component;

#[cfg(test)]
mod tests {
    #[test]
    fn formats_like_to_locale_string() {
        assert_eq!(super::to_locale_string(0), "0");
        assert_eq!(super::to_locale_string(999), "999");
        assert_eq!(super::to_locale_string(1234), "1,234");
        assert_eq!(super::to_locale_string(1234567), "1,234,567");
    }
}

