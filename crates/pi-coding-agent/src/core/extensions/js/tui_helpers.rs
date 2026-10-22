//! `@earendil-works/pi-tui` color and terminal-image helpers for JavaScript extensions
//! (`parseColor()`, `mixColors()`, `renderImage()`, `getCapabilities()`, ...), backed by the Rust
//! ports in `pi_tui::colors` / `pi_tui::terminal_image`. Values use the TS shapes: colors are
//! `{ kind: "indexed" | "rgb" | "oklch", ... }`, dimensions `{ widthPx, heightPx }`.

use pi_tui::colors::{self, Color, ColorMixSpace, TerminalColorMode};
use pi_tui::terminal_image::{
    self as ti, CapabilityOverrides, CellDimensions, ImageDimensions, ImageProtocol, ImageRenderOptions,
    Iterm2EncodeOptions, KittyEncodeOptions, TerminalCapabilities,
};
use serde_json::{Map, Value, json};

fn color_json(color: &Color) -> Value {
    match *color {
        Color::Indexed { index } => json!({ "kind": "indexed", "index": index }),
        Color::Rgb { r, g, b } => json!({ "kind": "rgb", "r": r, "g": g, "b": b }),
        Color::Oklch { l, c, h } => json!({ "kind": "oklch", "l": l, "c": c, "h": h }),
    }
}

fn color_from(value: &Value) -> anyhow::Result<Color> {
    let n = |k: &str| value.get(k).and_then(Value::as_f64).unwrap_or(0.0);
    Ok(match value.get("kind").and_then(Value::as_str) {
        Some("indexed") => Color::Indexed { index: n("index") as u8 },
        Some("rgb") => Color::Rgb { r: n("r"), g: n("g"), b: n("b") },
        Some("oklch") => Color::Oklch { l: n("l"), c: n("c"), h: n("h") },
        _ => anyhow::bail!("invalid color: {value}"),
    })
}

fn mode_from(value: &Value) -> TerminalColorMode {
    match value.as_str() {
        Some("truecolor") => TerminalColorMode::Truecolor,
        _ => TerminalColorMode::Color256,
    }
}

fn mode_json(mode: TerminalColorMode) -> Value {
    json!(match mode {
        TerminalColorMode::Truecolor => "truecolor",
        TerminalColorMode::Color256 => "256color",
    })
}

fn protocol_json(protocol: Option<ImageProtocol>) -> Value {
    match protocol {
        Some(ImageProtocol::Kitty) => json!("kitty"),
        Some(ImageProtocol::Iterm2) => json!("iterm2"),
        None => Value::Null,
    }
}

fn protocol_from(value: &Value) -> Option<ImageProtocol> {
    match value.as_str() {
        Some("kitty") => Some(ImageProtocol::Kitty),
        Some("iterm2") => Some(ImageProtocol::Iterm2),
        _ => None,
    }
}

fn caps_json(caps: TerminalCapabilities) -> Value {
    json!({ "images": protocol_json(caps.images), "trueColor": caps.true_color, "hyperlinks": caps.hyperlinks })
}

fn caps_from(value: &Value) -> TerminalCapabilities {
    TerminalCapabilities {
        images: protocol_from(&value["images"]),
        true_color: value["trueColor"].as_bool().unwrap_or(false),
        hyperlinks: value["hyperlinks"].as_bool().unwrap_or(false),
    }
}

fn dims_json(width: u32, height: u32) -> Value {
    json!({ "widthPx": width, "heightPx": height })
}

fn dims_from(value: &Value) -> ImageDimensions {
    ImageDimensions {
        width_px: value["widthPx"].as_f64().unwrap_or(0.0) as u32,
        height_px: value["heightPx"].as_f64().unwrap_or(0.0) as u32,
    }
}

fn opt_usize(value: &Value) -> Option<usize> {
    value.as_f64().map(|v| v.max(0.0) as usize)
}

fn opt_u32(value: &Value) -> Option<u32> {
    value.as_f64().map(|v| v.max(0.0) as u32)
}

/// `width` / `height` of `encodeITerm2()`: numbers are cell counts, strings pass through.
fn size_arg(value: &Value) -> Option<String> {
    match value {
        Value::Number(n) => Some(n.to_string()),
        Value::String(s) => Some(s.clone()),
        _ => None,
    }
}

