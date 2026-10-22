//! Port of `packages/coding-agent/src/utils/tools-manager.ts`.

use serde::{Deserialize, Serialize};
use std::sync::Arc;

#[derive(Clone)]
pub struct ToolConfig {
    pub name: String,
    pub repo: String,
    pub binary_name: String,
    pub system_binary_names: Option<Vec<String>>,
    pub tag_prefix: String,
    pub get_asset_name: Arc<dyn Fn(String, String, String) -> Option<String> + Send + Sync>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum GetToolPathTool {
    #[serde(rename = "fd")]
    Fd,
    #[serde(rename = "rg")]
    Rg,
}

impl GetToolPathTool {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Fd => "fd",
            Self::Rg => "rg",
        }
    }
}

const NETWORK_TIMEOUT_MS: u64 = 10_000;
const DOWNLOAD_TIMEOUT_MS: u64 = 120_000;
const RETRYABLE_STATUS_CODES: &[u16] = &[408, 425, 429, 500, 502, 503, 504];

fn tools_dir() -> String {
    crate::config::get_bin_dir()
}

fn is_offline_mode_enabled() -> bool {
    match std::env::var("PI_OFFLINE") {
        Ok(value) if !value.is_empty() => {
            let lower = value.to_lowercase();
            value == "1" || lower == "true" || lower == "yes"
        }
        _ => false,
    }
}

/// Node's `os.platform()`.
fn node_platform() -> &'static str {
    match std::env::consts::OS {
        "macos" => "darwin",
        "windows" => "win32",
        other => other,
    }
}

/// Node's `os.arch()`.
fn node_arch() -> &'static str {
    match std::env::consts::ARCH {
        "x86_64" => "x64",
        "aarch64" => "arm64",
        "x86" => "ia32",
        other => other,
    }
}

fn asset_arch(architecture: &str) -> &'static str {
    if architecture == "arm64" { "aarch64" } else { "x86_64" }
}

fn tool_config(tool: GetToolPathTool) -> ToolConfig {
    match tool {
        GetToolPathTool::Fd => ToolConfig {
            name: "fd".to_string(),
            repo: "sharkdp/fd".to_string(),
            binary_name: "fd".to_string(),
            system_binary_names: Some(vec!["fd".to_string(), "fdfind".to_string()]),
            tag_prefix: "v".to_string(),
            get_asset_name: Arc::new(|version, plat, architecture| {
                let arch = asset_arch(&architecture);
                match plat.as_str() {
                    "darwin" => Some(format!("fd-v{version}-{arch}-apple-darwin.tar.gz")),
                    "linux" => Some(format!("fd-v{version}-{arch}-unknown-linux-musl.tar.gz")),
                    "win32" => Some(format!("fd-v{version}-{arch}-pc-windows-msvc.zip")),
                    _ => None,
                }
            }),
        },
        GetToolPathTool::Rg => ToolConfig {
            name: "ripgrep".to_string(),
            repo: "BurntSushi/ripgrep".to_string(),
            binary_name: "rg".to_string(),
            system_binary_names: None,
            tag_prefix: String::new(),
            get_asset_name: Arc::new(|version, plat, architecture| {
                let arch = asset_arch(&architecture);
                match plat.as_str() {
                    "darwin" => Some(format!("ripgrep-{version}-{arch}-apple-darwin.tar.gz")),
                    "linux" => Some(format!("ripgrep-{version}-{arch}-unknown-linux-musl.tar.gz")),
                    "win32" => Some(format!("ripgrep-{version}-{arch}-pc-windows-msvc.zip")),
                    _ => None,
                }
            }),
        },
    }
}

/// Check if a command exists in PATH by trying to run it.
fn command_exists(cmd: &str) -> bool {
    std::process::Command::new(cmd)
        .arg("--version")
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .output()
        .is_ok()
}

fn binary_ext() -> &'static str {
    if node_platform() == "win32" { ".exe" } else { "" }
}

/// Get the path to a tool (system-wide or in our tools dir).
pub fn get_tool_path(tool: GetToolPathTool) -> Option<String> {
    let config = tool_config(tool);

    // Check our tools directory first.
    let local_path = std::path::Path::new(&tools_dir()).join(format!("{}{}", config.binary_name, binary_ext()));
    if local_path.exists() {
        return Some(local_path.to_string_lossy().into_owned());
    }

    // Check system PATH - if found, just return the command name (it's in PATH).
    let system_binary_names = config
        .system_binary_names
        .clone()
        .unwrap_or_else(|| vec![config.binary_name.clone()]);
    system_binary_names.into_iter().find(|name| command_exists(name))
}

