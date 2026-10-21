//! Differential test: terminal image helpers versus the TypeScript reference (capability
//! detection from the environment, encoders, cell sizing, rendering, Kitty placement and
//! cropping, image dimension parsing and text fallbacks).
//! One test function: capabilities, cell dimensions and Kitty metadata are process-global.

use pi_tui::terminal_image::*;
use serde_json::{Value, json};

const ENV_KEYS: [&str; 16] = [
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

fn protocol(v: &Value) -> Option<ImageProtocol> {
    match v.as_str() {
        Some("kitty") => Some(ImageProtocol::Kitty),
        Some("iterm2") => Some(ImageProtocol::Iterm2),
        _ => None,
    }
}

fn caps_json(c: TerminalCapabilities) -> Value {
    json!({
        "images": match c.images { Some(ImageProtocol::Kitty) => json!("kitty"), Some(ImageProtocol::Iterm2) => json!("iterm2"), None => Value::Null },
        "trueColor": c.true_color,
        "hyperlinks": c.hyperlinks,
    })
}

fn dims(v: &Value) -> ImageDimensions {
    ImageDimensions { width_px: v["widthPx"].as_u64().unwrap() as u32, height_px: v["heightPx"].as_u64().unwrap() as u32 }
}

fn dims_json(d: Option<ImageDimensions>) -> Value {
    d.map_or(Value::Null, |d| json!({"widthPx": d.width_px, "heightPx": d.height_px}))
}

fn opt_usize(v: &Value) -> Option<usize> {
    v.as_u64().map(|n| n as usize)
}

#[test]
fn terminal_image_helpers_match_typescript_reference() {
    let data: Value = serde_json::from_str(include_str!("fixtures/terminal_image_cases.json")).unwrap();
    let mut failures: Vec<String> = Vec::new();
    let mut check = |what: String, got: Value, want: &Value| {
        if &got != want {
            failures.push(format!("{what}\n  got  {got}\n  want {want}"));
        }
    };

    // SAFETY: this binary has a single test; nothing reads the environment concurrently.
    let saved: Vec<(&str, Option<String>)> = ENV_KEYS.iter().map(|k| (*k, std::env::var(k).ok())).collect();
    for case in data["detect"].as_array().unwrap() {
        for k in ENV_KEYS {
            unsafe { std::env::remove_var(k) };
        }
        for (k, v) in case["env"].as_object().unwrap() {
            unsafe { std::env::set_var(k, v.as_str().unwrap()) };
        }
        let probe = case["probe"].as_bool().unwrap();
        let got = caps_json(detect_capabilities(Some(&move || probe)));
        check(format!("detect {}", case["env"]), got, &case["caps"]);
    }
    for (k, v) in saved {
        match v {
            Some(v) => unsafe { std::env::set_var(k, v) },
            None => unsafe { std::env::remove_var(k) },
        }
    }

    for case in data["isImage"].as_array().unwrap() {
        let line = case[0].as_str().unwrap();
        check(format!("is_image_line({line:?})"), json!(is_image_line(line)), &case[1]);
    }

    for case in data["kitty"].as_array().unwrap() {
        let o = &case["opts"];
        let opts = KittyEncodeOptions {
            columns: opt_usize(&o["columns"]),
            rows: opt_usize(&o["rows"]),
            image_id: o["imageId"].as_u64().map(|n| n as u32),
            move_cursor: o["moveCursor"].as_bool(),
        };
        check(format!("encode_kitty {o}"), json!(encode_kitty(case["data"].as_str().unwrap(), opts)), &case["seq"]);
    }
    for case in data["iterm"].as_array().unwrap() {
        let o = &case["opts"];
        let s = |v: &Value| match v {
            Value::Number(n) => Some(n.to_string()),
            Value::String(s) => Some(s.clone()),
            _ => None,
        };
        let opts = Iterm2EncodeOptions {
            width: s(&o["width"]),
            height: s(&o["height"]),
            name: o["name"].as_str().map(str::to_string),
            preserve_aspect_ratio: o["preserveAspectRatio"].as_bool(),
            inline: o["inline"].as_bool(),
        };
        check(format!("encode_iterm2 {o}"), json!(encode_iterm2(case["data"].as_str().unwrap(), &opts)), &case["seq"]);
    }

    for case in data["size"].as_array().unwrap() {
        let cell = CellDimensions {
            width_px: case["cell"]["widthPx"].as_u64().unwrap() as u32,
            height_px: case["cell"]["heightPx"].as_u64().unwrap() as u32,
        };
        let size = calculate_image_cell_size(
            dims(&case["dims"]),
            case["maxW"].as_u64().unwrap() as usize,
            opt_usize(&case["maxH"]),
            cell,
            case["opt"].as_bool().unwrap(),
        );
        check(format!("cell size {case}"), json!({"columns": size.columns, "rows": size.rows}), &case["size"]);
        let rows = calculate_image_rows(dims(&case["dims"]), case["maxW"].as_u64().unwrap() as usize, Some(cell));
        check(format!("rows {case}"), json!(rows), &case["rows"]);
    }

    for (i, case) in data["render"].as_array().unwrap().iter().enumerate() {
        set_capabilities(TerminalCapabilities { images: protocol(&case["protocol"]), true_color: true, hyperlinks: false });
        set_cell_dimensions(CellDimensions {
            width_px: case["cell"]["widthPx"].as_u64().unwrap() as u32,
            height_px: case["cell"]["heightPx"].as_u64().unwrap() as u32,
        });
        let o = &case["opts"];
        let result = render_image(
            case["data"].as_str().unwrap(),
            dims(&case["dims"]),
            &ImageRenderOptions {
                max_width_cells: opt_usize(&o["maxWidthCells"]),
                max_height_cells: opt_usize(&o["maxHeightCells"]),
                preserve_aspect_ratio: o["preserveAspectRatio"].as_bool(),
                image_id: o["imageId"].as_u64().map(|n| n as u32),
                move_cursor: o["moveCursor"].as_bool(),
            },
        );
        let got = result.as_ref().map_or(Value::Null, |r| {
            let mut v = json!({"sequence": r.sequence, "columns": r.columns, "rows": r.rows});
            if let Some(id) = r.image_id {
                v["imageId"] = json!(id);
            }
            v
        });
        check(format!("render {i}"), got, &case["result"]);
        let Some(line) = case["line"].as_str() else { continue };
        check(format!("render {i} rows"), json!(get_kitty_image_placement_rows(line)), &case["rowsOf"]);
        let meta = get_kitty_image_metadata(line).map_or(Value::Null, |m| {
            json!({"imageId": m.image_id, "columns": m.columns, "rows": m.rows, "widthPx": m.width_px, "heightPx": m.height_px})
        });
        check(format!("render {i} metadata"), meta, &case["meta"]);
        let placement = get_kitty_image_placement(line).map_or(Value::Null, |p| {
            json!({
                "imageId": p.image_id,
                "transmissionGeneration": p.transmission_generation,
                "transmissionBytes": p.transmission_bytes,
                "estimatedDecodedBytes": p.estimated_decoded_bytes,
                "rows": p.rows,
                "sequence": p.sequence,
                "replacementLine": p.replacement_line,
            })
        });
        check(format!("render {i} placement"), placement, &case["placement"]);
        let (hidden, visible) = (case["crop"][0].as_i64().unwrap(), case["crop"][1].as_i64().unwrap());
        let cropped = if hidden < 0 { line.to_string() } else { crop_kitty_image_line(line, hidden as usize, visible as usize) };
        check(format!("render {i} crop {hidden} {visible}"), json!(cropped), &case["crop"][2]);
    }
    set_cell_dimensions(CellDimensions { width_px: 9, height_px: 18 });

    for case in data["dims"].as_array().unwrap() {
        let (mime, d) = (case[0].as_str().unwrap(), case[1].as_str().unwrap());
        check(format!("dimensions {mime} {d}"), dims_json(get_image_dimensions(d, mime)), &case[2]);
    }

    for case in data["fallback"].as_array().unwrap() {
        set_capabilities(TerminalCapabilities { images: None, true_color: true, hyperlinks: case["hyper"].as_bool().unwrap() });
        let d = if case["dims"].is_null() { None } else { Some(dims(&case["dims"])) };
        let text = image_fallback("image/png", d, case["f"].as_str());
        check(format!("fallback {case}"), json!(text), &case["text"]);
    }

    if !failures.is_empty() {
        let n = failures.len();
        failures.truncate(8);
        panic!("{n} mismatches:\n{}", failures.join("\n"));
    }
}
