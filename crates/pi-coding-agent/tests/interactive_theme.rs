//! Ports of the interactive theme tests (`theme-style.test.ts`, `theme-detection.test.ts`,
//! `theme-export.test.ts`, `scrollbar-theme.test.ts`, `theme-picker.test.ts`, `themed-text.test.ts`,
//! `chat-viewport.test.ts`).
//!
//! The theme module keeps global state (terminal colors, the active theme, `PI_CODING_AGENT_DIR`), so
//! every test takes [`lock`] to run serially within this binary.

use std::path::PathBuf;
use std::sync::Arc;

use parking_lot::{Mutex, MutexGuard};
use pi_coding_agent::modes::interactive::chat_viewport::{ChatViewportOptions, create_chat_viewport};
use pi_coding_agent::modes::interactive::theme::theme::{
    GetThemeExportColorsResult, Theme, ThemeInfo, ThemeJsonValidator, ThemeStyle, ThemeStyleBg, ThemeStyleFg,
    TerminalTheme, detect_color_fg_bg_theme, detect_terminal_theme, get_available_themes,
    get_available_themes_with_paths, get_theme_by_name, get_theme_export_colors, init_theme,
    load_theme_from_path, parse_auto_theme_setting, resolve_theme_setting, set_registered_themes,
    set_terminal_colors, set_theme_json_validator, theme,
};
use pi_coding_agent::modes::interactive::theme::theme_schema::validate_theme_json;
use pi_coding_agent::modes::interactive::theme::theme_tokens::{ThemeBg, ThemeColor, ThemeToken};
use pi_tui::colors::{Color, TerminalColorMode, TextAttributes, TextStyle, color_to_hex, okhsl_color, style_text};
use pi_tui::tui::{Component, Container};
use pi_tui::terminal_colors::{RgbColor, TerminalColors};
use pi_tui::terminal_image::{TerminalCapabilities, reset_capabilities_cache, set_capabilities};
use serde_json::{Value, json};

static LOCK: Mutex<()> = Mutex::new(());

struct Guard {
    _lock: MutexGuard<'static, ()>,
    dirs: Vec<PathBuf>,
    prev_agent_dir: Option<String>,
}

impl Drop for Guard {
    fn drop(&mut self) {
        set_terminal_colors(Arc::new(TerminalColors::default()));
        reset_capabilities_cache();
        set_registered_themes(Vec::new());
        // SAFETY: tests in this binary are serialized by LOCK.
        unsafe {
            match &self.prev_agent_dir {
                Some(dir) => std::env::set_var("PI_CODING_AGENT_DIR", dir),
                None => std::env::remove_var("PI_CODING_AGENT_DIR"),
            }
        }
        for dir in self.dirs.drain(..) {
            let _ = std::fs::remove_dir_all(dir);
        }
    }
}

fn lock() -> Guard {
    let lock = LOCK.lock();
    Guard { _lock: lock, dirs: Vec::new(), prev_agent_dir: std::env::var("PI_CODING_AGENT_DIR").ok() }
}

impl Guard {
    fn temp_dir(&mut self, prefix: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("{prefix}{}-{}", std::process::id(), rand_suffix()));
        std::fs::create_dir_all(&dir).unwrap();
        self.dirs.push(dir.clone());
        dir
    }

    /// Point `PI_CODING_AGENT_DIR` at a fresh directory with an empty `themes` folder.
    fn stub_agent_dir(&mut self) -> PathBuf {
        let root = self.temp_dir("pi-theme-agent-");
        let agent_dir = root.join("agent");
        std::fs::create_dir_all(agent_dir.join("themes")).unwrap();
        // SAFETY: tests in this binary are serialized by LOCK.
        unsafe { std::env::set_var("PI_CODING_AGENT_DIR", &agent_dir) };
        agent_dir
    }

    /// Load a copy of a built-in theme, modified by `edit`.
    fn load_theme(&mut self, base: &str, edit: impl FnOnce(&mut Value)) -> Arc<Theme> {
        let mut json = builtin_theme_json(base);
        edit(&mut json);
        let dir = self.temp_dir("pi-theme-style-");
        let path = dir.join(format!("{}.json", json["name"].as_str().unwrap()));
        std::fs::write(&path, serde_json::to_string(&json).unwrap()).unwrap();
        load_theme_from_path(path.to_str().unwrap(), Some(TerminalColorMode::Truecolor)).unwrap()
    }
}

