//! Terminal image protocols (Kitty, iTerm2), capability detection and image helpers.
//! Port of `packages/tui/src/terminal-image.ts`.

use std::collections::HashMap;
use std::sync::{LazyLock, Mutex};

use base64::Engine;

use crate::colors::TerminalColorMode;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImageProtocol {
    Kitty,
    Iterm2,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TerminalCapabilities {
    pub images: Option<ImageProtocol>,
    pub true_color: bool,
    pub hyperlinks: bool,
}

/// Partial capability overrides.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct CapabilityOverrides {
    /// `Some(None)` forces "no images".
    pub images: Option<Option<ImageProtocol>>,
    pub true_color: Option<bool>,
    pub hyperlinks: Option<bool>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CellDimensions {
    pub width_px: u32,
    pub height_px: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ImageDimensions {
    pub width_px: u32,
    pub height_px: u32,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ImageRenderOptions {
    pub max_width_cells: Option<usize>,
    pub max_height_cells: Option<usize>,
    pub preserve_aspect_ratio: Option<bool>,
    /// Kitty image ID. If provided, reuses/replaces the existing image with this ID.
    pub image_id: Option<u32>,
    /// Whether Kitty should apply its default cursor movement after placement.
    pub move_cursor: Option<bool>,
}

struct CapState {
    cached: Option<TerminalCapabilities>,
    overrides: CapabilityOverrides,
    cell: CellDimensions,
}

static CAPS: LazyLock<Mutex<CapState>> = LazyLock::new(|| {
    Mutex::new(CapState {
        cached: None,
        overrides: CapabilityOverrides::default(),
        cell: CellDimensions { width_px: 9, height_px: 18 },
    })
});

fn caps() -> std::sync::MutexGuard<'static, CapState> {
    CAPS.lock().unwrap_or_else(|e| e.into_inner())
}

pub fn get_cell_dimensions() -> CellDimensions {
    caps().cell
}

pub fn set_cell_dimensions(dims: CellDimensions) {
    caps().cell = dims;
}

fn env_lower(name: &str) -> String {
    std::env::var(name).unwrap_or_default().to_lowercase()
}

fn env_set(name: &str) -> bool {
    std::env::var_os(name).is_some_and(|v| !v.is_empty())
}

/// Checks whether the attached tmux client forwards OSC 8 hyperlinks.
fn probe_tmux_hyperlinks() -> bool {
    let output = std::process::Command::new("tmux")
        .args(["display-message", "-p", "#{client_termfeatures}"])
        .stdin(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .output();
    match output {
        Ok(out) if out.status.success() => {
            String::from_utf8_lossy(&out.stdout).split(',').any(|f| f.trim() == "hyperlinks")
        }
        _ => false,
    }
}

fn detect_from_environment(tmux_forwards_hyperlink: &dyn Fn() -> bool) -> TerminalCapabilities {
    let term_program = env_lower("TERM_PROGRAM");
    let terminal_emulator = env_lower("TERMINAL_EMULATOR");
    let term = env_lower("TERM");
    let color_term = env_lower("COLORTERM");
    let has_true_color_hint = color_term == "truecolor" || color_term == "24bit" || term.ends_with("-direct");
    let is_windows_console = cfg!(windows);
    let caps = |images, true_color, hyperlinks| TerminalCapabilities { images, true_color, hyperlinks };

    if env_set("TMUX") || term.starts_with("tmux") {
        return caps(None, has_true_color_hint, tmux_forwards_hyperlink());
    }
    if term.starts_with("screen") {
        return caps(None, has_true_color_hint, false);
    }
    if term_program == "herdr" {
        return caps(Some(ImageProtocol::Kitty), has_true_color_hint, true);
    }
    if term_program == "rex" {
        return caps(Some(ImageProtocol::Kitty), true, true);
    }
    if env_set("KITTY_WINDOW_ID") || term_program == "kitty" {
        return caps(Some(ImageProtocol::Kitty), true, true);
    }
    if term_program == "ghostty" || term.contains("ghostty") || env_set("GHOSTTY_RESOURCES_DIR") {
        return caps(Some(ImageProtocol::Kitty), true, true);
    }
    if env_set("WEZTERM_PANE") || term_program == "wezterm" {
        return caps(Some(ImageProtocol::Kitty), true, true);
    }
    if term_program == "warpterminal" || env_set("WARP_SESSION_ID") || env_set("WARP_TERMINAL_SESSION_UUID") {
        return caps(Some(ImageProtocol::Kitty), true, true);
    }
    if env_set("ITERM_SESSION_ID") || term_program == "iterm.app" {
        return caps(Some(ImageProtocol::Iterm2), true, true);
    }
    if env_set("WT_SESSION") {
        return caps(None, true, true);
    }
    if term_program == "alacritty" || term_program == "vscode" || term_program == "zed" {
        return caps(None, true, true);
    }
    if terminal_emulator == "jetbrains-jediterm" {
        return caps(None, true, false);
    }
    if is_windows_console {
        return caps(None, true, false);
    }
    caps(None, has_true_color_hint, false)
}

fn parse_boolean_override(value: Option<String>) -> Option<bool> {
    match value.as_deref() {
        Some("1") => Some(true),
        Some("0") => Some(false),
        _ => None,
    }
}

/// Detect capabilities from the environment, applying `PI_HYPERLINKS`, `PI_IMAGE_PROTOCOL`
/// and `PI_TRUE_COLOR` overrides.
pub fn detect_capabilities(tmux_forwards_hyperlink: Option<&dyn Fn() -> bool>) -> TerminalCapabilities {
    let hyperlinks = parse_boolean_override(std::env::var("PI_HYPERLINKS").ok());
    let mut detected = match (hyperlinks, tmux_forwards_hyperlink) {
        (Some(h), _) => detect_from_environment(&move || h),
        (None, Some(f)) => detect_from_environment(f),
        (None, None) => detect_from_environment(&probe_tmux_hyperlinks),
    };
    let image_protocol = std::env::var("PI_IMAGE_PROTOCOL").ok().map(|s| s.to_lowercase());
    match image_protocol.as_deref() {
        Some("kitty") => detected.images = Some(ImageProtocol::Kitty),
        Some("iterm2") => detected.images = Some(ImageProtocol::Iterm2),
        Some("none") | Some("0") => detected.images = None,
        _ => {}
    }
    if let Some(tc) = parse_boolean_override(std::env::var("PI_TRUE_COLOR").ok()) {
        detected.true_color = tc;
    }
    if let Some(h) = hyperlinks {
        detected.hyperlinks = h;
    }
    detected
}

/// The cached terminal capabilities (detected on first use).
pub fn get_capabilities() -> TerminalCapabilities {
    let state = caps();
    if let Some(c) = state.cached {
        return c;
    }
    let overrides = state.overrides;
    drop(state);
    let mut detected = match overrides.hyperlinks {
        Some(h) => detect_capabilities(Some(&move || h)),
        None => detect_capabilities(None),
    };
    if let Some(images) = overrides.images {
        detected.images = images;
    }
    if let Some(tc) = overrides.true_color {
        detected.true_color = tc;
    }
    if let Some(h) = overrides.hyperlinks {
        detected.hyperlinks = h;
    }
    let mut state = caps();
    state.cached = Some(detected);
    detected
}

pub fn get_terminal_color_mode(capabilities: Option<TerminalCapabilities>) -> TerminalColorMode {
    let caps = capabilities.unwrap_or_else(get_capabilities);
    if caps.true_color { TerminalColorMode::Truecolor } else { TerminalColorMode::Color256 }
}

pub fn reset_capabilities_cache() {
    caps().cached = None;
}

/// Override selected auto-detected capabilities.
pub fn set_capability_overrides(overrides: CapabilityOverrides) {
    let mut state = caps();
    if state.overrides == overrides {
        return;
    }
    state.overrides = overrides;
    state.cached = None;
}

/// Override the cached capabilities. Useful in tests to exercise both code paths.
pub fn set_capabilities(caps_value: TerminalCapabilities) {
    caps().cached = Some(caps_value);
}

const KITTY_PREFIX: &str = "\x1b_G";
const ITERM2_PREFIX: &str = "\x1b]1337;File=";

/// Whether a rendered line contains an inline image sequence.
pub fn is_image_line(line: &str) -> bool {
    line.starts_with(KITTY_PREFIX)
        || line.starts_with(ITERM2_PREFIX)
        || line.contains(KITTY_PREFIX)
        || line.contains(ITERM2_PREFIX)
}

/// Generate a random image ID in `[1, 0xffffffff]` for the Kitty graphics protocol.
pub fn allocate_image_id() -> u32 {
    use std::collections::hash_map::RandomState;
    use std::hash::{BuildHasher, Hasher};
    let mut h = RandomState::new().build_hasher();
    h.write_u128(std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_nanos()));
    ((h.finish() % 0xfffffffe) + 1) as u32
}

/// Options for [`encode_kitty`].
#[derive(Debug, Clone, Copy, Default)]
pub struct KittyEncodeOptions {
    pub columns: Option<usize>,
    pub rows: Option<usize>,
    pub image_id: Option<u32>,
    /// Default: true.
    pub move_cursor: Option<bool>,
}

pub fn encode_kitty(base64_data: &str, options: KittyEncodeOptions) -> String {
    const CHUNK_SIZE: usize = 4096;
    let mut params: Vec<String> = vec!["a=T".into(), "f=100".into(), "q=2".into()];
    if options.move_cursor == Some(false) {
        params.push("C=1".into());
    }
    if let Some(c) = options.columns.filter(|c| *c != 0) {
        params.push(format!("c={c}"));
    }
    if let Some(r) = options.rows.filter(|r| *r != 0) {
        params.push(format!("r={r}"));
    }
    if let Some(i) = options.image_id.filter(|i| *i != 0) {
        params.push(format!("i={i}"));
    }
    let params = params.join(",");
    if base64_data.len() <= CHUNK_SIZE {
        return format!("\x1b_G{params};{base64_data}\x1b\\");
    }
    let mut out = String::new();
    let mut offset = 0;
    let mut first = true;
    while offset < base64_data.len() {
        let end = (offset + CHUNK_SIZE).min(base64_data.len());
        let chunk = &base64_data[offset..end];
        let is_last = offset + CHUNK_SIZE >= base64_data.len();
        if first {
            out.push_str(&format!("\x1b_G{params},m=1;{chunk}\x1b\\"));
            first = false;
        } else if is_last {
            out.push_str(&format!("\x1b_Gm=0;{chunk}\x1b\\"));
        } else {
            out.push_str(&format!("\x1b_Gm=1;{chunk}\x1b\\"));
        }
        offset += CHUNK_SIZE;
    }
    out
}

/// Delete a Kitty graphics image by ID (also frees the image data).
pub fn delete_kitty_image(image_id: u32) -> String {
    format!("\x1b_Ga=d,d=I,i={image_id},q=2\x1b\\")
}

/// Delete all visible Kitty graphics images (also frees the image data).
pub fn delete_all_kitty_images() -> String {
    "\x1b_Ga=d,d=A,q=2\x1b\\".to_string()
}

/// Delete all visible Kitty placements while retaining their uploaded image data.
pub fn delete_all_kitty_placements() -> String {
    "\x1b_Ga=d,d=a,q=2\x1b\\".to_string()
}

/// Node's `Buffer.byteLength(str, "base64")`.
fn base64_byte_length(s: &str) -> usize {
    let mut len = s.len();
    if s.ends_with('=') {
        len -= 1;
        if len > 0 && s.as_bytes()[len - 1] == b'=' {
            len -= 1;
        }
    }
    (len * 3) >> 2
}

/// Lenient base64 decoding like Node's `Buffer.from(str, "base64")`.
pub fn decode_base64_lenient(s: &str) -> Vec<u8> {
    let cleaned: String = s
        .chars()
        .filter_map(|c| match c {
            'A'..='Z' | 'a'..='z' | '0'..='9' | '+' | '/' => Some(c),
            '-' => Some('+'),
            '_' => Some('/'),
            _ => None,
        })
        .collect();
    let mut cleaned = cleaned;
    if cleaned.len() % 4 == 1 {
        cleaned.pop();
    }
    base64::engine::general_purpose::STANDARD_NO_PAD.decode(cleaned.as_bytes()).unwrap_or_default()
}

/// Options for [`encode_iterm2`].
#[derive(Debug, Clone, Default)]
pub struct Iterm2EncodeOptions {
    pub width: Option<String>,
    pub height: Option<String>,
    pub name: Option<String>,
    pub preserve_aspect_ratio: Option<bool>,
    pub inline: Option<bool>,
}

pub fn encode_iterm2(base64_data: &str, options: &Iterm2EncodeOptions) -> String {
    let mut params = vec![
        format!("inline={}", if options.inline != Some(false) { 1 } else { 0 }),
        format!("size={}", base64_byte_length(base64_data)),
    ];
    if let Some(w) = &options.width {
        params.push(format!("width={w}"));
    }
    if let Some(h) = &options.height {
        params.push(format!("height={h}"));
    }
    if let Some(name) = options.name.as_ref().filter(|n| !n.is_empty()) {
        params.push(format!("name={}", base64::engine::general_purpose::STANDARD.encode(name.as_bytes())));
    }
    if options.preserve_aspect_ratio == Some(false) {
        params.push("preserveAspectRatio=0".into());
    }
    format!("\x1b]1337;File={}:{base64_data}\x07", params.join(";"))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ImageCellSize {
    pub columns: usize,
    pub rows: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct KittyImageMetadata {
    pub image_id: u32,
    pub columns: usize,
    pub rows: usize,
    pub width_px: u32,
    pub height_px: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KittyImagePlacement {
    pub image_id: u32,
    pub transmission_generation: u64,
    pub transmission_bytes: usize,
    pub estimated_decoded_bytes: u64,
    pub rows: usize,
    pub sequence: String,
    pub replacement_line: String,
}

struct MetadataRegistry {
    entries: HashMap<u32, (KittyImageMetadata, u64)>,
    order: Vec<u32>,
    generation: u64,
}

static KITTY_METADATA: LazyLock<Mutex<MetadataRegistry>> =
    LazyLock::new(|| Mutex::new(MetadataRegistry { entries: HashMap::new(), order: Vec::new(), generation: 0 }));

pub fn register_kitty_image_metadata(metadata: KittyImageMetadata) {
    let mut reg = KITTY_METADATA.lock().unwrap_or_else(|e| e.into_inner());
    reg.generation += 1;
    let generation = reg.generation;
    reg.order.retain(|id| *id != metadata.image_id);
    reg.order.push(metadata.image_id);
    reg.entries.insert(metadata.image_id, (metadata, generation));
    if reg.entries.len() > 1000 {
        let oldest = reg.order.remove(0);
        reg.entries.remove(&oldest);
    }
}

/// Returns the controls between `\x1b_G` and the first `;` and the byte offset of the match.
fn kitty_controls(line: &str) -> Option<(usize, &str)> {
    let start = line.find(KITTY_PREFIX)?;
    let rest = &line[start + KITTY_PREFIX.len()..];
    let semi = rest.find(';')?;
    Some((start, &rest[..semi]))
}

fn control_value<'a>(controls: &'a str, key: &str) -> Option<&'a str> {
    controls.split(',').find_map(|c| {
        let (k, v) = c.split_once('=')?;
        if k == key && !v.is_empty() && v.bytes().all(|b| b.is_ascii_digit()) { Some(v) } else { None }
    })
}

fn registered_metadata_from_controls(controls: &str) -> Option<(KittyImageMetadata, u64)> {
    let id: u64 = control_value(controls, "i")?.parse().ok()?;
    let reg = KITTY_METADATA.lock().unwrap_or_else(|e| e.into_inner());
    reg.entries.get(&u32::try_from(id).ok()?).copied()
}

fn explicit_kitty_rows(controls: &str) -> Option<usize> {
    let rows: usize = control_value(controls, "r")?.parse().ok()?;
    if rows > 0 { Some(rows) } else { None }
}

pub fn get_kitty_image_metadata(line: &str) -> Option<KittyImageMetadata> {
    let (_, controls) = kitty_controls(line)?;
    registered_metadata_from_controls(controls).map(|(m, _)| m)
}

/// Number of rows covered by an image placement, without scanning its payload.
pub fn get_kitty_image_placement_rows(line: &str) -> Option<usize> {
    let (_, controls) = kitty_controls(line)?;
    explicit_kitty_rows(controls).or_else(|| registered_metadata_from_controls(controls).map(|(m, _)| m.rows))
}

const KITTY_PLACEMENT_CONTROL_KEYS: [&str; 17] =
    ["i", "p", "x", "y", "w", "h", "X", "Y", "c", "r", "C", "U", "z", "P", "Q", "H", "V"];

/// Build a placement-only command for an image line emitted by [`render_image`].
pub fn get_kitty_image_placement(line: &str) -> Option<KittyImagePlacement> {
    let (match_index, first_controls) = kitty_controls(line)?;
    let (metadata, generation) = registered_metadata_from_controls(first_controls)?;
    let mut command_start = match_index;
    let mut command_controls = first_controls.to_string();
    let transmission_end;
    loop {
        let terminator = line[command_start + KITTY_PREFIX.len()..].find("\x1b\\")? + command_start + KITTY_PREFIX.len();
        let end = terminator + 2;
        let more = command_controls.split(',').any(|c| c == "m=1");
        if !more {
            transmission_end = end;
            break;
        }
        command_start = end;
        if !line[command_start..].starts_with(KITTY_PREFIX) {
            return None;
        }
        let controls_end = line[command_start + KITTY_PREFIX.len()..].find(';')? + command_start + KITTY_PREFIX.len();
        command_controls = line[command_start + KITTY_PREFIX.len()..controls_end].to_string();
    }
    let controls: Vec<&str> = first_controls
        .split(',')
        .filter(|c| KITTY_PLACEMENT_CONTROL_KEYS.contains(&c.split('=').next().unwrap_or("")))
        .collect();
    let sequence = format!("\x1b_Ga=p,q=2,{}\x1b\\", controls.join(","));
    Some(KittyImagePlacement {
        image_id: metadata.image_id,
        transmission_generation: generation,
        transmission_bytes: transmission_end - match_index,
        estimated_decoded_bytes: metadata.width_px as u64 * metadata.height_px as u64 * 4,
        rows: explicit_kitty_rows(first_controls).unwrap_or(metadata.rows),
        replacement_line: format!("{}{}{}", &line[..match_index], sequence, &line[transmission_end..]),
        sequence,
    })
}

pub fn crop_kitty_image_line(line: &str, hidden_rows: usize, visible_rows: usize) -> String {
    let Some(metadata) = get_kitty_image_metadata(line) else {
        return line.to_string();
    };
    let Some((match_index, controls)) = kitty_controls(line) else {
        return line.to_string();
    };
    if hidden_rows >= metadata.rows || visible_rows == 0 {
        return line.to_string();
    }
    let cropped_rows = visible_rows.min(metadata.rows - hidden_rows);
    if hidden_rows == 0 && cropped_rows == metadata.rows {
        return line.to_string();
    }
    let h = metadata.height_px as u64;
    let rows = metadata.rows as u64;
    let source_y = (h * hidden_rows as u64) / rows;
    let source_end = (h * (hidden_rows + cropped_rows) as u64).div_ceil(rows);
    let source_height = (source_end.min(h).saturating_sub(source_y)).max(1);
    let mut new_controls: Vec<String> = controls
        .split(',')
        .filter(|c| !(c.starts_with("y=") || c.starts_with("h=") || c.starts_with("r=")))
        .map(String::from)
        .collect();
    new_controls.push(format!("y={source_y}"));
    new_controls.push(format!("h={source_height}"));
    new_controls.push(format!("r={cropped_rows}"));
    let after = &line[match_index + KITTY_PREFIX.len() + controls.len() + 1..];
    format!("{}\x1b_G{};{}", &line[..match_index], new_controls.join(","), after)
}

fn choose_less_distorted_cell_count(upper: usize, ideal: f64) -> usize {
    if upper <= 1 {
        return upper;
    }
    let lower = upper - 1;
    let upper_d = (upper as f64 / ideal).max(ideal / upper as f64);
    let lower_d = (lower as f64 / ideal).max(ideal / lower as f64);
    if lower_d < upper_d { lower } else { upper }
}

pub fn calculate_image_cell_size(
    image: ImageDimensions,
    max_width_cells: usize,
    max_height_cells: Option<usize>,
    cell: CellDimensions,
    optimize_aspect_ratio: bool,
) -> ImageCellSize {
    let max_width = max_width_cells.max(1);
    let max_height = max_height_cells.map(|h| h.max(1));
    let image_width = image.width_px.max(1) as f64;
    let image_height = image.height_px.max(1) as f64;
    let cw = cell.width_px as f64;
    let ch = cell.height_px as f64;

    let width_scale = (max_width as f64 * cw) / image_width;
    let height_scale = match max_height {
        None => width_scale,
        Some(h) => (h as f64 * ch) / image_height,
    };
    let scale = width_scale.min(height_scale);
    let scaled_w = image_width * scale;
    let scaled_h = image_height * scale;
    let mut columns = ((scaled_w / cw).ceil() as usize).clamp(1, max_width);
    let mut rows = ((scaled_h / ch).ceil() as usize).max(1);
    if let Some(h) = max_height {
        rows = rows.min(h);
    }
    if !optimize_aspect_ratio {
        return ImageCellSize { columns, rows };
    }
    if width_scale <= height_scale {
        let ideal_rows = (columns as f64 * cw * image_height) / (image_width * ch);
        rows = choose_less_distorted_cell_count(rows, ideal_rows);
    } else {
        let ideal_cols = (rows as f64 * ch * image_width) / (image_height * cw);
        columns = choose_less_distorted_cell_count(columns, ideal_cols);
    }
    ImageCellSize { columns, rows }
}

pub fn calculate_image_rows(image: ImageDimensions, target_width_cells: usize, cell: Option<CellDimensions>) -> usize {
    calculate_image_cell_size(
        image,
        target_width_cells,
        None,
        cell.unwrap_or(CellDimensions { width_px: 9, height_px: 18 }),
        false,
    )
    .rows
}

pub fn get_png_dimensions(base64_data: &str) -> Option<ImageDimensions> {
    let b = decode_base64_lenient(base64_data);
    if b.len() < 24 || b[0] != 0x89 || b[1] != 0x50 || b[2] != 0x4e || b[3] != 0x47 {
        return None;
    }
    let w = u32::from_be_bytes([b[16], b[17], b[18], b[19]]);
    let h = u32::from_be_bytes([b[20], b[21], b[22], b[23]]);
    Some(ImageDimensions { width_px: w, height_px: h })
}

pub fn get_jpeg_dimensions(base64_data: &str) -> Option<ImageDimensions> {
    let b = decode_base64_lenient(base64_data);
    if b.len() < 2 || b[0] != 0xff || b[1] != 0xd8 {
        return None;
    }
    let mut offset = 2usize;
    while offset + 9 < b.len() {
        if b[offset] != 0xff {
            offset += 1;
            continue;
        }
        let marker = b[offset + 1];
        if (0xc0..=0xc2).contains(&marker) {
            let h = u16::from_be_bytes([b[offset + 5], b[offset + 6]]) as u32;
            let w = u16::from_be_bytes([b[offset + 7], b[offset + 8]]) as u32;
            return Some(ImageDimensions { width_px: w, height_px: h });
        }
        if offset + 3 >= b.len() {
            return None;
        }
        let length = u16::from_be_bytes([b[offset + 2], b[offset + 3]]) as usize;
        if length < 2 {
            return None;
        }
        offset += 2 + length;
    }
    None
}

pub fn get_gif_dimensions(base64_data: &str) -> Option<ImageDimensions> {
    let b = decode_base64_lenient(base64_data);
    if b.len() < 10 || (&b[..6] != b"GIF87a" && &b[..6] != b"GIF89a") {
        return None;
    }
    let w = u16::from_le_bytes([b[6], b[7]]) as u32;
    let h = u16::from_le_bytes([b[8], b[9]]) as u32;
    Some(ImageDimensions { width_px: w, height_px: h })
}

pub fn get_webp_dimensions(base64_data: &str) -> Option<ImageDimensions> {
    let b = decode_base64_lenient(base64_data);
    if b.len() < 30 || &b[0..4] != b"RIFF" || &b[8..12] != b"WEBP" {
        return None;
    }
    match &b[12..16] {
        b"VP8 " => {
            let w = (u16::from_le_bytes([b[26], b[27]]) & 0x3fff) as u32;
            let h = (u16::from_le_bytes([b[28], b[29]]) & 0x3fff) as u32;
            Some(ImageDimensions { width_px: w, height_px: h })
        }
        b"VP8L" => {
            let bits = u32::from_le_bytes([b[21], b[22], b[23], b[24]]);
            Some(ImageDimensions { width_px: (bits & 0x3fff) + 1, height_px: ((bits >> 14) & 0x3fff) + 1 })
        }
        b"VP8X" => {
            let w = (b[24] as u32 | (b[25] as u32) << 8 | (b[26] as u32) << 16) + 1;
            let h = (b[27] as u32 | (b[28] as u32) << 8 | (b[29] as u32) << 16) + 1;
            Some(ImageDimensions { width_px: w, height_px: h })
        }
        _ => None,
    }
}

pub fn get_image_dimensions(base64_data: &str, mime_type: &str) -> Option<ImageDimensions> {
    match mime_type {
        "image/png" => get_png_dimensions(base64_data),
        "image/jpeg" => get_jpeg_dimensions(base64_data),
        "image/gif" => get_gif_dimensions(base64_data),
        "image/webp" => get_webp_dimensions(base64_data),
        _ => None,
    }
}

/// Result of [`render_image`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RenderedImage {
    pub sequence: String,
    pub columns: usize,
    pub rows: usize,
    pub image_id: Option<u32>,
}

pub fn render_image(base64_data: &str, image: ImageDimensions, options: &ImageRenderOptions) -> Option<RenderedImage> {
    let caps = get_capabilities();
    let protocol = caps.images?;
    let max_width = options.max_width_cells.unwrap_or(80);
    let size = calculate_image_cell_size(
        image,
        max_width,
        options.max_height_cells,
        get_cell_dimensions(),
        protocol == ImageProtocol::Kitty,
    );
    match protocol {
        ImageProtocol::Kitty => {
            if let Some(image_id) = options.image_id {
                register_kitty_image_metadata(KittyImageMetadata {
                    image_id,
                    columns: size.columns,
                    rows: size.rows,
                    width_px: image.width_px,
                    height_px: image.height_px,
                });
            }
            let sequence = encode_kitty(
                base64_data,
                KittyEncodeOptions {
                    columns: Some(size.columns),
                    rows: Some(size.rows),
                    image_id: options.image_id,
                    move_cursor: options.move_cursor,
                },
            );
            Some(RenderedImage { sequence, columns: size.columns, rows: size.rows, image_id: options.image_id })
        }
        ImageProtocol::Iterm2 => {
            let sequence = encode_iterm2(
                base64_data,
                &Iterm2EncodeOptions {
                    width: Some(size.columns.to_string()),
                    height: Some("auto".into()),
                    preserve_aspect_ratio: Some(options.preserve_aspect_ratio.unwrap_or(true)),
                    ..Default::default()
                },
            );
            Some(RenderedImage { sequence, columns: size.columns, rows: size.rows, image_id: None })
        }
    }
}

/// Wrap text in an OSC 8 hyperlink sequence.
pub fn hyperlink(text: &str, url: &str) -> String {
    format!("\x1b]8;;{url}\x1b\\{text}\x1b]8;;\x1b\\")
}

fn home_dir() -> Option<String> {
    std::env::var("HOME").ok().filter(|h| !h.is_empty())
}

fn shorten_image_path(filename: &str) -> String {
    if let Some(home) = home_dir()
        && (filename == home || filename.starts_with(&format!("{home}/")) || filename.starts_with(&format!("{home}\\")))
    {
        return format!("~{}", &filename[home.len()..]);
    }
    filename.to_string()
}

/// Text fallback when the terminal cannot render inline images.
pub fn image_fallback(mime_type: &str, dimensions: Option<ImageDimensions>, filename: Option<&str>) -> String {
    let mut parts: Vec<String> = Vec::new();
    if let Some(filename) = filename.filter(|f| !f.is_empty()) {
        let display = shorten_image_path(filename);
        let path = std::path::Path::new(filename);
        if get_capabilities().hyperlinks && path.is_absolute() {
            match url::Url::from_file_path(path) {
                Ok(u) => parts.push(hyperlink(&display, u.as_str())),
                Err(_) => parts.push(display),
            }
        } else {
            parts.push(display);
        }
    }
    parts.push(format!("[{mime_type}]"));
    if let Some(d) = dimensions {
        parts.push(format!("{}x{}", d.width_px, d.height_px));
    }
    format!("[Image: {}]", parts.join(" "))
}
