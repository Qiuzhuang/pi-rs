// node:child_process

const EventEmitter = internal.EventEmitter;
const Buffer = internal.Buffer;
const children = new Map();

function normalizeStdio(stdio) {
  if (stdio === undefined || stdio === null) return ["pipe", "pipe", "pipe"];
  if (typeof stdio === "string") return [stdio, stdio, stdio];
  const out = [];
  for (let i = 0; i < 3; i++) {
    const s = stdio[i];
    if (s === undefined || s === null || s === "pipe" || s === "overlapped") out.push("pipe");
    else if (s === "inherit" || s === i || s === process.stdin || s === process.stdout || s === process.stderr) out.push("inherit");
    else if (s === "ignore") out.push("ignore");
    else out.push("pipe");
  }
  return out;
}

function envPairs(env) {
  const out = [];
  for (const [k, v] of Object.entries(env ?? process.env)) {
    if (v === undefined) continue;
    out.push(k, String(v));
  }
  return out;
}

function resolveCwd(cwd) {
  if (cwd === undefined || cwd === null) return process.cwd();
  if (cwd instanceof internal.URL) return decodeURIComponent(cwd.pathname);
  return internal.path.resolve(String(cwd));
}

function normalizeSpawnArgs(command, args, options) {
  if (!Array.isArray(args)) {
    options = args ?? {};
    args = [];
  }
  options = { ...(options ?? {}) };
  let file = String(command);
  let argv = args.map(String);
  if (options.shell) {
    const cmd = [file, ...argv].join(" ");
    file = typeof options.shell === "string" ? options.shell : "/bin/sh";
    argv = ["-c", cmd];
  }
  return { file, args: argv, options };
}

function spawnError(file, args, code, errno, syscall = "spawn") {
  const err = new Error(`${syscall} ${file} ${code}`);
  err.errno = errno;
  err.code = code;
  err.syscall = `${syscall} ${file}`;
  err.path = file;
  err.spawnargs = args;
  return err;
}

class ChildProcess extends EventEmitter {
  constructor() {
    super();
    this.pid = undefined;
    this.exitCode = null;
    this.signalCode = null;
    this.killed = false;
    this.connected = false;
    this.stdin = null;
    this.stdout = null;
    this.stderr = null;
    this.stdio = [null, null, null];
    this.spawnfile = undefined;
    this.spawnargs = [];
  }
  kill(signal = "SIGTERM") {
    if (this._id === undefined || this.exitCode !== null || this.signalCode !== null) return false;
    const sig = typeof signal === "number" ? signal : internal.signals[signal];
    if (sig === undefined) {
      const err = new TypeError(`Unknown signal: ${signal}`);
      err.code = "ERR_UNKNOWN_SIGNAL";
      throw err;
    }
    const ok = ops.childKill(this._id, sig);
    if (ok) this.killed = true;
    return ok;
  }
  ref() {
    return this;
  }
  unref() {
    return this;
  }
  disconnect() {}
  send() {
    return false;
  }
  [Symbol.dispose]() {
    this.kill();
  }
}

internal.childEvent = (id, kind, a, b) => {
  const child = children.get(id);
  if (!child) return;
  switch (kind) {
    case "stdout":
      child.stdout?.push(Buffer.from(a.buffer, a.byteOffset, a.byteLength));
      break;
    case "stderr":
      child.stderr?.push(Buffer.from(a.buffer, a.byteOffset, a.byteLength));
      break;
    case "exit":
      child.exitCode = a;
      child.signalCode = b;
      if (child._timeout) clearTimeout(child._timeout);
      child.emit("exit", a, b);
      break;
    case "close":
      children.delete(id);
      child.stdout?.push(null);
      child.stderr?.push(null);
      // Let stream 'end' events flush before 'close'.
      setImmediate(() => child.emit("close", a, b));
      break;
  }
};