fn rand_suffix() -> String {
    use std::sync::atomic::{AtomicU64, Ordering};
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().subsec_nanos();
    format!("{nanos}-{}", COUNTER.fetch_add(1, Ordering::Relaxed))
}

fn builtin_theme_json(base: &str) -> Value {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("src/modes/interactive/theme")
        .join(format!("{base}.json"));
    serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
}

fn rgb(r: f64, g: f64, b: f64) -> RgbColor {
    RgbColor { r, g, b }
}

fn color(theme: &Theme, token: ThemeToken) -> Color {
    theme.color(token).unwrap()
}

// ---------------------------------------------------------------------------------------------
// theme-style.test.ts
// ---------------------------------------------------------------------------------------------

#[test]
fn renders_theme_tokens_the_same_as_the_generic_text_styler() {
    let mut g = lock();
    let theme = g.load_theme("dark", |_| {});
    let styled = theme.style(
        "Ready",
        &ThemeStyle {
            fg: Some(ThemeStyleFg::ThemeColor(ThemeColor::Success)),
            bg: Some(ThemeStyleBg::ThemeBg(ThemeBg::ToolSuccessBg)),
            bold: Some(true),
            ..Default::default()
        },
    );
    let expected = style_text(
        "Ready",
        &TextStyle {
            attrs: TextAttributes { bold: true, ..Default::default() },
            fg: Some(color(&theme, ThemeToken::Success)),
            bg: Some(color(&theme, ThemeToken::ToolSuccessBg)),
        },
        TerminalColorMode::Truecolor,
    );
    assert_eq!(styled, expected);
}

#[test]
fn loads_oklch_theme_values() {
    let mut g = lock();
    let theme = g.load_theme("dark", |json| {
        json["colors"]["accent"] = json!("oklch(62% 0.1 200)");
    });
    match color(&theme, ThemeToken::Accent) {
        Color::Oklch { l, c, h } => {
            assert!((l - 0.62).abs() < 1e-9);
            assert!((c - 0.1).abs() < 1e-9);
            assert!((h - 200.0).abs() < 1e-9);
        }
        other => panic!("expected oklch, got {other:?}"),
    }
}

#[test]
fn loads_okhsl_theme_values_including_through_variables() {
    let mut g = lock();
    let theme = g.load_theme("dark", |json| {
        json["vars"]["brand"] = json!("okhsl(250 60% 55%)");
        json["colors"]["accent"] = json!("brand");
        json["colors"]["error"] = json!("okhsl(20 90% 60%)");
    });
    assert_eq!(
        color_to_hex(&color(&theme, ThemeToken::Accent)),
        color_to_hex(&okhsl_color(250.0, 0.6, 0.55).unwrap())
    );
    assert_eq!(
        color_to_hex(&color(&theme, ThemeToken::Error)),
        color_to_hex(&okhsl_color(20.0, 0.9, 0.6).unwrap())
    );
}

#[test]
fn detects_the_appearance_unless_it_is_declared() {
    let mut g = lock();
    assert_eq!(g.load_theme("dark", |_| {}).appearance(), TerminalTheme::Dark);
    assert_eq!(g.load_theme("light", |_| {}).appearance(), TerminalTheme::Light);
    for (base, expected) in [("dark", TerminalTheme::Dark), ("light", TerminalTheme::Light)] {
        let theme = g.load_theme(base, |json| {
            json.as_object_mut().unwrap().remove("appearance");
        });
        assert_eq!(theme.appearance(), expected);
    }
    assert_eq!(
        g.load_theme("dark", |json| json["appearance"] = json!("light")).appearance(),
        TerminalTheme::Light
    );

    // Palette colors 0-15 follow the terminal palette, so such themes follow the terminal background.
    let palette_only = g.load_theme("dark", |json| {
        json.as_object_mut().unwrap().remove("appearance");
        let colors = json["colors"].as_object_mut().unwrap();
        for (key, value) in colors.iter_mut() {
            *value = json!(if key.ends_with("Bg") { 0 } else { 7 });
        }
    });
    assert_eq!(palette_only.appearance(), TerminalTheme::Dark);
    set_terminal_colors(Arc::new(TerminalColors { background: Some(rgb(250.0, 250.0, 250.0)), ..Default::default() }));
    assert_eq!(palette_only.appearance(), TerminalTheme::Light);
}

