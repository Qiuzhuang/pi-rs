//! Ports of visible-width.test.ts, truncate-to-width.test.ts,
//! regression-slice-by-column-ansi-order.test.ts and regression-regional-indicator-width.test.ts.

use pi_tui::utils::*;

#[test]
fn measures_styled_ascii_without_counting_escape_sequences() {
    assert_eq!(visible_width("\x1b[38;5;4mhello\x1b[39m world"), 11);
    assert_eq!(visible_width("\x1b]8;;https://example.com\x07link\x1b]8;;\x07"), 4);
    assert_eq!(visible_width("\x1b]133;A\x1b\\prompt"), 6);
    assert_eq!(visible_width("\x1b_pi:c\x07cursor"), 6);
}

#[test]
fn counts_tabs_as_three_columns_in_styled_text() {
    assert_eq!(visible_width("\x1b[1ma\tb\x1b[22m"), 5);
}

#[test]
fn measures_styled_non_ascii_text() {
    assert_eq!(visible_width("\x1b[31m日本\x1b[39m ok"), 7);
    assert_eq!(visible_width("\x1b[31m─→\x1b[39m"), 2);
}

#[test]
fn treats_unterminated_escape_sequences_as_zero_width_control_characters() {
    assert_eq!(visible_width("\x1b[31"), 3);
    assert_eq!(visible_width("a\x1b"), 1);
}

#[test]
fn keeps_output_within_width_for_very_large_unicode_input() {
    let text = "🙂界".repeat(100_000);
    let truncated = truncate_to_width(&text, 40, "…", false);
    assert!(visible_width(&truncated) <= 40);
    assert!(truncated.ends_with("…\x1b[0m"));
}

#[test]
fn preserves_ansi_styling_for_kept_text_and_resets_around_ellipsis() {
    let text = format!("\x1b[31m{}\x1b[0m", "hello ".repeat(1000));
    let truncated = truncate_to_width(&text, 20, "…", false);
    assert!(visible_width(&truncated) <= 20);
    assert!(truncated.contains("\x1b[31m"));
    assert!(truncated.ends_with("\x1b[0m…\x1b[0m"));
}

#[test]
fn closes_bel_terminated_osc8_link_when_truncating_label() {
    let open = "\x1b]8;;https://example.com\x07";
    let close = "\x1b]8;;\x07";
    let text = format!("{open}some-longer-label-here{close}");
    assert_eq!(truncate_to_width(&text, 15, "...", false), format!("{open}some-longer-{close}\x1b[0m...\x1b[0m"));
}

#[test]
fn handles_malformed_ansi_prefixes_without_hanging() {
    let text = format!("abc\x1bnot-ansi {}", "🙂".repeat(1000));
    let truncated = truncate_to_width(&text, 20, "…", false);
    assert!(visible_width(&truncated) <= 20);
}

#[test]
fn clips_wide_ellipsis_safely() {
    assert_eq!(truncate_to_width("abcdef", 1, "🙂", false), "");
    assert_eq!(truncate_to_width("abcdef", 2, "🙂", false), "\x1b[0m🙂\x1b[0m");
    assert!(visible_width(&truncate_to_width("abcdef", 2, "🙂", false)) <= 2);
}

#[test]
fn returns_original_text_when_it_fits_even_if_ellipsis_too_wide() {
    assert_eq!(truncate_to_width("a", 2, "🙂", false), "a");
    assert_eq!(truncate_to_width("界", 2, "🙂", false), "界");
}

#[test]
fn pads_truncated_output_to_requested_width() {
    let truncated = truncate_to_width("🙂界🙂界🙂界", 8, "…", true);
    assert_eq!(visible_width(&truncated), 8);
}

#[test]
fn adds_trailing_reset_when_truncating_without_ellipsis() {
    let truncated = truncate_to_width(&format!("\x1b[31m{}", "hello".repeat(100)), 10, "", false);
    assert!(visible_width(&truncated) <= 10);
    assert!(truncated.ends_with("\x1b[0m"));
}

#[test]
fn keeps_contiguous_prefix() {
    let truncated = truncate_to_width("🙂\t界 \x1b_abc\x07", 7, "…", true);
    assert_eq!(truncated, "🙂\t\x1b[0m…\x1b[0m ");
}

#[test]
fn counts_tabs_inline_and_skips_ansi_inline() {
    assert_eq!(visible_width("\t\x1b[31m界\x1b[0m"), 5);
}

