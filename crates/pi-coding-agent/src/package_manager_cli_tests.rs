//! Tests ported from `packages/coding-agent/test/package-command-paths.test.ts` (the package
//! command paths that do not need the full `main()` or network).

use super::*;
use crate::config::ENV_AGENT_DIR;

use crate::core::package_manager::TEST_ENV_LOCK as CLI_LOCK;

struct Env {
    _guard: std::sync::MutexGuard<'static, ()>,
    _dir: tempfile::TempDir,
    agent_dir: String,
    project_dir: String,
    package_dir: String,
    original_cwd: std::path::PathBuf,
    original_agent_dir: Option<String>,
}

impl Env {
    fn new() -> Self {
        let guard = CLI_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let dir = tempfile::tempdir().unwrap();
        let temp = canonicalize_path(&dir.path().to_string_lossy());
        let agent_dir = node_join(&[&temp, "agent"]);
        let project_dir = node_join(&[&temp, "project"]);
        let package_dir = node_join(&[&temp, "local-package"]);
        for d in [&agent_dir, &project_dir, &package_dir] {
            std::fs::create_dir_all(d).unwrap();
        }
        let original_cwd = std::env::current_dir().unwrap();
        let original_agent_dir = std::env::var(ENV_AGENT_DIR).ok();
        unsafe { std::env::set_var(ENV_AGENT_DIR, &agent_dir) };
        std::env::set_current_dir(&project_dir).unwrap();
        set_exit_code(0);
        CAPTURE.with(|c| *c.borrow_mut() = Some((Vec::new(), Vec::new())));
        Self {
            _guard: guard,
            _dir: dir,
            agent_dir,
            project_dir,
            package_dir,
            original_cwd,
            original_agent_dir,
        }
    }

    fn stdout(&self) -> String {
        CAPTURE.with(|c| c.borrow().as_ref().map(|(o, _)| o.join("\n")).unwrap_or_default())
    }

    fn stderr(&self) -> String {
        CAPTURE.with(|c| c.borrow().as_ref().map(|(_, e)| e.join("\n")).unwrap_or_default())
    }

    fn write(&self, path: &str, content: &str) {
        std::fs::create_dir_all(crate::utils::paths::node_dirname(path)).unwrap();
        std::fs::write(path, content).unwrap();
    }

    fn settings_packages(&self, path: &str) -> Vec<String> {
        let value: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
        value
            .get("packages")
            .and_then(|p| p.as_array())
            .map(|a| a.iter().filter_map(|v| v.as_str().map(str::to_string)).collect())
            .unwrap_or_default()
    }
}

impl Drop for Env {
    fn drop(&mut self) {
        CAPTURE.with(|c| *c.borrow_mut() = None);
        let _ = std::env::set_current_dir(&self.original_cwd);
        match &self.original_agent_dir {
            Some(v) => unsafe { std::env::set_var(ENV_AGENT_DIR, v) },
            None => unsafe { std::env::remove_var(ENV_AGENT_DIR) },
        }
        set_exit_code(0);
    }
}

fn args(a: &[&str]) -> Vec<String> {
    a.iter().map(|s| s.to_string()).collect()
}

async fn run(a: &[&str]) {
    assert!(handle_package_command(&args(a), None).await.unwrap());
}

fn exit_code() -> Option<i32> {
    crate::utils::process_exit::exit_code()
}

#[tokio::test]
async fn persists_global_relative_local_package_paths_relative_to_settings() {
    let env = Env::new();
    let relative_pkg_dir = node_join(&[&env.project_dir, "packages", "local-package"]);
    std::fs::create_dir_all(&relative_pkg_dir).unwrap();
    run(&["install", "./packages/local-package"]).await;
    let packages = env.settings_packages(&node_join(&[&env.agent_dir, "settings.json"]));
    assert_eq!(packages.len(), 1);
    assert_eq!(
        canonicalize_path(&node_join(&[&env.agent_dir, &packages[0]])),
        canonicalize_path(&relative_pkg_dir)
    );
    assert!(env.stdout().contains("Installed ./packages/local-package"));
}

