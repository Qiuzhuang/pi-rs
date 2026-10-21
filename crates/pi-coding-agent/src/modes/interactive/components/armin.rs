//! Port of `packages/coding-agent/src/modes/interactive/components/armin.ts`.

use pi_tui::tui::Component;
use pi_tui::tui::Tui;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

pub const ARMIN_WIDTH: u64 = 31;

pub const ARMIN_HEIGHT: u64 = 36;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Effect {
    #[serde(rename = "typewriter")]
    Typewriter,
    #[serde(rename = "scanline")]
    Scanline,
    #[serde(rename = "rain")]
    Rain,
    #[serde(rename = "fade")]
    Fade,
    #[serde(rename = "crt")]
    Crt,
    #[serde(rename = "glitch")]
    Glitch,
    #[serde(rename = "dissolve")]
    Dissolve,
}

impl Effect {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Typewriter => "typewriter",
            Self::Scanline => "scanline",
            Self::Rain => "rain",
            Self::Fade => "fade",
            Self::Crt => "crt",
            Self::Glitch => "glitch",
            Self::Dissolve => "dissolve",
        }
    }
}

pub fn is_armin_pixel(x: f64, y: f64) -> bool {
    todo!("port modes/interactive/components/armin.ts:isArminPixel")
}

/// TS: `implements Component`
pub struct ArminComponent {
    // TODO(port): private state (interior mutability; methods take &self)
}

impl ArminComponent {
    pub fn new(ui: Arc<Tui>) -> Self {
        todo!("port modes/interactive/components/armin.ts:ArminComponent.constructor")
    }

    pub fn dispose(&self) {
        todo!("port modes/interactive/components/armin.ts:ArminComponent.dispose")
    }
}

impl Component for ArminComponent {
    fn render(&self, width: usize) -> Vec<String> {
        todo!("port modes/interactive/components/armin.ts:ArminComponent.render")
    }

    fn invalidate(&self) {
        todo!("port modes/interactive/components/armin.ts:ArminComponent.invalidate")
    }
}