fn pi_user_agent() -> String {
    use pi_ai::utils::identity::{ClientIdentity, UserAgentMode, UserAgentNamed, format_user_agent};
    format_user_agent(Some(&ClientIdentity {
        user_agent: Some(UserAgentMode::Named(UserAgentNamed::Detailed)),
        attribution: None,
        version: Some(crate::config::VERSION.to_string()),
    }))
}

/// `fetchWithRetry`: up to two immediate retries on transport errors and transient statuses,
/// within an overall time budget.
async fn fetch_with_retry(
    client: &reqwest::Client,
    url: &str,
    headers: &[(&str, String)],
    timeout_ms: u64,
) -> anyhow::Result<reqwest::Response> {
    const MAX_RETRIES: usize = 2;
    let deadline = tokio::time::Instant::now() + std::time::Duration::from_millis(timeout_ms);
    let mut attempt = 0usize;
    loop {
        let mut request = client.get(url);
        for (name, value) in headers {
            request = request.header(*name, value);
        }
        let result = tokio::time::timeout_at(deadline, request.send()).await;
        match result {
            Err(_) => anyhow::bail!("The operation was aborted due to timeout"),
            Ok(Ok(response)) => {
                let should_retry =
                    RETRYABLE_STATUS_CODES.contains(&response.status().as_u16()) && attempt < MAX_RETRIES;
                if !should_retry {
                    return Ok(response);
                }
            }
            Ok(Err(err)) => {
                if attempt >= MAX_RETRIES {
                    return Err(anyhow::Error::new(err).context("fetch failed"));
                }
            }
        }
        attempt += 1;
    }
}

/// Resolve the latest release version from the release page redirect.
/// The api.github.com releases endpoint counts against the anonymous API
/// quota (60 requests/hour per IP), which is permanently exhausted behind
/// shared egress IPs such as corporate proxies and CI runners. The web
/// endpoint answers with a redirect to the tagged release at no quota cost
/// and lives on the same origin as the binary download itself.
pub async fn get_latest_version(repo: &str) -> anyhow::Result<String> {
    get_latest_version_from("https://github.com", repo).await
}

/// `getLatestVersion` against an arbitrary GitHub origin (tests point it at a local server).
async fn get_latest_version_from(origin: &str, repo: &str) -> anyhow::Result<String> {
    let client = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .build()?;
    let response = fetch_with_retry(
        &client,
        &format!("{origin}/{repo}/releases/latest"),
        &[("User-Agent", pi_user_agent())],
        NETWORK_TIMEOUT_MS,
    )
    .await?;
    let status = response.status().as_u16();
    let location = if (300..400).contains(&status) {
        response
            .headers()
            .get(reqwest::header::LOCATION)
            .and_then(|v| v.to_str().ok())
            .map(str::to_string)
    } else {
        None
    };
    // Only the status and headers matter here. Discard the body.
    drop(response);
    let Some(location) = location.filter(|l| !l.is_empty()) else {
        anyhow::bail!("Failed to resolve latest {repo} release: HTTP {status} without redirect");
    };
    let base = url::Url::parse(origin)?;
    let resolved = base.join(&location)?;
    let tag = resolved.path().rsplit('/').next().unwrap_or_default().to_string();
    if tag.is_empty() || !location.contains("/releases/tag/") {
        anyhow::bail!("Failed to resolve latest {repo} release: unexpected redirect to {location}");
    }
    let decoded = url::form_urlencoded::parse(format!("x={}", tag.replace('+', "%2B")).as_bytes())
        .next()
        .map(|(_, v)| v.into_owned())
        .unwrap_or(tag);
    Ok(decoded.strip_prefix('v').map(str::to_string).unwrap_or(decoded))
}

/// Download a file from URL.
async fn download_file(url: &str, dest: &str) -> anyhow::Result<()> {
    use futures::StreamExt;
    use tokio::io::AsyncWriteExt;
    let client = reqwest::Client::new();
    let response = fetch_with_retry(&client, url, &[], DOWNLOAD_TIMEOUT_MS).await?;
    if !response.status().is_success() {
        anyhow::bail!("Download failed with HTTP {}: {url}", response.status().as_u16());
    }
    let mut file = tokio::fs::File::create(dest).await?;
    let mut stream = response.bytes_stream();
    while let Some(chunk) = stream.next().await {
        file.write_all(&chunk?).await?;
    }
    file.flush().await?;
    Ok(())
}

