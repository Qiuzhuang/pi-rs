//! Port of `packages/coding-agent/src/migrations.ts`.
//!
//! One-time migrations that run on startup.

use crate::config::{CONFIG_DIR_NAME, get_agent_dir, get_bin_dir};
use crate::core::keybindings::migrate_keybindings_config;
use crate::utils::chalk;
use crate::utils::text::strip_bom;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::path::{Path, PathBuf};

const MIGRATION_GUIDE_URL: &str =
    "https://github.com/earendil-works/pi/blob/main/packages/coding-agent/CHANGELOG.md#extensions-migration";
const EXTENSIONS_DOC_URL: &str = "https://github.com/earendil-works/pi/blob/main/packages/coding-agent/docs/extensions.md";

fn path_string(path: impl AsRef<Path>) -> String {
    path.as_ref().to_string_lossy().into_owned()
}

fn read_json(path: &Path) -> anyhow::Result<Value> {
    let content = std::fs::read_to_string(path)?;
    Ok(serde_json::from_str(&strip_bom(&content))?)
}

/// `JSON.stringify(value, null, 2)`.
fn to_pretty_json(value: &Value) -> String {
    serde_json::to_string_pretty(value).unwrap_or_else(|_| "null".to_string())
}

fn write_private_file(path: &Path, content: &str) -> std::io::Result<()> {
    use std::io::Write;
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(path)?;
    file.write_all(content.as_bytes())
}

/// Migrate legacy oauth.json and settings.json apiKeys to auth.json.
///
/// @returns Array of provider names that were migrated
pub fn migrate_auth_to_auth_json() -> Vec<String> {
    let agent_dir = PathBuf::from(get_agent_dir());
    let auth_path = agent_dir.join("auth.json");
    let oauth_path = agent_dir.join("oauth.json");
    let settings_path = agent_dir.join("settings.json");

    // Skip if auth.json already exists
    if auth_path.exists() {
        return Vec::new();
    }

    let mut migrated: Map<String, Value> = Map::new();
    let mut providers: Vec<String> = Vec::new();

    // Migrate oauth.json
    if oauth_path.exists() {
        let result = (|| -> anyhow::Result<()> {
            let oauth = read_json(&oauth_path)?;
            if let Value::Object(oauth) = oauth {
                for (provider, cred) in oauth {
                    let mut entry = Map::new();
                    entry.insert("type".into(), Value::String("oauth".into()));
                    if let Value::Object(cred) = cred {
                        entry.extend(cred);
                    }
                    migrated.insert(provider.clone(), Value::Object(entry));
                    providers.push(provider);
                }
            }
            std::fs::rename(&oauth_path, format!("{}.migrated", path_string(&oauth_path)))?;
            Ok(())
        })();
        // Skip on error
        let _ = result;
    }

    // Migrate settings.json apiKeys
    if settings_path.exists() {
        let result = (|| -> anyhow::Result<()> {
            let mut settings = read_json(&settings_path)?;
            let api_keys = settings.get("apiKeys").cloned();
            if let Some(Value::Object(api_keys)) = api_keys {
                for (provider, key) in api_keys {
                    if !migrated.contains_key(&provider)
                        && let Value::String(key) = key
                    {
                        let mut entry = Map::new();
                        entry.insert("type".into(), Value::String("api_key".into()));
                        entry.insert("key".into(), Value::String(key));
                        migrated.insert(provider.clone(), Value::Object(entry));
                        providers.push(provider);
                    }
                }
                if let Value::Object(map) = &mut settings {
                    map.shift_remove("apiKeys");
                }
                std::fs::write(&settings_path, to_pretty_json(&settings))?;
            }
            Ok(())
        })();
        // Skip on error
        let _ = result;
    }

    if !migrated.is_empty() {
        if let Some(parent) = auth_path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let _ = write_private_file(&auth_path, &to_pretty_json(&Value::Object(migrated)));
    }

    providers
}

