#![cfg(unix)]

use std::sync::Arc;
use std::time::{Duration, Instant};

use parking_lot::Mutex;
use pi_mcp::{
    ConnectionState, McpClient, McpClientOptions, McpRequestOptions, McpTransport, StdioTransport,
    StdioTransportOptions,
};
use serde_json::json;

fn fixture(name: &str) -> String {
    format!("{}/tests/fixtures/{name}", env!("CARGO_MANIFEST_DIR"))
}

#[tokio::test]
async fn connects_to_a_newline_delimited_mcp_server_and_captures_stderr() {
    let stderr: Arc<Mutex<Vec<String>>> = Arc::default();
    let seen = stderr.clone();
    let transport = StdioTransport::new(
        StdioTransportOptions::new("sh")
            .args([fixture("stdio-server.sh")])
            .on_stderr(move |chunk| seen.lock().push(chunk.to_string())),
    );
    let client = McpClient::new(McpClientOptions::new("stdio-test", "1.0.0"));
    client.connect(Arc::new(transport.clone())).await.unwrap();
    assert_eq!(
        serde_json::to_value(
            client
                .list_tools(McpRequestOptions::default())
                .await
                .unwrap()
        )
        .unwrap(),
        json!([{ "name": "echo", "inputSchema": { "type": "object" } }])
    );
    let result = client
        .call_tool(
            "echo",
            json!({ "text": "hello" }).as_object().cloned(),
            McpRequestOptions::default(),
        )
        .await
        .unwrap();
    assert_eq!(
        serde_json::to_value(result).unwrap(),
        json!({ "content": [{ "type": "text", "text": "hello" }] })
    );
    assert!(transport.pid().is_some());
    tokio::time::sleep(Duration::from_millis(10)).await;
    assert!(stderr.lock().join("").contains("stdio fixture ready"));
    assert!(transport.stderr().contains("stdio fixture ready"));
    client.close().await.unwrap();
    assert_eq!(client.connection_state(), ConnectionState::Closed);
}

#[tokio::test]
async fn passes_env_and_cwd_and_reports_spawn_failures() {
    let directory = std::env::temp_dir();
    let transport = StdioTransport::new(
        StdioTransportOptions::new("sh")
            .args([
                "-c",
                "echo \"$PI_MCP_TEST_VALUE $(pwd)\" >&2; exec sh \"$0\"",
                &fixture("stdio-server.sh"),
            ])
            .cwd(&directory)
            .env("PI_MCP_TEST_VALUE", "configured"),
    );
    let client = McpClient::new(McpClientOptions::new("stdio-test", "1.0.0"));
    client.connect(Arc::new(transport.clone())).await.unwrap();
    client.ping(McpRequestOptions::default()).await.unwrap();
    let canonical = std::fs::canonicalize(&directory).unwrap();
    let stderr = transport.stderr();
    assert!(stderr.contains("configured"), "{stderr}");
    assert!(
        stderr.contains(&*canonical.to_string_lossy())
            || stderr.contains(&*directory.to_string_lossy()),
        "{stderr}"
    );
    client.close().await.unwrap();

    let missing = StdioTransport::new(StdioTransportOptions::new(
        "pi-mcp-definitely-missing-command",
    ));
    let client = McpClient::new(McpClientOptions::new("stdio-test", "1.0.0"));
    let error = client.connect(Arc::new(missing)).await.unwrap_err();
    assert_eq!(
        error.to_string(),
        "spawn pi-mcp-definitely-missing-command ENOENT"
    );
    assert_eq!(client.connection_state(), ConnectionState::Closed);
}

#[tokio::test]
async fn reports_invalid_lines_and_closes_when_the_server_exits() {
    let transport = StdioTransport::new(
        StdioTransportOptions::new("sh").args(["-c", "echo 'not json'; printf '{\"jsonrpc\"'"]),
    );
    let errors: Arc<Mutex<Vec<String>>> = Arc::default();
    let seen = errors.clone();
    let _ = transport
        .events()
        .on_error(move |error| seen.lock().push(error.to_string()));
    let (closed, wait) = tokio::sync::oneshot::channel::<()>();
    let closed = Mutex::new(Some(closed));
    let _ = transport.events().on_close(move || {
        if let Some(closed) = closed.lock().take() {
            let _ = closed.send(());
        }
    });
    transport.start().await.unwrap();
    tokio::time::timeout(Duration::from_secs(5), wait)
        .await
        .unwrap()
        .unwrap();
    let errors = errors.lock().clone();
    assert_eq!(errors.len(), 2, "{errors:?}");
    assert_eq!(
        errors[1],
        "MCP stdio server closed with an incomplete JSON-RPC message"
    );
}

/// Whether `pid` is gone (or a zombie nobody reaped yet).
fn process_gone(pid: i32) -> bool {
    // SAFETY: kill with signal 0 only checks for existence.
    if unsafe { libc::kill(pid, 0) } != 0 {
        return true;
    }
    std::fs::read_to_string(format!("/proc/{pid}/stat"))
        .map(|stat| {
            stat.rsplit(')')
                .next()
                .is_some_and(|rest| rest.trim_start().starts_with('Z'))
        })
        .unwrap_or(false)
}

#[tokio::test]
async fn kills_a_server_that_ignores_shutdown_including_its_children() {
    let transport = StdioTransport::new(
        StdioTransportOptions::new("sh")
            .args([fixture("stubborn-server.sh")])
            .close_timeout_ms(100),
    );
    let client = McpClient::new(McpClientOptions::new("stdio-test", "1.0.0"));
    client.connect(Arc::new(transport.clone())).await.unwrap();
    let mut grandchild = None;
    for _ in 0..100 {
        let stderr = transport.stderr();
        if let Some(pid) = stderr
            .split_whitespace()
            .skip_while(|word| *word != "grandchild")
            .nth(1)
        {
            grandchild = pid.parse::<i32>().ok();
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    let grandchild = grandchild.expect("grandchild pid on stderr");

    let started = Instant::now();
    client.close().await.unwrap();
    assert!(started.elapsed() < Duration::from_secs(5));
    let mut gone = false;
    for _ in 0..100 {
        if process_gone(grandchild) {
            gone = true;
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    assert!(gone, "grandchild {grandchild} survived");
}
