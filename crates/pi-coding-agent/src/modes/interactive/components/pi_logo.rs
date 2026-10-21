//! Port of `packages/coding-agent/src/modes/interactive/components/pi-logo.ts`.

use crate::modes::interactive::theme::theme::theme;
use pi_tui::colors::{Color, background_ansi, foreground_ansi};
use pi_tui::terminal::is_apple_terminal_session;

const CORAL: Color = Color::Rgb { r: 228.0, g: 138.0, b: 122.0 };
const BLUE: Color = Color::Rgb { r: 79.0, g: 142.0, b: 179.0 };
const YELLOW: Color = Color::Rgb { r: 234.0, g: 182.0, b: 93.0 };
const RESET: &str = "\x1b[0m";

/// The pi logo: 4 cells wide and 2 lines tall. Each cell shows two square pixels with half blocks:
///
/// coral coral coral .
/// blue  .     coral .
/// blue  blue  .     yellow
/// blue  .     .     yellow
///
/// The brand colors stay fixed across themes; they follow the terminal's color mode.
pub fn pi_logo_lines() -> (String, String) {
    let mode = theme().get_color_mode();
    let fg = |color: &Color| foreground_ansi(color, mode);
    // The fourth cell of the top line is empty, so it is padded to the same width as the bottom line.
    let top = format!(
        "{}{}▀{RESET}{}▀█{RESET} ",
        fg(&CORAL),
        background_ansi(&BLUE, mode),
        fg(&CORAL)
    );
    let bottom = format!("{}█▀{RESET} {}█{RESET}", fg(&BLUE), fg(&YELLOW));
    (top, bottom)
}

/// Whether the terminal renders the half-block logo correctly. Apple Terminal draws gaps between rows and
/// misaligns the half blocks, so it gets the text wordmark instead.
pub fn supports_pi_logo() -> bool {
    !is_apple_terminal_session()
}

/// Text fallback for the logo: "Pi" with the logo's coral and yellow.
pub fn pi_wordmark() -> String {
    let mode = theme().get_color_mode();
    format!(
        "{}P{RESET}{}i{RESET}",
        foreground_ansi(&CORAL, mode),
        foreground_ansi(&YELLOW, mode)
    )
}