#[test]
fn renders_empty_tokens_as_terminal_defaults_and_reports_concrete_colors_for_them() {
    let mut g = lock();
    let theme = g.load_theme("dark", |json| {
        json["colors"]["text"] = json!("");
        json["colors"]["userMessageBg"] = json!("");
    });
    assert_eq!(theme.fg(&ThemeColor::Text, "x"), "\x1b[39mx\x1b[39m");
    assert_eq!(theme.bg(&ThemeBg::UserMessageBg, "x"), "\x1b[49mx\x1b[49m");
    assert_eq!(color_to_hex(&color(&theme, ThemeToken::Text)), "#e5e5e7");
    assert_eq!(color_to_hex(&color(&theme, ThemeToken::UserMessageBg)), "#000000");

    set_terminal_colors(Arc::new(TerminalColors {
        foreground: Some(rgb(200.0, 210.0, 220.0)),
        background: Some(rgb(10.0, 20.0, 30.0)),
        ..Default::default()
    }));
    assert_eq!(color_to_hex(&color(&theme, ThemeToken::Text)), "#c8d2dc");
    assert_eq!(color_to_hex(&color(&theme, ThemeToken::UserMessageBg)), "#0a141e");
}

// ---------------------------------------------------------------------------------------------
// theme-detection.test.ts
// ---------------------------------------------------------------------------------------------

#[test]
fn detect_color_fg_bg_classifies_the_last_field_by_palette_index_like_vim() {
    let _g = lock();
    let env = |v: &str| Some(json!({ "COLORFGBG": v }));
    assert_eq!(detect_color_fg_bg_theme(env("15;0")), Some(TerminalTheme::Dark));
    assert_eq!(detect_color_fg_bg_theme(env("0;7;15")), Some(TerminalTheme::Light));
    // Solarized Dark's background is bright black.
    assert_eq!(detect_color_fg_bg_theme(env("12;8")), Some(TerminalTheme::Dark));
    // rxvt writes "default" when the background is not a palette color.
    assert_eq!(detect_color_fg_bg_theme(env("15;default")), None);
    assert_eq!(detect_color_fg_bg_theme(Some(json!({}))), None);
}

#[test]
fn detect_terminal_theme_prefers_background_then_scheme_then_colorfgbg_then_dark() {
    let _g = lock();
    let env = || Some(json!({ "COLORFGBG": "0;15" }));
    let bg = |r, g, b| Some(Arc::new(TerminalColors { background: Some(rgb(r, g, b)), ..Default::default() }));
    let empty = || Some(Arc::new(TerminalColors::default()));
    assert_eq!(detect_terminal_theme(bg(8.0, 8.0, 8.0), Some(&TerminalTheme::Light), env()), TerminalTheme::Dark);
    assert_eq!(detect_terminal_theme(empty(), Some(&TerminalTheme::Dark), env()), TerminalTheme::Dark);
    assert_eq!(detect_terminal_theme(empty(), None, env()), TerminalTheme::Light);
    assert_eq!(detect_terminal_theme(empty(), None, Some(json!({}))), TerminalTheme::Dark);
}