#[test]
fn counts_indic_conjunct_spacing_code_points() {
    assert_eq!(visible_width("र्क"), 2);
    assert_eq!(visible_width("नेटवर्क"), 5);
    assert_eq!(visible_width("सर्वाधिकार सुरक्षित। ऑर्डर पर क्लिक करें"), 33);
    assert_eq!(visible_width("র্ক"), 2);
    assert_eq!(visible_width("ર્ક"), 2);
    assert_eq!(visible_width("ର୍କ"), 2);
    assert_eq!(visible_width("ర్క"), 2);
    assert_eq!(visible_width("ര്‍ക"), 2);
}

#[test]
fn keeps_ordinary_combining_marks_zero_width() {
    assert_eq!(visible_width("e\u{0301}"), 1);
    assert_eq!(visible_width("čřžůú"), 5);
    assert_eq!(visible_width("שָׁ"), 1);
    assert_eq!(visible_width("بّ"), 1);
    assert_eq!(visible_width("རྐ"), 1);
    assert_eq!(visible_width("ᜠ᜴"), 1);
    assert_eq!(visible_width("가〮"), 2);
    assert_eq!(visible_width("가〯"), 2);
}

#[test]
fn keeps_cjk_and_japanese_width_accounting() {
    assert_eq!(visible_width("网络"), 4);
    assert_eq!(visible_width("ネットワーク"), 12);
    assert_eq!(visible_width("が"), 2);
    assert_eq!(visible_width("か\u{3099}"), 2);
}

#[test]
fn counts_myanmar_marks() {
    for s in ["ကာ", "ကေ", "က်", "ကျ", "ကြ", "ကဳ", "ကဴ", "ကဵ", "ကး"] {
        assert_eq!(visible_width(s), 2, "{s}");
    }
    assert_eq!(visible_width("ကို"), 1);
    assert_eq!(visible_width("က္"), 1);
}

#[test]
fn keeps_thai_and_lao_am_clusters_at_normal_width() {
    assert_eq!(visible_width("ำ"), 1);
    assert_eq!(visible_width("ຳ"), 1);
    assert_eq!(visible_width("กำ"), 2);
    assert_eq!(visible_width("ກຳ"), 2);
}

#[test]
fn normalizes_thai_and_lao_am_vowels_for_output() {
    assert_eq!(normalize_terminal_output("ำ"), "ํา");
    assert_eq!(normalize_terminal_output("ຳ"), "ໍາ");
    assert_eq!(visible_width(&normalize_terminal_output("ำabc")), visible_width("ำabc"));
    assert_eq!(visible_width(&normalize_terminal_output("ຳabc")), visible_width("ຳabc"));
}

#[test]
fn slice_keeps_reset_at_slice_start_after_earlier_style_codes() {
    let line = "\x1b[32mfoo\x1b[39m bar";
    assert_eq!(slice_by_column(line, 3, 4, true), "\x1b[32m\x1b[39m bar");
}

#[test]
fn slice_does_not_leak_color_after_highlighted_token() {
    let line = "Another \x1b[35malpha\x1b[39m line with \x1b[35mbeta\x1b[39m later.";
    assert_eq!(slice_by_column(line, 13, 100, true), "\x1b[35m\x1b[39m line with \x1b[35mbeta\x1b[39m later.");
}

#[test]
fn partial_flag_is_full_width() {
    assert_eq!(visible_width("🇨"), 2);
    assert_eq!(visible_width("      - 🇨"), 10);
}

#[test]
fn wraps_partial_flag_list_line_before_overflow() {
    let wrapped = wrap_text_with_ansi("      - 🇨", 9);
    assert_eq!(wrapped.len(), 2);
    assert_eq!(visible_width(&wrapped[0]), 7);
    assert_eq!(visible_width(&wrapped[1]), 2);
}

#[test]
fn all_regional_indicator_singletons_are_width_2() {
    for cp in 0x1f1e6..=0x1f1ffu32 {
        let s = char::from_u32(cp).unwrap().to_string();
        assert_eq!(visible_width(&s), 2, "U+{cp:X}");
    }
}

#[test]
fn full_flag_pairs_are_width_2() {
    for flag in ["🇯🇵", "🇺🇸", "🇬🇧", "🇨🇳", "🇩🇪", "🇫🇷"] {
        assert_eq!(visible_width(flag), 2, "{flag}");
    }
}

#[test]
fn common_streaming_emoji_intermediates_are_stable() {
    for s in ["👍", "👍🏻", "✅", "⚡", "⚡️", "👨", "👨‍💻", "🏳️‍🌈"] {
        assert_eq!(visible_width(s), 2, "{s}");
    }
}
