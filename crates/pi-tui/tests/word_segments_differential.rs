//! Differential test: word segmentation versus V8's `Intl.Segmenter` (ICU).

use pi_tui::utils::word_segments;
use serde_json::Value;

fn mismatches(cases: &Value) -> Vec<String> {
    let mut bad = Vec::new();
    for case in cases.as_array().unwrap() {
        let text = case[0].as_str().unwrap();
        let want: Vec<(String, bool)> = case[1]
            .as_array()
            .unwrap()
            .iter()
            .map(|s| (s[0].as_str().unwrap().to_string(), s[1].as_bool().unwrap()))
            .collect();
        let got: Vec<(String, bool)> =
            word_segments(text).into_iter().map(|s| (s.segment.to_string(), s.is_word_like)).collect();
        if got != want {
            bad.push(format!("{text:?}\n  got  {got:?}\n  want {want:?}"));
        }
    }
    bad
}

/// ICU4X's CJ dictionary heuristics differ slightly from ICU4C's (unknown Han pairs and
/// long katakana runs), so a small number of contrived strings segment differently.
/// The thresholds guard against regressions.
#[test]
fn segmentation_matches_v8_closely() {
    let data: Value = serde_json::from_str(include_str!("fixtures/word_segments_cases.json")).unwrap();
    let bad = mismatches(&data["cj"]);
    let total = data["cj"].as_array().unwrap().len();
    eprintln!("cj mismatches: {} of {total}", bad.len());
    for b in bad.iter().take(5) {
        eprintln!("{b}");
    }
    let bad_all = mismatches(&data["all"]);
    eprintln!("all mismatches: {} of {}", bad_all.len(), data["all"].as_array().unwrap().len());
    for b in bad_all.iter().take(5) {
        eprintln!("{b}");
    }
    assert!(bad.len() <= 90, "too many CJ segmentation mismatches: {}", bad.len());
    assert!(bad_all.len() <= 30, "too many mixed-script segmentation mismatches: {}", bad_all.len());
}