#[test]
fn detect_terminal_theme_follows_the_foreground_when_text_is_readable_that_way() {
    let _g = lock();
    let colors = |bg: RgbColor, fg: Option<RgbColor>| {
        Some(Arc::new(TerminalColors { background: Some(bg), foreground: fg, ..Default::default() }))
    };
    let background = rgb(118.0, 118.0, 118.0);
    let white = rgb(255.0, 255.0, 255.0);
    assert_eq!(detect_terminal_theme(colors(background, None), None, None), TerminalTheme::Light);
    assert_eq!(detect_terminal_theme(colors(background, Some(white)), None, None), TerminalTheme::Dark);
    // White text cannot reach 4.5:1 on mid-gray.
    let mid_gray = rgb(128.0, 128.0, 128.0);
    assert_eq!(detect_terminal_theme(colors(mid_gray, Some(white)), None, None), TerminalTheme::Light);
}

#[test]
fn theme_color_mode_uses_terminal_capabilities() {
    let _g = lock();
    set_capabilities(TerminalCapabilities { images: None, true_color: false, hyperlinks: false });
    let ansi256 = get_theme_by_name("dark").expect("dark theme not found");
    assert_eq!(ansi256.get_color_mode(), TerminalColorMode::Color256);
    let accent = ansi256.get_fg_ansi(&ThemeColor::Accent);
    assert!(accent.starts_with("\x1b[38;5;") && accent.ends_with('m'), "{accent:?}");

    set_capabilities(TerminalCapabilities { images: None, true_color: true, hyperlinks: false });
    let truecolor = get_theme_by_name("dark").expect("dark theme not found");
    assert_eq!(truecolor.get_color_mode(), TerminalColorMode::Truecolor);
    let accent = truecolor.get_fg_ansi(&ThemeColor::Accent);
    assert!(accent.starts_with("\x1b[38;2;") && accent.ends_with('m'), "{accent:?}");
}

#[test]
fn parses_and_resolves_automatic_theme_settings() {
    let _g = lock();
    let parsed = parse_auto_theme_setting(Some("light/dark")).unwrap();
    assert_eq!(parsed.light_theme, "light");
    assert_eq!(parsed.dark_theme, "dark");
    assert_eq!(resolve_theme_setting(Some("dark"), &TerminalTheme::Light).as_deref(), Some("dark"));
    assert_eq!(resolve_theme_setting(Some("light/dark"), &TerminalTheme::Light).as_deref(), Some("light"));
    assert_eq!(resolve_theme_setting(Some("light/dark"), &TerminalTheme::Dark).as_deref(), Some("dark"));
    assert_eq!(resolve_theme_setting(Some("light/dark/extra"), &TerminalTheme::Dark), None);
}

// ---------------------------------------------------------------------------------------------
// theme-export.test.ts
// ---------------------------------------------------------------------------------------------

fn write_custom_theme(agent_dir: &std::path::Path, file: &str, theme: &Value) -> PathBuf {
    let path = agent_dir.join("themes").join(file);
    std::fs::write(&path, serde_json::to_string_pretty(theme).unwrap()).unwrap();
    path
}

#[test]
fn export_resolves_variable_references_using_the_same_syntax_as_colors() {
    let mut g = lock();
    let agent_dir = g.stub_agent_dir();
    let mut custom = builtin_theme_json("dark");
    custom["name"] = json!("custom-export-vars");
    custom["vars"]["pageBgVar"] = json!("#112233");
    custom["vars"]["pageBgAlias"] = json!("pageBgVar");
    custom["vars"]["infoBgVar"] = json!("#445566");
    custom["vars"]["cardBgVar"] = json!("#223344");
    custom["export"] = json!({ "pageBg": "pageBgAlias", "cardBg": "cardBgVar", "infoBg": "infoBgVar" });
    write_custom_theme(&agent_dir, "custom-export-vars.json", &custom);

    assert_eq!(
        get_theme_export_colors(Some("custom-export-vars")),
        GetThemeExportColorsResult {
            page_bg: Some("#112233".into()),
            card_bg: Some("#223344".into()),
            info_bg: Some("#445566".into()),
        }
    );
}

