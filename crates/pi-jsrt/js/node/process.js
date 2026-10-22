// global `process`, `global`

const EventEmitter = internal.EventEmitter;
const config = ops.config;

function makeWriteStream(fd) {
  const s = new EventEmitter();
  s.fd = fd;
  s.writable = true;
  s._isStdio = true;
  const tty = ops.isatty(fd);
  s.isTTY = tty || undefined;
  const size = () => (tty ? ops.termSize(fd) : null);
  Object.defineProperty(s, "columns", { get: () => size()?.[0] ?? (tty ? 80 : undefined), enumerable: true, configurable: true });
  Object.defineProperty(s, "rows", { get: () => size()?.[1] ?? (tty ? 24 : undefined), enumerable: true, configurable: true });
  s.getWindowSize = () => [s.columns ?? 80, s.rows ?? 24];
  s.hasColors = (count = 16) => tty && count <= 256;
  s.getColorDepth = () => (tty ? 8 : 1);
  s.write = (chunk, encoding, cb) => {
    if (typeof encoding === "function") {
      cb = encoding;
      encoding = undefined;
    }
    if (typeof chunk === "string" && encoding && encoding !== "utf8" && encoding !== "utf-8") {
      chunk = internal.Buffer.from(chunk, encoding);
    }
    ops.write(fd, chunk);
    if (typeof cb === "function") queueMicrotask(() => cb(null));
    return true;
  };
  s.end = (chunk, encoding, cb) => {
    if (typeof chunk === "function") {
      cb = chunk;
      chunk = undefined;
    }
    if (chunk !== undefined && chunk !== null) s.write(chunk, encoding);
    if (typeof cb === "function") queueMicrotask(cb);
    return s;
  };
  s.cork = () => {};
  s.uncork = () => {};
  s.setDefaultEncoding = () => s;
  s.cursorTo = (x, y, cb) => {
    s.write(typeof y === "number" ? `\x1b[${y + 1};${x + 1}H` : `\x1b[${x + 1}G`);
    if (typeof cb === "function") cb();
    return true;
  };
  s.moveCursor = (dx, dy, cb) => {
    let out = "";
    if (dx < 0) out += `\x1b[${-dx}D`;
    else if (dx > 0) out += `\x1b[${dx}C`;
    if (dy < 0) out += `\x1b[${-dy}A`;
    else if (dy > 0) out += `\x1b[${dy}B`;
    s.write(out);
    if (typeof cb === "function") cb();
    return true;
  };
  s.clearLine = (dir, cb) => {
    s.write(dir < 0 ? "\x1b[1K" : dir > 0 ? "\x1b[0K" : "\x1b[2K");
    if (typeof cb === "function") cb();
    return true;
  };
  s.clearScreenDown = (cb) => {
    s.write("\x1b[0J");
    if (typeof cb === "function") cb();
    return true;
  };
  return s;
}

function makeStdin() {
  const s = new EventEmitter();
  s.fd = 0;
  s.isTTY = ops.isatty(0) || undefined;
  s.readable = true;
  s.setRawMode = () => s;
  s.setEncoding = () => s;
  s.resume = () => s;
  s.pause = () => s;
  s.ref = () => s;
  s.unref = () => s;
  s.read = () => null;
  s[Symbol.asyncIterator] = async function* () {};
  return s;
}

const process = new EventEmitter();
const startTime = ops.now();
let exitCode;

