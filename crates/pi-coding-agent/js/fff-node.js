/**
 * `@ff-labs/fff-node` — virtual module served by the pi-rs extension runtime.
 *
 * `pi-fff` imports this package for `FileFinder` (fuzzy file search, autocomplete, frecency
 * tracking, content grep). The real npm package loads `libfff_c` (the fff engine shared
 * library) through the `ffi-rs` N-API addon, a `.node` binary that pi-jsrt cannot load.
 *
 * This module reproduces the npm package's public API (same camelCase names and shapes as
 * `@ff-labs/fff-node` 0.9.6) and drives the same native engine directly: the Rust side
 * (`src/core/extensions/js/fff.rs`) dlopens `libfff_c` from the
 * `@ff-labs/fff-bin-<platform>` package and calls its C API. Instance handles are registered
 * in a process-wide table and appear here as plain integer ids.
 *
 * Behavior notes vs. the npm package:
 * - The npm layer's own result parser reads a handful of fields (`fuzzyScore`, `isBinary`,
 *   `isDefinition` on grep matches; `exactMatch` on scores) from stale offsets, so they are
 *   silently zero there; the Rust side reads the correct layout and they carry real values.
 * - `restartIndex` resolves to a Result object like every other method (the npm layer
 *   accidentally returns its raw envelope); `isDestroyed` additionally answers `true` once
 *   the Rust-side instance table no longer knows the id (a destroyed runtime), where the npm
 *   layer would throw on the next use instead.
 */

// The extension runtime installs `__piHost.call` before evaluating extension modules (the
// same pattern `bridge.js` and `pi-tui.js` use for host calls). Returns the raw
// `{ ok, value, error }` envelope; callers translate it like upstream's `ffi.js` callers do.
function host(f, ...args) {
  return globalThis.__piHost.call("fff", { fn: f, args });
}

// ---------------------------------------------------------------------------
// Platform/binary resolution (mirrors `platform.js` + `binary.js` of the npm package)
// ---------------------------------------------------------------------------

const TRIPLE_TO_NPM_PACKAGE = {
  "x86_64-unknown-linux-musl": "linux-x64-musl",
  "x86_64-unknown-linux-gnu": "linux-x64-gnu",
  "aarch64-unknown-linux-musl": "linux-arm64-musl",
  "aarch64-unknown-linux-gnu": "linux-arm64-gnu",
  "x86_64-apple-darwin": "darwin-x64",
  "aarch64-apple-darwin": "darwin-arm64",
  "x86_64-pc-windows-msvc": "win32-x64",
  "aarch64-pc-windows-msvc": "win32-arm64",
};

const PLATFORM_TO_LIB_NAME = {
  darwin: "libfff_c.dylib",
  linux: "libfff_c.so",
  win32: "fff_c.dll",
};

export function getPlatformString() {
  // pi-jsrt does not guarantee a Node-style `process` global in extension modules, so this
  // is best-effort cosmetics only: the Rust side (`fff.rs::discover_binary`) performs the
  // authoritative library lookup for the host triple.
  let arch;
  let platform;
  try {
    arch = globalThis.process && globalThis.process.arch ? globalThis.process.arch : undefined;
    platform = globalThis.process && globalThis.process.platform ? globalThis.process.platform : undefined;
  } catch {
    return null;
  }
  if (!arch || !platform) return null;
  switch (platform) {
    case "darwin":
      return arch === "arm64" ? "aarch64-apple-darwin" : arch === "x64" ? "x86_64-apple-darwin" : null;
    case "linux": {
      // glibc vs musl is decided by the Rust side, which knows the build triple; the
      // `PI_FFF_C_LIB` override and the platform npm package search cover both.
      const archPart = arch === "arm64" ? "aarch64" : "x86_64";
      return `${archPart}-unknown-linux-gnu`;
    }
    case "win32":
      return arch === "arm64" ? "aarch64-pc-windows-msvc" : "x86_64-pc-windows-msvc";
    default:
      return null;
  }
}

export function getNpmPlatformString() {
  const triple = getPlatformString();
  return triple ? TRIPLE_TO_NPM_PACKAGE[triple] : null;
}

export function getLibraryName() {
  let platform;
  try {
    platform = globalThis.process && globalThis.process.platform ? globalThis.process.platform : undefined;
  } catch {
    platform = undefined;
  }
  return PLATFORM_TO_LIB_NAME[platform] ?? null;
}

