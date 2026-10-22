//! Opt-in tests against real npm packages installed in `PI_JSRT_NPM_DIR` (skipped when unset):
//!
//! ```sh
//! npm install @anthropic-ai/sdk openai execa glob node-fetch
//! PI_JSRT_NPM_DIR=$PWD cargo test -p pi-jsrt --test npm_packages
//! ```
//!
//! (`tests/modules.rs::real_npm_packages` covers ms, chalk, typebox, yaml, zod, diff, minimatch,
//! uuid, lodash-es, nanoid and strip-ansi.) The SDKs talk to a local mock server.

use std::path::{Path, PathBuf};

use pi_jsrt::{JsRuntime, RuntimeOptions};
use serde_json::{Value, json};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

fn npm_dir() -> Option<PathBuf> {
    match std::env::var("PI_JSRT_NPM_DIR") {
        Ok(dir) => Some(dir.into()),
        Err(_) => {
            eprintln!("skipping: PI_JSRT_NPM_DIR not set");
            None
        }
    }
}

/// A mock server answering every request with `(content-type, body)` chosen by request path.
async fn mock_server(route: fn(&str) -> (&'static str, String)) -> String {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    tokio::spawn(async move {
        loop {
            let Ok((mut socket, _)) = listener.accept().await else { return };
            tokio::spawn(async move {
                let mut buf = Vec::new();
                let mut chunk = [0u8; 8192];
                let head = loop {
                    let Ok(n) = socket.read(&mut chunk).await else { return };
                    if n == 0 {
                        return;
                    }
                    buf.extend_from_slice(&chunk[..n]);
                    let text = String::from_utf8_lossy(&buf).to_string();
                    if let Some(end) = text.find("\r\n\r\n") {
                        let length = text[..end]
                            .lines()
                            .find_map(|l| l.to_ascii_lowercase().strip_prefix("content-length:").map(|v| v.trim().parse::<usize>().unwrap_or(0)))
                            .unwrap_or(0);
                        if buf.len() >= end + 4 + length {
                            break text;
                        }
                    }
                };
                let path = head.split_whitespace().nth(1).unwrap_or("/").to_string();
                let (content_type, body) = route(&path);
                let response = format!(
                    "HTTP/1.1 200 OK\r\ncontent-type: {content_type}\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
                    body.len()
                );
                let _ = socket.write_all(response.as_bytes()).await;
                let _ = socket.shutdown().await;
            });
        }
    });
    base
}

fn sse(events: &[Value], named: bool) -> String {
    events
        .iter()
        .map(|e| match named {
            true => format!("event: {}\ndata: {e}\n\n", e["type"].as_str().unwrap()),
            false => format!("data: {e}\n\n"),
        })
        .collect::<String>()
}

