//! End-to-end model / thinking-level persistence through the `pi` binary, against a local
//! OpenAI-compatible SSE server (a `models.json` custom provider with reasoning models).
//!
//! Matches pi 1.3.0 side by side:
//! - RPC `set_model` / `set_thinking_level` / `cycle_model` / `cycle_thinking_level` change the session
//!   only (`model_change` / `thinking_level_change` entries), never settings.json.
//! - `pi -c` and `--session` restore the session's last model and thinking level; a plain start uses
//!   the saved defaults (`defaultProvider`/`defaultModel`/`defaultThinkingLevel`, then
//!   `modelThinkingLevels`).
//! - The session file is written with the first assistant message, so selections made without a
//!   prompt are not restored by `pi -c`.
//! - `enabledModels` scopes cycling (`isScoped`), and the scope's first model starts new sessions.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::time::Duration;

use serde_json::{Value, json};

/// A minimal OpenAI chat-completions SSE server: every request gets a short streamed text reply.
fn start_mock_server() -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { continue };
            std::thread::spawn(move || {
                let mut reader = BufReader::new(stream.try_clone().unwrap());
                let mut content_length = 0usize;
                let mut request_line = String::new();
                if reader.read_line(&mut request_line).is_err() {
                    return;
                }
                loop {
                    let mut line = String::new();
                    if reader.read_line(&mut line).unwrap_or(0) == 0 || line == "\r\n" {
                        break;
                    }
                    if let Some((name, value)) = line.split_once(':')
                        && name.eq_ignore_ascii_case("content-length")
                    {
                        content_length = value.trim().parse().unwrap_or(0);
                    }
                }
                let mut body = vec![0u8; content_length];
                let _ = reader.read_exact(&mut body);
                let model = serde_json::from_slice::<Value>(&body)
                    .ok()
                    .and_then(|v| v["model"].as_str().map(str::to_string))
                    .unwrap_or_default();
                let chunk = |delta: Value, finish: Value| {
                    let mut c = json!({
                        "id": "c1", "object": "chat.completion.chunk", "created": 0, "model": model,
                        "choices": [{ "index": 0, "delta": delta, "finish_reason": finish }]
                    });
                    if !finish.is_null() {
                        c["usage"] = json!({ "prompt_tokens": 10, "completion_tokens": 3, "total_tokens": 13 });
                    }
                    format!("data: {c}\n\n")
                };
                let mut sse = String::new();
                sse.push_str(&chunk(json!({ "role": "assistant", "content": "Hello" }), Value::Null));
                sse.push_str(&chunk(json!({ "content": " there." }), Value::Null));
                sse.push_str(&chunk(json!({}), json!("stop")));
                sse.push_str("data: [DONE]\n\n");
                let head = "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nCache-Control: no-cache\r\nConnection: close\r\n\r\n";
                let _ = stream.write_all(head.as_bytes());
                let _ = stream.write_all(sse.as_bytes());
                let _ = stream.flush();
            });
        }
    });
    port
}

struct Env {
    _dir: tempfile::TempDir,
    agent_dir: PathBuf,
    cwd: PathBuf,
}

impl Env {
    fn new(settings: Option<Value>) -> Env {
        let port = start_mock_server();
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().canonicalize().unwrap();
        let agent_dir = root.join("agent");
        let cwd = root.join("project");
        std::fs::create_dir_all(&agent_dir).unwrap();
        std::fs::create_dir_all(&cwd).unwrap();
        let model = |id: &str, reasoning: bool| {
            json!({ "id": id, "name": id, "reasoning": reasoning, "contextWindow": 32000, "maxTokens": 4096 })
        };
        let models = json!({ "providers": { "mock": {
            "baseUrl": format!("http://127.0.0.1:{port}/v1"),
            "api": "openai-completions",
            "apiKey": "mock-key",
            "models": [model("alpha", true), model("beta", true), model("gamma", true), model("plain", false)]
        } } });
        std::fs::write(agent_dir.join("models.json"), models.to_string()).unwrap();
        if let Some(settings) = settings {
            std::fs::write(agent_dir.join("settings.json"), settings.to_string()).unwrap();
        }
        Env { _dir: dir, agent_dir, cwd }
    }

