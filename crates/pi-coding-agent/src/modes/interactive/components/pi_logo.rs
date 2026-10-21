//! Port of `packages/coding-agent/src/modes/interactive/components/pi-logo.ts`.

/// The pi logo: 4 cells wide and 2 lines tall. Each cell shows two square pixels with half blocks:
///
/// coral coral coral .
/// blue  .     coral .
/// blue  blue  .     yellow
/// blue  .     .     yellow
///
/// The brand colors stay fixed across themes; they follow the terminal's color mode.
pub fn pi_logo_lines() -> (String, String) {
    todo!("port modes/interactive/components/pi-logo.ts:piLogoLines")
}

/// Whether the terminal renders the half-block logo correctly. Apple Terminal draws gaps between rows and
/// misaligns the half blocks, so it gets the text wordmark instead.
pub fn supports_pi_logo() -> bool {
    todo!("port modes/interactive/components/pi-logo.ts:supportsPiLogo")
}

/// Text fallback for the logo: "Pi" with the logo's coral and yellow.
pub fn pi_wordmark() -> String {
    todo!("port modes/interactive/components/pi-logo.ts:piWordmark")
}