fn err(message: String) -> anyhow::Error {
    anyhow::anyhow!(message)
}

/// Dispatch one helper by its TS name; `None` when `f` is not one of them.
pub(crate) fn dispatch(f: &str, args: &[Value]) -> Option<anyhow::Result<Value>> {
    let a = |i: usize| args.get(i).cloned().unwrap_or(Value::Null);
    let n = |i: usize| a(i).as_f64().unwrap_or(f64::NAN);
    let s = |i: usize| a(i).as_str().unwrap_or_default().to_string();
    let run = || -> anyhow::Result<Value> {
        Ok(match f {
            "indexedColor" => color_json(&colors::indexed_color(a(0).as_f64().map(|v| v as i64).unwrap_or(-1)).map_err(err)?),
            "rgbColor" => color_json(&colors::rgb_color(n(0), n(1), n(2)).map_err(err)?),
            "oklchColor" => color_json(&colors::oklch_color(n(0), n(1), n(2)).map_err(err)?),
            "okhslColor" => color_json(&colors::okhsl_color(n(0), n(1), n(2)).map_err(err)?),
            "parseColor" => {
                let value = match a(0) {
                    Value::Number(v) => colors::ColorValue::Index(v.as_f64().unwrap_or(-1.0) as i64),
                    other => colors::ColorValue::Str(other.as_str().unwrap_or_default().to_string()),
                };
                color_json(&colors::parse_color(value).map_err(err)?)
            }
            "colorToRgb" => {
                let rgb = colors::color_to_rgb(&color_from(&a(0))?);
                json!({ "r": rgb.r, "g": rgb.g, "b": rgb.b })
            }
            "colorToOklch" => {
                let c = colors::color_to_oklch(&color_from(&a(0))?);
                json!({ "l": c.l, "c": c.c, "h": c.h })
            }
            "colorToOkhsl" => {
                let c = colors::color_to_okhsl(&color_from(&a(0))?);
                json!({ "h": c.h, "s": c.s, "l": c.l })
            }
            "colorToHex" => json!(colors::color_to_hex(&color_from(&a(0))?)),
            "mixColors" => {
                let space = match a(3).as_str() {
                    Some("srgb") => ColorMixSpace::Srgb,
                    _ => ColorMixSpace::Oklch,
                };
                color_json(&colors::mix_colors(&color_from(&a(0))?, &color_from(&a(1))?, n(2), space).map_err(err)?)
            }
            "foregroundAnsi" => json!(colors::foreground_ansi(&color_from(&a(0))?, mode_from(&a(1)))),
            "backgroundAnsi" => json!(colors::background_ansi(&color_from(&a(0))?, mode_from(&a(1)))),
            "oklabToOkhslLightness" => json!(pi_tui::oklab::oklab_to_okhsl_lightness(n(0))),
            "parseTerminalColorSchemeReport" => match pi_tui::terminal_colors::parse_terminal_color_scheme_report(&s(0)) {
                Some(pi_tui::terminal_colors::TerminalColorScheme::Dark) => json!("dark"),
                Some(pi_tui::terminal_colors::TerminalColorScheme::Light) => json!("light"),
                None => Value::Null,
            },
            "getOsc8LinkAtColumn" => json!(pi_tui::utils::get_osc8_link_at_column(&s(0), a(1).as_f64().unwrap_or(0.0).max(0.0) as usize)),
            "compositeTuiLine" => json!(pi_tui::tui::composite_tui_line(
                &s(0),
                &s(1),
                opt_usize(&a(2)).unwrap_or(0),
                opt_usize(&a(3)).unwrap_or(0),
                opt_usize(&a(4)).unwrap_or(0)
            )),
            "getCellDimensions" => {
                let d = ti::get_cell_dimensions();
                dims_json(d.width_px, d.height_px)
            }
            "setCellDimensions" => {
                let d = dims_from(&a(0));
                ti::set_cell_dimensions(CellDimensions { width_px: d.width_px, height_px: d.height_px });
                Value::Null
            }
            "detectCapabilities" => caps_json(ti::detect_capabilities(None)),
            "getCapabilities" => caps_json(ti::get_capabilities()),
            "setCapabilities" => {
                ti::set_capabilities(caps_from(&a(0)));
                Value::Null
            }
            "resetCapabilitiesCache" => {
                ti::reset_capabilities_cache();
                Value::Null
            }
            "setCapabilityOverrides" => {
                let o = a(0);
                let has = |k: &str| o.as_object().is_some_and(|m: &Map<String, Value>| m.contains_key(k));
                ti::set_capability_overrides(CapabilityOverrides {
                    images: has("images").then(|| protocol_from(&o["images"])),
                    true_color: o["trueColor"].as_bool(),
                    hyperlinks: o["hyperlinks"].as_bool(),
                });
                Value::Null
            }
            "getTerminalColorMode" => {
                let caps = a(0);
                mode_json(ti::get_terminal_color_mode((!caps.is_null()).then(|| caps_from(&caps))))
            }
            "allocateImageId" => json!(ti::allocate_image_id()),
            "encodeKitty" => {
                let o = a(1);
                json!(ti::encode_kitty(
                    &s(0),
                    KittyEncodeOptions {
                        columns: opt_usize(&o["columns"]),
                        rows: opt_usize(&o["rows"]),
                        image_id: opt_u32(&o["imageId"]),
                        move_cursor: o["moveCursor"].as_bool(),
                    }
                ))
            }
            "encodeITerm2" => {
                let o = a(1);
                json!(ti::encode_iterm2(
                    &s(0),
                    &Iterm2EncodeOptions {
                        width: size_arg(&o["width"]),
                        height: size_arg(&o["height"]),
                        name: o["name"].as_str().map(str::to_string),
                        preserve_aspect_ratio: o["preserveAspectRatio"].as_bool(),
                        inline: o["inline"].as_bool(),
                    }
                ))
            }
            "deleteKittyImage" => json!(ti::delete_kitty_image(opt_u32(&a(0)).unwrap_or(0))),
            "deleteAllKittyImages" => json!(ti::delete_all_kitty_images()),
            "calculateImageRows" => {
                let cell = a(2);
                let cell = (!cell.is_null()).then(|| {
                    let d = dims_from(&cell);
                    CellDimensions { width_px: d.width_px, height_px: d.height_px }
                });
                json!(ti::calculate_image_rows(dims_from(&a(0)), opt_usize(&a(1)).unwrap_or(0), cell))
            }
            "getPngDimensions" | "getJpegDimensions" | "getGifDimensions" | "getWebpDimensions" | "getImageDimensions" => {
                let dims = match f {
                    "getPngDimensions" => ti::get_png_dimensions(&s(0)),
                    "getJpegDimensions" => ti::get_jpeg_dimensions(&s(0)),
                    "getGifDimensions" => ti::get_gif_dimensions(&s(0)),
                    "getWebpDimensions" => ti::get_webp_dimensions(&s(0)),
                    _ => ti::get_image_dimensions(&s(0), &s(1)),
                };
                dims.map(|d| dims_json(d.width_px, d.height_px)).unwrap_or(Value::Null)
            }
            "renderImage" => {
                let o = a(2);
                let options = ImageRenderOptions {
                    max_width_cells: opt_usize(&o["maxWidthCells"]),
                    max_height_cells: opt_usize(&o["maxHeightCells"]),
                    preserve_aspect_ratio: o["preserveAspectRatio"].as_bool(),
                    image_id: opt_u32(&o["imageId"]),
                    move_cursor: o["moveCursor"].as_bool(),
                };
                match ti::render_image(&s(0), dims_from(&a(1)), &options) {
                    Some(r) => {
                        let mut out = json!({ "sequence": r.sequence, "columns": r.columns, "rows": r.rows });
                        if let Some(id) = r.image_id {
                            out["imageId"] = json!(id);
                        }
                        out
                    }
                    None => Value::Null,
                }
            }
            // `false` when the host registered no transcoder, `null` when conversion failed.
            "hostTranscodeToPng" => match pi_tui::components::image::image_transcoder() {
                Some(transcode) => json!(transcode(&s(0), &s(1))),
                None => json!(false),
            },
            "tuiKeybindings" => {
                let mut out = Map::new();
                for (id, definition) in pi_tui::keybindings::TUI_KEYBINDINGS.iter() {
                    let keys = match &definition.default_keys {
                        pi_tui::keybindings::KeySpec::One(k) => json!(k),
                        pi_tui::keybindings::KeySpec::Many(k) => json!(k),
                    };
                    let mut entry = json!({ "defaultKeys": keys });
                    if let Some(description) = &definition.description {
                        entry["description"] = json!(description);
                    }
                    out.insert(id.clone(), entry);
                }
                Value::Object(out)
            }
            "renderLatex" => json!(pi_tui::latex::render_latex(
                &s(0),
                pi_tui::latex::RenderLatexOptions { display: a(1)["display"].as_bool() == Some(true) }
            )),
            "formatProgramStatus" => {
                use pi_tui::program_status::{BlockedKind, ProgramState, ProgramStatus};
                let o = a(0);
                let state = match o["state"].as_str() {
                    Some("working") => ProgramState::Working,
                    Some("blocked") => ProgramState::Blocked,
                    Some("done") => ProgramState::Done,
                    Some("error") => ProgramState::Error,
                    Some("clear") => ProgramState::Clear,
                    _ => ProgramState::Idle,
                };
                let kind = match o["kind"].as_str() {
                    Some("permission") => Some(BlockedKind::Permission),
                    Some("question") => Some(BlockedKind::Question),
                    Some("auth") => Some(BlockedKind::Auth),
                    _ => None,
                };
                json!(pi_tui::program_status::format_program_status(&ProgramStatus {
                    state,
                    app: o["app"].as_str().map(str::to_string),
                    kind,
                    message: o["message"].as_str().map(str::to_string),
                }))
            }
            "styleText" | "styleTextWithAnsi" => {
                let o = if f == "styleText" { a(1) } else { a(3) };
                let attrs = colors::TextAttributes {
                    bold: o["bold"].as_bool().unwrap_or(false),
                    dim: o["dim"].as_bool().unwrap_or(false),
                    italic: o["italic"].as_bool().unwrap_or(false),
                    underline: o["underline"].as_bool().unwrap_or(false),
                    inverse: o["inverse"].as_bool().unwrap_or(false),
                    strikethrough: o["strikethrough"].as_bool().unwrap_or(false),
                };
                if f == "styleText" {
                    let color = |v: &Value| if v.is_object() { color_from(v).map(Some) } else { Ok(None) };
                    let style = colors::TextStyle { attrs, fg: color(&o["fg"])?, bg: color(&o["bg"])? };
                    json!(colors::style_text(&s(0), &style, mode_from(&a(2))))
                } else {
                    json!(colors::style_text_with_ansi(&s(0), a(1).as_str(), a(2).as_str(), &attrs))
                }
            }
            "isAppleTerminalSession" => json!(pi_tui::terminal::is_apple_terminal_session()),
            "hyperlink" => json!(ti::hyperlink(&s(0), &s(1))),
            "imageFallback" => {
                let dims = a(1);
                let dims = (!dims.is_null()).then(|| dims_from(&dims));
                json!(ti::image_fallback(&s(0), dims, a(2).as_str()))
            }
            _ => anyhow::bail!("not a tui helper"),
        })
    };
    const NAMES: &[&str] = &[
        "indexedColor", "rgbColor", "oklchColor", "okhslColor", "parseColor", "colorToRgb", "colorToOklch",
        "colorToOkhsl", "colorToHex", "mixColors", "foregroundAnsi", "backgroundAnsi", "oklabToOkhslLightness",
        "parseTerminalColorSchemeReport", "getOsc8LinkAtColumn", "compositeTuiLine", "getCellDimensions",
        "setCellDimensions", "detectCapabilities", "getCapabilities", "setCapabilities", "resetCapabilitiesCache",
        "setCapabilityOverrides", "getTerminalColorMode", "allocateImageId", "encodeKitty", "encodeITerm2",
        "deleteKittyImage", "deleteAllKittyImages", "calculateImageRows", "getPngDimensions", "getJpegDimensions",
        "getGifDimensions", "getWebpDimensions", "getImageDimensions", "renderImage", "hyperlink", "imageFallback",
        "hostTranscodeToPng", "tuiKeybindings", "renderLatex", "formatProgramStatus", "styleText", "styleTextWithAnsi",
        "isAppleTerminalSession",
    ];
    NAMES.contains(&f).then(run)
}