#[tokio::test]
async fn removes_local_packages_using_trailing_slash() {
    let env = Env::new();
    let settings_path = node_join(&[&env.agent_dir, "settings.json"]);
    let with_slash = format!("{}/", env.package_dir);
    run(&["install", &with_slash]).await;
    assert_eq!(env.settings_packages(&settings_path).len(), 1);
    run(&["remove", &with_slash]).await;
    assert!(env.settings_packages(&settings_path).is_empty());
    assert!(env.stdout().contains(&format!("Removed {with_slash}")));
}

fn write_project_packages(env: &Env) {
    env.write(
        &node_join(&[&env.project_dir, ".pi", "settings.json"]),
        r#"{"packages":["npm:@project/pkg"]}"#,
    );
}

#[tokio::test]
async fn list_skips_untrusted_project_package_settings() {
    let env = Env::new();
    write_project_packages(&env);
    run(&["list"]).await;
    assert!(env.stdout().contains("No packages installed."));
    assert!(!env.stdout().contains("Project packages:"));
}

#[tokio::test]
async fn list_uses_remembered_project_trust_and_overrides() {
    let env = Env::new();
    write_project_packages(&env);
    ProjectTrustStore::new(&env.agent_dir)
        .set(&env.project_dir, Some(true))
        .unwrap();
    run(&["list"]).await;
    assert!(env.stdout().contains("Project packages:"));
    assert!(env.stdout().contains("npm:@project/pkg"));
    assert!(!env.stdout().contains("No packages installed."));
    assert_eq!(exit_code(), Some(0));

    CAPTURE.with(|c| *c.borrow_mut() = Some((Vec::new(), Vec::new())));
    run(&["list", "--no-approve"]).await;
    assert!(env.stdout().contains("No packages installed."));
    assert!(!env.stdout().contains("Project packages:"));
}

#[tokio::test]
async fn list_approves_project_trust_with_approve() {
    let env = Env::new();
    write_project_packages(&env);
    run(&["list", "--approve"]).await;
    assert!(env.stdout().contains("Project packages:"));
    assert!(env.stdout().contains("npm:@project/pkg"));
}

#[tokio::test]
async fn list_uses_default_project_trust_unless_trust_json_overrides() {
    let env = Env::new();
    write_project_packages(&env);
    env.write(
        &node_join(&[&env.agent_dir, "settings.json"]),
        r#"{"defaultProjectTrust":"always"}"#,
    );
    run(&["list"]).await;
    assert!(env.stdout().contains("Project packages:"));

    CAPTURE.with(|c| *c.borrow_mut() = Some((Vec::new(), Vec::new())));
    ProjectTrustStore::new(&env.agent_dir)
        .set(&env.project_dir, Some(false))
        .unwrap();
    run(&["list"]).await;
    assert!(env.stdout().contains("No packages installed."));
    assert!(!env.stdout().contains("Project packages:"));
}

#[cfg(unix)]
#[tokio::test]
async fn update_uses_only_saved_project_trust() {
    let env = Env::new();
    let fake_npm = node_join(&[&env.agent_dir, "..", "fake-npm.sh"]);
    let record = node_join(&[&env.agent_dir, "..", "record.txt"]);
    env.write(&fake_npm, &format!("printf '%s ' \"$@\" > '{record}'\n"));
    env.write(
        &node_join(&[&env.project_dir, ".pi", "settings.json"]),
        &serde_json::json!({"packages": ["npm:fake-package"], "npmCommand": ["sh", fake_npm]}).to_string(),
    );
    // Default trust does not apply to update: only remembered decisions do.
    env.write(
        &node_join(&[&env.agent_dir, "settings.json"]),
        r#"{"defaultProjectTrust":"always"}"#,
    );
    run(&["update", "--extensions"]).await;
    assert!(!std::path::Path::new(&record).exists());
    assert_eq!(exit_code(), Some(0));

    ProjectTrustStore::new(&env.agent_dir)
        .set(&env.project_dir, Some(true))
        .unwrap();
    run(&["update", "--extensions"]).await;
    assert!(std::path::Path::new(&record).exists(), "{}", env.stderr());
    assert!(std::fs::read_to_string(&record).unwrap().contains("install fake-package@latest"));
    assert_eq!(exit_code(), Some(0));
}

