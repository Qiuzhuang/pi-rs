//! Inline image component (Kitty / iTerm2 graphics with a text fallback).
//! Port of `components/image.ts`.

use std::sync::{Arc, LazyLock, Mutex, MutexGuard, RwLock};

use crate::StyleFn;
use crate::terminal_image::{
    ImageDimensions, ImageProtocol, ImageRenderOptions, allocate_image_id, get_capabilities, get_cell_dimensions,
    get_image_dimensions, get_png_dimensions, image_fallback, render_image,
};
use crate::tui::Component;
use crate::utils::truncate_to_width;

/// Converts base64 image data to base64 PNG data (`None` if it cannot). Called during rendering.
pub type ImageTranscoder = Arc<dyn Fn(&str, &str) -> Option<String> + Send + Sync>;

static IMAGE_TRANSCODER: RwLock<Option<ImageTranscoder>> = RwLock::new(None);
/// Least recently used first, keyed by source data.
static PNG_CACHE: LazyLock<Mutex<Vec<(String, Option<String>)>>> = LazyLock::new(|| Mutex::new(Vec::new()));

/// Register the converter used for non-PNG images on Kitty terminals (which only accept PNG).
/// Without one, such images render as text fallbacks.
pub fn set_image_transcoder(transcoder: Option<ImageTranscoder>) {
    *IMAGE_TRANSCODER.write().unwrap_or_else(|e| e.into_inner()) = transcoder;
    PNG_CACHE.lock().unwrap_or_else(|e| e.into_inner()).clear();
}

/// The registered transcoder, if any (used by JavaScript extensions' `Image`, which shares the
/// host's registration as TS extensions share pi-tui's module state).
pub fn image_transcoder() -> Option<ImageTranscoder> {
    IMAGE_TRANSCODER.read().unwrap_or_else(|e| e.into_inner()).clone()
}

fn to_png(base64_data: &str, mime_type: &str) -> Option<String> {
    let transcoder = IMAGE_TRANSCODER.read().unwrap_or_else(|e| e.into_inner()).clone()?;
    let cached = {
        let mut cache = PNG_CACHE.lock().unwrap_or_else(|e| e.into_inner());
        cache.iter().position(|(k, _)| k == base64_data).map(|i| cache.remove(i).1)
    };
    let png = match cached {
        Some(v) => v,
        None => transcoder(base64_data, mime_type),
    };
    let mut cache = PNG_CACHE.lock().unwrap_or_else(|e| e.into_inner());
    cache.push((base64_data.to_string(), png.clone()));
    if cache.len() > 32 {
        cache.remove(0);
    }
    png
}

/// Image theme.
#[derive(Clone)]
pub struct ImageTheme {
    pub fallback_color: StyleFn,
}

/// Image options.
#[derive(Debug, Clone, Default)]
pub struct ImageOptions {
    pub max_width_cells: Option<usize>,
    pub max_height_cells: Option<usize>,
    pub filename: Option<String>,
    /// Kitty image ID; reused for animations/updates when given.
    pub image_id: Option<u32>,
}

struct State {
    image_id: Option<u32>,
    png_data: Option<String>,
    cache: Option<(usize, Vec<String>)>,
}

/// Renders an image with the terminal's graphics protocol, or a text fallback.
pub struct Image {
    base64_data: String,
    mime_type: String,
    dimensions: ImageDimensions,
    theme: ImageTheme,
    options: ImageOptions,
    st: Mutex<State>,
}

impl Image {
    pub fn new(
        base64_data: impl Into<String>,
        mime_type: impl Into<String>,
        theme: ImageTheme,
        options: ImageOptions,
        dimensions: Option<ImageDimensions>,
    ) -> Self {
        let base64_data = base64_data.into();
        let mime_type = mime_type.into();
        let dimensions = dimensions
            .or_else(|| get_image_dimensions(&base64_data, &mime_type))
            .unwrap_or(ImageDimensions { width_px: 800, height_px: 600 });
        let image_id = options.image_id;
        Self {
            base64_data,
            mime_type,
            dimensions,
            theme,
            options,
            st: Mutex::new(State { image_id, png_data: None, cache: None }),
        }
    }

    fn lock(&self) -> MutexGuard<'_, State> {
        self.st.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// The Kitty image ID used by this image, if any.
    pub fn get_image_id(&self) -> Option<u32> {
        self.lock().image_id
    }

    fn fallback(&self, width: usize) -> Vec<String> {
        let fallback = image_fallback(&self.mime_type, Some(self.dimensions), self.options.filename.as_deref());
        vec![truncate_to_width(&(self.theme.fallback_color)(&fallback), width, "...", false)]
    }
}

impl Component for Image {
    fn render(&self, width: usize) -> Vec<String> {
        if let Some((w, lines)) = &self.lock().cache
            && *w == width
        {
            return lines.clone();
        }
        let max_width = width.saturating_sub(2).min(self.options.max_width_cells.unwrap_or(60)).max(1);
        let cell = get_cell_dimensions();
        let default_max_height =
            ((max_width as f64 * cell.width_px as f64) / cell.height_px as f64).ceil().max(1.0) as usize;
        let max_height = self.options.max_height_cells.unwrap_or(default_max_height);

        let caps = get_capabilities();
        let mut data: Option<String> = Some(self.base64_data.clone());
        let mut dimensions = self.dimensions;
        if caps.images == Some(ImageProtocol::Kitty) && self.mime_type != "image/png" {
            let existing = self.lock().png_data.clone();
            let png = existing.or_else(|| to_png(&self.base64_data, &self.mime_type));
            self.lock().png_data = png.clone();
            data = png;
            if let Some(d) = &data {
                dimensions = get_png_dimensions(d).unwrap_or(dimensions);
            }
        }

        let lines = match (caps.images, data) {
            (Some(protocol), Some(data)) => {
                if protocol == ImageProtocol::Kitty && self.lock().image_id.is_none() {
                    self.lock().image_id = Some(allocate_image_id());
                }
                let image_id = self.lock().image_id;
                let result = render_image(
                    &data,
                    dimensions,
                    &ImageRenderOptions {
                        max_width_cells: Some(max_width),
                        max_height_cells: Some(max_height),
                        image_id,
                        move_cursor: Some(false),
                        ..Default::default()
                    },
                );
                match result {
                    Some(result) => {
                        if let Some(id) = result.image_id {
                            self.lock().image_id = Some(id);
                        }
                        let rows = result.rows.max(1);
                        if protocol == ImageProtocol::Kitty {
                            let mut lines = vec![result.sequence];
                            lines.extend(std::iter::repeat_n(String::new(), rows - 1));
                            lines
                        } else {
                            let mut lines: Vec<String> = std::iter::repeat_n(String::new(), rows - 1).collect();
                            let offset = rows - 1;
                            let move_up = if offset > 0 { format!("\x1b[{offset}A") } else { String::new() };
                            lines.push(format!("{move_up}{}", result.sequence));
                            lines
                        }
                    }
                    None => self.fallback(width),
                }
            }
            _ => self.fallback(width),
        };
        self.lock().cache = Some((width, lines.clone()));
        lines
    }

    fn invalidate(&self) {
        self.lock().cache = None;
    }
}
