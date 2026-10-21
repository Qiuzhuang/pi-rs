// WHATWG URL and URLSearchParams (parsing done by the Rust `url` crate).

const encoder = new internal.TextEncoder();
const decoder = new internal.TextDecoder();

function formEncode(str) {
  const bytes = encoder.encode(String(str));
  let out = "";
  for (const b of bytes) {
    if ((b >= 0x30 && b <= 0x39) || (b >= 0x41 && b <= 0x5a) || (b >= 0x61 && b <= 0x7a) || b === 0x2a || b === 0x2d || b === 0x2e || b === 0x5f) {
      out += String.fromCharCode(b);
    } else if (b === 0x20) {
      out += "+";
    } else {
      out += "%" + b.toString(16).toUpperCase().padStart(2, "0");
    }
  }
  return out;
}

function formDecode(str) {
  const s = str.replace(/\+/g, " ");
  const bytes = [];
  for (let i = 0; i < s.length; i++) {
    const c = s.charCodeAt(i);
    if (c === 0x25 && i + 2 < s.length + 0 && /^[0-9a-fA-F]{2}$/.test(s.slice(i + 1, i + 3))) {
      bytes.push(parseInt(s.slice(i + 1, i + 3), 16));
      i += 2;
    } else if (c < 0x80) {
      bytes.push(c);
    } else {
      // Non-ASCII: encode the code point as UTF-8.
      const cp = s.codePointAt(i);
      const enc = encoder.encode(String.fromCodePoint(cp));
      for (const b of enc) bytes.push(b);
      if (cp > 0xffff) i++;
    }
  }
  return decoder.decode(new Uint8Array(bytes));
}

const kList = Symbol("list");
const kUrl = Symbol("url");

class URLSearchParams {
  constructor(init = undefined) {
    this[kList] = [];
    this[kUrl] = null;
    if (init === undefined || init === null) return;
    if (typeof init === "object" || typeof init === "function") {
      if (typeof init[Symbol.iterator] === "function") {
        for (const pair of init) {
          const arr = Array.from(pair);
          if (arr.length !== 2) throw new TypeError("Each query pair must be an iterable [name, value] tuple");
          this[kList].push([String(arr[0]), String(arr[1])]);
        }
      } else {
        for (const key of Object.keys(init)) this[kList].push([key, String(init[key])]);
      }
    } else {
      this.#parse(String(init));
    }
  }
  #parse(str) {
    if (str.startsWith("?")) str = str.slice(1);
    this[kList] = [];
    for (const part of str.split("&")) {
      if (!part) continue;
      const eq = part.indexOf("=");
      const name = eq >= 0 ? part.slice(0, eq) : part;
      const value = eq >= 0 ? part.slice(eq + 1) : "";
      this[kList].push([formDecode(name), formDecode(value)]);
    }
  }
  _setFromUrl(search) {
    this.#parse(search);
  }
  #update() {
    if (this[kUrl]) {
      const s = this.toString();
      this[kUrl]._updateSearch(s ? `?${s}` : "");
    }
  }
  get size() {
    return this[kList].length;
  }
  append(name, value) {
    this[kList].push([String(name), String(value)]);
    this.#update();
  }
  delete(name, value) {
    name = String(name);
    this[kList] = this[kList].filter(([k, v]) => !(k === name && (value === undefined || v === String(value))));
    this.#update();
  }
  get(name) {
    name = String(name);
    const e = this[kList].find(([k]) => k === name);
    return e ? e[1] : null;
  }
  getAll(name) {
    name = String(name);
    return this[kList].filter(([k]) => k === name).map(([, v]) => v);
  }
  has(name, value) {
    name = String(name);
    return this[kList].some(([k, v]) => k === name && (value === undefined || v === String(value)));
  }
  set(name, value) {
    name = String(name);
    value = String(value);
    const idx = this[kList].findIndex(([k]) => k === name);
    if (idx < 0) {
      this[kList].push([name, value]);
    } else {
      this[kList][idx][1] = value;
      this[kList] = this[kList].filter(([k], i) => i <= idx || k !== name);
    }
    this.#update();
  }
  sort() {
    this[kList].sort((a, b) => (a[0] < b[0] ? -1 : a[0] > b[0] ? 1 : 0));
    this.#update();
  }
  forEach(callback, thisArg) {
    for (const [k, v] of this[kList]) callback.call(thisArg, v, k, this);
  }
  *keys() {
    for (const [k] of this[kList]) yield k;
  }
  *values() {
    for (const [, v] of this[kList]) yield v;
  }
  *entries() {
    for (const [k, v] of this[kList]) yield [k, v];
  }
  [Symbol.iterator]() {
    return this.entries();
  }
  toString() {
    return this[kList].map(([k, v]) => `${formEncode(k)}=${formEncode(v)}`).join("&");
  }
  get [Symbol.toStringTag]() {
    return "URLSearchParams";
  }
  [Symbol.for("nodejs.util.inspect.custom")]() {
    const inner = this[kList].map(([k, v]) => `${internal.inspect(k)} => ${internal.inspect(v)}`).join(", ");
    return `URLSearchParams {${inner ? ` ${inner} ` : ""}}`;
  }
}

