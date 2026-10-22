//! TCP ops backing `node:net` (and the HTTP server/client built on it in `js/node/http.js`).
//!
//! Listeners and sockets run on tokio. Accepted connections, received data and lifecycle
//! events are posted back to the JS thread and dispatched through
//! `internal.netEvent(id, kind, ...)`; writes from JS go to a per-socket writer task.

use std::cell::RefCell;
use std::collections::HashMap;
use std::net::{SocketAddr, ToSocketAddrs};

use boa_engine::{Context, JsArgs, JsResult, JsValue, js_string};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

use super::{bytes_arg, bytes_to_js, io_code, object};
use crate::engine::{self, OpFn, Poster, arg_f64, arg_string, js_str, state};

enum SockMsg {
    Data(Vec<u8>),
    End,
}

struct SocketCtl {
    tx: mpsc::UnboundedSender<SockMsg>,
    stop: CancellationToken,
}

thread_local! {
    static SOCKETS: RefCell<HashMap<u64, SocketCtl>> = RefCell::new(HashMap::new());
    static SERVERS: RefCell<HashMap<u64, CancellationToken>> = RefCell::new(HashMap::new());
}

fn dispatch(ctx: &mut Context, id: u64, args: Vec<JsValue>) {
    let Some(internal) = engine::try_internal(ctx) else { return };
    let Ok(f) = internal.get(js_string!("netEvent"), ctx) else { return };
    let Some(f) = f.as_function() else { return };
    let mut all = vec![JsValue::from(id as f64)];
    all.extend(args);
    if let Err(e) = f.call(&JsValue::undefined(), &all, ctx) {
        engine::report_uncaught(ctx, e);
    }
}

fn family(addr: &SocketAddr) -> &'static str {
    if addr.is_ipv4() { "IPv4" } else { "IPv6" }
}

/// `{ localAddress, localPort, localFamily, remoteAddress, remotePort, remoteFamily }`
fn endpoints(ctx: &mut Context, local: Option<SocketAddr>, remote: Option<SocketAddr>) -> JsResult<JsValue> {
    let mut entries = Vec::new();
    if let Some(l) = local {
        entries.push(("localAddress", js_str(&l.ip().to_string())));
        entries.push(("localPort", JsValue::from(l.port())));
        entries.push(("localFamily", js_str(family(&l))));
    }
    if let Some(r) = remote {
        entries.push(("remoteAddress", js_str(&r.ip().to_string())));
        entries.push(("remotePort", JsValue::from(r.port())));
        entries.push(("remoteFamily", js_str(family(&r))));
    }
    object(ctx, entries)
}

fn error_object(ctx: &mut Context, e: &std::io::Error, syscall: &str, address: Option<(&str, u16)>) -> JsResult<JsValue> {
    let code = io_code(e);
    let mut message = format!("{syscall} {code}");
    let mut entries = vec![
        ("code", js_str(code)),
        ("errno", JsValue::from(-e.raw_os_error().unwrap_or(0))),
        ("syscall", js_str(syscall)),
    ];
    if let Some((host, port)) = address {
        message.push_str(&format!(" {host}:{port}"));
        entries.push(("address", js_str(host)));
        entries.push(("port", JsValue::from(port)));
    }
    entries.insert(0, ("message", js_str(&message)));
    object(ctx, entries)
}

/// Start the reader and writer tasks of a connected stream and register it under `id`.
fn start_socket(tokio: &tokio::runtime::Handle, poster: Poster, id: u64, stream: tokio::net::TcpStream) -> SocketCtl {
    let (tx, mut rx) = mpsc::unbounded_channel::<SockMsg>();
    let stop = CancellationToken::new();
    let (mut reader, mut writer) = stream.into_split();
    let read_poster = poster.clone();
    let read_stop = stop.clone();
    let read_task = tokio.spawn(async move {
        let mut buf = vec![0u8; 64 * 1024];
        loop {
            let n = tokio::select! {
                _ = read_stop.cancelled() => return false,
                r = reader.read(&mut buf) => r,
            };
            match n {
                Ok(0) => {
                    read_poster.post(move |ctx| dispatch(ctx, id, vec![js_str("end")]));
                    return false;
                }
                Ok(n) => {
                    let chunk = buf[..n].to_vec();
                    read_poster.post(move |ctx| {
                        if let Ok(b) = bytes_to_js(&chunk, ctx) {
                            dispatch(ctx, id, vec![js_str("data"), b]);
                        }
                    });
                }
                Err(e) => {
                    read_poster.post(move |ctx| {
                        if let Ok(err) = error_object(ctx, &e, "read", None) {
                            dispatch(ctx, id, vec![js_str("error"), err]);
                        }
                    });
                    return true;
                }
            }
        }
    });
    let write_poster = poster.clone();
    let write_stop = stop.clone();
    let write_task = tokio.spawn(async move {
        loop {
            let msg = tokio::select! {
                _ = write_stop.cancelled() => return false,
                m = rx.recv() => m,
            };
            match msg {
                Some(SockMsg::Data(data)) => {
                    if let Err(e) = writer.write_all(&data).await {
                        write_poster.post(move |ctx| {
                            if let Ok(err) = error_object(ctx, &e, "write", None) {
                                dispatch(ctx, id, vec![js_str("error"), err]);
                            }
                        });
                        return true;
                    }
                }
                Some(SockMsg::End) | None => {
                    let _ = writer.shutdown().await;
                    return false;
                }
            }
        }
    });
    tokio.spawn(async move {
        // `close` follows both directions: the peer ended (or an error) and JS ended its side
        // (`end()`), or `destroy()` stopped both.
        let read_error = read_task.await.unwrap_or(false);
        let write_error = write_task.await.unwrap_or(false);
        let had_error = read_error || write_error;
        poster.post(move |ctx| {
            SOCKETS.with(|s| s.borrow_mut().remove(&id));
            dispatch(ctx, id, vec![js_str("close"), JsValue::from(had_error)]);
        });
    });
    SocketCtl { tx, stop }
}