#[test]
fn export_converts_okhsl_colors_to_hex_because_css_does_not_support_them() {
    let mut g = lock();
    let agent_dir = g.stub_agent_dir();
    let mut custom = builtin_theme_json("dark");
    custom["name"] = json!("custom-export-okhsl");
    custom["vars"] = json!({ "card": "okhsl(250 20% 20%)" });
    custom["export"] = json!({ "pageBg": "okhsl(250 20% 15%)", "cardBg": "card", "infoBg": "oklch(30% 0.05 80)" });
    write_custom_theme(&agent_dir, "custom-export-okhsl.json", &custom);

    assert_eq!(
        get_theme_export_colors(Some("custom-export-okhsl")),
        GetThemeExportColorsResult {
            page_bg: Some(color_to_hex(&okhsl_color(250.0, 0.2, 0.15).unwrap())),
            card_bg: Some(color_to_hex(&okhsl_color(250.0, 0.2, 0.2).unwrap())),
            info_bg: Some("oklch(30% 0.05 80)".into()),
        }
    );
}

#[test]
fn export_resolves_recursive_vars_and_converts_256_color_values_to_hex() {
    let mut g = lock();
    let agent_dir = g.stub_agent_dir();
    let mut custom = builtin_theme_json("dark");
    custom["name"] = json!("custom-export-recursive");
    custom["vars"]["deepPageBg"] = json!("#abcdef");
    custom["vars"]["pageBgAlias"] = json!("deepPageBg");
    custom["vars"]["cardBgAnsi"] = json!(24);
    custom["export"] = json!({ "pageBg": "pageBgAlias", "cardBg": "cardBgAnsi", "infoBg": "" });
    write_custom_theme(&agent_dir, "custom-export-recursive.json", &custom);

    assert_eq!(
        get_theme_export_colors(Some("custom-export-recursive")),
        GetThemeExportColorsResult {
            page_bg: Some("#abcdef".into()),
            card_bg: Some("#005f87".into()),
            info_bg: None,
        }
    );
}

// ---------------------------------------------------------------------------------------------
// scrollbar-theme.test.ts
// ---------------------------------------------------------------------------------------------

fn install_validator() {
    let validator: ThemeJsonValidator = Arc::new(|label: String, json: Value| validate_theme_json(&label, json));
    set_theme_json_validator(&validator);
}

#[test]
fn scrollbar_tokens_fall_back_when_omitted() {
    let mut g = lock();
    install_validator();
    for (token, fallback) in [
        (ThemeColor::ScrollbarTrack, ThemeColor::Muted),
        (ThemeColor::ScrollbarThumb, ThemeColor::Text),
    ] {
        let key = serde_json::to_value(token).unwrap().as_str().unwrap().to_string();
        let theme = g.load_theme("dark", |json| {
            json["name"] = json!(format!("missing-{key}-theme"));
            json["colors"].as_object_mut().unwrap().remove(&key);
        });
        assert_eq!(theme.get_fg_ansi(&token), theme.get_fg_ansi(&fallback), "{key}");
    }
}

#[test]
fn uses_explicitly_configured_scrollbar_colors() {
    let mut g = lock();
    install_validator();
    let theme = g.load_theme("dark", |json| {
        json["name"] = json!("custom-scrollbar-theme");
        json["colors"]["scrollbarTrack"] = json!("#654321");
        json["colors"]["scrollbarThumb"] = json!("#123456");
    });
    assert_eq!(theme.get_fg_ansi(&ThemeColor::ScrollbarTrack), "\x1b[38;2;101;67;33m");
    assert_eq!(theme.get_fg_ansi(&ThemeColor::ScrollbarThumb), "\x1b[38;2;18;52;86m");
}

#[test]
fn search_highlights_fall_back_to_selection_and_text_colors() {
    let mut g = lock();
    install_validator();
    let theme = g.load_theme("dark", |json| {
        json["name"] = json!("legacy-search-theme");
        let colors = json["colors"].as_object_mut().unwrap();
        colors.remove("searchMatchBg");
        colors.remove("searchMatchText");
    });
    assert_eq!(theme.get_bg_ansi(&ThemeBg::SearchMatchBg), theme.get_bg_ansi(&ThemeBg::SelectedBg));
    assert_eq!(theme.get_fg_ansi(&ThemeColor::SearchMatchText), theme.get_fg_ansi(&ThemeColor::Text));
}

