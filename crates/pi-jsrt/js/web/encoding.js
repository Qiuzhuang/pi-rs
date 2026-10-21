// TextEncoder / TextDecoder / atob / btoa

class TextEncoder {
  get encoding() {
    return "utf-8";
  }
  encode(input = "") {
    return ops.utf8Encode(String(input));
  }
  encodeInto(source, destination) {
    const bytes = ops.utf8Encode(String(source));
    let written = bytes.length;
    let read = String(source).length;
    if (written > destination.length) {
      // Encode only complete code points that fit.
      written = 0;
      read = 0;
      for (const ch of String(source)) {
        const b = ops.utf8Encode(ch);
        if (written + b.length > destination.length) break;
        destination.set(b, written);
        written += b.length;
        read += ch.length;
      }
      return { read, written };
    }
    destination.set(bytes, 0);
    return { read, written };
  }
  get [Symbol.toStringTag]() {
    return "TextEncoder";
  }
}

const decoderLabels = {
  "utf-8": "utf-8",
  utf8: "utf-8",
  "unicode-1-1-utf-8": "utf-8",
  "utf-16le": "utf-16le",
  "utf-16": "utf-16le",
  ucs2: "utf-16le",
  "ucs-2": "utf-16le",
  latin1: "windows-1252",
  "iso-8859-1": "windows-1252",
  ascii: "windows-1252",
  "us-ascii": "windows-1252",
  "windows-1252": "windows-1252",
};

function toU8(input) {
  if (input === undefined) return new Uint8Array(0);
  if (input instanceof Uint8Array) return input;
  if (input instanceof ArrayBuffer || (typeof SharedArrayBuffer !== "undefined" && input instanceof SharedArrayBuffer)) {
    return new Uint8Array(input);
  }
  if (ArrayBuffer.isView(input)) return new Uint8Array(input.buffer, input.byteOffset, input.byteLength);
  throw new TypeError('The "input" argument must be an instance of ArrayBuffer or ArrayBufferView');
}

// Length of an incomplete UTF-8 sequence at the end of `bytes` (0 if complete).
function incompleteTail(bytes) {
  const n = bytes.length;
  for (let i = 1; i <= Math.min(3, n); i++) {
    const b = bytes[n - i];
    if ((b & 0xc0) === 0x80) continue; // continuation byte
    let need = 0;
    if ((b & 0xe0) === 0xc0) need = 2;
    else if ((b & 0xf0) === 0xe0) need = 3;
    else if ((b & 0xf8) === 0xf0) need = 4;
    return need > i ? i : 0;
  }
  return 0;
}

class TextDecoder {
  #encoding;
  #fatal;
  #ignoreBOM;
  #pending = null;
  #bomSeen = false;
  constructor(label = "utf-8", options = {}) {
    const enc = decoderLabels[String(label).trim().toLowerCase()];
    if (!enc) {
      const err = new RangeError(`The "${label}" encoding is not supported`);
      err.code = "ERR_ENCODING_NOT_SUPPORTED";
      throw err;
    }
    this.#encoding = enc;
    this.#fatal = Boolean(options && options.fatal);
    this.#ignoreBOM = Boolean(options && options.ignoreBOM);
  }
  get encoding() {
    return this.#encoding;
  }
  get fatal() {
    return this.#fatal;
  }
  get ignoreBOM() {
    return this.#ignoreBOM;
  }
  decode(input, options = {}) {
    let bytes = toU8(input);
    const stream = Boolean(options && options.stream);
    if (this.#pending) {
      const merged = new Uint8Array(this.#pending.length + bytes.length);
      merged.set(this.#pending, 0);
      merged.set(bytes, this.#pending.length);
      bytes = merged;
      this.#pending = null;
    }
    let out;
    if (this.#encoding === "utf-8") {
      if (stream) {
        const tail = incompleteTail(bytes);
        if (tail) {
          this.#pending = bytes.slice(bytes.length - tail);
          bytes = bytes.subarray(0, bytes.length - tail);
        }
      }
      if (!this.#ignoreBOM && !this.#bomSeen && bytes.length >= 3 && bytes[0] === 0xef && bytes[1] === 0xbb && bytes[2] === 0xbf) {
        bytes = bytes.subarray(3);
      }
      out = ops.utf8Decode(bytes, this.#fatal);
    } else if (this.#encoding === "utf-16le") {
      if (stream && bytes.length % 2) {
        this.#pending = bytes.slice(bytes.length - 1);
        bytes = bytes.subarray(0, bytes.length - 1);
      }
      let s = "";
      for (let i = 0; i + 1 < bytes.length; i += 2) s += String.fromCharCode(bytes[i] | (bytes[i + 1] << 8));
      if (!this.#ignoreBOM && !this.#bomSeen && s.charCodeAt(0) === 0xfeff) s = s.slice(1);
      out = s;
    } else {
      let s = "";
      for (let i = 0; i < bytes.length; i++) s += String.fromCharCode(bytes[i]);
      out = s;
    }
    if (bytes.length) this.#bomSeen = true;
    if (!stream) this.#bomSeen = false;
    return out;
  }
  get [Symbol.toStringTag]() {
    return "TextDecoder";
  }
}

function btoa(data) {
  if (arguments.length === 0) throw new TypeError("The \"data\" argument must be specified");
  const s = String(data);
  const bytes = new Uint8Array(s.length);
  for (let i = 0; i < s.length; i++) {
    const c = s.charCodeAt(i);
    if (c > 255) {
      const err = new Error("Invalid character");
      err.name = "InvalidCharacterError";
      throw err;
    }
    bytes[i] = c;
  }
  return ops.base64Encode(bytes, false);
}

function atob(data) {
  if (arguments.length === 0) throw new TypeError("The \"data\" argument must be specified");
  const s = String(data).replace(/[\t\n\f\r ]/g, "");
  if (/[^A-Za-z0-9+/=]/.test(s) || s.length % 4 === 1 || /=[^=]/.test(s)) {
    const err = new Error("The string to be decoded is not correctly encoded.");
    err.name = "InvalidCharacterError";
    throw err;
  }
  const bytes = ops.base64Decode(s, false);
  let out = "";
  for (let i = 0; i < bytes.length; i++) out += String.fromCharCode(bytes[i]);
  return out;
}

internal.TextEncoder = TextEncoder;
internal.TextDecoder = TextDecoder;
for (const [name, value] of Object.entries({ TextEncoder, TextDecoder, atob, btoa })) {
  internal.hidden(globalThis, name, value);
}