/// `ops.netListen(host, port) -> { id, address, port, family } | { error }`
fn op_net_listen(_: &JsValue, args: &[JsValue], ctx: &mut Context) -> JsResult<JsValue> {
    let host = arg_string(args, 0, ctx)?;
    let port = arg_f64(args, 1, ctx)? as u16;
    let host = if host.is_empty() { "0.0.0.0".to_string() } else { host };
    let bound = (host.as_str(), port)
        .to_socket_addrs()
        .and_then(|mut addrs| addrs.next().ok_or_else(|| std::io::Error::from(std::io::ErrorKind::AddrNotAvailable)))
        .and_then(std::net::TcpListener::bind);
    let listener = match bound {
        Ok(l) => l,
        Err(e) => {
            let err = error_object(ctx, &e, "listen", Some((&host, port)))?;
            return object(ctx, vec![("error", err)]);
        }
    };
    let local = listener.local_addr().ok();
    let st = state(ctx);
    if let Err(e) = listener.set_nonblocking(true) {
        let err = error_object(ctx, &e, "listen", Some((&host, port)))?;
        return object(ctx, vec![("error", err)]);
    }
    let listener = {
        let _guard = st.tokio.enter();
        match tokio::net::TcpListener::from_std(listener) {
            Ok(l) => l,
            Err(e) => {
                let err = error_object(ctx, &e, "listen", Some((&host, port)))?;
                return object(ctx, vec![("error", err)]);
            }
        }
    };
    let id = st.next_id();
    let stop = CancellationToken::new();
    SERVERS.with(|s| s.borrow_mut().insert(id, stop.clone()));
    let poster = st.poster();
    let tokio = st.tokio.clone();
    let shared = st.shared.clone();
    st.tokio.spawn(async move {
        loop {
            let accepted = tokio::select! {
                _ = stop.cancelled() => break,
                a = listener.accept() => a,
            };
            let Ok((stream, remote)) = accepted else { continue };
            let _ = stream.set_nodelay(true);
            let local = stream.local_addr().ok();
            let socket_id = shared.next_id();
            let tokio2 = tokio.clone();
            let poster2 = poster.clone();
            poster.post(move |ctx| {
                let ctl = start_socket(&tokio2, poster2, socket_id, stream);
                SOCKETS.with(|s| s.borrow_mut().insert(socket_id, ctl));
                if let Ok(info) = endpoints(ctx, local, Some(remote)) {
                    dispatch(ctx, id, vec![js_str("connection"), JsValue::from(socket_id as f64), info]);
                }
            });
        }
        poster.post(move |ctx| {
            SERVERS.with(|s| s.borrow_mut().remove(&id));
            dispatch(ctx, id, vec![js_str("close")]);
        });
    });
    let local = local.unwrap_or_else(|| SocketAddr::from(([0, 0, 0, 0], port)));
    object(
        ctx,
        vec![
            ("id", JsValue::from(id as f64)),
            ("address", js_str(&local.ip().to_string())),
            ("port", JsValue::from(local.port())),
            ("family", js_str(family(&local))),
        ],
    )
}

