//! Ports of select-list.test.ts and settings-list.test.ts.

use std::sync::{Arc, Mutex};

use pi_tui::components::select_list::*;
use pi_tui::components::settings_list::*;
use pi_tui::tui::Component;
use pi_tui::utils::visible_width;

fn item(v: &str, d: &str) -> SelectItem {
    SelectItem::new(v, v, Some(d.to_string()))
}

fn visible_index_of(line: &str, text: &str) -> usize {
    let idx = line.find(text).expect("text present");
    visible_width(&line[..idx])
}

#[test]
fn normalizes_multiline_descriptions() {
    let list = SelectList::new(vec![item("test", "Line one\nLine two\nLine three")], 5, SelectListTheme::plain(), Default::default());
    let r = list.render(100);
    assert!(!r[0].contains('\n'));
    assert!(r[0].contains("Line one Line two Line three"));
}

#[test]
fn keeps_descriptions_aligned_when_primary_truncated() {
    let list = SelectList::new(
        vec![
            item("short", "short description"),
            item("very-long-command-name-that-needs-truncation", "long description"),
        ],
        5,
        SelectListTheme::plain(),
        Default::default(),
    );
    let r = list.render(80);
    assert_eq!(visible_index_of(&r[0], "short description"), visible_index_of(&r[1], "long description"));
}

#[test]
fn configured_min_and_max_primary_widths() {
    let layout = SelectListLayoutOptions { min_primary_column_width: Some(12), max_primary_column_width: Some(20), truncate_primary: None };
    let list = SelectList::new(vec![item("a", "first"), item("bb", "second")], 5, SelectListTheme::plain(), layout.clone());
    let r = list.render(80);
    assert_eq!(visible_index_of(&r[0], "first"), 14);
    assert_eq!(visible_index_of(&r[1], "second"), 14);
    let list = SelectList::new(
        vec![item("very-long-command-name-that-needs-truncation", "first"), item("short", "second")],
        5,
        SelectListTheme::plain(),
        layout,
    );
    let r = list.render(80);
    assert_eq!(visible_index_of(&r[0], "first"), 22);
    assert_eq!(visible_index_of(&r[1], "second"), 22);
}

#[test]
fn custom_primary_truncation_preserves_alignment() {
    let layout = SelectListLayoutOptions {
        min_primary_column_width: Some(12),
        max_primary_column_width: Some(12),
        truncate_primary: Some(Arc::new(|ctx: &SelectListTruncatePrimaryContext| {
            if ctx.text.chars().count() <= ctx.max_width {
                ctx.text.to_string()
            } else {
                let keep: String = ctx.text.chars().take(ctx.max_width.saturating_sub(1)).collect();
                format!("{keep}…")
            }
        })),
    };
    let list = SelectList::new(
        vec![item("very-long-command-name-that-needs-truncation", "first"), item("short", "second")],
        5,
        SelectListTheme::plain(),
        layout,
    );
    let r = list.render(80);
    assert!(r[0].contains('…'));
    assert_eq!(visible_index_of(&r[0], "first"), visible_index_of(&r[1], "second"));
}

fn settings_theme() -> SettingsListTheme {
    SettingsListTheme {
        label: Arc::new(|t: &str, _| t.to_string()),
        value: Arc::new(|t: &str, _| t.to_string()),
        description: Arc::new(|t: &str| t.to_string()),
        cursor: "> ".into(),
        hint: Arc::new(|t: &str| t.to_string()),
    }
}

fn settings_items() -> Vec<SettingItem> {
    vec![SettingItem {
        id: "tui-mode".into(),
        label: "TUI mode".into(),
        current_value: "regular".into(),
        values: Some(vec!["regular".into(), "fullscreen".into()]),
        ..Default::default()
    }]
}

#[test]
fn includes_spaces_in_active_search() {
    let changes = Arc::new(Mutex::new(Vec::new()));
    let c2 = changes.clone();
    let list = SettingsList::new(
        settings_items(),
        10,
        settings_theme(),
        Arc::new(move |id, v| c2.lock().unwrap().push((id, v))),
        Arc::new(|| {}),
        SettingsListOptions { enable_search: true },
    );
    for ch in "TUI mode".chars() {
        list.handle_input(&ch.to_string());
    }
    assert!(changes.lock().unwrap().is_empty());
    assert!(list.render(80)[0].contains("TUI mode"));
    list.handle_input("\r");
    assert_eq!(*changes.lock().unwrap(), vec![("tui-mode".to_string(), "fullscreen".to_string())]);
}

#[test]
fn space_changes_setting_before_search() {
    let changes = Arc::new(Mutex::new(Vec::new()));
    let c2 = changes.clone();
    let list = SettingsList::new(
        settings_items(),
        10,
        settings_theme(),
        Arc::new(move |id, v| c2.lock().unwrap().push((id, v))),
        Arc::new(|| {}),
        SettingsListOptions { enable_search: true },
    );
    list.handle_input(" ");
    assert_eq!(*changes.lock().unwrap(), vec![("tui-mode".to_string(), "fullscreen".to_string())]);
}
