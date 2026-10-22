//! Port of `packages/coding-agent/src/utils/clipboard-image.ts`.

use crate::utils::clipboard_command::{RunClipboardCommandOptions, run_clipboard_command};
use crate::utils::wsl::EnvMap;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ClipboardImage {
    pub bytes: Vec<u8>,
    pub mime_type: String,
}

const SUPPORTED_IMAGE_MIME_TYPES: [&str; 4] = ["image/png", "image/jpeg", "image/webp", "image/gif"];

const DEFAULT_LIST_TIMEOUT_MS: i64 = 1000;
const DEFAULT_POWERSHELL_TIMEOUT_MS: i64 = 5000;

fn env_get(env: Option<&EnvMap>, key: &str) -> Option<String> {
    match env {
        Some(env) => env.get(key).cloned(),
        None => std::env::var(key).ok(),
    }
}

/// `env` defaults to the process environment.
pub fn is_wayland_session(env: Option<&EnvMap>) -> bool {
    env_get(env, "WAYLAND_DISPLAY").is_some_and(|v| !v.is_empty())
        || env_get(env, "XDG_SESSION_TYPE").as_deref() == Some("wayland")
}

fn base_mime_type(mime_type: &str) -> String {
    mime_type.split(';').next().unwrap_or(mime_type).trim().to_lowercase()
}

pub fn extension_for_image_mime_type(mime_type: &str) -> Option<String> {
    match base_mime_type(mime_type).as_str() {
        "image/png" => Some("png".to_string()),
        "image/jpeg" => Some("jpg".to_string()),
        "image/webp" => Some("webp".to_string()),
        "image/gif" => Some("gif".to_string()),
        _ => None,
    }
}

fn select_preferred_image_mime_type(mime_types: &[String]) -> Option<String> {
    let normalized: Vec<(String, String)> = mime_types
        .iter()
        .map(|t| t.trim())
        .filter(|t| !t.is_empty())
        .map(|t| (t.to_string(), base_mime_type(t)))
        .collect();
    for preferred in SUPPORTED_IMAGE_MIME_TYPES {
        if let Some((raw, _)) = normalized.iter().find(|(_, base)| base == preferred) {
            return Some(raw.clone());
        }
    }
    normalized
        .into_iter()
        .find(|(_, base)| base.starts_with("image/"))
        .map(|(raw, _)| raw)
}

fn is_supported_image_mime_type(mime_type: &str) -> bool {
    let base = base_mime_type(mime_type);
    SUPPORTED_IMAGE_MIME_TYPES.contains(&base.as_str())
}

/// Convert unsupported image formats to PNG.
/// Returns `None` if conversion is unavailable or fails.
async fn convert_to_png(bytes: &[u8]) -> Option<Vec<u8>> {
    let photon = crate::utils::photon::load_photon().await.ok().flatten()?;
    let _ = photon;
    let bytes = bytes.to_vec();
    tokio::task::spawn_blocking(move || {
        crate::utils::photon::PhotonImage::new_from_byteslice(&bytes)
            .ok()?
            .get_bytes()
            .ok()
    })
    .await
    .ok()
    .flatten()
}

fn lines_of(bytes: &[u8]) -> Vec<String> {
    String::from_utf8_lossy(bytes)
        .split('\n')
        .map(|t| t.trim_end_matches('\r').trim().to_string())
        .filter(|t| !t.is_empty())
        .collect()
}

fn list_options() -> RunClipboardCommandOptions {
    RunClipboardCommandOptions {
        timeout_ms: Some(DEFAULT_LIST_TIMEOUT_MS),
        ..Default::default()
    }
}

/// Image read result: `None` means the backend failed; `Some(None)` means it has no image. An empty
/// Wayland clipboard must not fall through to stale X11 clipboard contents.
type BackendRead = Option<Option<ClipboardImage>>;

