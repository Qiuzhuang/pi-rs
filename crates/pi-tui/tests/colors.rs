//! Port of colors.test.ts.

use pi_tui::colors::*;

#[test]
fn parses_hex_and_oklch_and_rejects_everything_else() {
    assert_eq!(parse_color("#abc").unwrap(), Color::Rgb { r: 170.0, g: 187.0, b: 204.0 });
    assert_eq!(parse_color("oklch(62% 0.1 200)").unwrap(), Color::Oklch { l: 0.62, c: 0.1, h: 200.0 });
    assert!(parse_color("").unwrap_err().contains("Invalid color value"));
    assert!(parse_color("red").unwrap_err().contains("Invalid color value"));
}

#[test]
fn gamut_maps_oklch_to_srgb() {
    let rgb = |c: Color| {
        let v = color_to_rgb(&c);
        (v.r, v.g, v.b)
    };
    assert_eq!(rgb(oklch_color(0.627955, 0.257683, 29.2339).unwrap()), (255.0, 0.0, 0.0));
    assert_eq!(rgb(oklch_color(1.0, 0.3, 150.0).unwrap()), (255.0, 255.0, 255.0));
    assert_eq!(rgb(oklch_color(0.0, 0.3, 150.0).unwrap()), (0.0, 0.0, 0.0));
}

#[test]
fn parses_okhsl_and_round_trips() {
    assert_eq!(parse_color("okhsl(29.23 100% 56.8%)").unwrap(), rgb_color(255.0, 0.0, 0.0).unwrap());
    assert_eq!(parse_color("OKHSL(250deg 60% 55%)").unwrap(), okhsl_color(250.0, 0.6, 0.55).unwrap());
    assert!(parse_color("okhsl(250 160% 55%)").unwrap_err().contains("s must be between 0 and 1"));
    for hex in ["#4f8eb3", "#20242a", "#f8f9fa"] {
        let hsl = color_to_okhsl(&parse_color(hex).unwrap());
        assert_eq!(color_to_hex(&okhsl_color(hsl.h, hsl.s, hsl.l).unwrap()), hex);
    }
}

#[test]
fn styles_text_and_closes_in_reverse_order() {
    let style = TextStyle {
        fg: Some(rgb_color(18.0, 52.0, 86.0).unwrap()),
        bg: Some(indexed_color(9).unwrap()),
        attrs: TextAttributes { bold: true, italic: true, ..Default::default() },
    };
    assert_eq!(
        style_text("Ready", &style, TerminalColorMode::Truecolor),
        "\x1b[38;2;18;52;86m\x1b[48;5;9m\x1b[1m\x1b[3mReady\x1b[23m\x1b[22m\x1b[49m\x1b[39m"
    );
    let s = style_text(
        "Ready",
        &TextStyle { fg: Some(rgb_color(18.0, 52.0, 86.0).unwrap()), ..Default::default() },
        TerminalColorMode::Color256,
    );
    assert!(s.starts_with("\x1b[38;5;") && s.ends_with("mReady\x1b[39m"));
}
