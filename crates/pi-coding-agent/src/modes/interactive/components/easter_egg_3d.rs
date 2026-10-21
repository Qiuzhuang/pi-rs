//! Port of `packages/coding-agent/src/modes/interactive/components/easter-egg-3d.ts`.

use pi_tui::tui::Component;
use pi_tui::tui::Tui;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

/// Fullscreen 3D easter eggs: the pi logo (header logo click) and Armin (/arminsayshi). Both are bitmaps built from
/// one block per pixel. The current screen dissolves into braille dust while the model spins in the center and its
/// blocks play a sliding puzzle. Leaving plays the same timeline backwards, so the screen reassembles.
///
/// The pi logo lifts off the header, flies to the center, and grows; the dust spreads out from the header logo.
/// Armin grows out of a speck at the center; the dust spreads out from the center.
///
/// The blocks are ray cast per braille dot. A braille cell holds 2x4 roughly square dots, so one pixel of a
/// half-block bitmap is exactly 2x2 dots, and the header logo (4x2 cells) is 8x8 dots when it lifts off.
pub type Rgb = (f64, f64, f64);

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScreenCell {
    pub text: String,
    /// 1 or 2 for a grapheme, 0 for the second column of a wide grapheme.
    pub width: i64,
    pub fg: Rgb,
    /// Undefined when the cell uses the terminal's default background.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bg: Option<Rgb>,
    /// Seconds until the cell turns into braille dust.
    pub delay: i64,
    /// Number of braille dots the glyph turns into.
    #[serde(with = "pi_ai::serde_util::js_f64")]
    pub ink: f64,
    #[serde(with = "pi_ai::serde_util::js_f64")]
    pub seed: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Star {
    /// Braille character with the single dot the star occupies.
    pub glyph: String,
    /// Color the star leans toward; it only ever shows a small step from the background toward it.
    pub tint: Rgb,
    /// Largest step from the background toward the tint, from 0 to 1.
    #[serde(with = "pi_ai::serde_util::js_f64")]
    pub strength: f64,
    #[serde(with = "pi_ai::serde_util::js_f64")]
    pub phase: f64,
    /// Flicker speed in radians per second.
    #[serde(with = "pi_ai::serde_util::js_f64")]
    pub speed: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Box {
    pub min: (i64, i64, i64),
    pub max: (i64, i64, i64),
    pub color: Rgb,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Pose {
    #[serde(with = "pi_ai::serde_util::js_f64")]
    pub center_x: f64,
    #[serde(with = "pi_ai::serde_util::js_f64")]
    pub center_y: f64,
    /// Braille dots per model pixel at depth 0.
    #[serde(with = "pi_ai::serde_util::js_f64")]
    pub scale: f64,
    #[serde(with = "pi_ai::serde_util::js_f64")]
    pub yaw: f64,
    #[serde(with = "pi_ai::serde_util::js_f64")]
    pub pitch: f64,
    #[serde(with = "pi_ai::serde_util::js_f64")]
    pub roll: f64,
}

/// Grid position of a block: column and row in the bitmap, and layer (-1, 0, 1) in depth.
pub type Cell3 = (f64, f64, f64);

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelBlocks {
    pub home: Cell3,
    pub color: Rgb,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelOrigin {
    pub column: i64,
    pub row: i64,
}

/// A bitmap built from one block per foreground pixel. Each is a block that the puzzle slides around.
#[derive(Clone)]
pub struct Model {
    /// Bitmap size in pixels.
    pub columns: i64,
    pub rows: i64,
    pub blocks: Vec<ModelBlocks>,
    /// Camera distance from the model's center, in pixels.
    pub camera_distance: f64,
    /// Farthest distance of any block corner from the center, with blocks on the outer depth layers.
    pub radius: f64,
    /// Largest share of the screen width the spinning model covers.
    pub width_share: i64,
    /// Number of blocks the puzzle moves per step, given a random number from 0 to 1.
    pub puzzle_moves: Arc<dyn Fn(f64) -> f64 + Send + Sync>,
    /// Top-left cell of the model's half-block rendering on screen. The model lifts off from there and replaces it.
    /// Undefined to grow out of the center.
    pub origin: Option<ModelOrigin>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Face {
    #[serde(with = "pi_ai::serde_util::js_f64")]
    pub axis: f64,
    /// Plane coordinate on `axis` in object space.
    #[serde(with = "pi_ai::serde_util::js_f64")]
    pub plane: f64,
    pub u_min: i64,
    pub u_max: i64,
    pub v_min: i64,
    pub v_max: i64,
    /// Projected bounds in braille dots, inclusive.
    pub min_x: i64,
    pub min_y: i64,
    pub max_x: i64,
    pub max_y: i64,
    #[serde(with = "pi_ai::serde_util::js_f64")]
    pub red: f64,
    #[serde(with = "pi_ai::serde_util::js_f64")]
    pub green: f64,
    #[serde(with = "pi_ai::serde_util::js_f64")]
    pub blue: f64,
}

/// Which easter egg to play. The pi logo lifts off the header logo, whose top-left cell is at `column`, `row`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all_fields = "camelCase")]
pub enum EasterEgg3d {
    #[serde(rename = "pi-logo")]
    PiLogo { column: i64, row: i64 },
    #[serde(rename = "armin")]
    Armin {},
}

/// Show the animation as a fullscreen overlay until it is dismissed. The overlay takes focus and mouse input and
/// returns focus when hidden, so the rest of the UI keeps running underneath untouched.
pub async fn play_easter_egg3d(
    tui: Arc<Tui>,
    screen: &[String],
    egg: &EasterEgg3d,
) -> anyhow::Result<()> {
    todo!("port modes/interactive/components/easter-egg-3d.ts:playEasterEgg3d")
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EasterEgg3dAnimationColors {
    pub foreground: Rgb,
    pub background: Rgb,
}

/// TS: `implements Component`
pub struct EasterEgg3dAnimation {
    // TODO(port): private state (interior mutability; methods take &self)
}

impl EasterEgg3dAnimation {
    pub fn new(
        tui: Arc<Tui>,
        screen: &[String],
        model: &Model,
        colors: &EasterEgg3dAnimationColors,
        on_done: Arc<dyn Fn() + Send + Sync>,
    ) -> Self {
        todo!("port modes/interactive/components/easter-egg-3d.ts:EasterEgg3dAnimation.constructor")
    }

    /// Play the exit animation. A second call skips it.
    pub fn close(&self) {
        todo!("port modes/interactive/components/easter-egg-3d.ts:EasterEgg3dAnimation.close")
    }
}

impl Component for EasterEgg3dAnimation {
    fn render(&self, width: usize) -> Vec<String> {
        todo!("port modes/interactive/components/easter-egg-3d.ts:EasterEgg3dAnimation.render")
    }

    fn handle_input(&self, data: &str) {
        todo!("port modes/interactive/components/easter-egg-3d.ts:EasterEgg3dAnimation.handleInput")
    }

    fn handles_input(&self) -> bool {
        true
    }

    fn invalidate(&self) {
        todo!("port modes/interactive/components/easter-egg-3d.ts:EasterEgg3dAnimation.invalidate")
    }
}