function spawn(command, args, options) {
  const n = normalizeSpawnArgs(command, args, options);
  const opts = n.options;
  const { Readable, Writable } = internal.stream;
  const child = new ChildProcess();
  const stdio = normalizeStdio(opts.stdio);
  child.spawnfile = n.file;
  child.spawnargs = [opts.argv0 ?? n.file, ...n.args];
  if (opts.signal?.aborted) {
    process.nextTick(() => child.emit("error", Object.assign(new internal.DOMException("The operation was aborted", "AbortError"), { code: "ABORT_ERR" })));
    return child;
  }
  const res = ops.spawn(n.file, n.args, {
    cwd: resolveCwd(opts.cwd),
    env: envPairs(opts.env),
    stdio,
    detached: Boolean(opts.detached),
    argv0: opts.argv0,
  });
  if (res.error) {
    process.nextTick(() => {
      child.emit("error", spawnError(n.file, n.args, res.code, res.errno));
      child.emit("close", -2, null);
    });
    return child;
  }
  child._id = res.id;
  child.pid = res.pid;
  children.set(res.id, child);
  if (stdio[0] === "pipe") {
    child.stdin = new Writable({
      write(chunk, encoding, cb) {
        if (!ops.childWrite(res.id, chunk)) {
          const err = new Error("write EPIPE");
          err.code = "EPIPE";
          err.syscall = "write";
          cb(err);
          return;
        }
        cb();
      },
      final(cb) {
        ops.childCloseStdin(res.id);
        cb();
      },
      destroy(err, cb) {
        ops.childCloseStdin(res.id);
        cb(err);
      },
    });
    child.stdin.on("error", () => {});
  }
  if (stdio[1] === "pipe") child.stdout = new Readable({ read() {} });
  if (stdio[2] === "pipe") child.stderr = new Readable({ read() {} });
  child.stdio = [child.stdin, child.stdout, child.stderr];
  if (opts.timeout > 0) {
    child._timeout = setTimeout(() => child.kill(opts.killSignal ?? "SIGTERM"), opts.timeout);
  }
  if (opts.signal) {
    const onAbort = () => {
      child.kill(opts.killSignal ?? "SIGTERM");
      const err = new internal.DOMException("The operation was aborted", "AbortError");
      err.code = "ABORT_ERR";
      err.cause = opts.signal.reason;
      child.emit("error", err);
    };
    opts.signal.addEventListener("abort", onAbort, { once: true });
    child.once("exit", () => opts.signal.removeEventListener("abort", onAbort));
  }
  process.nextTick(() => child.emit("spawn"));
  return child;
}

function collectOutput(child, options, finish) {
  const encoding = options.encoding === undefined ? "utf8" : options.encoding;
  const maxBuffer = options.maxBuffer ?? 1024 * 1024;
  const out = [];
  const err = [];
  let outLen = 0;
  let errLen = 0;
  let killedForBuffer = false;
  child.stdout?.on("data", (c) => {
    outLen += c.length;
    if (outLen > maxBuffer && !killedForBuffer) {
      killedForBuffer = true;
      child.kill(options.killSignal ?? "SIGTERM");
    }
    out.push(c);
  });
  child.stderr?.on("data", (c) => {
    errLen += c.length;
    if (errLen > maxBuffer && !killedForBuffer) {
      killedForBuffer = true;
      child.kill(options.killSignal ?? "SIGTERM");
    }
    err.push(c);
  });
  const conv = (chunks) => {
    const b = Buffer.concat(chunks);
    return encoding && encoding !== "buffer" ? b.toString(encoding) : b;
  };
  let spawnErr = null;
  child.on("error", (e) => {
    spawnErr = e;
  });
  child.on("close", (code, signal) => finish(spawnErr, code, signal, conv(out), conv(err), killedForBuffer));
}

function makeExecError(cmd, code, signal, stdout, stderr, killed) {
  const err = new Error(`Command failed: ${cmd}\n${typeof stderr === "string" ? stderr : stderr.toString()}`);
  err.code = code;
  err.killed = killed;
  err.signal = signal;
  err.cmd = cmd;
  err.stdout = stdout;
  err.stderr = stderr;
  return err;
}

function exec(command, options, callback) {
  if (typeof options === "function") {
    callback = options;
    options = {};
  }
  options = { ...(options ?? {}) };
  const child = spawn(command, [], { ...options, shell: options.shell ?? true });
  collectOutput(child, options, (spawnErr, code, signal, stdout, stderr, bufferKill) => {
    if (!callback) return;
    if (spawnErr) {
      spawnErr.stdout = stdout;
      spawnErr.stderr = stderr;
      callback(spawnErr, stdout, stderr);
    } else if (code !== 0 || signal) {
      const err = makeExecError(command, code, signal, stdout, stderr, child.killed || bufferKill);
      if (bufferKill) err.code = "ERR_CHILD_PROCESS_STDIO_MAXBUFFER";
      callback(err, stdout, stderr);
    } else callback(null, stdout, stderr);
  });
  return child;
}