    fn command(&self, args: &[&str]) -> Command {
        let mut command = Command::new(env!("CARGO_BIN_EXE_pi"));
        command
            .args(args)
            .current_dir(&self.cwd)
            .env_clear()
            .env("PATH", std::env::var_os("PATH").unwrap_or_default())
            .env("HOME", self.agent_dir.parent().unwrap())
            .env("PI_CODING_AGENT_DIR", &self.agent_dir)
            .env("PI_OFFLINE", "1")
            .env("PI_SKIP_VERSION_CHECK", "1");
        command
    }

    /// Run `pi --mode rpc <args>`, send `commands` one at a time (waiting for each response, and for
    /// `agent_end` after a prompt), then close stdin. Returns each command's response `data`.
    fn rpc(&self, args: &[&str], commands: &[Value]) -> Vec<Value> {
        let mut full_args = vec!["--mode", "rpc"];
        full_args.extend_from_slice(args);
        let mut child = self
            .command(&full_args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("spawn pi");
        let stdout = child.stdout.take().unwrap();
        let (tx, rx) = mpsc::channel::<Value>();
        std::thread::spawn(move || {
            for line in BufReader::new(stdout).lines() {
                let Ok(line) = line else { break };
                if let Ok(value) = serde_json::from_str::<Value>(&line) {
                    let _ = tx.send(value);
                }
            }
        });
        let mut stdin = child.stdin.take().unwrap();
        let mut results = Vec::new();
        for (index, command) in commands.iter().enumerate() {
            let id = format!("c{index}");
            let mut command = command.clone();
            command["id"] = json!(id);
            writeln!(stdin, "{command}").unwrap();
            stdin.flush().unwrap();
            let is_prompt = command["type"] == "prompt";
            let (mut response, mut ended) = (None, !is_prompt);
            while response.is_none() || !ended {
                let message = rx
                    .recv_timeout(Duration::from_secs(60))
                    .unwrap_or_else(|_| panic!("no reply to {command}"));
                if message["type"] == "response" && message["id"] == id {
                    assert_eq!(message["success"], json!(true), "{command} failed: {message}");
                    response = Some(message["data"].clone());
                } else if message["type"] == "agent_end" {
                    ended = true;
                }
            }
            results.push(response.unwrap());
        }
        drop(stdin);
        let status = child.wait().unwrap();
        assert!(status.success(), "pi --mode rpc exited with {status}");
        results
    }

    /// `(model, thinkingLevel)` from `get_state` of a fresh `pi --mode rpc <args>`.
    fn startup_state(&self, args: &[&str]) -> (String, String) {
        state_pair(&self.rpc(args, &[json!({ "type": "get_state" })])[0])
    }

    fn print(&self, args: &[&str]) {
        let output = self.command(args).stdin(Stdio::null()).output().expect("spawn pi");
        assert!(output.status.success(), "pi {args:?} failed: {}", String::from_utf8_lossy(&output.stderr));
    }

    fn settings(&self) -> Value {
        std::fs::read_to_string(self.agent_dir.join("settings.json"))
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_else(|| json!({}))
    }

    fn session_files(&self) -> Vec<PathBuf> {
        fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
            for entry in std::fs::read_dir(dir).into_iter().flatten().flatten() {
                let path = entry.path();
                if path.is_dir() {
                    walk(&path, out);
                } else if path.extension().is_some_and(|e| e == "jsonl") {
                    out.push(path);
                }
            }
        }
        let mut files = Vec::new();
        walk(&self.agent_dir.join("sessions"), &mut files);
        files.sort();
        files
    }

