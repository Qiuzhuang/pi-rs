// node:util, node:util/types

const kCustomPromisify = Symbol.for("nodejs.util.promisify.custom");

function promisify(original) {
  if (typeof original !== "function") {
    const err = new TypeError('The "original" argument must be of type function');
    err.code = "ERR_INVALID_ARG_TYPE";
    throw err;
  }
  if (original[kCustomPromisify]) {
    const fn = original[kCustomPromisify];
    return Object.defineProperty(fn, kCustomPromisify, { value: fn, enumerable: false, writable: false, configurable: true });
  }
  function fn(...args) {
    return new Promise((resolve, reject) => {
      original.call(this, ...args, (err, ...values) => {
        if (err) return reject(err);
        resolve(values.length > 1 ? values : values[0]);
      });
    });
  }
  Object.setPrototypeOf(fn, Object.getPrototypeOf(original));
  Object.defineProperty(fn, kCustomPromisify, { value: fn, enumerable: false, writable: false, configurable: true });
  return Object.defineProperties(fn, Object.getOwnPropertyDescriptors(original));
}
promisify.custom = kCustomPromisify;

function callbackify(original) {
  return function (...args) {
    const cb = args.pop();
    original.apply(this, args).then(
      (v) => process.nextTick(cb, null, v),
      (err) => {
        if (!err) {
          const e = new Error("Promise was rejected with a falsy value");
          e.reason = err;
          err = e;
        }
        process.nextTick(cb, err);
      },
    );
  };
}

function deprecate(fn, msg, code) {
  let warned = false;
  return function (...args) {
    if (!warned) {
      warned = true;
      process.emitWarning?.(msg, "DeprecationWarning", code);
    }
    return new.target ? Reflect.construct(fn, args, new.target) : fn.apply(this, args);
  };
}

function inherits(ctor, superCtor) {
  Object.defineProperty(ctor, "super_", { value: superCtor, writable: true, configurable: true });
  Object.setPrototypeOf(ctor.prototype, superCtor.prototype);
}

function isDeepStrictEqual(a, b, strict = true, seen = new Map()) {
  if (Object.is(a, b)) return true;
  if (!strict && a == b && (typeof a !== "object" || a === null) && (typeof b !== "object" || b === null)) return true;
  if (typeof a !== "object" || typeof b !== "object" || a === null || b === null) {
    return !strict && a == b;
  }
  if (strict && Object.getPrototypeOf(a) !== Object.getPrototypeOf(b)) return false;
  if (seen.get(a) === b) return true;
  seen.set(a, b);
  if (a instanceof Date) return b instanceof Date && a.getTime() === b.getTime();
  if (a instanceof RegExp) return b instanceof RegExp && String(a) === String(b);
  if (a instanceof Error && b instanceof Error && (a.message !== b.message || a.name !== b.name)) return false;
  if (ArrayBuffer.isView(a)) {
    if (!ArrayBuffer.isView(b) || a.length !== b.length) return false;
    for (let i = 0; i < a.length; i++) if (!Object.is(a[i], b[i])) return false;
    return true;
  }
  if (a instanceof Map) {
    if (!(b instanceof Map) || a.size !== b.size) return false;
    for (const [k, v] of a) if (!b.has(k) || !isDeepStrictEqual(v, b.get(k), strict, seen)) return false;
    return true;
  }
  if (a instanceof Set) {
    if (!(b instanceof Set) || a.size !== b.size) return false;
    for (const v of a) {
      if (b.has(v)) continue;
      let found = false;
      for (const w of b) if (isDeepStrictEqual(v, w, strict, seen)) found = true;
      if (!found) return false;
    }
    return true;
  }
  if (Array.isArray(a) !== Array.isArray(b)) return false;
  const ka = Object.keys(a);
  const kb = Object.keys(b);
  if (ka.length !== kb.length) return false;
  for (const k of ka) {
    if (!Object.prototype.hasOwnProperty.call(b, k)) return false;
    if (!isDeepStrictEqual(a[k], b[k], strict, seen)) return false;
  }
  return true;
}
internal.isDeepEqual = isDeepStrictEqual;