Object.assign(process, {
  title: "pi",
  // pi's `engines` minimum (">=22.19.0"), the version pi-ai reports in User-Agent/Stainless
  // headers too; it satisfies the version checks of the packages in tests/npm_packages.rs.
  // No `napi`/`openssl` entries: native addons cannot load, and the crypto is not OpenSSL.
  version: "v22.19.0",
  versions: {
    node: "22.19.0", pi_jsrt: config.version, boa: "0.22.0", v8: "0.0", uv: "1.51.0", modules: "127",
    zlib: "1.3.1", brotli: "1.1.0", unicode: "16.0",
  },
  release: { name: "node", lts: "Jod" },
  platform: config.platform,
  arch: config.arch,
  pid: config.pid,
  ppid: config.ppid,
  execPath: config.execPath,
  execArgv: [],
  argv: Array.from(config.argv),
  argv0: config.argv[0] ?? "pi",
  env: config.env,
  features: { inspector: false, ipv6: true, tls: true, typescript: "strip" },
  config: { variables: {} },
  allowedNodeEnvironmentFlags: new Set(),
  stdout: makeWriteStream(1),
  stderr: makeWriteStream(2),
  stdin: makeStdin(),
  cwd: () => ops.cwd(),
  chdir: (dir) => ops.chdir(String(dir)),
  uptime: () => (ops.now() - startTime) / 1000,
  hrtime: Object.assign(
    function hrtime(prev) {
      const ns = ops.hrtime();
      let sec = Math.floor(ns / 1e9);
      let nsec = Math.floor(ns % 1e9);
      if (prev) {
        sec -= prev[0];
        nsec -= prev[1];
        if (nsec < 0) {
          sec -= 1;
          nsec += 1e9;
        }
      }
      return [sec, nsec];
    },
    { bigint: () => BigInt(Math.floor(ops.hrtime())) },
  ),
  memoryUsage: Object.assign(() => ops.memoryUsage(), { rss: () => ops.memoryUsage().rss }),
  cpuUsage: (prev) => {
    const u = ops.cpuUsage();
    return prev ? { user: u.user - prev.user, system: u.system - prev.system } : u;
  },
  resourceUsage: () => ({ ...ops.cpuUsage(), maxRSS: ops.memoryUsage().rss / 1024 }),
  nextTick(callback, ...args) {
    if (typeof callback !== "function") throw new TypeError('The "callback" argument must be of type function');
    queueMicrotask(() => callback(...args));
  },
  emitWarning(warning, type, code) {
    if (typeof type === "object" && type) {
      code = type.code;
      type = type.type;
    }
    const w = warning instanceof Error ? warning : Object.assign(new Error(String(warning)), { name: type || "Warning" });
    if (code) w.code = code;
    if (process.listenerCount("warning") > 0) {
      queueMicrotask(() => process.emit("warning", w));
    } else if (!process.noDeprecation || w.name !== "DeprecationWarning") {
      ops.write(2, `(pi:${process.pid}) ${code ? `[${code}] ` : ""}${w.name}: ${w.message}\n`);
    }
  },
  exit(code) {
    if (code !== undefined) exitCode = code;
    const c = Number(exitCode ?? 0) | 0;
    try {
      process.emit("exit", c);
    } catch {}
    ops.exit(c);
    const err = new Error(`process.exit(${c}) called`);
    err.code = "ERR_PROCESS_EXIT";
    err.exitCode = c;
    throw err;
  },
  abort() {
    process.exit(134);
  },
  kill(pid, signal = "SIGTERM") {
    const signals = internal.signals;
    const sig = typeof signal === "number" ? signal : signals[signal];
    if (sig === undefined) throw new TypeError(`Unknown signal: ${signal}`);
    return ops.kill(pid, sig);
  },
  umask: () => 0o22,
  getuid: () => ops.osInfo().uid,
  geteuid: () => ops.osInfo().uid,
  getgid: () => ops.osInfo().gid,
  getegid: () => ops.osInfo().gid,
  getgroups: () => [ops.osInfo().gid],
  binding(name) {
    throw new Error(`No such module: ${name}`);
  },
  setUncaughtExceptionCaptureCallback() {},
  hasUncaughtExceptionCaptureCallback: () => false,
  setSourceMapsEnabled() {},
  getBuiltinModule(id) {
    const name = String(id).replace(/^node:/, "");
    return internal.hasBuiltin(name) ? internal.requireBuiltin(name) : undefined;
  },
  emit: EventEmitter.prototype.emit,
});
Object.defineProperty(process, "exitCode", {
  get: () => exitCode,
  set: (v) => {
    exitCode = v;
  },
  enumerable: true,
  configurable: true,
});
process.report = { getReport: () => ({ header: { glibcVersionRuntime: undefined } }) };
Object.defineProperty(process, Symbol.toStringTag, { value: "process", enumerable: false, configurable: true, writable: true });

internal.signals = {
  SIGHUP: 1,
  SIGINT: 2,
  SIGQUIT: 3,
  SIGILL: 4,
  SIGTRAP: 5,
  SIGABRT: 6,
  SIGBUS: 7,
  SIGFPE: 8,
  SIGKILL: 9,
  SIGUSR1: 10,
  SIGSEGV: 11,
  SIGUSR2: 12,
  SIGPIPE: 13,
  SIGALRM: 14,
  SIGTERM: 15,
  SIGCHLD: 17,
  SIGCONT: 18,
  SIGSTOP: 19,
  SIGTSTP: 20,
  SIGTTIN: 21,
  SIGTTOU: 22,
  SIGWINCH: 28,
};
if (config.platform === "darwin") {
  Object.assign(internal.signals, { SIGBUS: 10, SIGUSR1: 30, SIGSEGV: 11, SIGUSR2: 31, SIGCHLD: 20, SIGCONT: 19, SIGSTOP: 17, SIGTSTP: 18 });
}

// `require` for classic scripts (like `node -e`), relative to the cwd.
internal.hidden(globalThis, "require", internal.makeRequire(`${ops.cwd()}/[eval]`));

internal.process = process;
internal.hidden(globalThis, "process", process);
internal.hidden(globalThis, "global", globalThis);
internal.defineBuiltin("process", () => process);