fn find_binary_recursively(root_dir: &std::path::Path, binary_file_name: &str) -> Option<std::path::PathBuf> {
    let mut stack = vec![root_dir.to_path_buf()];
    while let Some(current_dir) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&current_dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let full_path = entry.path();
            let Ok(file_type) = entry.file_type() else {
                continue;
            };
            if file_type.is_file() && entry.file_name().to_string_lossy() == binary_file_name {
                return Some(full_path);
            }
            if file_type.is_dir() {
                stack.push(full_path);
            }
        }
    }
    None
}

fn format_spawn_failure(result: &std::io::Result<std::process::Output>) -> String {
    match result {
        Err(err) => {
            let (code, _) = crate::core::tools::node_fs::io_error_code(err);
            format!("spawnSync {code}")
        }
        Ok(output) => {
            let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
            if !stderr.is_empty() {
                return stderr;
            }
            let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
            if !stdout.is_empty() {
                return stdout;
            }
            format!(
                "exit status {}",
                output.status.code().map(|c| c.to_string()).unwrap_or_else(|| "unknown".to_string())
            )
        }
    }
}

fn run_extraction_command(command: &str, args: &[&str]) -> Option<String> {
    let result = std::process::Command::new(command)
        .args(args)
        .stdin(std::process::Stdio::null())
        .output();
    if let Ok(output) = &result
        && output.status.success()
    {
        return None;
    }
    Some(format!("{command}: {}", format_spawn_failure(&result)))
}

fn extract_tar_gz_archive(archive_path: &str, extract_dir: &str, asset_name: &str) -> anyhow::Result<()> {
    if let Some(failure) = run_extraction_command("tar", &["xzf", archive_path, "-C", extract_dir]) {
        anyhow::bail!("Failed to extract {asset_name}: {failure}");
    }
    Ok(())
}

fn get_windows_tar_command() -> String {
    if let Ok(system_root) = std::env::var("SystemRoot")
        && !system_root.is_empty()
    {
        let system_tar = std::path::Path::new(&system_root).join("System32").join("tar.exe");
        if system_tar.exists() {
            return system_tar.to_string_lossy().into_owned();
        }
    }
    "tar.exe".to_string()
}

fn extract_zip_archive(archive_path: &str, extract_dir: &str, asset_name: &str) -> anyhow::Result<()> {
    let mut failures: Vec<String> = Vec::new();
    if node_platform() == "win32" {
        // Windows ships bsdtar as tar.exe, which supports zip files. Prefer the
        // System32 binary over Git Bash's GNU tar, which does not handle zip archives.
        match run_extraction_command(&get_windows_tar_command(), &["xf", archive_path, "-C", extract_dir]) {
            None => return Ok(()),
            Some(failure) => failures.push(failure),
        }
        let script = "& { param($archive, $destination) $ErrorActionPreference = 'Stop'; Expand-Archive -LiteralPath $archive -DestinationPath $destination -Force }";
        match run_extraction_command(
            "powershell.exe",
            &[
                "-NoLogo",
                "-NoProfile",
                "-NonInteractive",
                "-ExecutionPolicy",
                "Bypass",
                "-Command",
                script,
                archive_path,
                extract_dir,
            ],
        ) {
            None => return Ok(()),
            Some(failure) => failures.push(failure),
        }
    } else {
        match run_extraction_command("unzip", &["-q", archive_path, "-d", extract_dir]) {
            None => return Ok(()),
            Some(failure) => failures.push(failure),
        }
        match run_extraction_command("tar", &["xf", archive_path, "-C", extract_dir]) {
            None => return Ok(()),
            Some(failure) => failures.push(failure),
        }
    }
    anyhow::bail!("Failed to extract {asset_name}: {}", failures.join("; "))
}