fn route(path: &str) -> (&'static str, String) {
    if path.starts_with("/v1/messages") {
        let events = [
            json!({"type":"message_start","message":{"id":"msg_1","type":"message","role":"assistant","content":[],"model":"mock","stop_reason":null,"stop_sequence":null,"usage":{"input_tokens":5,"output_tokens":1}}}),
            json!({"type":"content_block_start","index":0,"content_block":{"type":"text","text":""}}),
            json!({"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"hello "}}),
            json!({"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"anthropic"}}),
            json!({"type":"content_block_stop","index":0}),
            json!({"type":"message_delta","delta":{"stop_reason":"end_turn","stop_sequence":null},"usage":{"output_tokens":3}}),
            json!({"type":"message_stop"}),
        ];
        ("text/event-stream", sse(&events, true))
    } else if path.starts_with("/v1/chat/completions") {
        let chunk = |content: Value, finish: Value| {
            json!({"id":"c1","object":"chat.completion.chunk","created":1,"model":"mock","choices":[{"index":0,"delta":{"content":content},"finish_reason":finish}]})
        };
        let mut body = sse(&[chunk(json!("hello "), Value::Null), chunk(json!("openai"), Value::Null), chunk(Value::Null, json!("stop"))], false);
        body.push_str("data: [DONE]\n\n");
        ("text/event-stream", body)
    } else {
        ("application/json", json!({ "path": path }).to_string())
    }
}

async fn run(dir: &Path, source: &str) -> Value {
    let rt = JsRuntime::new(RuntimeOptions {
        cwd: Some(dir.to_path_buf()),
        ..Default::default()
    })
    .unwrap();
    let file = dir.join(format!("pi-jsrt-npm-test-{}.mjs", uuid::Uuid::new_v4()));
    std::fs::write(&file, source).unwrap();
    let result = rt.import(file.display().to_string()).await;
    let _ = std::fs::remove_file(&file);
    let ns = result.unwrap_or_else(|e| panic!("{e}\n{:?}", e.as_js().and_then(|j| j.stack.clone())));
    ns.get("default").await.unwrap()
}

/// `@anthropic-ai/sdk` (streaming with `messages.stream` and `create({ stream: true })`) and
/// `openai` (streamed chat completions). Exercises the vendored boa_ast fix (computed class
/// method keys) and `FinalizationRegistry` cleanup jobs not blocking the job queue.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn llm_sdks_stream_from_a_mock_server() {
    let Some(dir) = npm_dir() else { return };
    let base = mock_server(route).await;
    let v = run(
        &dir,
        &format!(
            r#"
import Anthropic from "@anthropic-ai/sdk";
import OpenAI from "openai";
const anthropic = new Anthropic({{ apiKey: "sk-test", baseURL: "{base}" }});
const stream = anthropic.messages.stream({{ model: "m", max_tokens: 10, messages: [{{ role: "user", content: "hi" }}] }});
const deltas = [];
stream.on("text", (t) => deltas.push(t));
const final = await stream.finalMessage();
let raw = "";
for await (const event of await anthropic.messages.create({{ model: "m", max_tokens: 10, stream: true, messages: [{{ role: "user", content: "hi" }}] }})) {{
  if (event.type === "content_block_delta") raw += event.delta.text;
}}
const openai = new OpenAI({{ apiKey: "sk-test", baseURL: "{base}/v1" }});
let oai = "";
for await (const chunk of await openai.chat.completions.create({{ model: "m", stream: true, messages: [{{ role: "user", content: "hi" }}] }})) {{
  oai += chunk.choices[0]?.delta?.content ?? "";
}}
export default {{ deltas, text: final.content[0].text, stop: final.stop_reason, raw, oai }};
"#
        ),
    )
    .await;
    assert_eq!(
        v,
        json!({ "deltas": ["hello ", "anthropic"], "text": "hello anthropic", "stop": "end_turn", "raw": "hello anthropic", "oai": "hello openai" })
    );
}

/// execa (child processes through Node streams), glob (fs walking), node-fetch (ES module with a
/// trailing `export { ... }` list, which needs the vendored boa_parser fix).
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn node_tooling_packages() {
    let Some(dir) = npm_dir() else { return };
    let v = run(
        &dir,
        r#"
import { execa } from "execa";
import { glob } from "glob";
import fetch, { Headers as NfHeaders } from "node-fetch";
const echo = await execa("echo", ["hi"]);
const piped = await execa("cat", { input: "from stdin" });
const failed = await execa("sh", ["-c", "exit 3"], { reject: false });
const files = (await glob("node_modules/execa/package.json")).sort();
export default {
  echo: echo.stdout, piped: piped.stdout, failed: failed.exitCode,
  files, fetch: typeof fetch, headers: new NfHeaders({ a: "1" }).get("a"),
};
"#,
    )
    .await;
    assert_eq!(
        v,
        json!({ "echo": "hi", "piped": "from stdin", "failed": 3, "files": ["node_modules/execa/package.json"], "fetch": "function", "headers": "1" })
    );
}