#[tokio::test]
async fn blocks_local_package_changes_when_untrusted() {
    let env = Env::new();
    env.write(&node_join(&[&env.project_dir, ".pi", "settings.json"]), "{}");
    run(&["install", "-l", "./local-package"]).await;
    assert!(env
        .stderr()
        .contains("Project is not trusted. Use --approve to modify local package config."));
    assert_eq!(exit_code(), Some(1));
}

#[tokio::test]
async fn allows_local_install_to_initialize_fresh_project_settings() {
    let env = Env::new();
    run(&["install", "-l", &env.package_dir]).await;
    let packages = env.settings_packages(&node_join(&[&env.project_dir, ".pi", "settings.json"]));
    assert_eq!(packages.len(), 1);
    assert_eq!(
        canonicalize_path(&node_join(&[&env.project_dir, ".pi", &packages[0]])),
        canonicalize_path(&env.package_dir)
    );
    assert_eq!(exit_code(), Some(0));
}

#[tokio::test]
async fn shows_install_help_and_friendly_errors() {
    let env = Env::new();
    run(&["install", "--help"]).await;
    assert!(env.stdout().contains("Usage:"));
    assert!(env.stdout().contains("pi install <source> [-l]"));
    assert!(env.stderr().is_empty());
    assert_eq!(exit_code(), Some(0));

    run(&["install", "--unknown"]).await;
    assert!(env.stderr().contains("Unknown option --unknown for \"install\"."));
    assert!(env
        .stderr()
        .contains("Use \"pi --help\" or \"pi install <source> [-l] [--approve|--no-approve]\"."));
    assert_eq!(exit_code(), Some(1));

    set_exit_code(0);
    run(&["install"]).await;
    assert!(env.stderr().contains("Missing install source."));
    assert!(env.stderr().contains("Usage: pi install <source> [-l]"));
    assert_eq!(exit_code(), Some(1));
}

#[tokio::test]
async fn rejects_update_models_combined_with_another_target() {
    let env = Env::new();
    run(&["update", "--models", "--self"]).await;
    assert!(env.stderr().contains("--models cannot be combined with --self"));
    assert_eq!(exit_code(), Some(1));
}