/// Migrate sessions from ~/.pi/agent/*.jsonl to proper session directories.
///
/// Bug in v0.30.0: Sessions were saved to ~/.pi/agent/ instead of
/// ~/.pi/agent/sessions/<encoded-cwd>/. This migration moves them
/// to the correct location based on the cwd in their session header.
///
/// See: https://github.com/earendil-works/pi-mono/issues/320
pub fn migrate_sessions_from_agent_root() {
    let agent_dir = PathBuf::from(get_agent_dir());

    // Find all .jsonl files directly in agentDir (not in subdirectories)
    let Ok(read_dir) = std::fs::read_dir(&agent_dir) else {
        return;
    };
    let files: Vec<PathBuf> = read_dir
        .filter_map(Result::ok)
        .filter(|entry| entry.file_name().to_string_lossy().ends_with(".jsonl"))
        .map(|entry| agent_dir.join(entry.file_name()))
        .collect();

    for file in files {
        let result = (|| -> anyhow::Result<()> {
            // Read first line to get session header
            let content = std::fs::read(&file)?;
            let content = String::from_utf8_lossy(&content);
            let first_line = content.split('\n').next().unwrap_or("");
            if first_line.trim().is_empty() {
                return Ok(());
            }

            let header: Value = serde_json::from_str(first_line)?;
            if header.get("type").and_then(Value::as_str) != Some("session") {
                return Ok(());
            }
            let cwd = match header.get("cwd") {
                Some(Value::String(cwd)) if !cwd.is_empty() => cwd.clone(),
                _ => return Ok(()),
            };

            // Compute the correct session directory (same encoding as session-manager.ts)
            let trimmed = cwd
                .strip_prefix('/')
                .or_else(|| cwd.strip_prefix('\\'))
                .unwrap_or(&cwd);
            let safe_path = format!("--{}--", trimmed.replace(['/', '\\', ':'], "-"));
            let correct_dir = agent_dir.join("sessions").join(safe_path);

            // Create directory if needed
            if !correct_dir.exists() {
                std::fs::create_dir_all(&correct_dir)?;
            }

            // Move the file
            let Some(file_name) = file.file_name() else {
                return Ok(());
            };
            let new_path = correct_dir.join(file_name);

            if new_path.exists() {
                return Ok(()); // Skip if target exists
            }

            std::fs::rename(&file, &new_path)?;
            Ok(())
        })();
        // Skip files that can't be migrated
        let _ = result;
    }
}

/// Migrate commands/ to prompts/ if needed.
/// Works for both regular directories and symlinks.
fn migrate_commands_to_prompts(base_dir: &Path, label: &str) -> bool {
    let commands_dir = base_dir.join("commands");
    let prompts_dir = base_dir.join("prompts");

    if commands_dir.exists() && !prompts_dir.exists() {
        match std::fs::rename(&commands_dir, &prompts_dir) {
            Ok(()) => {
                println!("{}", chalk::green(&format!("Migrated {label} commands/ → prompts/")));
                return true;
            }
            Err(err) => {
                println!(
                    "{}",
                    chalk::yellow(&format!(
                        "Warning: Could not migrate {label} commands/ to prompts/: {err}"
                    ))
                );
            }
        }
    }
    false
}

fn migrate_keybindings_config_file() {
    let config_path = PathBuf::from(get_agent_dir()).join("keybindings.json");
    if !config_path.exists() {
        return;
    }

    let result = (|| -> anyhow::Result<()> {
        let parsed = read_json(&config_path)?;
        let Value::Object(parsed) = parsed else {
            return Ok(());
        };
        let raw: indexmap::IndexMap<String, Value> = parsed.into_iter().collect();
        let result = migrate_keybindings_config(raw);
        if !result.migrated {
            return Ok(());
        }
        let config: Map<String, Value> = result.config.into_iter().collect();
        std::fs::write(&config_path, format!("{}\n", to_pretty_json(&Value::Object(config))))?;
        Ok(())
    })();
    // Ignore malformed files during migration
    let _ = result;
}

/// Move fd/rg binaries from tools/ to bin/ if they exist.
fn migrate_tools_to_bin() {
    let agent_dir = PathBuf::from(get_agent_dir());
    let tools_dir = agent_dir.join("tools");
    let bin_dir = PathBuf::from(get_bin_dir());

    if !tools_dir.exists() {
        return;
    }

    let binaries = ["fd", "rg", "fd.exe", "rg.exe"];
    let mut moved_any = false;

    for bin in binaries {
        let old_path = tools_dir.join(bin);
        let new_path = bin_dir.join(bin);

        if old_path.exists() {
            if !bin_dir.exists() {
                let _ = std::fs::create_dir_all(&bin_dir);
            }
            if !new_path.exists() {
                if std::fs::rename(&old_path, &new_path).is_ok() {
                    moved_any = true;
                }
            } else {
                // Target exists, just delete the old one
                let _ = std::fs::remove_file(&old_path).or_else(|_| std::fs::remove_dir_all(&old_path));
            }
        }
    }

    if moved_any {
        println!("{}", chalk::green("Migrated managed binaries tools/ → bin/"));
    }
}