#[test]
fn uses_explicitly_configured_search_highlight_colors() {
    let mut g = lock();
    install_validator();
    let theme = g.load_theme("dark", |json| {
        json["name"] = json!("custom-search-theme");
        json["colors"]["searchMatchBg"] = json!("#112233");
        json["colors"]["searchMatchText"] = json!("#223344");
    });
    assert_eq!(theme.get_bg_ansi(&ThemeBg::SearchMatchBg), "\x1b[48;2;17;34;51m");
    assert_eq!(theme.get_fg_ansi(&ThemeColor::SearchMatchText), "\x1b[38;2;34;51;68m");
}

// ---------------------------------------------------------------------------------------------
// theme-picker.test.ts
// ---------------------------------------------------------------------------------------------

#[test]
fn theme_picker_uses_custom_theme_content_names_instead_of_file_names() {
    let mut g = lock();
    let agent_dir = g.stub_agent_dir();
    set_registered_themes(Vec::new());
    let mut custom = builtin_theme_json("dark");
    custom["name"] = json!("bar");
    let theme_path = write_custom_theme(&agent_dir, "foo.json", &custom);

    let names = get_available_themes();
    assert!(names.contains(&"bar".to_string()));
    assert!(!names.contains(&"foo".to_string()));
    let with_paths = get_available_themes_with_paths();
    assert!(with_paths.contains(&ThemeInfo { name: "bar".into(), path: Some(theme_path.to_string_lossy().into()) }));
    assert!(!with_paths.iter().any(|t| t.name == "foo"));
}

// ---------------------------------------------------------------------------------------------
// themed-text.test.ts
// ---------------------------------------------------------------------------------------------

#[test]
fn themed_text_builds_lazily_and_rebuilds_with_the_current_theme_after_invalidation() {
    use pi_coding_agent::modes::interactive::components::themed_text::ThemedText;
    use std::sync::atomic::{AtomicUsize, Ordering};
    let _g = lock();
    init_theme(Some("dark"), Some(false));
    let builds = Arc::new(AtomicUsize::new(0));
    let counter = builds.clone();
    let text = ThemedText::new(
        Arc::new(move || {
            counter.fetch_add(1, Ordering::SeqCst);
            theme().fg(&ThemeColor::Accent, "hello")
        }),
        None,
        None,
    );
    assert_eq!(builds.load(Ordering::SeqCst), 0);
    let dark = text.render(20).join("");

    init_theme(Some("light"), Some(false));
    assert_eq!(text.render(20).join(""), dark);
    text.invalidate();
    assert!(text.render(20).join("").contains(&theme().get_fg_ansi(&ThemeColor::Accent)));
    assert_eq!(builds.load(Ordering::SeqCst), 2);
    init_theme(Some("dark"), Some(false));
}

// ---------------------------------------------------------------------------------------------
// chat-viewport.test.ts
// ---------------------------------------------------------------------------------------------

#[test]
fn chat_viewport_defaults_the_transcript_scrollbar_to_auto_and_accepts_overrides() {
    use pi_tui::components::scroll_view::ScrollViewScrollbar as ScrollbarMode;
    let options = |scrollbar: Option<ScrollbarMode>| ChatViewportOptions {
        document: Container::new_ref(),
        pending_messages: Container::new_ref(),
        status: Container::new_ref(),
        editor: Container::new_ref(),
        footer: Container::new_ref(),
        widgets_above: None,
        widgets_below: None,
        scrollbar,
        scrollbar_track_style: None,
        scrollbar_thumb_style: None,
    };
    let automatic = create_chat_viewport(&options(None));
    let hidden = create_chat_viewport(&options(Some(ScrollbarMode::Hidden)));
    assert_eq!(automatic.transcript.scrollbar(), ScrollbarMode::Auto);
    assert_eq!(hidden.transcript.scrollbar(), ScrollbarMode::Hidden);
}