const tag = (v) => Object.prototype.toString.call(v).slice(8, -1);
const types = {
  isPromise: (v) => v instanceof Promise,
  isDate: (v) => v instanceof Date,
  isRegExp: (v) => v instanceof RegExp,
  isMap: (v) => v instanceof Map,
  isSet: (v) => v instanceof Set,
  isWeakMap: (v) => v instanceof WeakMap,
  isWeakSet: (v) => v instanceof WeakSet,
  isNativeError: (v) => v instanceof Error,
  isTypedArray: (v) => ArrayBuffer.isView(v) && !(v instanceof DataView),
  isUint8Array: (v) => v instanceof Uint8Array,
  isUint16Array: (v) => v instanceof Uint16Array,
  isUint32Array: (v) => v instanceof Uint32Array,
  isInt8Array: (v) => v instanceof Int8Array,
  isInt16Array: (v) => v instanceof Int16Array,
  isInt32Array: (v) => v instanceof Int32Array,
  isFloat32Array: (v) => v instanceof Float32Array,
  isFloat64Array: (v) => v instanceof Float64Array,
  isBigInt64Array: (v) => v instanceof BigInt64Array,
  isBigUint64Array: (v) => v instanceof BigUint64Array,
  isArrayBuffer: (v) => v instanceof ArrayBuffer,
  isSharedArrayBuffer: (v) => typeof SharedArrayBuffer !== "undefined" && v instanceof SharedArrayBuffer,
  isAnyArrayBuffer: (v) => v instanceof ArrayBuffer || (typeof SharedArrayBuffer !== "undefined" && v instanceof SharedArrayBuffer),
  isArrayBufferView: (v) => ArrayBuffer.isView(v),
  isDataView: (v) => v instanceof DataView,
  isAsyncFunction: (v) => typeof v === "function" && tag(v) === "AsyncFunction",
  isGeneratorFunction: (v) => typeof v === "function" && /GeneratorFunction$/.test(tag(v)),
  isGeneratorObject: (v) => tag(v) === "Generator",
  isMapIterator: (v) => tag(v) === "Map Iterator",
  isSetIterator: (v) => tag(v) === "Set Iterator",
  isBoxedPrimitive: (v) => v instanceof Number || v instanceof String || v instanceof Boolean || v instanceof BigInt || v instanceof Symbol,
  isNumberObject: (v) => v instanceof Number,
  isStringObject: (v) => v instanceof String,
  isBooleanObject: (v) => v instanceof Boolean,
  isSymbolObject: (v) => typeof v === "object" && v instanceof Symbol,
  isProxy: () => false,
  isExternal: () => false,
  isModuleNamespaceObject: (v) => tag(v) === "Module",
  isKeyObject: () => false,
  isCryptoKey: () => false,
};