/// Check for deprecated hooks/ and tools/ directories.
/// Note: tools/ may contain fd/rg binaries extracted by pi, so only warn if it has other files.
fn check_deprecated_extension_dirs(base_dir: &Path, label: &str) -> Vec<String> {
    let hooks_dir = base_dir.join("hooks");
    let tools_dir = base_dir.join("tools");
    let mut warnings: Vec<String> = Vec::new();

    if hooks_dir.exists() {
        warnings.push(format!(
            "{label} hooks/ directory found. Hooks have been renamed to extensions."
        ));
    }

    if tools_dir.exists() {
        // Check if tools/ contains anything other than fd/rg (which are auto-extracted binaries)
        if let Ok(entries) = std::fs::read_dir(&tools_dir) {
            let has_custom_tools = entries.filter_map(Result::ok).any(|entry| {
                let name = entry.file_name().to_string_lossy().into_owned();
                let lower = name.to_lowercase();
                lower != "fd"
                    && lower != "rg"
                    && lower != "fd.exe"
                    && lower != "rg.exe"
                    && !name.starts_with('.') // Ignore .DS_Store and other hidden files
            });
            if has_custom_tools {
                warnings.push(format!(
                    "{label} tools/ directory contains custom tools. Custom tools have been merged into extensions."
                ));
            }
        }
    }

    warnings
}

/// Run extension system migrations (commands→prompts) and collect warnings about deprecated directories.
fn migrate_extension_system(cwd: &str) -> Vec<String> {
    let agent_dir = PathBuf::from(get_agent_dir());
    let project_dir = Path::new(cwd).join(CONFIG_DIR_NAME);

    // Migrate commands/ to prompts/
    migrate_commands_to_prompts(&agent_dir, "Global");
    migrate_commands_to_prompts(&project_dir, "Project");

    // Check for deprecated directories
    let mut warnings = check_deprecated_extension_dirs(&agent_dir, "Global");
    warnings.extend(check_deprecated_extension_dirs(&project_dir, "Project"));
    warnings
}

/// Read one keypress from stdin (raw mode when stdin is a terminal).
fn wait_for_keypress() {
    use std::io::Read;
    #[cfg(unix)]
    {
        use std::io::IsTerminal;
        let fd = libc::STDIN_FILENO;
        let is_tty = std::io::stdin().is_terminal();
        // SAFETY: termios is plain data; tcgetattr/tcsetattr only touch the stdin descriptor.
        let mut original: libc::termios = unsafe { std::mem::zeroed() };
        let raw_enabled = is_tty && unsafe { libc::tcgetattr(fd, &mut original) } == 0 && {
            let mut raw = original;
            unsafe { libc::cfmakeraw(&mut raw) };
            unsafe { libc::tcsetattr(fd, libc::TCSANOW, &raw) == 0 }
        };
        let mut buf = [0u8; 64];
        let _ = std::io::stdin().read(&mut buf);
        if raw_enabled {
            unsafe { libc::tcsetattr(fd, libc::TCSANOW, &original) };
        }
    }
    #[cfg(not(unix))]
    {
        let mut buf = [0u8; 64];
        let _ = std::io::stdin().read(&mut buf);
    }
}

/// Print deprecation warnings and wait for keypress.
pub async fn show_deprecation_warnings(warnings: &[String]) -> anyhow::Result<()> {
    if warnings.is_empty() {
        return Ok(());
    }

    for warning in warnings {
        println!("{}", chalk::yellow(&format!("Warning: {warning}")));
    }
    println!("{}", chalk::yellow("\nMove your extensions to the extensions/ directory."));
    println!("{}", chalk::yellow(&format!("Migration guide: {MIGRATION_GUIDE_URL}")));
    println!("{}", chalk::yellow(&format!("Documentation: {EXTENSIONS_DOC_URL}")));
    println!("{}", chalk::dim("\nPress any key to continue..."));

    tokio::task::spawn_blocking(wait_for_keypress).await?;
    println!();
    Ok(())
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RunMigrationsResult {
    pub migrated_auth_providers: Vec<String>,
    pub deprecation_warnings: Vec<String>,
}

/// Run all migrations. Called once on startup.
///
/// @returns Object with migration results and deprecation warnings
pub fn run_migrations(cwd: &str) -> RunMigrationsResult {
    let migrated_auth_providers = migrate_auth_to_auth_json();
    migrate_sessions_from_agent_root();
    migrate_tools_to_bin();
    migrate_keybindings_config_file();
    let deprecation_warnings = migrate_extension_system(cwd);
    RunMigrationsResult {
        migrated_auth_providers,
        deprecation_warnings,
    }
}
