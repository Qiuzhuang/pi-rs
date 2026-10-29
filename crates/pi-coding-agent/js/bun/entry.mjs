// Entry point of the bun backend (host_bun.rs): runs pi's JavaScript extensions in a bun
// subprocess. Line-delimited JSON protocol:
//
//   stdin (fd 0): Rust -> JS requests          {"id", "type": "call" | "import", ...}
//   stdout (fd 1): JS -> Rust messages         host calls, responses, jsout, exit
//   fd (FIFO):    synchronous host responses   {"id", "type": "hostResp", ...}
//
// The sync channel is a FIFO whose path arrives in PI_RS_HOST_FIFO. The child opens it
// itself after startup (bun renumbers inherited fds, so a passed fd would not survive).
//
// __piHost.call is synchronous. While it waits for the FIFO reply it PUMPS stdin: frames
// that arrive are dispatched through handleLine, so a synchronous host-call handler may
// call back into JS (hasMethod, render, ...). That re-entrancy matches the in-process Boa
// backend. Only frames answered synchronously can be served while pumping: a handler that
// returns a promise responds after the outer call unwinds, so host-call handlers must not
// depend on async nested results (same rule as Boa, where re-entry runs on one stack).
import { openSync, readSync, writeFileSync, constants as FS } from "node:fs";
import { Readable } from "node:stream";
import { StringDecoder } from "node:string_decoder";

const FD_SYNC =
	process.env.PI_RS_HOST_FIFO !== undefined
		? openSync(process.env.PI_RS_HOST_FIFO, FS.O_RDONLY | FS.O_NONBLOCK)
		: null;
// Non-blocking view of stdin (a fresh file description on the same pipe; the blocking
// stream reader is unaffected). Used to serve nested calls while a sync call is waiting.
let FD_IN_NB = -1;
try {
	FD_IN_NB = openSync("/proc/self/fd/0", FS.O_RDONLY | FS.O_NONBLOCK);
} catch {}
const SLEEP = new Int32Array(new SharedArrayBuffer(4));
let pumpDepth = 0;
const HANDLE_KEY = "$jsHandle";

function line(obj) {
	writeFileSync(1, JSON.stringify(obj) + "\n");
}

// JS stdout (console.*, process.stdout.write) goes to Rust's stdout sink as tagged messages,
// so fd 1 stays clean for the protocol.
function jsOut(data) {
	line({ type: "jsout", data: String(data) });
}
{
	const fmt = (a) => {
		if (typeof a === "string") return a;
		if (a instanceof Error) return a.stack ?? String(a);
		try {
			return JSON.stringify(a, (_k, v) =>
				typeof v === "function" ? "[Function]" : typeof v === "symbol" ? String(v) : v);
		} catch {
			return String(a);
		}
	};
	for (const m of ["log", "info", "warn", "error", "debug", "trace"]) {
		console[m] = (...args) => jsOut(args.map(fmt).join(" ") + "\n");
	}
	console.dir = (a) => jsOut(fmt(a) + "\n");
	console.table = (a) => jsOut(fmt(a) + "\n");
	process.stdout = {
		write: (d) => {
			jsOut(d);
			return true;
		},
		end: () => {},
		rows: process.stdout.rows,
		columns: process.stdout.columns,
	};
}

// ---- JS value handles -------------------------------------------------------------
// Functions and objects live here; Rust sees {"$jsHandle": id} markers.
const handles = new Map();
let nextHandle = 1;
function mark(value) {
	if (value === null || value === undefined) return value;
	const t = typeof value;
	if (t === "function" || t === "object") {
		const id = nextHandle++;
		handles.set(id, value);
		return { [HANDLE_KEY]: id };
	}
	return value;
}
function unmark(value) {
	if (value === null || value === undefined || typeof value !== "object") return value;
	if (!Array.isArray(value) && Object.keys(value).length === 1 && value[HANDLE_KEY] !== undefined) {
		return handles.get(value[HANDLE_KEY]);
	}
	if (Array.isArray(value)) return value.map(unmark);
	const out = {};
	for (const k of Object.keys(value)) out[k] = unmark(value[k]);
	return out;
}
globalThis.__piHandles = { mark, get: (id) => handles.get(id) };

