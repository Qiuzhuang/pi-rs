//! `node:net` and `node:http` over real TCP sockets (loopback).

use pi_jsrt::{JsRuntime, RuntimeOptions};
use serde_json::json;

async fn run(source: &str) -> serde_json::Value {
    let rt = JsRuntime::new(RuntimeOptions::default()).unwrap();
    let ns = tokio::time::timeout(std::time::Duration::from_secs(30), rt.eval_module(source, "net-test.mjs"))
        .await
        .expect("timed out")
        .unwrap_or_else(|e| panic!("{e}\n{:?}", e.as_js().and_then(|j| j.stack.clone())));
    ns.get("default").await.unwrap()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn tcp_echo_server_and_client() {
    let v = run(r#"
        import net from "node:net";
        const events = [];
        const server = net.createServer((socket) => {
            events.push(`server:connection:${socket.remoteAddress}`);
            socket.on("data", (d) => socket.write(`echo:${d}`));
            socket.on("end", () => { events.push("server:end"); socket.end(); });
        });
        await new Promise((r) => server.listen(0, "127.0.0.1", r));
        const { port, address, family } = server.address();
        const client = net.connect(port, "127.0.0.1");
        client.setEncoding("utf8");
        let received = "";
        client.on("data", (d) => { received += d; if (received === "echo:hello") client.end(); });
        await new Promise((r) => client.on("connect", r));
        client.write("hello");
        await new Promise((r) => client.on("close", r));
        await new Promise((r) => server.close(r));
        const refused = await new Promise((resolve) => {
            const s = net.connect(port, "127.0.0.1");
            s.on("error", (e) => resolve(e.code));
        });
        export default {
            received, address, family, ok: port > 0, events, refused,
            ip: [net.isIP("127.0.0.1"), net.isIP("::1"), net.isIP("fe80::1%eth0"), net.isIP("::ffff:1.2.3.4"), net.isIP("x"), net.isIPv6("1:2:3:4:5:6:7:8:9")],
        };
    "#)
    .await;
    assert_eq!(
        v,
        json!({
            "received": "echo:hello", "address": "127.0.0.1", "family": "IPv4", "ok": true,
            "events": ["server:connection:127.0.0.1", "server:end"], "refused": "ECONNREFUSED",
            "ip": [4, 6, 6, 6, 0, false]
        })
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn http_server_client_and_connect_tunnel() {
    let v = run(r#"
        import http from "node:http";
        import net from "node:net";
        const upstream = net.createServer((s) => s.on("data", (d) => s.write(`tunnel:${d}`)));
        await new Promise((r) => upstream.listen(0, "127.0.0.1", r));
        const server = http.createServer(async (req, res) => {
            let body = "";
            for await (const chunk of req) body += chunk;
            if (req.url === "/chunked") {
                res.writeHead(201, { "x-test": "yes" });
                res.write("part1-");
                res.end("part2");
                return;
            }
            res.setHeader("content-type", "application/json");
            res.end(JSON.stringify({ method: req.method, url: req.url, body, host: req.headers.host, te: req.headers["transfer-encoding"] ?? null }));
        });
        server.on("connect", (req, socket, head) => {
            const [host, port] = req.url.split(":");
            const upstreamSocket = net.connect(Number(port), host, () => {
                socket.write("HTTP/1.1 200 Connection Established\r\n\r\n");
                upstreamSocket.pipe(socket);
                socket.pipe(upstreamSocket);
            });
        });
        await new Promise((r) => server.listen(0, "127.0.0.1", r));
        const { port } = server.address();
        const fetchText = (path, method = "GET", body) => new Promise((resolve, reject) => {
            const req = http.request({ host: "127.0.0.1", port, path, method, headers: body ? { "content-type": "text/plain" } : {} }, (res) => {
                res.setEncoding("utf8");
                let text = "";
                res.on("data", (d) => (text += d));
                res.on("end", () => resolve({ status: res.statusCode, headers: res.headers, text }));
            });
            req.on("error", reject);
            if (body) req.write(body);
            req.end();
        });
        const post = await fetchText("/echo?x=1", "POST", "hi there");
        const chunked = await fetchText("/chunked");
        const viaFetch = await (await fetch(`http://127.0.0.1:${port}/f`, { method: "PUT", body: "abc" })).json();
        const tunnel = await new Promise((resolve, reject) => {
            const req = http.request({ host: "127.0.0.1", port, method: "CONNECT", path: `127.0.0.1:${upstream.address().port}` });
            req.on("connect", (res, socket) => {
                socket.setEncoding("utf8");
                socket.on("data", (d) => { resolve(`${res.statusCode} ${d}`); socket.destroy(); });
                socket.write("ping");
            });
            req.on("error", reject);
            req.end();
        });
        server.close();
        upstream.close();
        export default { post: JSON.parse(post.text), postStatus: post.status, chunked: [chunked.status, chunked.headers["x-test"], chunked.headers["transfer-encoding"], chunked.text], viaFetch, tunnel };
    "#)
    .await;
    assert_eq!(v["post"], json!({ "method": "POST", "url": "/echo?x=1", "body": "hi there", "host": v["post"]["host"], "te": "chunked" }));
    assert!(v["post"]["host"].as_str().unwrap().starts_with("127.0.0.1:"));
    assert_eq!(v["postStatus"], json!(200));
    assert_eq!(v["chunked"], json!([201, "yes", "chunked", "part1-part2"]));
    assert_eq!(v["viaFetch"]["method"], json!("PUT"));
    assert_eq!(v["viaFetch"]["body"], json!("abc"));
    assert_eq!(v["tunnel"], json!("200 tunnel:ping"));
}