#[tokio::test]
async fn suggests_configured_source_when_update_omits_npm_prefix() {
    let env = Env::new();
    let settings_path = node_join(&[&env.agent_dir, "settings.json"]);
    env.write(&settings_path, r#"{"packages":["npm:pi-formatter"]}"#);
    run(&["update", "pi-formatter"]).await;
    assert!(env.stderr().contains("Did you mean npm:pi-formatter?"));
    assert!(!env.stdout().contains("Updated pi-formatter"));
    assert_eq!(exit_code(), Some(1));
    assert!(env.settings_packages(&settings_path).contains(&"npm:pi-formatter".to_string()));
}

#[test]
fn parses_package_command_targets() {
    let parse = |a: &[&str]| parse_package_command(&args(a)).unwrap();
    assert!(parse_package_command(&args(&["config"])).is_none());
    assert_eq!(parse(&["uninstall", "x"]).command, PackageCommand::Remove);
    let o = parse(&["update"]);
    assert_eq!(o.update_target, Some(UpdateTarget::SelfValue {}));
    assert!(o.show_extensions_skipped_note);
    assert_eq!(parse(&["update", "--all"]).update_target, Some(UpdateTarget::All {}));
    assert_eq!(parse(&["update", "--self", "--extensions"]).update_target, Some(UpdateTarget::All {}));
    assert_eq!(parse(&["update", "pi", "--extensions"]).update_target, Some(UpdateTarget::All {}));
    assert_eq!(parse(&["update", "self"]).update_target, Some(UpdateTarget::SelfValue {}));
    assert_eq!(
        parse(&["update", "npm:x"]).update_target,
        Some(UpdateTarget::Extensions { source: Some("npm:x".into()) })
    );
    assert_eq!(
        parse(&["update", "--extension", "npm:x"]).update_target,
        Some(UpdateTarget::Extensions { source: Some("npm:x".into()) })
    );
    assert_eq!(parse(&["update", "--extension"]).missing_option_value.as_deref(), Some("--extension"));
    assert_eq!(
        parse(&["update", "--extension", "a", "--extension", "b"]).conflicting_options.as_deref(),
        Some("--extension can only be provided once")
    );
    assert_eq!(
        parse(&["update", "--all", "npm:x"]).conflicting_options.as_deref(),
        Some("--all cannot be combined with a positional source")
    );
    assert_eq!(
        parse(&["update", "npm:x", "--self"]).conflicting_options.as_deref(),
        Some("positional update targets cannot be combined with --self, --extensions, or --all")
    );
    assert_eq!(parse(&["list", "-l"]).invalid_option.as_deref(), Some("-l"));
    assert_eq!(parse(&["install", "--force", "x"]).invalid_option.as_deref(), Some("--force"));
    assert_eq!(parse(&["install", "a", "b"]).invalid_argument.as_deref(), Some("b"));
    assert_eq!(parse(&["list", "-na"]).project_trust_override, Some(false));
    assert_eq!(parse(&["remove", "x", "-l", "-a"]).project_trust_override, Some(true));
}

#[test]
fn validates_managed_release_versions() {
    assert!(is_managed_release_version("1.2.3"));
    assert!(is_managed_release_version("1.2.3-beta.1+build.5"));
    assert!(!is_managed_release_version("1.2"));
    assert!(!is_managed_release_version("../1.2.3"));
}

#[test]
fn rejects_invalid_managed_install_markers() {
    let _guard = CLI_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let dir = tempfile::tempdir().unwrap();
    let root = canonicalize_path(&dir.path().to_string_lossy());
    let release = node_join(&[&root, "releases", "1.0.0"]);
    std::fs::create_dir_all(&release).unwrap();
    let previous_root = std::env::var("PI_MANAGED_INSTALL_ROOT").ok();
    let previous_pkg = std::env::var("PI_PACKAGE_DIR").ok();
    unsafe {
        std::env::set_var("PI_MANAGED_INSTALL_ROOT", &root);
        std::env::set_var("PI_PACKAGE_DIR", &release);
    }
    let missing = get_active_managed_install_root().unwrap_err().to_string();
    // A UTF-8 BOM is accepted.
    std::fs::write(
        node_join(&[&root, MANAGED_INSTALL_MARKER]),
        "\u{feff}{\"kind\":\"pi-managed-install\",\"schemaVersion\":1,\"layout\":\"releases-v1\"}\n",
    )
    .unwrap();
    let valid = get_active_managed_install_root().unwrap();
    // A package dir outside the releases is not managed.
    unsafe { std::env::set_var("PI_PACKAGE_DIR", &root) };
    let outside = get_active_managed_install_root().unwrap();
    unsafe {
        match previous_root {
            Some(v) => std::env::set_var("PI_MANAGED_INSTALL_ROOT", v),
            None => std::env::remove_var("PI_MANAGED_INSTALL_ROOT"),
        }
        match previous_pkg {
            Some(v) => std::env::set_var("PI_PACKAGE_DIR", v),
            None => std::env::remove_var("PI_PACKAGE_DIR"),
        }
    }
    assert!(missing.starts_with("Managed install marker is missing or invalid: "));
    assert_eq!(valid, Some(root));
    assert_eq!(outside, None);
}