// Convert a JS value to plain JSON, replacing functions with handle markers (class instances
// become plain objects of their own enumerable properties, like the Boa backend's convert).
function toPlain(value, seen = new WeakSet()) {
	if (value === null || value === undefined) return value;
	const t = typeof value;
	if (t === "function") {
		if (value.__piHostCb !== undefined) return { $piHostCb: value.__piHostCb };
		return mark(value);
	}
	if (t !== "object") return t === "symbol" ? String(value) : value;
	if (seen.has(value)) throw new Error("circular structure in host call");
	seen.add(value);
	if (Array.isArray(value)) return value.map((v) => toPlain(v, seen));
	const out = {};
	for (const k of Object.keys(value)) out[k] = toPlain(value[k], seen);
	return out;
}

// ---- host calls -------------------------------------------------------------------
let nextMsg = 1;
const hostPending = new Map();
// Synchronous responses can arrive out of stack order: a nested host call whose op
// answers without re-entering JS finishes while an outer call is still blocked, so its
// line reaches the FIFO first. The FIFO is one byte stream shared by every waiting call,
// so it is parsed by ONE shared accumulator: every complete line is routed by id, early
// arrivals wait in `syncArrived`, and a half line stays in the accumulator for whoever
// reads next (a per-call buffer would lose it when a nested call returns first).
const syncArrived = new Map();
// Raw bytes until a newline: decoding a chunk boundary in the middle of a multi-byte
// character (→, box glyphs in completion rows) would corrupt the JSON and lose a response.
let fifoAcc = Buffer.alloc(0);
const fifoTmp = Buffer.alloc(65536);

function hostFinish(resp) {
	if (resp.error) {
		const e = new Error(resp.error.message ?? "host error");
		if (resp.error.stack) e.stack = resp.error.stack;
		throw e;
	}
	return unmark(resp.result);
}

// Reads FIFO until EAGAIN, parsing every complete line. Returns the response for `target`
// when this pass contains it; routes other lines (stashed responses, defensive frames).
function fifoDrain(target) {
	for (;;) {
		const cut = fifoAcc.indexOf(0x0a);
		if (cut >= 0) {
			const text = fifoAcc.toString("utf8", 0, cut);
			fifoAcc = fifoAcc.subarray(cut + 1);
			const resp = JSON.parse(text);
			if (resp.id === target) return resp;
			if (resp.type === "host" || resp.type === "hostAsync" || resp.type === "call") {
				// Defensive: routed frames never use the sync channel; keep them moving.
				handleLine(text);
				continue;
			}
			syncArrived.set(resp.id, resp);
			continue;
		}
		if (fifoAcc.length > 16 * 1024 * 1024) throw new Error("host response too large");
		let n;
		try {
			n = readSync(FD_SYNC, fifoTmp, 0, fifoTmp.length, null);
		} catch (e) {
			if (e.code === "EAGAIN") return undefined;
			throw e;
		}
		if (n === 0) throw new Error("pi extension host closed");
		const merged = Buffer.allocUnsafe(fifoAcc.length + n);
		fifoAcc.copy(merged, 0);
		fifoTmp.copy(merged, fifoAcc.length, 0, n);
		fifoAcc = merged;
	}
}

function hostCall(op, payload) {
	if (FD_SYNC === null) throw new Error("no synchronous host channel (PI_RS_HOST_FIFO)");
	const id = nextMsg++;
	line({ id, type: "host", op, payload: toPlain(payload) });
	pumpDepth++;
	try {
		for (;;) {
			const mine = syncArrived.get(id);
			if (mine !== undefined) {
				syncArrived.delete(id);
				return hostFinish(mine);
			}
			const resp = fifoDrain(id);
			if (resp !== undefined) return hostFinish(resp);
			// Nothing on the sync channel yet: serve whatever the host queued on stdin,
			// then pace briefly. Atomics.wait sleeps this thread only (there is none other).
			pumpStdin();
			Atomics.wait(SLEEP, 0, 0, 1);
		}
	} finally {
		pumpDepth--;
	}
}

