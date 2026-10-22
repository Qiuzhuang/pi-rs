//! Differential test for key parsing/matching against the TypeScript implementation.
//! Inputs include every string literal from keys.test.ts plus generated sequences.

use pi_tui::keys::*;
use serde_json::Value;

#[test]
fn keys_match_typescript_reference() {
    // SAFETY: tests in this binary do not read the environment concurrently.
    unsafe { std::env::remove_var("WT_SESSION") };
    let data: Value = serde_json::from_str(include_str!("fixtures/keys_cases.json")).unwrap();
    let ids: Vec<&str> = data["keyIds"].as_array().unwrap().iter().map(|v| v.as_str().unwrap()).collect();
    let mut failures = Vec::new();
    for case in data["cases"].as_array().unwrap() {
        let kitty = case["kitty"].as_bool().unwrap();
        set_kitty_protocol_active(kitty);
        let d = case["d"].as_str().unwrap();
        let parse = parse_key(d).map(Value::from).unwrap_or(Value::Null);
        if parse != case["parse"] {
            failures.push(format!("[kitty={kitty}] parse_key({d:?}) = {parse:?}, want {:?}", case["parse"]));
        }
        let printable = decode_printable_key(d).map(Value::from).unwrap_or(Value::Null);
        if printable != case["printable"] {
            failures.push(format!("[kitty={kitty}] printable({d:?}) = {printable:?}, want {:?}", case["printable"]));
        }
        if is_key_release(d) != case["rel"] || is_key_repeat(d) != case["rep"] {
            failures.push(format!("[kitty={kitty}] release/repeat mismatch for {d:?}"));
        }
        let want: Vec<u64> = case["m"].as_array().unwrap().iter().map(|v| v.as_u64().unwrap()).collect();
        let got: Vec<u64> =
            ids.iter().enumerate().filter(|(_, id)| matches_key(d, id)).map(|(i, _)| i as u64).collect();
        if got != want {
            let names = |v: &[u64]| v.iter().map(|i| ids[*i as usize]).collect::<Vec<_>>();
            failures.push(format!(
                "[kitty={kitty}] matches_key({d:?}): got {:?}, want {:?}",
                names(&got),
                names(&want)
            ));
        }
    }
    set_kitty_protocol_active(false);
    if !failures.is_empty() {
        panic!("{} mismatches:\n{}", failures.len(), failures.iter().take(20).cloned().collect::<Vec<_>>().join("\n"));
    }
}