/// Download and install a tool.
async fn download_tool(tool: GetToolPathTool) -> anyhow::Result<String> {
    let config = tool_config(tool);
    let plat = node_platform();
    let architecture = node_arch();

    // fd is pinned on darwin/x64, so skip the version lookup there.
    let version = if tool == GetToolPathTool::Fd && plat == "darwin" && architecture == "x64" {
        "10.3.0".to_string()
    } else {
        get_latest_version(&config.repo).await?
    };

    let Some(asset_name) = (config.get_asset_name)(version.clone(), plat.to_string(), architecture.to_string()) else {
        anyhow::bail!("Unsupported platform: {plat}/{architecture}");
    };

    let tools_dir = tools_dir();
    std::fs::create_dir_all(&tools_dir)?;

    let download_url = format!(
        "https://github.com/{}/releases/download/{}{version}/{asset_name}",
        config.repo, config.tag_prefix
    );
    let tools_path = std::path::Path::new(&tools_dir);
    let archive_path = tools_path.join(&asset_name).to_string_lossy().into_owned();
    let binary_file_name = format!("{}{}", config.binary_name, binary_ext());
    let binary_path = tools_path.join(&binary_file_name);

    download_file(&download_url, &archive_path).await?;

    // Extract into a unique temp directory. fd and rg downloads can run concurrently
    // during startup, so sharing a fixed directory causes races.
    let random = uuid::Uuid::new_v4().simple().to_string();
    let extract_dir = tools_path.join(format!(
        "extract_tmp_{}_{}_{}_{}",
        config.binary_name,
        std::process::id(),
        chrono::Utc::now().timestamp_millis(),
        &random[..8]
    ));
    std::fs::create_dir_all(&extract_dir)?;
    let extract_dir_str = extract_dir.to_string_lossy().into_owned();

    let result = (|| -> anyhow::Result<()> {
        if asset_name.ends_with(".tar.gz") {
            extract_tar_gz_archive(&archive_path, &extract_dir_str, &asset_name)?;
        } else if asset_name.ends_with(".zip") {
            extract_zip_archive(&archive_path, &extract_dir_str, &asset_name)?;
        } else {
            anyhow::bail!("Unsupported archive format: {asset_name}");
        }

        // Find the binary in extracted files. Some archives contain files directly
        // at root, others nest under a versioned subdirectory.
        let stem = asset_name
            .strip_suffix(".tar.gz")
            .or_else(|| asset_name.strip_suffix(".zip"))
            .unwrap_or(&asset_name);
        let extracted_dir = extract_dir.join(stem);
        let candidates = [extracted_dir.join(&binary_file_name), extract_dir.join(&binary_file_name)];
        let extracted_binary = candidates
            .iter()
            .find(|candidate| candidate.exists())
            .cloned()
            .or_else(|| find_binary_recursively(&extract_dir, &binary_file_name));
        let Some(extracted_binary) = extracted_binary else {
            anyhow::bail!(
                "Binary not found in archive: expected {binary_file_name} under {extract_dir_str}"
            );
        };
        std::fs::rename(&extracted_binary, &binary_path)?;

        // Make executable (Unix only).
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&binary_path, std::fs::Permissions::from_mode(0o755))?;
        }
        Ok(())
    })();

    // Cleanup
    let _ = std::fs::remove_file(&archive_path);
    let _ = std::fs::remove_dir_all(&extract_dir);
    result?;
    Ok(binary_path.to_string_lossy().into_owned())
}

