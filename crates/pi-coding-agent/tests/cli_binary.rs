//! CLI tests that spawn the `pi` binary: ports of `test/session-file-invalid.test.ts`,
//! `test/stdout-cleanliness.test.ts` and `test/startup-session-name.test.ts`. TS spawns
//! `node src/cli.ts`; the fake npm of the stdout test is a shell script instead of a Node script.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

struct Output {
    code: Option<i32>,
    stdout: String,
    stderr: String,
}

fn run_pi(args: &[&str], cwd: &Path, agent_dir: &Path, offline: bool) -> Output {
    let mut command = Command::new(env!("CARGO_BIN_EXE_pi"));
    command
        .args(args)
        .current_dir(cwd)
        .env("PI_CODING_AGENT_DIR", agent_dir)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    if offline {
        command.env("PI_OFFLINE", "1");
    }
    let output = command.output().expect("spawn pi");
    Output {
        code: output.status.code(),
        stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
    }
}

fn temp_root() -> (tempfile::TempDir, PathBuf, PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    let agent_dir = root.join("agent");
    let project_dir = root.join("project");
    std::fs::create_dir_all(&agent_dir).unwrap();
    std::fs::create_dir_all(&project_dir).unwrap();
    (dir, agent_dir, project_dir)
}

// session-file-invalid.test.ts
#[test]
fn prints_a_friendly_error_and_preserves_non_session_file_content() {
    let (dir, agent_dir, project_dir) = temp_root();
    let session_file = dir.path().canonicalize().unwrap().join("not-a-session.log");
    let original = "{\"type\":\"event\",\"data\":\"not a session\"}\n";
    std::fs::write(&session_file, original).unwrap();

    let result = run_pi(&["--session", session_file.to_str().unwrap(), "-p", "hi"], &project_dir, &agent_dir, true);

    assert_eq!(result.code, Some(1), "{}", result.stderr);
    assert!(
        result.stderr.contains(&format!("Error: Session file is not a valid pi session: {}", session_file.display())),
        "{}",
        result.stderr
    );
    assert!(!result.stderr.contains("SessionManager.open"));
    assert!(!result.stderr.contains("at "), "{}", result.stderr);
    assert_eq!(std::fs::read_to_string(&session_file).unwrap(), original);
}

// stdout-cleanliness.test.ts
fn stdout_project() -> (tempfile::TempDir, PathBuf, PathBuf) {
    let (dir, agent_dir, project_dir) = temp_root();
    let config_dir = project_dir.join(".pi");
    std::fs::create_dir_all(&config_dir).unwrap();
    let fake_npm = dir.path().join("fake-npm.sh");
    std::fs::write(&fake_npm, "echo \"changed 1 package in 471ms\"\necho \"found 0 vulnerabilities\"\nexit 0\n").unwrap();
    let settings = serde_json::json!({
        "packages": ["npm:fake-package"],
        "npmCommand": ["sh", fake_npm.to_string_lossy()],
    });
    std::fs::write(config_dir.join("settings.json"), serde_json::to_string_pretty(&settings).unwrap()).unwrap();
    (dir, agent_dir, project_dir)
}

#[test]
fn prints_plain_help_to_stdout_when_stdout_is_redirected() {
    let (_dir, agent_dir, project_dir) = stdout_project();
    let result = run_pi(&["--help"], &project_dir, &agent_dir, false);
    assert_eq!(result.code, Some(0), "{}", result.stderr);
    assert!(result.stdout.contains("Usage:"));
    assert!(!result.stderr.contains("Usage:"));
    assert!(!result.stderr.contains("changed 1 package in 471ms"));
    assert!(!result.stderr.contains("found 0 vulnerabilities"));
}

#[test]
fn keeps_stdout_empty_for_mode_json_help_while_routing_trusted_startup_chatter_to_stderr() {
    let (_dir, agent_dir, project_dir) = stdout_project();
    let result = run_pi(&["--mode", "json", "--help", "--approve"], &project_dir, &agent_dir, false);
    assert_eq!(result.code, Some(0), "{}", result.stderr);
    assert_eq!(result.stdout, "");
    assert!(result.stderr.contains("changed 1 package in 471ms"), "{}", result.stderr);
    assert!(result.stderr.contains("found 0 vulnerabilities"), "{}", result.stderr);
    assert!(result.stderr.contains("Usage:"));
}

// startup-session-name.test.ts
#[test]
fn sets_name_on_the_selected_session_before_runtime_model_validation() {
    let (dir, agent_dir, project_dir) = temp_root();
    let session_file = dir.path().join("session.jsonl");
    let timestamp = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
    let header = serde_json::json!({
        "type": "session", "version": 3, "id": "existing-session", "timestamp": timestamp,
        "cwd": project_dir.to_string_lossy(),
    });
    let message = serde_json::json!({
        "type": "message", "id": "assistant-1", "parentId": null, "timestamp": timestamp,
        "message": {
            "role": "assistant", "content": [{ "type": "text", "text": "hello" }],
            "provider": "anthropic", "model": "claude-sonnet-4-5", "timestamp": chrono::Utc::now().timestamp_millis()
        }
    });
    std::fs::write(&session_file, format!("{header}\n{message}\n")).unwrap();

    let result = run_pi(
        &[
            "--session",
            session_file.to_str().unwrap(),
            "--name",
            "  CLI Named Session  ",
            "--model",
            "missing-model",
            "-p",
            "hi",
        ],
        &project_dir,
        &agent_dir,
        true,
    );

    assert_eq!(result.code, Some(1), "{}", result.stderr);
    let names: Vec<String> = std::fs::read_to_string(&session_file)
        .unwrap()
        .trim()
        .lines()
        .map(|line| serde_json::from_str::<serde_json::Value>(line).unwrap())
        .filter(|entry| entry["type"] == "session_info")
        .map(|entry| entry["name"].as_str().unwrap_or_default().to_string())
        .collect();
    assert_eq!(names, vec!["CLI Named Session"]);
}