function hostCallAsync(op, payload) {
	const id = nextMsg++;
	line({ id, type: "hostAsync", op, payload: toPlain(payload) });
	return new Promise((resolve, reject) => {
		hostPending.set(id, {
			resolve: (v) => { resolve(v); },
			reject: (e) => { reject(e); },
		});
	});
}
globalThis.__piHost = { call: hostCall, callAsync: hostCallAsync };
// ---- protocol (stdin, event-driven) ------------------------------------------------
let buf = "";
function feedStdin(data) {
	buf += data;
	let i;
	while ((i = buf.indexOf("\n")) >= 0) {
		const l = buf.slice(0, i);
		buf = buf.slice(i + 1);
		if (l) handleLine(l);
	}
}
// Serve Rust -> JS frames while a synchronous host call blocks the event loop.
const stdinDecoder = new StringDecoder("utf8");
function pumpStdin() {
	if (FD_IN_NB < 0) return;
	const ch = Buffer.alloc(65536);
	for (;;) {
		let n;
		try {
			n = readSync(FD_IN_NB, ch, 0, ch.length, null);
		} catch (e) {
			if (e.code === "EAGAIN") return;
			throw e;
		}
		if (n === 0) throw new Error("pi extension host closed");
		feedStdin(stdinDecoder.write(ch.subarray(0, n)));
	}
}
process.stdin.setEncoding("utf8");
process.stdin.on("data", (chunk) => feedStdin(chunk));
function handleLine(l) {
	let m;
	try {
		m = JSON.parse(l);
	} catch {
		return;
	}
	if (m.type === "hostResp") {
		const p = hostPending.get(m.id);
		if (!p) return;
		hostPending.delete(m.id);
		if (m.error) {
			const e = new Error(m.error.message ?? "host error");
			if (m.error.stack) e.stack = m.error.stack;
			p.reject(e);
		} else {
			p.resolve(unmark(m.result));
		}
		return;
	}
	if (m.type === "call") {
		const h = handles.get(m.handle);
		if (h === undefined) {
			return line({ id: m.id, error: { message: `unknown JS handle ${m.handle}` } });
		}
		let r;
		try {
			const args = (m.args ?? []).map(unmark);
			r = m.method !== undefined ? h[m.method](...args) : h(...args);
		} catch (e) {
			return line({ id: m.id, error: { message: String(e?.message ?? e), stack: e?.stack } });
		}
		if (r && typeof r.then === "function") {
			r.then(
				(v) => line({ id: m.id, result: toPlain(v) }),
				(e) => line({ id: m.id, error: { message: String(e?.message ?? e), stack: e?.stack } }),
			);
		} else {
			line({ id: m.id, result: toPlain(r) });
		}
		return;
	}
	if (m.type === "get") {
		const h = handles.get(m.handle);
		if (h === undefined) {
			return line({ id: m.id, error: { message: `unknown JS handle ${m.handle}` } });
		}
		try {
			line({ id: m.id, result: toPlain(h[m.name]) });
		} catch (e) {
			line({ id: m.id, error: { message: String(e?.message ?? e), stack: e?.stack } });
		}
		return;
	}
	if (m.type === "import") {
		import(m.specifier)
			.then((ns) => {
				const id = nextHandle++;
				handles.set(id, ns);
				line({ id: m.id, result: { [HANDLE_KEY]: id } });
			})
			.catch((e) => {
				line({ id: m.id, error: { message: String(e?.message ?? e), stack: e?.stack } });
			});
		return;
	}
	line({ id: m.id ?? 0, error: { message: `unknown message type ${m.type}` } });
}

// ---- process environment -----------------------------------------------------------
// Extensions never read the protocol pipe or terminate the process.
process.argv = ["pi"];
process.execArgv = [];
Object.defineProperty(process, "stdin", {
	configurable: true,
	value: Object.assign(new Readable({ read() {} }), { isTTY: false }),
});
const failExit = (code) => {
	line({ type: "exit", code: code ?? 0 });
	writeFileSync(2, `process.exit(${code}) is not supported in pi extensions\n`);
};
process.exit = (code) => failExit(code);
const failUncaught = (e) => {
	writeFileSync(2, `Uncaught ${e?.stack ?? e}\n`);
};
process.on("uncaughtException", failUncaught);
process.on("unhandledRejection", (e) => {
	writeFileSync(2, `Unhandled rejection ${e?.stack ?? e}\n`);
});

line({ type: "ready" });
