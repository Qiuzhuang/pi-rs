//! Port of `packages/tui/test/terminal-image.test.ts`: image line detection, capability
//! detection, iTerm2/Kitty encoding and placement, image cell sizing, text fallbacks,
//! transcoding and OSC 8 hyperlinks.
//!
//! Capabilities, cell dimensions, the transcoder and the environment are process-global, so
//! every test holds [`Global`] for its duration; dropping it restores the defaults.

use std::sync::{Arc, Mutex, MutexGuard};

use base64::Engine;
use pi_tui::components::image::{Image, ImageOptions, ImageTheme, set_image_transcoder};
use pi_tui::terminal_image::*;
use pi_tui::tui::Component;
use pi_tui::utils::visible_width;

static LOCK: Mutex<()> = Mutex::new(());

const ENV_KEYS: &[&str] = &[
    "TERM",
    "TERM_PROGRAM",
    "TERMINAL_EMULATOR",
    "COLORTERM",
    "TMUX",
    "KITTY_WINDOW_ID",
    "GHOSTTY_RESOURCES_DIR",
    "WEZTERM_PANE",
    "ITERM_SESSION_ID",
    "WT_SESSION",
    "CMUX_WORKSPACE_ID",
    "WARP_SESSION_ID",
    "WARP_TERMINAL_SESSION_UUID",
    "PI_HYPERLINKS",
    "PI_IMAGE_PROTOCOL",
    "PI_TRUE_COLOR",
];

/// Serializes the tests; on drop resets capabilities, cell size (9x18) and the transcoder.
struct Global {
    _lock: MutexGuard<'static, ()>,
}

fn global() -> Global {
    let lock = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    Global { _lock: lock }
}

impl Drop for Global {
    fn drop(&mut self) {
        set_image_transcoder(None);
        set_capability_overrides(CapabilityOverrides::default());
        reset_capabilities_cache();
        set_cell_dimensions(CellDimensions { width_px: 9, height_px: 18 });
    }
}