export function getBinaryPath() {
  return host("libPath").value ?? null;
}

export function binaryExists() {
  return host("libPath").value != null;
}

export function isAvailable() {
  return host("available").value === true;
}

// ---------------------------------------------------------------------------
// Options (mirrors `types.js` normalization)
// ---------------------------------------------------------------------------

// Empty string means "no database" (`optional_cstr` in fff-c maps NULL/"" to `None`), exactly
// likenpm's `FileFinder.create` passing `options.frecencyDbPath ?? ""`. A DB-less finder holds
// no database environment, which is what makes pi-fff's db-less fallback work when the DB is
// already open elsewhere (another session in this process/language server).
export function resolveFrecencyDbPath(options) {
  return (options && options.frecencyDbPath) || "";
}

export function resolveHistoryDbPath(options) {
  return (options && options.historyDbPath) || "";
}

export function normalizeOptions(options) {
  const opts = options ?? {};
  const out = {
    basePath: opts.basePath,
    frecencyDbPath: resolveFrecencyDbPath(opts),
    historyDbPath: resolveHistoryDbPath(opts),
    enableMmapCache: !(opts.disableMmapCache ?? false),
    enableContentIndexing: !(opts.disableContentIndexing ?? opts.disableMmapCache ?? false),
    watch: !(opts.disableWatch ?? false),
    aiMode: opts.aiMode ?? false,
    logFilePath: opts.logFilePath ?? "",
    logLevel: opts.logLevel ?? "",
    cacheBudgetMaxFiles: opts.cacheBudgetMaxFiles ?? 0,
    cacheBudgetMaxBytes: opts.cacheBudgetMaxBytes ?? 0,
    cacheBudgetMaxFileSize: opts.cacheBudgetMaxFileSize ?? 0,
    enableFsRootScanning: opts.enableFsRootScanning ?? false,
    enableHomeDirScanning: opts.enableHomeDirScanning ?? false,
  };
  return out;
}

// ---------------------------------------------------------------------------// ---------------------------------------------------------------------------
// FileFinder (mirrors the public surface of `finder.js`)
// ---------------------------------------------------------------------------

const DESTROYED = { ok: false, error: "FileFinder instance has been destroyed." };

export class FileFinder {
  static create(options) {
    const opts = normalizeOptions(options);
    if (!opts.basePath) throw new Error("basePath is required");
    // Like the npm layer, a missing/incompatible native binary throws here (its `loadLibrary`
    // equivalent throws before any FFI call); call-level failures return a `Result.err`
    // envelope so callers can fall back (pi-fff retries with a DB-less finder).
    if (!isAvailable()) {
      throw new Error(`FFF native library not found (looked for ${getBinaryPath() ?? "libfff_c"})`);
    }
    const result = host("create", opts);
    if (!result || !result.ok) {
      return result ?? { ok: false, error: "fff_create_instance_with failed" };
    }
    const finder = new FileFinder();
    finder.handle = result.value;
    finder.destroyed = false;
    return { ok: true, value: finder };
  }

  constructor() {
    this.handle = null;
    this.destroyed = false;
  }

  destroy() {
    if (this.destroyed) return;
    this.destroyed = true;
    if (this.handle != null) {
      try {
        host("destroy", this.handle);
      } finally {
        this.handle = null;
      }
    }
  }

  isDestroyed() {
    return this.destroyed;
  }

  _id() {
    return this.destroyed || this.handle == null ? null : this.handle;
  }

  healthCheck() {
    // The npm layer does not require a live instance here; a destroyed finder (handle null)
    // falls back to the limited static check, exactly like its `healthCheckStatic` sibling.
    return host("healthCheck", this.destroyed ? null : this.handle, "");
  }

  static healthCheckStatic(basePath) {
    return host("healthCheck", null, basePath || "");
  }

  // npm returns `Promise<Result<boolean>>`; awaiting the plain Result works identically for
  // consumers, and the blocking variant is exposed for callers that do not await.
  async waitForScan(timeoutMs) {
    return this.waitForScanBlocking(timeoutMs);
  }

  waitForScanBlocking(timeoutMs) {
    const id = this._id();
    if (id === null) return DESTROYED;
    const result = host("waitForScan", id, timeoutMs ?? 0);
    if (!result || !result.ok) return result ?? DESTROYED;
    if (result.value === false) return { ok: false, error: "Timed out waiting for scan to complete" };
    return { ok: true, value: true };
  }