function execFile(file, args, options, callback) {
  if (typeof args === "function") {
    callback = args;
    args = [];
    options = {};
  } else if (!Array.isArray(args)) {
    callback = options;
    options = args ?? {};
    args = [];
  }
  if (typeof options === "function") {
    callback = options;
    options = {};
  }
  options = { ...(options ?? {}) };
  const child = spawn(file, args, options);
  const cmd = [file, ...args].join(" ");
  collectOutput(child, options, (spawnErr, code, signal, stdout, stderr, bufferKill) => {
    if (!callback) return;
    if (spawnErr) {
      spawnErr.stdout = stdout;
      spawnErr.stderr = stderr;
      callback(spawnErr, stdout, stderr);
    } else if (code !== 0 || signal) callback(makeExecError(cmd, code, signal, stdout, stderr, child.killed || bufferKill), stdout, stderr);
    else callback(null, stdout, stderr);
  });
  return child;
}

const promisifyExec = (fn) =>
  function (...args) {
    let child;
    const p = new Promise((resolve, reject) => {
      child = fn(...args, (err, stdout, stderr) => {
        if (err) {
          err.stdout = stdout;
          err.stderr = stderr;
          reject(err);
        } else resolve({ stdout, stderr });
      });
    });
    p.child = child;
    return p;
  };
exec[Symbol.for("nodejs.util.promisify.custom")] = promisifyExec(exec);
execFile[Symbol.for("nodejs.util.promisify.custom")] = promisifyExec(execFile);

function spawnSync(command, args, options) {
  const n = normalizeSpawnArgs(command, args, options);
  const opts = n.options;
  const stdio = normalizeStdio(opts.stdio);
  let input = opts.input;
  if (typeof input === "string") input = Buffer.from(input, opts.encoding && opts.encoding !== "buffer" ? opts.encoding : "utf8");
  const killSignal = typeof opts.killSignal === "number" ? opts.killSignal : internal.signals[opts.killSignal ?? "SIGTERM"];
  const r = ops.spawnSync(n.file, n.args, { cwd: resolveCwd(opts.cwd), env: envPairs(opts.env), stdio }, input ?? null, opts.timeout ?? 0, killSignal);
  const encoding = opts.encoding && opts.encoding !== "buffer" ? opts.encoding : null;
  const conv = (u8) => {
    if (u8 === undefined) return null;
    const b = Buffer.from(u8.buffer, u8.byteOffset, u8.byteLength);
    return encoding ? b.toString(encoding) : b;
  };
  if (r.error && r.code !== "ETIMEDOUT") {
    return {
      pid: 0,
      output: null,
      stdout: null,
      stderr: null,
      status: null,
      signal: null,
      error: spawnError(n.file, n.args, r.code, r.errno, "spawnSync"),
    };
  }
  const stdout = stdio[1] === "pipe" ? conv(r.stdout) : null;
  const stderr = stdio[2] === "pipe" ? conv(r.stderr) : null;
  const result = { pid: r.pid, output: [null, stdout, stderr], stdout, stderr, status: r.status, signal: r.signal };
  if (r.error) result.error = spawnError(n.file, n.args, "ETIMEDOUT", -110, "spawnSync");
  return result;
}

function checkSync(result, cmd) {
  if (result.error) {
    result.error.stdout = result.stdout;
    result.error.stderr = result.stderr;
    throw result.error;
  }
  if (result.status !== 0 || result.signal) {
    const stderr = result.stderr ?? "";
    const err = new Error(`Command failed: ${cmd}${stderr.length ? `\n${stderr.toString()}` : ""}`);
    Object.assign(err, { status: result.status, signal: result.signal, output: result.output, pid: result.pid, stdout: result.stdout, stderr: result.stderr });
    throw err;
  }
  return result.stdout;
}

function execSync(command, options = {}) {
  const stdio = options.stdio ?? ["pipe", "pipe", "inherit"];
  const result = spawnSync(command, [], { ...options, stdio, shell: options.shell ?? true });
  return checkSync(result, command);
}

function execFileSync(file, args, options) {
  if (!Array.isArray(args)) {
    options = args ?? {};
    args = [];
  }
  options = options ?? {};
  const stdio = options.stdio ?? ["pipe", "pipe", "inherit"];
  const result = spawnSync(file, args, { ...options, stdio });
  return checkSync(result, [file, ...args].join(" "));
}

function fork() {
  throw new Error("child_process.fork() is not supported in pi-jsrt");
}

const cp = { spawn, exec, execFile, spawnSync, execSync, execFileSync, fork, ChildProcess };
internal.defineBuiltin("child_process", () => cp);