async fn read_clipboard_image_via_wl_paste() -> BackendRead {
    let list = run_clipboard_command("wl-paste", &["--list-types".to_string()], Some(&list_options()))
        .await
        .ok()
        .flatten()?;
    let types = lines_of(&list);
    let Some(selected_type) = select_preferred_image_mime_type(&types) else {
        return Some(None);
    };
    let data = run_clipboard_command(
        "wl-paste",
        &["--type".to_string(), selected_type.clone(), "--no-newline".to_string()],
        None,
    )
    .await
    .ok()
    .flatten()?;
    if data.is_empty() {
        return Some(None);
    }
    Some(Some(ClipboardImage {
        bytes: data,
        mime_type: base_mime_type(&selected_type),
    }))
}

/// On WSL, the Linux clipboard (Wayland/X11) does not receive image data from
/// Windows screenshots (Win+Shift+S). PowerShell can access the Windows clipboard
/// directly, so we use it as a fallback.
async fn read_clipboard_image_via_power_shell() -> Option<ClipboardImage> {
    let tmp_file = std::env::temp_dir().join(format!("pi-wsl-clip-{}.png", uuid::Uuid::new_v4()));
    let tmp_path = tmp_file.to_string_lossy().into_owned();
    let result = async {
        let win_path_result = run_clipboard_command(
            "wslpath",
            &["-w".to_string(), tmp_path.clone()],
            Some(&list_options()),
        )
        .await
        .ok()
        .flatten()?;
        let win_path = String::from_utf8_lossy(&win_path_result).trim().to_string();
        if win_path.is_empty() {
            return None;
        }
        let ps_quoted_win_path = win_path.replace('\'', "''");
        let ps_script = [
            "Add-Type -AssemblyName System.Windows.Forms".to_string(),
            "Add-Type -AssemblyName System.Drawing".to_string(),
            format!("$path = '{ps_quoted_win_path}'"),
            "$img = [System.Windows.Forms.Clipboard]::GetImage()".to_string(),
            "if ($img) { $img.Save($path, [System.Drawing.Imaging.ImageFormat]::Png); Write-Output 'ok' } else { Write-Output 'empty' }".to_string(),
        ]
        .join("; ");
        let result = run_clipboard_command(
            "powershell.exe",
            &["-NoProfile".to_string(), "-Command".to_string(), ps_script],
            Some(&RunClipboardCommandOptions {
                timeout_ms: Some(DEFAULT_POWERSHELL_TIMEOUT_MS),
                ..Default::default()
            }),
        )
        .await
        .ok()
        .flatten()?;
        if String::from_utf8_lossy(&result).trim() != "ok" {
            return None;
        }
        let bytes = std::fs::read(&tmp_file).ok()?;
        if bytes.is_empty() {
            return None;
        }
        Some(ClipboardImage {
            bytes,
            mime_type: "image/png".to_string(),
        })
    }
    .await;
    // Ignore cleanup errors.
    let _ = std::fs::remove_file(&tmp_file);
    result
}

async fn read_clipboard_image_via_xclip() -> BackendRead {
    let targets = run_clipboard_command(
        "xclip",
        &["-selection", "clipboard", "-t", "TARGETS", "-o"].map(str::to_string),
        Some(&list_options()),
    )
    .await
    .ok()
    .flatten()?;
    let candidate_types = lines_of(&targets);
    let Some(preferred) = select_preferred_image_mime_type(&candidate_types) else {
        return Some(None);
    };
    let data = run_clipboard_command(
        "xclip",
        &[
            "-selection".to_string(),
            "clipboard".to_string(),
            "-t".to_string(),
            preferred.clone(),
            "-o".to_string(),
        ],
        None,
    )
    .await
    .ok()
    .flatten()?;
    if data.is_empty() {
        return Some(None);
    }
    Some(Some(ClipboardImage {
        bytes: data,
        mime_type: base_mime_type(&preferred),
    }))
}