    /// Selection and message entries of the only session file: `model:p/id`, `thinking:level`,
    /// `user`, `assistant:p/id`.
    fn session_entries(&self) -> Vec<String> {
        let files = self.session_files();
        assert_eq!(files.len(), 1, "{files:?}");
        std::fs::read_to_string(&files[0])
            .unwrap()
            .lines()
            .filter_map(|line| {
                let entry: Value = serde_json::from_str(line).unwrap();
                match entry["type"].as_str()? {
                    "model_change" => Some(format!(
                        "model:{}/{}",
                        entry["provider"].as_str()?,
                        entry["modelId"].as_str()?
                    )),
                    "thinking_level_change" => Some(format!("thinking:{}", entry["thinkingLevel"].as_str()?)),
                    "message" => match entry["message"]["role"].as_str()? {
                        "user" => Some("user".to_string()),
                        "assistant" => Some(format!(
                            "assistant:{}/{}",
                            entry["message"]["provider"].as_str()?,
                            entry["message"]["model"].as_str()?
                        )),
                        _ => None,
                    },
                    _ => None,
                }
            })
            .collect()
    }
}

fn state_pair(state: &Value) -> (String, String) {
    (
        format!("{}/{}", state["model"]["provider"].as_str().unwrap(), state["model"]["id"].as_str().unwrap()),
        state["thinkingLevel"].as_str().unwrap().to_string(),
    )
}

fn pair(model: &str, thinking: &str) -> (String, String) {
    (model.to_string(), thinking.to_string())
}

fn saved_defaults(settings: &Value) -> Value {
    json!({
        "defaultProvider": settings["defaultProvider"],
        "defaultModel": settings["defaultModel"],
        "defaultThinkingLevel": settings["defaultThinkingLevel"],
    })
}

#[test]
fn rpc_changes_are_session_scoped_and_continue_restores_them() {
    let env = Env::new(None);
    let results = env.rpc(
        &[],
        &[
            json!({ "type": "get_state" }),
            json!({ "type": "set_model", "provider": "mock", "modelId": "beta" }),
            json!({ "type": "set_thinking_level", "level": "high" }),
            json!({ "type": "cycle_model" }),
            json!({ "type": "cycle_thinking_level" }),
            json!({ "type": "prompt", "message": "hello" }),
            json!({ "type": "get_state" }),
        ],
    );
    assert_eq!(state_pair(&results[0]), pair("mock/alpha", "medium"));
    assert_eq!(results[1]["id"], "beta");
    // Cycling keeps the current level when there is no saved or per-model default.
    assert_eq!(results[3]["model"]["id"], "gamma");
    assert_eq!(results[3]["thinkingLevel"], "high");
    assert_eq!(results[3]["isScoped"], false);
    assert_eq!(results[4], json!({ "level": "off" }));
    assert_eq!(state_pair(&results[6]), pair("mock/gamma", "off"));

    // Nothing was saved as a default.
    let settings = env.settings();
    assert_eq!(saved_defaults(&settings), json!({ "defaultProvider": null, "defaultModel": null, "defaultThinkingLevel": null }));
    assert_eq!(
        env.session_entries(),
        [
            "model:mock/alpha",
            "thinking:medium",
            "model:mock/beta",
            "thinking:high",
            "model:mock/gamma",
            "thinking:off",
            "user",
            "assistant:mock/gamma",
        ]
    );

    // `-c` restores the session's model and thinking level; a plain start does not.
    assert_eq!(env.startup_state(&["-c"]), pair("mock/gamma", "off"));
    assert_eq!(env.startup_state(&[]), pair("mock/alpha", "medium"));
    // An explicit flag still wins over the session.
    assert_eq!(env.startup_state(&["-c", "--thinking", "low"]), pair("mock/gamma", "low"));
    assert_eq!(env.startup_state(&["-c", "--model", "mock/beta:minimal"]), pair("mock/beta", "minimal"));
}

