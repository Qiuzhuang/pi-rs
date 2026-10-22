//! Node modules backed by native Rust code: `node:zlib`, `node:crypto` (ciphers, keys,
//! signatures, KDFs, WebCrypto), `node:tls`/`https`, and the `WebAssembly` global.

use pi_jsrt::{JsRuntime, RuntimeOptions};
use serde_json::{Value, json};

async fn run(source: &str) -> Value {
    let rt = JsRuntime::new(RuntimeOptions::default()).unwrap();
    let ns = tokio::time::timeout(std::time::Duration::from_secs(60), rt.eval_module(source, "native-test.mjs"))
        .await
        .expect("timed out")
        .unwrap_or_else(|e| panic!("{e}\n{:?}", e.as_js().and_then(|j| j.stack.clone())));
    ns.get("default").await.unwrap()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn zlib_one_shot_callback_promise_and_errors() {
    let v = run(r#"
        import zlib from "node:zlib";
        import { promisify } from "node:util";
        const text = "hello hello hello hello zlib ".repeat(50);
        const roundTrips = {};
        for (const [c, d] of [["deflate", "inflate"], ["gzip", "gunzip"], ["deflateRaw", "inflateRaw"], ["gzip", "unzip"], ["deflate", "unzip"], ["brotliCompress", "brotliDecompress"]]) {
            const packed = zlib[`${c}Sync`](text);
            roundTrips[`${c}/${d}`] = zlib[`${d}Sync`](packed).toString() === text && packed.length < text.length;
        }
        const viaCallback = await new Promise((resolve, reject) => zlib.gzip(text, (err, buf) => err ? reject(err) : zlib.gunzip(buf, (e, out) => e ? reject(e) : resolve(out.toString()))));
        const viaPromise = (await promisify(zlib.inflate)(await promisify(zlib.deflate)(Buffer.from("promised")))).toString();
        const err = (f) => { try { f(); return "no error"; } catch (e) { return [e.message, e.code, e.errno]; } };
        const cbErr = await new Promise((resolve) => zlib.inflate(Buffer.from("nope"), (e) => resolve(e.code)));
        export default {
            roundTrips, viaCallback: viaCallback === text, viaPromise,
            hex: zlib.deflateSync("abc").toString("hex"),
            gzipHex: zlib.gzipSync("abc").toString("hex"),
            badHeader: err(() => zlib.inflateSync(Buffer.from("hello"))),
            truncated: err(() => zlib.inflateSync(zlib.deflateSync("abc").subarray(0, 5))),
            badArg: err(() => zlib.gzipSync(42)),
            cbErr,
            crc: [zlib.crc32("hello"), zlib.crc32("world", zlib.crc32("hello "))],
            consts: [zlib.constants.Z_SYNC_FLUSH, zlib.constants.BROTLI_PARAM_QUALITY, zlib.Z_BEST_COMPRESSION],
        };
    "#)
    .await;
    assert_eq!(
        v,
        json!({
            "roundTrips": {"deflate/inflate": true, "gzip/gunzip": true, "deflateRaw/inflateRaw": true, "gzip/unzip": true, "deflate/unzip": true, "brotliCompress/brotliDecompress": true},
            "viaCallback": true, "viaPromise": "promised",
            "hex": "789c4b4c4a0600024d0127",
            "gzipHex": "1f8b08000000000000034b4c4a0600c241243503000000",
            "badHeader": ["incorrect header check", "Z_DATA_ERROR", -3],
            "truncated": ["unexpected end of file", "Z_BUF_ERROR", -5],
            "badArg": ["The \"buffer\" argument must be of type string or an instance of Buffer, TypedArray, DataView, or ArrayBuffer. Received type number (42)", "ERR_INVALID_ARG_TYPE", null],
            "cbErr": "Z_DATA_ERROR",
            "crc": [907060870, 222957957],
            "consts": [2, 1, 9],
        })
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn zlib_streams_flush_and_internal_handle() {
    let v = run(r#"
        import zlib from "node:zlib";
        import { pipeline } from "node:stream/promises";
        import { Readable, Writable } from "node:stream";
        const chunks = [];
        const big = Buffer.alloc(100000, "abcdefghij");
        await pipeline(Readable.from([big.subarray(0, 50000), big.subarray(50000)]), zlib.createGzip(), zlib.createGunzip({ chunkSize: 1024 }),
            new Writable({ write(c, e, cb) { chunks.push(c); cb(); } }));
        const out = Buffer.concat(chunks);
        // Sync flush: everything written so far becomes decodable.
        const deflate = zlib.createDeflate();
        const flushed = [];
        deflate.on("data", (d) => flushed.push(d));
        deflate.write("partial data");
        await new Promise((r) => deflate.flush(zlib.constants.Z_SYNC_FLUSH, r));
        const partial = zlib.inflateSync(Buffer.concat(flushed), { finishFlush: zlib.constants.Z_SYNC_FLUSH }).toString();
        // Error event on corrupt input.
        const streamErr = await new Promise((resolve) => {
            const g = zlib.createGunzip();
            g.on("error", (e) => resolve(e.message));
            g.end(Buffer.from("not gzip at all"));
        });
        // Node's internal handle as used by ssh2 (Z_PARTIAL_FLUSH, bounded output buffers).
        const Handle = zlib.createInflate()._handle.constructor;
        const state = new Uint32Array(2);
        const def = new Handle(zlib.constants.DEFLATE);
        def.init(15, -1, 8, 0, state, () => {}, undefined);
        const input = Buffer.from("ssh packet payload ".repeat(20));
        const packets = [];
        let off = 0;
        for (;;) {
            const outBuf = Buffer.alloc(16);
            def.writeSync(zlib.constants.Z_PARTIAL_FLUSH, input, off, off === 0 ? input.length : 0, outBuf, 0, 16);
            packets.push(outBuf.subarray(0, 16 - state[0]));
            if (state[0] !== 0) break;
            off = input.length;
        }
        const inf = new Handle(zlib.constants.INFLATE);
        inf.init(15, -1, 8, 0, state, () => {}, undefined);
        const compressed = Buffer.concat(packets);
        const outBuf = Buffer.alloc(1000);
        inf.writeSync(zlib.constants.Z_PARTIAL_FLUSH, compressed, 0, compressed.length, outBuf, 0, 1000);
        export default {
            same: out.equals(big), chunkMax: Math.max(...chunks.map((c) => c.length)),
            partial, streamErr,
            handle: outBuf.subarray(0, 1000 - state[0]).toString() === input.toString(),
            pieces: packets.length > 1,
        };
    "#)
    .await;
    assert_eq!(v, json!({ "same": true, "chunkMax": 1024, "partial": "partial data", "streamErr": "incorrect header check", "handle": true, "pieces": true }));
}