function stripVTControlCharacters(str) {
  return String(str).replace(/[\u001B\u009B][[\]()#;?]*(?:(?:(?:(?:;[-a-zA-Z\d/#&.:=?%@~_]+)*|[a-zA-Z\d]+(?:;[-a-zA-Z\d/#&.:=?%@~_]*)*)?\u0007)|(?:(?:\d{1,4}(?:;\d{0,4})*)?[\dA-PR-TZcf-ntqry=><~]))/g, "");
}

function styleText(format, text) {
  const formats = Array.isArray(format) ? format : [format];
  let out = String(text);
  for (const f of formats) {
    const code = internal.inspect.colors[f];
    if (!code) throw new TypeError(`The argument 'format' must be one of: ${Object.keys(internal.inspect.colors).join(", ")}. Received '${f}'`);
    out = `\x1b[${code[0]}m${out}\x1b[${code[1]}m`;
  }
  return out;
}

function debuglog(section) {
  const enabled = new RegExp(`^(${(process.env.NODE_DEBUG || "").replace(/[|\\{}()[\]^$+?.]/g, "\\$&").replace(/\*/g, ".*").replace(/,/g, "|")})$`, "i").test(section);
  const fn = (...args) => {
    if (enabled) process.stderr.write(`${section.toUpperCase()} ${process.pid}: ${internal.format(...args)}\n`);
  };
  fn.enabled = enabled;
  return fn;
}

function parseArgs(config = {}) {
  const { args = process.argv.slice(2), options = {}, strict = true, allowPositionals = !strict } = config;
  const values = {};
  const positionals = [];
  const shortMap = {};
  for (const [name, opt] of Object.entries(options)) {
    if (opt.short) shortMap[opt.short] = name;
    if (opt.default !== undefined) values[name] = opt.default;
  }
  const set = (name, value) => {
    const opt = options[name];
    if (!opt) {
      if (strict) {
        const err = new TypeError(`Unknown option '--${name}'`);
        err.code = "ERR_PARSE_ARGS_UNKNOWN_OPTION";
        throw err;
      }
      values[name] = value ?? true;
      return;
    }
    if (opt.multiple) (values[name] = Array.isArray(values[name]) && values[name] !== opt.default ? values[name] : []).push(value);
    else values[name] = value;
  };
  for (let i = 0; i < args.length; i++) {
    const arg = args[i];
    if (arg === "--") {
      positionals.push(...args.slice(i + 1));
      break;
    }
    if (arg.startsWith("--")) {
      const eq = arg.indexOf("=");
      const name = eq > 0 ? arg.slice(2, eq) : arg.slice(2);
      const opt = options[name];
      if (opt?.type === "string") set(name, eq > 0 ? arg.slice(eq + 1) : args[++i]);
      else set(name, eq > 0 ? arg.slice(eq + 1) : true);
    } else if (arg.startsWith("-") && arg.length > 1) {
      for (let j = 1; j < arg.length; j++) {
        const name = shortMap[arg[j]] ?? arg[j];
        const opt = options[name];
        if (opt?.type === "string") {
          set(name, j + 1 < arg.length ? arg.slice(j + 1) : args[++i]);
          break;
        }
        set(name, true);
      }
    } else {
      if (!allowPositionals && strict) {
        const err = new TypeError(`Unexpected argument '${arg}'. This command does not take positional arguments`);
        err.code = "ERR_PARSE_ARGS_UNEXPECTED_POSITIONAL";
        throw err;
      }
      positionals.push(arg);
    }
  }
  return { values, positionals };
}

const util = {
  format: internal.format,
  formatWithOptions: internal.formatWithOptions,
  inspect: internal.inspect,
  promisify,
  callbackify,
  deprecate,
  inherits,
  isDeepStrictEqual: (a, b) => isDeepStrictEqual(a, b, true),
  types,
  TextEncoder: internal.TextEncoder,
  TextDecoder: internal.TextDecoder,
  stripVTControlCharacters,
  styleText,
  debuglog,
  debug: debuglog,
  parseArgs,
  toUSVString: (s) => String(s).toWellFormed(),
  isArray: Array.isArray,
  isBoolean: (v) => typeof v === "boolean",
  isNull: (v) => v === null,
  isNullOrUndefined: (v) => v === null || v === undefined,
  isNumber: (v) => typeof v === "number",
  isString: (v) => typeof v === "string",
  isSymbol: (v) => typeof v === "symbol",
  isUndefined: (v) => v === undefined,
  isRegExp: types.isRegExp,
  isObject: (v) => v !== null && typeof v === "object",
  isDate: types.isDate,
  isError: (v) => v instanceof Error,
  isFunction: (v) => typeof v === "function",
  isPrimitive: (v) => v === null || (typeof v !== "object" && typeof v !== "function"),
  getSystemErrorName: (n) => ({ [-2]: "ENOENT", [-13]: "EACCES", [-17]: "EEXIST" })[n] ?? `Unknown system error ${n}`,
  aborted: (signal) =>
    new Promise((resolve) => {
      if (signal.aborted) resolve();
      else signal.addEventListener("abort", () => resolve(), { once: true });
    }),
};
internal.util = util;
internal.defineBuiltin("util", () => util);
internal.defineBuiltin("util/types", () => types);