#[test]
fn saved_defaults_and_per_model_levels_seed_new_sessions_only() {
    let env = Env::new(Some(json!({
        "defaultProvider": "mock",
        "defaultModel": "beta",
        "defaultThinkingLevel": "low",
        "modelThinkingLevels": { "mock/beta": "high", "mock/alpha": "minimal" }
    })));
    let results = env.rpc(
        &[],
        &[
            json!({ "type": "get_state" }),
            json!({ "type": "cycle_model" }),
            json!({ "type": "cycle_model" }),
            json!({ "type": "cycle_model" }),
            json!({ "type": "prompt", "message": "hello" }),
        ],
    );
    // The per-model level beats the global default for the saved model.
    assert_eq!(state_pair(&results[0]), pair("mock/beta", "high"));
    // beta -> gamma (global default) -> plain (no reasoning) -> alpha (per-model level).
    assert_eq!((results[1]["model"]["id"].clone(), results[1]["thinkingLevel"].clone()), (json!("gamma"), json!("low")));
    assert_eq!((results[2]["model"]["id"].clone(), results[2]["thinkingLevel"].clone()), (json!("plain"), json!("off")));
    assert_eq!((results[3]["model"]["id"].clone(), results[3]["thinkingLevel"].clone()), (json!("alpha"), json!("minimal")));

    // The settings file is untouched by the session's changes.
    assert_eq!(
        saved_defaults(&env.settings()),
        json!({ "defaultProvider": "mock", "defaultModel": "beta", "defaultThinkingLevel": "low" })
    );
    assert_eq!(env.startup_state(&["-c"]), pair("mock/alpha", "minimal"));
    assert_eq!(env.startup_state(&[]), pair("mock/beta", "high"));
}

#[test]
fn print_mode_selection_is_recorded_and_restored() {
    let env = Env::new(None);
    env.print(&["-p", "one", "--model", "mock/beta", "--thinking", "high"]);
    env.print(&["-p", "two", "-c"]);
    assert_eq!(
        env.session_entries(),
        ["model:mock/beta", "thinking:high", "user", "assistant:mock/beta", "user", "assistant:mock/beta"]
    );
    let session = env.session_files()[0].to_string_lossy().into_owned();
    assert_eq!(env.startup_state(&["-c"]), pair("mock/beta", "high"));
    assert_eq!(env.startup_state(&["--session", &session]), pair("mock/beta", "high"));
    assert_eq!(env.startup_state(&[]), pair("mock/alpha", "medium"));
}

#[test]
fn selections_without_a_prompt_do_not_create_a_session_file() {
    let env = Env::new(Some(json!({ "defaultProvider": "mock", "defaultModel": "gamma" })));
    env.rpc(
        &[],
        &[
            json!({ "type": "set_model", "provider": "mock", "modelId": "beta" }),
            json!({ "type": "set_thinking_level", "level": "high" }),
        ],
    );
    assert!(env.session_files().is_empty(), "{:?}", env.session_files());
    // Nothing to continue: `-c` starts a new session from the saved defaults.
    assert_eq!(env.startup_state(&["-c"]), pair("mock/gamma", "medium"));
}

#[test]
fn enabled_models_scope_cycling_and_new_sessions() {
    let env = Env::new(Some(json!({ "enabledModels": ["mock/gamma:high", "mock/alpha"] })));
    let results = env.rpc(
        &[],
        &[
            json!({ "type": "get_state" }),
            json!({ "type": "cycle_model" }),
            json!({ "type": "cycle_model" }),
            json!({ "type": "prompt", "message": "hello" }),
        ],
    );
    // The scope's first model starts new sessions, with the scope's thinking level.
    assert_eq!(state_pair(&results[0]), pair("mock/gamma", "high"));
    assert_eq!(results[1]["model"]["id"], "alpha");
    assert_eq!(results[1]["isScoped"], true);
    assert_eq!(results[2]["model"]["id"], "gamma");
    assert_eq!(results[2]["thinkingLevel"], "high");
    // Continuing keeps the session's selection rather than the scope's first model.
    env.rpc(&["-c"], &[json!({ "type": "set_model", "provider": "mock", "modelId": "beta" }), json!({ "type": "prompt", "message": "again" })]);
    assert_eq!(env.startup_state(&["-c"]), pair("mock/beta", "high"));
    assert_eq!(env.startup_state(&[]), pair("mock/gamma", "high"));
}