const kParts = Symbol("parts");
const kSearchParams = Symbol("searchParams");

function invalidUrl(input) {
  const err = new TypeError(`Invalid URL`);
  err.code = "ERR_INVALID_URL";
  err.input = input;
  return err;
}

class URL {
  constructor(url, base = undefined) {
    if (arguments.length === 0) throw new TypeError('The "url" argument must be specified');
    const href = String(url);
    const parts = base === undefined ? ops.urlParse(href) : ops.urlParse(href, String(base));
    if (!parts) throw invalidUrl(href);
    this[kParts] = parts;
    this[kSearchParams] = new URLSearchParams(parts.search);
    this[kSearchParams][kUrl] = this;
  }
  static canParse(url, base = undefined) {
    return (base === undefined ? ops.urlParse(String(url)) : ops.urlParse(String(url), String(base))) !== null;
  }
  static parse(url, base = undefined) {
    try {
      return new URL(url, base);
    } catch {
      return null;
    }
  }
  _updateSearch(search) {
    this[kParts] = ops.urlSet(this[kParts].href, "search", search) || this[kParts];
  }
  #set(field, value) {
    const next = ops.urlSet(this[kParts].href, field, String(value));
    if (next) this[kParts] = next;
    if (field === "search" || field === "href") this[kSearchParams]._setFromUrl(this[kParts].search);
  }
  get href() {
    return this[kParts].href;
  }
  set href(v) {
    this.#set("href", v);
  }
  get origin() {
    return this[kParts].origin;
  }
  get protocol() {
    return this[kParts].protocol;
  }
  set protocol(v) {
    this.#set("protocol", v);
  }
  get username() {
    return this[kParts].username;
  }
  set username(v) {
    this.#set("username", v);
  }
  get password() {
    return this[kParts].password;
  }
  set password(v) {
    this.#set("password", v);
  }
  get host() {
    return this[kParts].host;
  }
  set host(v) {
    this.#set("host", v);
  }
  get hostname() {
    return this[kParts].hostname;
  }
  set hostname(v) {
    this.#set("hostname", v);
  }
  get port() {
    return this[kParts].port;
  }
  set port(v) {
    this.#set("port", v);
  }
  get pathname() {
    return this[kParts].pathname;
  }
  set pathname(v) {
    this.#set("pathname", v);
  }
  get search() {
    return this[kParts].search;
  }
  set search(v) {
    this.#set("search", v);
  }
  get searchParams() {
    return this[kSearchParams];
  }
  get hash() {
    return this[kParts].hash;
  }
  set hash(v) {
    this.#set("hash", v);
  }
  toString() {
    return this.href;
  }
  toJSON() {
    return this.href;
  }
  get [Symbol.toStringTag]() {
    return "URL";
  }
  [Symbol.for("nodejs.util.inspect.custom")](depth, options) {
    const obj = {
      href: this.href,
      origin: this.origin,
      protocol: this.protocol,
      username: this.username,
      password: this.password,
      host: this.host,
      hostname: this.hostname,
      port: this.port,
      pathname: this.pathname,
      search: this.search,
      searchParams: this.searchParams,
      hash: this.hash,
    };
    return `URL ${internal.inspect(obj, options)}`;
  }
}

internal.URL = URL;
internal.URLSearchParams = URLSearchParams;
internal.hidden(globalThis, "URL", URL);
internal.hidden(globalThis, "URLSearchParams", URLSearchParams);