  getScanProgress() {
    const id = this._id();
    if (id === null) return DESTROYED;
    return host("getScanProgress", id);
  }

  isScanning() {
    const id = this._id();
    if (id === null) return false;
    return host("isScanning", id).value === true;
  }

  isWarmupComplete() {
    const progress = this.getScanProgress();
    return progress.ok ? progress.value.isWarmupComplete === true : false;
  }

  getBasePath() {
    const id = this._id();
    if (id === null) return DESTROYED;
    return host("getBasePath", id);
  }

  fileSearch(query, options) {
    const id = this._id();
    if (id === null) return DESTROYED;
    const opts = options ?? {};
    return host(
      "search",
      id,
      query,
      opts.currentFile ?? "",
      opts.maxThreads ?? 0,
      opts.pageIndex ?? 0,
      opts.pageSize ?? 0,
      opts.comboBoostMultiplier ?? 0,
      opts.minComboCount ?? 0,
    );
  }

  glob(pattern, options) {
    const id = this._id();
    if (id === null) return DESTROYED;
    const opts = options ?? {};
    return host("glob", id, pattern, opts.currentFile ?? "", opts.maxThreads ?? 0, opts.pageIndex ?? 0, opts.pageSize ?? 0);
  }

  directorySearch(query, options) {
    const id = this._id();
    if (id === null) return DESTROYED;
    const opts = options ?? {};
    return host(
      "directorySearch",
      id,
      query,
      opts.currentFile ?? "",
      opts.maxThreads ?? 0,
      opts.pageIndex ?? 0,
      opts.pageSize ?? 0,
    );
  }

  mixedSearch(query, options) {
    const id = this._id();
    if (id === null) return DESTROYED;
    const opts = options ?? {};
    return host(
      "mixedSearch",
      id,
      query,
      opts.currentFile ?? "",
      opts.maxThreads ?? 0,
      opts.pageIndex ?? 0,
      opts.pageSize ?? 0,
      opts.comboBoostMultiplier ?? 0,
      opts.minComboCount ?? 0,
    );
  }

  grep(query, options) {
    const id = this._id();
    if (id === null) return DESTROYED;
    const opts = options ?? {};
    // Defaults match the npm layer: 0 = library default, mode "plain" (numeric 0) = literal.
    return host(
      "grep",
      id,
      query,
      opts.mode ?? "plain",
      opts.maxFileSize ?? 0,
      opts.maxMatchesPerFile ?? 0,
      opts.smartCase ?? true,
      opts.cursor?._offset ?? opts.fileOffset ?? 0,
      opts.pageSize ?? 0,
      opts.timeBudgetMs ?? 0,
      opts.beforeContext ?? 0,
      opts.afterContext ?? 0,
      opts.classifyDefinitions ?? false,
    );
  }

  multiGrep(options) {
    if (!options || !Array.isArray(options.patterns)) throw new Error("patterns array is required");
    if (options.patterns.length === 0) return { ok: false, error: "patterns array must have at least 1 element" };
    const id = this._id();
    if (id === null) return DESTROYED;
    // The native call takes newline-separated patterns and a raw constraint string, with
    // 0 meaning "library default" — exactly how the npm layer forwards them.
    return host(
      "multiGrep",
      id,
      options.patterns.join("\n"),
      options.constraints ?? "",
      options.maxFileSize ?? 0,
      options.maxMatchesPerFile ?? 0,
      options.smartCase ?? true,
      options.cursor?._offset ?? options.fileOffset ?? 0,
      options.pageSize ?? 0,
      options.timeBudgetMs ?? 0,
      options.beforeContext ?? 0,
      options.afterContext ?? 0,
      options.classifyDefinitions ?? false,
    );
  }

  scanFiles() {
    const id = this._id();
    if (id === null) return DESTROYED;
    return host("scanFiles", id);
  }

  restartIndex(path) {
    const id = this._id();
    if (id === null) return DESTROYED;
    return host("restartIndex", id, path ?? "");
  }

  refreshGitStatus() {
    const id = this._id();
    if (id === null) return DESTROYED;
    return host("refreshGitStatus", id);
  }

  trackQuery(query, path) {
    const id = this._id();
    if (id === null) return DESTROYED;
    return host("trackQuery", id, query, path);
  }

  getHistoricalQuery(limit) {
    const id = this._id();
    if (id === null) return DESTROYED;
    return host("historicalQuery", id, limit ?? 0);
  }
}