/// Termux package names for tools.
fn termux_package(tool: GetToolPathTool) -> &'static str {
    match tool {
        GetToolPathTool::Fd => "fd",
        GetToolPathTool::Rg => "ripgrep",
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ToolStatusType {
    #[serde(rename = "info")]
    Info,
    #[serde(rename = "warning")]
    Warning,
}

impl ToolStatusType {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Info => "info",
            Self::Warning => "warning",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolStatus {
    pub r#type: ToolStatusType,
    pub message: String,
}

/// Ensure a tool is available, downloading if necessary.
/// Reports progress through `onStatus`; status messages are otherwise silent.
/// Returns the tool path, or undefined if unavailable.
pub async fn ensure_tool(
    tool: GetToolPathTool,
    on_status: Option<Arc<dyn Fn(ToolStatus) + Send + Sync>>,
) -> anyhow::Result<Option<String>> {
    let existing = tokio::task::spawn_blocking(move || get_tool_path(tool)).await.ok().flatten();
    if let Some(existing) = existing {
        return Ok(Some(existing));
    }

    let config = tool_config(tool);
    let report = |status_type: ToolStatusType, message: String| {
        if let Some(on_status) = &on_status {
            on_status(ToolStatus {
                r#type: status_type,
                message,
            });
        }
    };

    if is_offline_mode_enabled() {
        report(
            ToolStatusType::Warning,
            format!("{} not found. Offline mode enabled, skipping download.", config.name),
        );
        return Ok(None);
    }

    // On Android/Termux, Linux binaries don't work due to Bionic libc incompatibility.
    // Users must install via pkg.
    if node_platform() == "android" {
        report(
            ToolStatusType::Warning,
            format!("{} not found. Install with: pkg install {}", config.name, termux_package(tool)),
        );
        return Ok(None);
    }

    // Tool not found - download it.
    report(ToolStatusType::Info, format!("{} not found. Downloading...", config.name));
    match download_tool(tool).await {
        Ok(path) => {
            report(ToolStatusType::Info, format!("{} installed to {path}", config.name));
            Ok(Some(path))
        }
        Err(err) => {
            // Include the error cause chain (depth-capped).
            let mut messages: Vec<String> = Vec::new();
            for cause in err.chain().take(5) {
                let message = cause.to_string();
                if !messages.contains(&message) {
                    messages.push(message);
                }
            }
            report(
                ToolStatusType::Warning,
                format!(
                    "Failed to download {}: {}",
                    config.name,
                    if messages.is_empty() { err.to_string() } else { messages.join(": ") }
                ),
            );
            Ok(None)
        }
    }
}

#[cfg(test)]
mod tests {
    //! `tools-manager.test.ts` "getLatestVersion" (the `ensureTool` cases are in
    //! `tests/agent_dir_env.rs`, since they change the agent dir, PATH and proxy variables).
    use super::*;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    /// Answer every request with `status`, an optional `location` and `body`; returns the origin
    /// and the recorded request heads.
    async fn serve(
        status: u16,
        location: Option<&str>,
        body: &str,
    ) -> (String, Arc<parking_lot::Mutex<Vec<String>>>) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let origin = format!("http://{}", listener.local_addr().unwrap());
        let requests: Arc<parking_lot::Mutex<Vec<String>>> = Arc::default();
        let location = location.map(|l| format!("location: {l}\r\n")).unwrap_or_default();
        let response = format!(
            "HTTP/1.1 {status} X\r\n{location}content-length: {}\r\nconnection: close\r\n\r\n{body}",
            body.len()
        );
        let recorded = requests.clone();
        tokio::spawn(async move {
            while let Ok((mut socket, _)) = listener.accept().await {
                let mut buf = vec![0u8; 8192];
                let n = socket.read(&mut buf).await.unwrap_or(0);
                recorded.lock().push(String::from_utf8_lossy(&buf[..n]).into_owned());
                let _ = socket.write_all(response.as_bytes()).await;
                let _ = socket.shutdown().await;
            }
        });
        (origin, requests)
    }

    #[tokio::test]
    async fn resolves_the_version_from_the_release_page_redirect() {
        let (origin, requests) = serve(302, Some("https://github.com/sharkdp/fd/releases/tag/v10.4.2"), "").await;
        assert_eq!(get_latest_version_from(&origin, "sharkdp/fd").await.unwrap(), "10.4.2");
        // The redirect is read, not followed (`redirect: "manual"`).
        let requests = requests.lock();
        assert_eq!(requests.len(), 1);
        assert!(requests[0].starts_with("GET /sharkdp/fd/releases/latest HTTP/1.1\r\n"), "{}", requests[0]);
    }

    #[tokio::test]
    async fn keeps_tags_without_a_v_prefix_intact() {
        let (origin, _) = serve(302, Some("https://github.com/BurntSushi/ripgrep/releases/tag/15.2.0"), "").await;
        assert_eq!(get_latest_version_from(&origin, "BurntSushi/ripgrep").await.unwrap(), "15.2.0");
    }

    #[tokio::test]
    async fn resolves_relative_redirect_targets() {
        let (origin, _) = serve(302, Some("/sharkdp/fd/releases/tag/v10.4.2"), "").await;
        assert_eq!(get_latest_version_from(&origin, "sharkdp/fd").await.unwrap(), "10.4.2");
    }

    #[tokio::test]
    async fn discards_the_redirect_response_body() {
        let (origin, _) =
            serve(302, Some("https://github.com/sharkdp/fd/releases/tag/v10.4.2"), "<html></html>").await;
        assert_eq!(get_latest_version_from(&origin, "sharkdp/fd").await.unwrap(), "10.4.2");
    }

    #[tokio::test]
    async fn fails_clearly_when_the_endpoint_does_not_redirect() {
        let (origin, _) = serve(404, None, "not found").await;
        let err = get_latest_version_from(&origin, "sharkdp/fd").await.unwrap_err();
        assert_eq!(err.to_string(), "Failed to resolve latest sharkdp/fd release: HTTP 404 without redirect");
    }

    #[tokio::test]
    async fn fails_clearly_when_the_redirect_does_not_point_at_a_release_tag() {
        let (origin, _) = serve(302, Some("https://github.com/login"), "").await;
        let err = get_latest_version_from(&origin, "sharkdp/fd").await.unwrap_err();
        assert_eq!(
            err.to_string(),
            "Failed to resolve latest sharkdp/fd release: unexpected redirect to https://github.com/login"
        );
    }
}