/// `withEnv`: clear every capability-related variable, apply `overrides`, run `f`, restore.
fn with_env<T>(overrides: &[(&str, &str)], f: impl FnOnce() -> T) -> T {
    let saved: Vec<(&str, Option<String>)> = ENV_KEYS.iter().map(|k| (*k, std::env::var(k).ok())).collect();
    // SAFETY: environment access in this binary is serialized by LOCK (held via `global()`).
    unsafe {
        for key in ENV_KEYS {
            std::env::remove_var(key);
        }
        for (k, v) in overrides {
            std::env::set_var(k, v);
        }
    }
    struct Restore(Vec<(&'static str, Option<String>)>);
    impl Drop for Restore {
        fn drop(&mut self) {
            for (k, v) in &self.0 {
                // SAFETY: as above.
                unsafe {
                    match v {
                        Some(v) => std::env::set_var(k, v),
                        None => std::env::remove_var(k),
                    }
                }
            }
        }
    }
    let _restore = Restore(saved);
    f()
}

fn caps(images: Option<ImageProtocol>, true_color: bool, hyperlinks: bool) -> TerminalCapabilities {
    TerminalCapabilities { images, true_color, hyperlinks }
}

fn kitty_caps() {
    set_capabilities(caps(Some(ImageProtocol::Kitty), true, true));
}

fn cell(width_px: u32, height_px: u32) {
    set_cell_dimensions(CellDimensions { width_px, height_px });
}

fn dims(width_px: u32, height_px: u32) -> Option<ImageDimensions> {
    Some(ImageDimensions { width_px, height_px })
}

fn plain_theme() -> ImageTheme {
    ImageTheme { fallback_color: Arc::new(|v: &str| v.to_string()) }
}

fn render(data: &str, width: u32, height: u32, options: ImageRenderOptions) -> RenderedImage {
    render_image(data, ImageDimensions { width_px: width, height_px: height }, &options).expect("rendered")
}

fn home() -> String {
    std::env::var("HOME").expect("HOME")
}

// ---- isImageLine: iTerm2 image protocol ---------------------------------------------------

#[test]
fn should_detect_iterm2_image_escape_sequence_at_start_of_line() {
    assert!(is_image_line("\x1b]1337;File=size=100,100;inline=1:base64encodeddata==\x07"));
}

#[test]
fn should_detect_iterm2_image_escape_sequence_with_text_before_it() {
    assert!(is_image_line("Some text \x1b]1337;File=size=100,100;inline=1:base64data==\x07 more text"));
}

#[test]
fn should_detect_iterm2_image_escape_sequence_in_middle_of_long_line() {
    assert!(is_image_line("Text before image...\x1b]1337;File=inline=1:verylongbase64data==...text after"));
}

#[test]
fn should_detect_iterm2_image_escape_sequence_at_end_of_line() {
    assert!(is_image_line("Regular text ending with \x1b]1337;File=inline=1:base64data==\x07"));
}

#[test]
fn should_detect_minimal_iterm2_image_escape_sequence() {
    assert!(is_image_line("\x1b]1337;File=:\x07"));
}

// ---- isImageLine: Kitty image protocol ----------------------------------------------------

#[test]
fn should_detect_kitty_image_escape_sequence_at_start_of_line() {
    assert!(is_image_line("\x1b_Ga=T,f=100,t=f,d=base64data...\x1b\\\x1b_Gm=i=1;\x1b\\"));
}

#[test]
fn should_detect_kitty_image_escape_sequence_with_text_before_it() {
    assert!(is_image_line("Output: \x1b_Ga=T,f=100;data...\x1b\\\x1b_Gm=i=1;\x1b\\"));
}

#[test]
fn should_detect_kitty_image_escape_sequence_with_padding() {
    assert!(is_image_line("  \x1b_Ga=T,f=100...\x1b\\\x1b_Gm=i=1;\x1b\\  "));
}

// ---- isImageLine: bug regression tests ----------------------------------------------------

#[test]
fn should_detect_image_sequences_in_very_long_lines() {
    let long_line =
        format!("Text prefix \x1b]1337;File=size=800,600;inline=1:{} suffix", "A".repeat(100).repeat(3000));
    assert!(long_line.len() > 300_000);
    assert!(is_image_line(&long_line));
}

#[test]
fn should_detect_image_sequences_when_terminal_does_not_support_images() {
    assert!(is_image_line("Read image file [image/jpeg]\x1b]1337;File=inline=1:base64data==\x07"));
}

#[test]
fn should_detect_image_sequences_with_ansi_codes_before_them() {
    assert!(is_image_line("\x1b[31mError output \x1b]1337;File=inline=1:image==\x07"));
}

#[test]
fn should_detect_image_sequences_with_ansi_codes_after_them() {
    assert!(is_image_line("\x1b_Ga=T,f=100:data...\x1b\\\x1b_Gm=i=1;\x1b\\\x1b[0m reset"));
}

// ---- isImageLine: negative cases ----------------------------------------------------------

#[test]
fn should_not_detect_images_in_plain_text_lines() {
    assert!(!is_image_line("This is just a regular text line without any escape sequences"));
}

#[test]
fn should_not_detect_images_in_lines_with_only_ansi_codes() {
    assert!(!is_image_line("\x1b[31mRed text\x1b[0m and \x1b[32mgreen text\x1b[0m"));
}

#[test]
fn should_not_detect_images_in_lines_with_cursor_movement_codes() {
    assert!(!is_image_line("\x1b[1A\x1b[2KLine cleared and moved up"));
}

#[test]
fn should_not_detect_images_in_lines_with_partial_iterm2_sequences() {
    assert!(!is_image_line("Some text with ]1337;File but missing ESC at start"));
}

#[test]
fn should_not_detect_images_in_lines_with_partial_kitty_sequences() {
    assert!(!is_image_line("Some text with _G but missing ESC at start"));
}

#[test]
fn should_not_detect_images_in_empty_lines() {
    assert!(!is_image_line(""));
}

#[test]
fn should_not_detect_images_in_lines_with_newlines_only() {
    assert!(!is_image_line("\n"));
    assert!(!is_image_line("\n\n"));
}

// ---- isImageLine: mixed content -----------------------------------------------------------

#[test]
fn should_detect_images_when_line_has_both_kitty_and_iterm2_sequences() {
    assert!(is_image_line("Kitty: \x1b_Ga=T...\x1b\\\x1b_Gm=i=1;\x1b\\ iTerm2: \x1b]1337;File=inline=1:data==\x07"));
}

#[test]
fn should_detect_image_in_line_with_multiple_text_and_image_segments() {
    assert!(is_image_line("Start \x1b]1337;File=img1==\x07 middle \x1b]1337;File=img2==\x07 end"));
}

#[test]
fn should_not_falsely_detect_image_in_line_with_file_path_containing_keywords() {
    assert!(!is_image_line("/path/to/File_1337_backup/image.jpg"));
}

// ---- detectCapabilities -------------------------------------------------------------------

#[test]
fn defaults_to_hyperlinks_false_for_unknown_terminals() {
    let _g = global();
    with_env(&[], || {
        let c = detect_capabilities(None);
        assert!(!c.hyperlinks);
        assert_eq!(c.images, None);
    });
}

#[test]
fn applies_environment_overrides() {
    let _g = global();
    assert_eq!(
        with_env(&[("PI_HYPERLINKS", "1"), ("PI_IMAGE_PROTOCOL", "kitty"), ("PI_TRUE_COLOR", "1")], || {
            detect_capabilities(None)
        }),
        caps(Some(ImageProtocol::Kitty), true, true)
    );
    assert_eq!(
        with_env(
            &[("TERM_PROGRAM", "iterm.app"), ("PI_HYPERLINKS", "0"), ("PI_IMAGE_PROTOCOL", "none"), ("PI_TRUE_COLOR", "0")],
            || detect_capabilities(None)
        ),
        caps(None, false, false)
    );
}

#[test]
fn preserves_auto_detection_for_auto_environment_overrides() {
    let _g = global();
    assert_eq!(
        with_env(
            &[("TERM_PROGRAM", "ghostty"), ("PI_HYPERLINKS", "auto"), ("PI_IMAGE_PROTOCOL", "auto"), ("PI_TRUE_COLOR", "auto")],
            || detect_capabilities(None)
        ),
        caps(Some(ImageProtocol::Kitty), true, true)
    );
}

#[test]
fn applies_and_clears_programmatic_overrides() {
    let _g = global();
    with_env(&[("PI_HYPERLINKS", "1"), ("PI_IMAGE_PROTOCOL", "kitty"), ("PI_TRUE_COLOR", "1")], || {
        reset_capabilities_cache();
        set_capability_overrides(CapabilityOverrides {
            images: Some(None),
            true_color: Some(false),
            hyperlinks: Some(false),
        });
        assert_eq!(get_capabilities(), caps(None, false, false));
        set_capability_overrides(CapabilityOverrides::default());
        assert_eq!(get_capabilities(), caps(Some(ImageProtocol::Kitty), true, true));
    });
}

#[test]
fn bypasses_the_tmux_probe_when_hyperlinks_are_overridden() {
    let _g = global();
    let probed = std::cell::Cell::new(false);
    let c = with_env(
        &[("TMUX", "/tmp/tmux-1000/default,1234,0"), ("PI_HYPERLINKS", "1"), ("PI_IMAGE_PROTOCOL", "kitty")],
        || {
            detect_capabilities(Some(&|| {
                probed.set(true);
                false
            }))
        },
    );
    assert!(!probed.get());
    assert!(c.hyperlinks);
    assert_eq!(c.images, Some(ImageProtocol::Kitty));
}

#[test]
fn enables_hyperlinks_under_tmux_when_the_client_forwards_them() {
    let _g = global();
    with_env(&[("TMUX", "/tmp/tmux-1000/default,1234,0"), ("TERM_PROGRAM", "ghostty")], || {
        let c = detect_capabilities(Some(&|| true));
        assert!(c.hyperlinks);
        assert_eq!(c.images, None);
    });
}

#[test]
fn disables_hyperlinks_under_tmux_when_the_client_does_not_forward_them() {
    let _g = global();
    with_env(&[("TMUX", "/tmp/tmux-1000/default,1234,0"), ("TERM_PROGRAM", "ghostty")], || {
        let c = detect_capabilities(Some(&|| false));
        assert!(!c.hyperlinks);
        assert_eq!(c.images, None);
    });
}

#[test]
fn checks_tmux_capability_when_term_starts_with_tmux() {
    let _g = global();
    with_env(&[("TERM", "tmux-256color"), ("TERM_PROGRAM", "iterm.app")], || {
        let c = detect_capabilities(Some(&|| true));
        assert!(c.hyperlinks);
        assert_eq!(c.images, None);
        assert!(!detect_capabilities(Some(&|| false)).hyperlinks);
    });
}

#[test]
fn forces_hyperlinks_false_when_term_starts_with_screen() {
    let _g = global();
    with_env(&[("TERM", "screen-256color")], || {
        let c = detect_capabilities(None);
        assert!(!c.hyperlinks);
        assert_eq!(c.images, None);
    });
}

#[test]
fn enables_hyperlinks_and_kitty_images_for_herdr() {
    let _g = global();
    with_env(
        &[("TERM_PROGRAM", "herdr"), ("TERM", "xterm-256color"), ("COLORTERM", "truecolor"), ("KITTY_WINDOW_ID", "1")],
        || {
            let c = detect_capabilities(None);
            assert!(c.hyperlinks);
            assert_eq!(c.images, Some(ImageProtocol::Kitty));
            assert!(c.true_color);
        },
    );
    with_env(&[("TERM_PROGRAM", "herdr"), ("PI_HYPERLINKS", "0")], || {
        assert!(!detect_capabilities(None).hyperlinks);
    });
}

#[test]
fn enables_hyperlinks_without_images_for_rex() {
    let _g = global();
    with_env(&[("TERM_PROGRAM", "rex"), ("TERM", "xterm-256color"), ("KITTY_WINDOW_ID", "1")], || {
        let c = detect_capabilities(None);
        assert!(c.hyperlinks);
        assert_eq!(c.images, Some(ImageProtocol::Kitty));
        assert!(c.true_color);
    });
    with_env(&[("TERM_PROGRAM", "rex"), ("PI_HYPERLINKS", "0")], || {
        assert!(!detect_capabilities(None).hyperlinks);
    });
}

#[test]
fn enables_hyperlinks_for_ghostty() {
    let _g = global();
    with_env(&[("TERM_PROGRAM", "ghostty")], || assert!(detect_capabilities(None).hyperlinks));
}

#[test]
fn does_not_disable_ghostty_images_solely_because_cmux_is_present() {
    let _g = global();
    with_env(&[("TERM_PROGRAM", "ghostty"), ("CMUX_WORKSPACE_ID", "workspace")], || {
        let c = detect_capabilities(None);
        assert_eq!(c.images, Some(ImageProtocol::Kitty));
        assert!(c.hyperlinks);
    });
}

#[test]
fn enables_hyperlinks_for_kitty() {
    let _g = global();
    with_env(&[("KITTY_WINDOW_ID", "1")], || assert!(detect_capabilities(None).hyperlinks));
}

#[test]
fn enables_hyperlinks_for_wezterm() {
    let _g = global();
    with_env(&[("WEZTERM_PANE", "0")], || assert!(detect_capabilities(None).hyperlinks));
}

fn assert_warp(env: &[(&str, &str)]) {
    with_env(env, || {
        let c = detect_capabilities(None);
        assert_eq!(c.images, Some(ImageProtocol::Kitty));
        assert!(c.true_color);
        assert!(c.hyperlinks);
    });
}

#[test]
fn enables_images_and_hyperlinks_for_warp_via_term_program() {
    let _g = global();
    assert_warp(&[("TERM_PROGRAM", "WarpTerminal")]);
}

#[test]
fn enables_images_and_hyperlinks_for_warp_via_warp_session_id() {
    let _g = global();
    assert_warp(&[("WARP_SESSION_ID", "some-session-id")]);
}

#[test]
fn enables_images_and_hyperlinks_for_warp_via_warp_terminal_session_uuid() {
    let _g = global();
    assert_warp(&[("WARP_TERMINAL_SESSION_UUID", "d0e1a2e5-7ca7-44cd-9037-ac7222011161")]);
}

#[test]
fn disables_images_for_warp_inside_tmux() {
    let _g = global();
    with_env(
        &[("TERM_PROGRAM", "WarpTerminal"), ("TMUX", "/tmp/tmux-1000/default,1234,0"), ("TERM", "tmux-256color")],
        || {
            let c = detect_capabilities(Some(&|| true));
            assert_eq!(c.images, None);
            assert!(c.hyperlinks);
        },
    );
}

#[test]
fn enables_hyperlinks_for_iterm2() {
    let _g = global();
    with_env(&[("TERM_PROGRAM", "iterm.app")], || assert!(detect_capabilities(None).hyperlinks));
}

#[test]
fn enables_hyperlinks_for_vscode() {
    let _g = global();
    with_env(&[("TERM_PROGRAM", "vscode")], || assert!(detect_capabilities(None).hyperlinks));
}

#[test]
fn enables_alacritty_capabilities_for_zed() {
    let _g = global();
    with_env(&[("TERM_PROGRAM", "zed")], || assert_eq!(detect_capabilities(None), caps(None, true, true)));
}

#[test]
fn enables_truecolor_and_hyperlinks_for_windows_terminal_outside_multiplexers() {
    let _g = global();
    with_env(&[("WT_SESSION", "session"), ("TERM", "xterm-256color")], || {
        assert_eq!(detect_capabilities(None), caps(None, true, true));
    });
}

#[test]
fn enables_truecolor_without_hyperlinks_for_jetbrains_terminal() {
    let _g = global();
    with_env(&[("TERMINAL_EMULATOR", "JetBrains-JediTerm"), ("TERM", "xterm-256color")], || {
        assert_eq!(detect_capabilities(None), caps(None, true, false));
    });
}

#[test]
fn does_not_inherit_windows_terminal_truecolor_through_tmux() {
    let _g = global();
    with_env(&[("WT_SESSION", "session"), ("TMUX", "/tmp/tmux-1000/default,1234,0"), ("TERM", "tmux-256color")], || {
        assert_eq!(detect_capabilities(Some(&|| false)), caps(None, false, false));
    });
}

#[test]
fn trusts_explicit_truecolor_hints_through_tmux() {
    let _g = global();
    with_env(&[("COLORTERM", "truecolor"), ("TMUX", "/tmp/tmux-1000/default,1234,0"), ("TERM", "tmux-256color")], || {
        assert_eq!(detect_capabilities(Some(&|| false)), caps(None, true, false));
    });
}

#[test]
fn detects_truecolor_from_direct_color_term_values() {
    let _g = global();
    with_env(&[("TERM", "xterm-direct")], || assert!(detect_capabilities(Some(&|| false)).true_color));
}

// ---- iTerm2 image encoding ----------------------------------------------------------------

#[test]
fn includes_the_decoded_payload_size_in_osc_1337_metadata() {
    let sequence = encode_iterm2(
        "AAAA",
        &Iterm2EncodeOptions { width: Some("2".into()), height: Some("auto".into()), ..Default::default() },
    );
    assert_eq!(sequence, "\x1b]1337;File=inline=1;size=3;width=2;height=auto:AAAA\x07");
}

// ---- Kitty image cursor movement ----------------------------------------------------------

fn kitty_options(columns: usize, rows: usize, image_id: Option<u32>) -> KittyEncodeOptions {
    KittyEncodeOptions { columns: Some(columns), rows: Some(rows), image_id, move_cursor: Some(false) }
}

#[test]
fn can_request_no_terminal_side_cursor_movement() {
    let sequence = encode_kitty("AAAA", kitty_options(2, 2, None));
    assert!(sequence.starts_with("\x1b_Ga=T,f=100,q=2,C=1,c=2,r=2;"), "{sequence:?}");
}

#[test]
fn reads_explicit_placement_rows_without_registered_metadata() {
    let sequence = encode_kitty("AAAA", kitty_options(2, 3, None));
    assert_eq!(get_kitty_image_placement_rows(&sequence), Some(3));
}

#[test]
fn suppresses_kitty_replies_for_delete_commands() {
    assert_eq!(delete_kitty_image(42), "\x1b_Ga=d,d=I,i=42,q=2\x1b\\");
    assert_eq!(delete_all_kitty_images(), "\x1b_Ga=d,d=A,q=2\x1b\\");
    assert_eq!(delete_all_kitty_placements(), "\x1b_Ga=d,d=a,q=2\x1b\\");
}

#[test]
fn preserves_render_images_default_terminal_side_cursor_movement() {
    let _g = global();
    kitty_caps();
    cell(10, 10);
    let result = render("AAAA", 20, 20, ImageRenderOptions { max_width_cells: Some(2), ..Default::default() });
    assert!(!result.sequence.contains(",C=1,"));
    assert_eq!(result.rows, 2);
}

#[test]
fn can_opt_render_image_into_no_terminal_side_cursor_movement() {
    let _g = global();
    kitty_caps();
    cell(10, 10);
    let result =
        render("AAAA", 20, 20, ImageRenderOptions { max_width_cells: Some(2), move_cursor: Some(false), ..Default::default() });
    assert!(result.sequence.contains(",C=1,"));
    assert_eq!(result.rows, 2);
}

#[test]
fn registers_metadata_and_crops_a_partially_visible_placement() {
    let _g = global();
    kitty_caps();
    cell(10, 10);
    let result = render(
        "AAAA",
        100,
        100,
        ImageRenderOptions { max_width_cells: Some(3), image_id: Some(42), move_cursor: Some(false), ..Default::default() },
    );
    assert_eq!(
        get_kitty_image_metadata(&result.sequence),
        Some(KittyImageMetadata { image_id: 42, columns: 3, rows: 3, width_px: 100, height_px: 100 })
    );
    assert!(crop_kitty_image_line(&result.sequence, 2, 1).contains("y=66,h=34,r=1"));
}

#[test]
fn creates_placement_only_commands_for_uploaded_and_cropped_images() {
    let _g = global();
    register_kitty_image_metadata(KittyImageMetadata { image_id: 42, columns: 3, rows: 3, width_px: 100, height_px: 100 });
    let transmission = encode_kitty(&"A".repeat(8192), kitty_options(3, 3, Some(42)));
    let line = format!("left {} right", crop_kitty_image_line(&transmission, 2, 1));
    let placement = get_kitty_image_placement(&line).expect("placement");
    assert_eq!(get_kitty_image_placement_rows(&line), Some(1));
    assert_eq!(placement.transmission_bytes, line.len() - "left ".len() - " right".len());
    assert_eq!(placement.estimated_decoded_bytes, 100 * 100 * 4);
    assert_eq!(placement.rows, 1);
    assert_eq!(placement.sequence, "\x1b_Ga=p,q=2,C=1,c=3,i=42,y=66,h=34,r=1\x1b\\");
    assert_eq!(placement.replacement_line, format!("left {} right", placement.sequence));
    assert!(!placement.replacement_line.contains("AAAA"));
}

#[test]
fn honors_max_height_cells_by_reducing_rendered_width() {
    let _g = global();
    kitty_caps();
    cell(10, 10);
    let result =
        render("AAAA", 10, 100, ImageRenderOptions { max_width_cells: Some(10), max_height_cells: Some(5), ..Default::default() });
    assert_eq!(result.rows, 5);
    assert!(result.sequence.contains(",c=1,r=5"));
}

#[test]
fn caps_image_component_height_to_a_square_pixel_box_by_default() {
    let _g = global();
    kitty_caps();
    cell(10, 20);
    let image = Image::new(
        "AAAA",
        "image/png",
        plain_theme(),
        ImageOptions { max_width_cells: Some(10), ..Default::default() },
        dims(10, 100),
    );
    let lines = image.render(12);
    assert_eq!(lines.len(), 5);
    assert!(lines[0].contains(",c=1,r=5"));
}

#[test]
fn places_image_sequence_on_first_line_with_empty_padding_rows() {
    let _g = global();
    kitty_caps();
    cell(10, 10);
    let image = Image::new(
        "AAAA",
        "image/png",
        plain_theme(),
        ImageOptions { max_width_cells: Some(2), ..Default::default() },
        dims(20, 20),
    );
    let lines = image.render(4);
    let image_id = image.get_image_id().expect("a Kitty image id");
    assert!(lines[0].starts_with("\x1b_G"));
    assert!(lines[0].contains(",C=1,"));
    assert!(lines[0].contains(&format!(",i={image_id}")));
    assert!(lines[0].ends_with("\x1b\\"));
    assert_eq!(&lines[1..], [""]);
}

#[test]
fn truncates_long_image_fallback_lines_to_render_width() {
    let _g = global();
    set_capabilities(caps(None, false, false));
    let long_path = format!("{}/images/{}.png", home(), "generated-image-with-a-very-long-absolute-path".repeat(4));
    let width = 40;
    let image = Image::new(
        "AAAA",
        "image/png",
        ImageTheme { fallback_color: Arc::new(|v: &str| format!("\x1b[33m{v}\x1b[0m")) },
        ImageOptions { filename: Some(long_path), ..Default::default() },
        dims(1280, 720),
    );
    let lines = image.render(width);
    assert_eq!(lines.len(), 1);
    assert!(
        visible_width(&lines[0]) <= width,
        "fallback line wider than {width}: visible={} raw={:?}",
        visible_width(&lines[0]),
        lines[0]
    );
    assert!(lines[0].contains("..."), "expected ellipsis when truncating long fallback path");
    assert!(lines[0].contains('~'), "expected home-shortened path in fallback");
}

// ---- image cell sizing: Kitty -------------------------------------------------------------

#[test]
fn reserves_at_least_one_kitty_row_for_thin_images() {
    let _g = global();
    kitty_caps();
    cell(9, 18);
    let result = render("AAAA", 1200, 12, ImageRenderOptions { max_width_cells: Some(60), ..Default::default() });
    assert_eq!(result.rows, 1);
    assert!(result.sequence.contains(",c=60,r=1;"));
}

#[test]
fn keeps_kitty_placement_reserved_lines_and_cropping_metadata_consistent_across_width_changes() {
    let _g = global();
    kitty_caps();
    cell(9, 18);
    let image = Image::new(
        "AAAA",
        "image/png",
        plain_theme(),
        ImageOptions { max_width_cells: Some(60), image_id: Some(8938), ..Default::default() },
        dims(615, 86),
    );
    let lines = image.render(62);
    assert_eq!(lines.len(), 4);
    assert_eq!(&lines[1..], ["", "", ""]);
    assert!(lines[0].contains(",c=60,r=4,i=8938;"));
    assert_eq!(
        get_kitty_image_metadata(&lines[0]),
        Some(KittyImageMetadata { image_id: 8938, columns: 60, rows: 4, width_px: 615, height_px: 86 })
    );
    let cropped = crop_kitty_image_line(&lines[0], 1, 2);
    assert_eq!(
        get_kitty_image_placement(&cropped).map(|p| p.sequence),
        Some("\x1b_Ga=p,q=2,C=1,c=60,i=8938,y=21,h=44,r=2\x1b\\".to_string())
    );

    let narrower = image.render(32);
    assert_eq!(narrower.len(), 2);
    assert!(narrower[0].contains(",c=30,r=2,i=8938;"));
    assert_eq!(get_kitty_image_metadata(&narrower[0]).map(|m| m.rows), Some(2));
}

#[test]
fn keeps_the_ceiling_placement_when_rounding_down_would_increase_distortion() {
    let _g = global();
    kitty_caps();
    cell(15, 28);
    let result = render("AAAA", 615, 86, ImageRenderOptions { max_width_cells: Some(60), ..Default::default() });
    assert_eq!(result.rows, 5);
    assert!(result.sequence.contains(",c=60,r=5;"));
}

#[test]
fn keeps_height_limited_kitty_columns_reservations_and_crop_metadata_consistent() {
    let _g = global();
    kitty_caps();
    cell(14, 28);
    let image = Image::new(
        "AAAA",
        "image/png",
        plain_theme(),
        ImageOptions { max_width_cells: Some(30), image_id: Some(8938), ..Default::default() },
        dims(400, 900),
    );
    let lines = image.render(32);
    assert_eq!(lines.len(), 15);
    assert!(lines[0].contains(",c=13,r=15,i=8938;"));
    assert_eq!(
        get_kitty_image_metadata(&lines[0]),
        Some(KittyImageMetadata { image_id: 8938, columns: 13, rows: 15, width_px: 400, height_px: 900 })
    );
    assert_eq!(
        get_kitty_image_placement(&crop_kitty_image_line(&lines[0], 1, 2)).map(|p| p.sequence),
        Some("\x1b_Ga=p,q=2,C=1,c=13,i=8938,y=60,h=120,r=2\x1b\\".to_string())
    );
    let narrower = image.render(22);
    assert_eq!(narrower.len(), 10);
    assert!(narrower[0].contains(",c=9,r=10,i=8938;"));
}

#[test]
fn chooses_thin_kitty_widths_by_proportions_while_keeping_at_least_one_column() {
    let _g = global();
    kitty_caps();
    cell(1, 1);
    for (width_px, columns) in [(1, 1), (140, 1), (149, 2)] {
        let result = render(
            "AAAA",
            width_px,
            1000,
            ImageRenderOptions { max_width_cells: Some(30), max_height_cells: Some(10), ..Default::default() },
        );
        assert_eq!(result.columns, columns, "width {width_px}");
        assert_eq!(result.rows, 10);
        assert!(result.sequence.contains(&format!(",c={columns},r=10;")));
    }
}

// ---- image cell sizing: iTerm2 ------------------------------------------------------------

#[test]
fn keeps_iterm2s_ceiling_width_when_height_limited() {
    let _g = global();
    set_capabilities(caps(Some(ImageProtocol::Iterm2), true, true));
    cell(14, 28);
    let result =
        render("AAAA", 400, 900, ImageRenderOptions { max_width_cells: Some(30), max_height_cells: Some(15), ..Default::default() });
    assert_eq!(result.columns, 14);
    assert_eq!(result.rows, 15);
    assert_eq!(result.sequence, "\x1b]1337;File=inline=1;size=3;width=14;height=auto:AAAA\x07");
}

#[test]
fn keeps_iterm2s_ceiling_based_reserved_lines_and_cursor_offset() {
    let _g = global();
    set_capabilities(caps(Some(ImageProtocol::Iterm2), true, true));
    cell(9, 18);
    let image = Image::new(
        "AAAA",
        "image/png",
        plain_theme(),
        ImageOptions { max_width_cells: Some(60), ..Default::default() },
        dims(615, 86),
    );
    assert_eq!(
        image.render(62),
        vec![
            String::new(),
            String::new(),
            String::new(),
            String::new(),
            "\x1b[4A\x1b]1337;File=inline=1;size=3;width=60;height=auto:AAAA\x07".to_string(),
        ]
    );
}

// ---- imageFallback ------------------------------------------------------------------------

#[test]
fn shortens_home_prefixed_absolute_paths_without_hyperlinks() {
    let _g = global();
    set_capabilities(caps(None, false, false));
    let abs = format!("{}/.pi/agent/shot.png", home());
    assert_eq!(image_fallback("image/png", dims(1280, 720), Some(&abs)), "[Image: ~/.pi/agent/shot.png [image/png] 1280x720]");
}

#[test]
fn wraps_shortened_absolute_paths_in_osc_8_file_links_when_hyperlinks_are_enabled() {
    let _g = global();
    set_capabilities(caps(None, false, true));
    let abs = format!("{}/.pi/agent/shot.png", home());
    let result = image_fallback("image/png", dims(10, 10), Some(&abs));
    assert!(result.contains("\x1b]8;;file://"), "expected OSC 8 file link");
    assert!(result.contains(&abs), "file URL should target absolute path");
    // Visible text uses ~/... rather than the expanded home path.
    let mut visible = String::new();
    let mut rest = result.as_str();
    while let Some(start) = rest.find("\x1b]8;;") {
        visible.push_str(&rest[..start]);
        let after = &rest[start..];
        let end = after.find("\x1b\\").expect("OSC 8 terminator") + 2;
        rest = &after[end..];
    }
    visible.push_str(rest);
    assert_eq!(visible, "[Image: ~/.pi/agent/shot.png [image/png] 10x10]");
}

#[test]
fn leaves_bare_basenames_unchanged_and_does_not_hyperlink_them() {
    let _g = global();
    set_capabilities(caps(None, false, true));
    let result = image_fallback("image/png", dims(1, 1), Some("clankolas.png"));
    assert_eq!(result, "[Image: clankolas.png [image/png] 1x1]");
    assert!(!result.contains("\x1b]8;"), "basename must not be hyperlinked");
}

#[test]
fn omits_filename_segment_when_not_provided() {
    let _g = global();
    set_capabilities(caps(None, false, false));
    assert_eq!(image_fallback("image/png", dims(8, 6), None), "[Image: [image/png] 8x6]");
}

// ---- Image transcoding (Kitty only accepts PNG) -------------------------------------------

struct Transcoding {
    _g: Global,
    jpeg: String,
    png: String,
    calls: Arc<Mutex<Vec<String>>>,
}

impl Transcoding {
    fn new() -> Self {
        let g = global();
        let b64 = |b: &[u8]| base64::engine::general_purpose::STANDARD.encode(b);
        // Minimal PNG header (signature + IHDR) for a 40x10 image.
        let png = b64(&[
            0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 0, 0, 0, 0x0d, 0x49, 0x48, 0x44, 0x52, 0, 0, 0, 0x28, 0, 0, 0,
            0x0a,
        ]);
        kitty_caps();
        cell(10, 10);
        Self { _g: g, jpeg: b64(b"jpeg"), png, calls: Arc::new(Mutex::new(Vec::new())) }
    }
    fn transcoder(&self) -> pi_tui::components::image::ImageTranscoder {
        let (calls, jpeg, png) = (self.calls.clone(), self.jpeg.clone(), self.png.clone());
        Arc::new(move |data: &str, _mime: &str| {
            calls.lock().unwrap().push(data.to_string());
            (data == jpeg).then(|| png.clone())
        })
    }
    fn render(&self, data: &str, mime: &str) -> Vec<String> {
        Image::new(data, mime, plain_theme(), ImageOptions::default(), dims(20, 20)).render(20)
    }
    fn calls(&self) -> Vec<String> {
        self.calls.lock().unwrap().clone()
    }
}

#[test]
fn sends_converted_png_data_sized_from_the_png() {
    let t = Transcoding::new();
    set_image_transcoder(Some(t.transcoder()));
    let lines = t.render(&t.jpeg, "image/jpeg");
    assert!(lines[0].contains("f=100") && lines[0].contains(&format!(";{}\x1b\\", t.png)));
    // 40x10 PNG at 18 columns: 5 rows, not the 18 rows of the 20x20 source dimensions.
    assert_eq!(lines.len(), 5);
}

#[test]
fn renders_a_text_fallback_until_a_working_transcoder_is_registered() {
    let t = Transcoding::new();
    let image = Image::new(t.jpeg.clone(), "image/jpeg", plain_theme(), ImageOptions::default(), None);
    assert!(image.render(80)[0].starts_with("[Image: [image/jpeg]"));
    set_image_transcoder(Some(Arc::new(|_: &str, _: &str| None)));
    image.invalidate();
    assert!(image.render(80)[0].starts_with("[Image: [image/jpeg]"));
    set_image_transcoder(Some(t.transcoder()));
    image.invalidate();
    assert!(image.render(80)[0].contains("\x1b_G"));
}

#[test]
fn converts_each_image_once() {
    let t = Transcoding::new();
    set_image_transcoder(Some(t.transcoder()));
    let image = Image::new(t.jpeg.clone(), "image/jpeg", plain_theme(), ImageOptions::default(), None);
    image.render(80);
    t.render(&t.jpeg, "image/jpeg"); // A new instance hits the shared cache.
    for i in 0..40 {
        t.render(&format!("other-{i}"), "image/jpeg"); // Evicts the shared entry.
    }
    image.invalidate();
    image.render(40); // The instance keeps its own PNG.
    assert_eq!(t.calls().iter().filter(|c| **c == t.jpeg).count(), 1);
}

#[test]
fn does_not_convert_png_data_or_iterm2_output() {
    let t = Transcoding::new();
    set_image_transcoder(Some(t.transcoder()));
    assert!(t.render(&t.png, "image/png")[0].contains(&format!(";{}\x1b\\", t.png)));
    set_capabilities(caps(Some(ImageProtocol::Iterm2), true, true));
    assert!(t.render(&t.jpeg, "image/jpeg").last().unwrap().ends_with(&format!(":{}\x07", t.jpeg)));
    assert!(t.calls().is_empty());
}

// ---- hyperlink ----------------------------------------------------------------------------

#[test]
fn wraps_text_in_osc_8_open_and_close_sequences() {
    assert_eq!(hyperlink("click me", "https://example.com"), "\x1b]8;;https://example.com\x1b\\click me\x1b]8;;\x1b\\");
}

#[test]
fn preserves_ansi_styling_inside_the_hyperlink() {
    let styled = "\x1b[4m\x1b[34mclick me\x1b[0m";
    let result = hyperlink(styled, "https://example.com");
    assert!(result.starts_with("\x1b]8;;https://example.com\x1b\\"));
    assert!(result.contains(styled));
    assert!(result.ends_with("\x1b]8;;\x1b\\"));
}

#[test]
fn works_with_empty_text() {
    assert_eq!(hyperlink("", "https://example.com"), "\x1b]8;;https://example.com\x1b\\\x1b]8;;\x1b\\");
}

#[test]
fn works_with_file_uris() {
    let result = hyperlink("README.md", "file:///home/user/README.md");
    assert!(result.contains("file:///home/user/README.md"));
    assert!(result.contains("README.md"));
}
