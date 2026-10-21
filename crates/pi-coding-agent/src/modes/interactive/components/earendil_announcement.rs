//! Port of `packages/coding-agent/src/modes/interactive/components/earendil-announcement.ts`.

use crate::config::get_bundled_interactive_asset_path;
use crate::modes::interactive::components::dynamic_border::DynamicBorder;
use crate::modes::interactive::theme::theme::{ThemeColor, theme};
use base64::Engine;
use pi_tui::components::image::{Image, ImageOptions, ImageTheme};
use pi_tui::components::spacer::Spacer;
use pi_tui::components::text::Text;
use pi_tui::tui::{Component, ComponentRef, Container, MouseEvent, MouseResult};
use std::sync::{Arc, OnceLock};

const BLOG_URL: &str = "https://mariozechner.at/posts/2026-04-08-ive-sold-out/";
const IMAGE_FILENAME: &str = "clankolas.png";

/// Module-level cache of the announcement image (TS: `cachedImageBase64` / `attemptedImageLoad`).
static CACHED_IMAGE_BASE64: OnceLock<Option<String>> = OnceLock::new();

fn load_image_base64() -> Option<String> {
    CACHED_IMAGE_BASE64
        .get_or_init(|| {
            std::fs::read(get_bundled_interactive_asset_path(IMAGE_FILENAME))
                .ok()
                .map(|bytes| base64::engine::general_purpose::STANDARD.encode(bytes))
        })
        .clone()
}

fn accent_border() -> Arc<DynamicBorder> {
    Arc::new(DynamicBorder::new(Some(Arc::new(|text: String| theme().fg(&ThemeColor::Accent, &text)))))
}

/// TS: `extends Container`
pub struct EarendilAnnouncementComponent {
    container: Container,
}

impl Default for EarendilAnnouncementComponent {
    fn default() -> Self {
        Self::new()
    }
}

impl EarendilAnnouncementComponent {
    pub fn new() -> Self {
        let t = theme();
        let container = Container::new();
        container.add_child(accent_border());
        container.add_child(Arc::new(Text::new(
            t.bold(&t.fg(&ThemeColor::Accent, "pi has joined Earendil")),
            1,
            0,
            None,
        )));
        container.add_child(Arc::new(Spacer::new(1)));
        container.add_child(Arc::new(Text::new(t.fg(&ThemeColor::Muted, "Read the blog post:"), 1, 0, None)));
        container.add_child(Arc::new(Text::new(t.fg(&ThemeColor::MdLink, BLOG_URL), 1, 0, None)));
        container.add_child(Arc::new(Spacer::new(1)));

        if let Some(image_base64) = load_image_base64() {
            container.add_child(Arc::new(Image::new(
                image_base64,
                "image/png",
                ImageTheme { fallback_color: Arc::new(|text: &str| theme().fg(&ThemeColor::Muted, text)) },
                ImageOptions {
                    max_width_cells: Some(56),
                    filename: Some(IMAGE_FILENAME.to_string()),
                    ..Default::default()
                },
                None,
            )));
            container.add_child(Arc::new(Spacer::new(1)));
        }

        container.add_child(accent_border());
        Self { container }
    }
}

impl Component for EarendilAnnouncementComponent {
    fn render(&self, width: usize) -> Vec<String> {
        self.container.render(width)
    }

    fn invalidate(&self) {
        self.container.invalidate();
    }

    fn handle_mouse(&self, event: &MouseEvent) -> Option<MouseResult> {
        self.container.handle_mouse(event)
    }

    fn child_components(&self) -> Option<Vec<ComponentRef>> {
        self.container.child_components()
    }
}