async fn read_clipboard_image_via_native_clipboard() -> BackendRead {
    use pi_tui::native_platform::{ClipboardRead, get_native_clipboard};
    let clipboard = get_native_clipboard()?;
    match clipboard.get_image().await {
        Ok(ClipboardRead::Data(bytes)) if !bytes.is_empty() => {
            let mime_type = crate::utils::mime::detect_supported_image_mime_type(&bytes)
                .unwrap_or_else(|| "application/octet-stream".to_string());
            Some(Some(ClipboardImage { bytes, mime_type }))
        }
        Ok(ClipboardRead::Data(_)) | Ok(ClipboardRead::Empty) => Some(None),
        Ok(ClipboardRead::Unavailable) | Err(_) => None,
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReadClipboardImageOptions {
    /// Defaults to the process environment.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub env: Option<EnvMap>,
    /// Node platform name (`linux`, `darwin`, `win32`); defaults to the current platform.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub platform: Option<String>,
}

fn current_platform() -> &'static str {
    match std::env::consts::OS {
        "macos" => "darwin",
        "windows" => "win32",
        other => other,
    }
}

pub async fn read_clipboard_image(
    options: Option<&ReadClipboardImageOptions>,
) -> anyhow::Result<Option<ClipboardImage>> {
    let env = options.and_then(|o| o.env.as_ref());
    let platform = options
        .and_then(|o| o.platform.clone())
        .unwrap_or_else(|| current_platform().to_string());

    if env_get(env, "TERMUX_VERSION").is_some_and(|v| !v.is_empty()) {
        return Ok(None);
    }

    let mut image: BackendRead = None;
    if platform == "linux" {
        let wsl = crate::utils::wsl::is_wsl(env);
        if is_wayland_session(env) || wsl {
            image = read_clipboard_image_via_wl_paste().await;
        }
        if image.is_none() {
            image = read_clipboard_image_via_xclip().await;
        }
        // Preserve Linux's empty/unavailable distinction if Windows has no image.
        if !matches!(image, Some(Some(_))) && wsl
            && let Some(windows_image) = read_clipboard_image_via_power_shell().await
        {
            image = Some(Some(windows_image));
        }
        if image.is_none() {
            image = read_clipboard_image_via_native_clipboard().await;
        }
    } else {
        image = read_clipboard_image_via_native_clipboard().await;
    }

    let Some(Some(image)) = image else {
        return Ok(None);
    };

    // Convert unsupported formats (e.g., Windows DIB data wrapped as BMP) to PNG.
    if !is_supported_image_mime_type(&image.mime_type) {
        let Some(png_bytes) = convert_to_png(&image.bytes).await else {
            return Ok(None);
        };
        return Ok(Some(ClipboardImage {
            bytes: png_bytes,
            mime_type: "image/png".to_string(),
        }));
    }
    Ok(Some(image))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extension_for_mime_types() {
        assert_eq!(extension_for_image_mime_type("image/png").as_deref(), Some("png"));
        assert_eq!(extension_for_image_mime_type("IMAGE/JPEG; charset=x").as_deref(), Some("jpg"));
        assert_eq!(extension_for_image_mime_type("image/bmp"), None);
    }

    #[test]
    fn wayland_detection() {
        let mut env = EnvMap::new();
        assert!(!is_wayland_session(Some(&env)));
        env.insert("XDG_SESSION_TYPE".into(), "wayland".into());
        assert!(is_wayland_session(Some(&env)));
        let mut env = EnvMap::new();
        env.insert("WAYLAND_DISPLAY".into(), "wayland-0".into());
        assert!(is_wayland_session(Some(&env)));
    }

    #[test]
    fn prefers_supported_types() {
        let types = vec!["text/plain".to_string(), "image/bmp".to_string(), "image/png".to_string()];
        assert_eq!(select_preferred_image_mime_type(&types).as_deref(), Some("image/png"));
        let types = vec!["image/bmp".to_string()];
        assert_eq!(select_preferred_image_mime_type(&types).as_deref(), Some("image/bmp"));
        assert_eq!(select_preferred_image_mime_type(&["text/plain".to_string()]), None);
    }

    #[tokio::test]
    async fn termux_has_no_clipboard_image() {
        let mut env = EnvMap::new();
        env.insert("TERMUX_VERSION".into(), "1".into());
        let result = read_clipboard_image(Some(&ReadClipboardImageOptions {
            env: Some(env),
            platform: Some("linux".into()),
        }))
        .await
        .unwrap();
        assert!(result.is_none());
    }
}