/// `ops.netConnect(host, port) -> id`; emits `connect` (with endpoints) or `error`.
fn op_net_connect(_: &JsValue, args: &[JsValue], ctx: &mut Context) -> JsResult<JsValue> {
    let host = arg_string(args, 0, ctx)?;
    let port = arg_f64(args, 1, ctx)? as u16;
    let host = if host.is_empty() { "localhost".to_string() } else { host };
    let st = state(ctx);
    let id = st.next_id();
    let poster = st.poster();
    let tokio = st.tokio.clone();
    st.tokio.spawn(async move {
        match tokio::net::TcpStream::connect((host.as_str(), port)).await {
            Ok(stream) => {
                let _ = stream.set_nodelay(true);
                let local = stream.local_addr().ok();
                let remote = stream.peer_addr().ok();
                let poster2 = poster.clone();
                poster.post(move |ctx| {
                    let ctl = start_socket(&tokio, poster2, id, stream);
                    SOCKETS.with(|s| s.borrow_mut().insert(id, ctl));
                    if let Ok(info) = endpoints(ctx, local, remote) {
                        dispatch(ctx, id, vec![js_str("connect"), info]);
                    }
                });
            }
            Err(e) => {
                poster.post(move |ctx| {
                    if let Ok(err) = error_object(ctx, &e, "connect", Some((&host, port))) {
                        dispatch(ctx, id, vec![js_str("error"), err]);
                        dispatch(ctx, id, vec![js_str("close"), JsValue::from(true)]);
                    }
                });
            }
        }
    });
    Ok(JsValue::from(id as f64))
}

fn op_net_write(_: &JsValue, args: &[JsValue], ctx: &mut Context) -> JsResult<JsValue> {
    let id = arg_f64(args, 0, ctx)? as u64;
    let data = bytes_arg(args.get_or_undefined(1), ctx)?;
    let ok = SOCKETS.with(|s| s.borrow().get(&id).is_some_and(|c| c.tx.send(SockMsg::Data(data)).is_ok()));
    Ok(JsValue::from(ok))
}

fn op_net_end(_: &JsValue, args: &[JsValue], ctx: &mut Context) -> JsResult<JsValue> {
    let id = arg_f64(args, 0, ctx)? as u64;
    SOCKETS.with(|s| {
        if let Some(c) = s.borrow().get(&id) {
            let _ = c.tx.send(SockMsg::End);
        }
    });
    Ok(JsValue::undefined())
}

fn op_net_destroy(_: &JsValue, args: &[JsValue], ctx: &mut Context) -> JsResult<JsValue> {
    let id = arg_f64(args, 0, ctx)? as u64;
    SOCKETS.with(|s| {
        if let Some(c) = s.borrow().get(&id) {
            c.stop.cancel();
        }
    });
    Ok(JsValue::undefined())
}

fn op_net_server_close(_: &JsValue, args: &[JsValue], ctx: &mut Context) -> JsResult<JsValue> {
    let id = arg_f64(args, 0, ctx)? as u64;
    SERVERS.with(|s| {
        if let Some(stop) = s.borrow().get(&id) {
            stop.cancel();
        }
    });
    Ok(JsValue::undefined())
}

/// Close every listener and socket (runtime shutdown).
pub(crate) fn teardown(_ctx: &mut Context) {
    SERVERS.with(|s| s.borrow_mut().drain().for_each(|(_, stop)| stop.cancel()));
    SOCKETS.with(|s| s.borrow_mut().drain().for_each(|(_, c)| c.stop.cancel()));
}

pub(crate) const OPS: &[(&str, usize, OpFn)] = &[
    ("netListen", 2, op_net_listen),
    ("netConnect", 2, op_net_connect),
    ("netWrite", 2, op_net_write),
    ("netEnd", 1, op_net_end),
    ("netDestroy", 1, op_net_destroy),
    ("netServerClose", 1, op_net_server_close),
];

/// `ops.dnsLookup(hostname) -> Promise<[{ address, family }]>` (getaddrinfo via tokio).
fn op_dns_lookup(_: &JsValue, args: &[JsValue], ctx: &mut Context) -> JsResult<JsValue> {
    let host = arg_string(args, 0, ctx)?;
    Ok(engine::spawn_result(
        ctx,
        async move {
            match tokio::net::lookup_host((host.as_str(), 0)).await {
                Ok(addrs) => {
                    let mut seen = Vec::new();
                    for a in addrs {
                        let item = (a.ip().to_string(), if a.is_ipv4() { 4 } else { 6 });
                        if !seen.contains(&item) {
                            seen.push(item);
                        }
                    }
                    Ok(seen)
                }
                Err(e) => {
                    // getaddrinfo's EAI_NONAME / EAI_AGAIN surface as io errors without errno.
                    let code = if e.raw_os_error().is_some() { io_code(&e) } else { "ENOTFOUND" };
                    Err(crate::Error::Js(crate::error::JsException {
                        name: "Error".into(),
                        message: format!("getaddrinfo {code} {host}"),
                        stack: None,
                        code: Some(code.into()),
                    }))
                }
            }
        },
        |list, ctx| {
            let mut items = Vec::new();
            for (address, family) in list {
                items.push(object(ctx, vec![("address", js_str(&address)), ("family", JsValue::from(family))])?);
            }
            Ok(super::array(ctx, items))
        },
    ))
}

pub(crate) const DNS_OPS: &[(&str, usize, OpFn)] = &[("dnsLookup", 1, op_dns_lookup)];
