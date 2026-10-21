// node:crypto (hashing, HMAC, randomness)

const Buffer = internal.Buffer;
const hashes = ["md5", "sha1", "sha224", "sha256", "sha384", "sha512"];

function toBytes(data, encoding) {
  if (typeof data === "string") return Buffer.from(data, encoding ?? "utf8");
  if (ArrayBuffer.isView(data)) return new Uint8Array(data.buffer, data.byteOffset, data.byteLength);
  if (data instanceof ArrayBuffer) return new Uint8Array(data);
  throw new TypeError('The "data" argument must be of type string or an instance of Buffer, TypedArray, or DataView.');
}

function checkAlg(alg) {
  const a = String(alg).toLowerCase().replace(/-/g, "");
  if (!hashes.includes(a)) {
    const err = new Error("Digest method not supported");
    err.code = "ERR_CRYPTO_INVALID_DIGEST";
    throw err;
  }
  return a;
}

class Hash {
  #alg;
  #chunks = [];
  #done = false;
  constructor(alg) {
    this.#alg = checkAlg(alg);
  }
  update(data, encoding) {
    if (this.#done) {
      const err = new Error("Digest already called");
      err.code = "ERR_CRYPTO_HASH_FINALIZED";
      throw err;
    }
    this.#chunks.push(Buffer.from(toBytes(data, encoding)));
    return this;
  }
  digest(encoding) {
    if (this.#done) {
      const err = new Error("Digest already called");
      err.code = "ERR_CRYPTO_HASH_FINALIZED";
      throw err;
    }
    this.#done = true;
    const out = ops.hash(this.#alg, Buffer.concat(this.#chunks));
    const b = Buffer.from(out.buffer, out.byteOffset, out.byteLength);
    return encoding && encoding !== "buffer" ? b.toString(encoding) : b;
  }
  copy() {
    const h = new Hash(this.#alg);
    for (const c of this.#chunks) h.update(c);
    return h;
  }
}

class Hmac {
  #alg;
  #key;
  #chunks = [];
  constructor(alg, key) {
    this.#alg = checkAlg(alg);
    this.#key = typeof key === "object" && key?.export ? key.export() : toBytes(key);
  }
  update(data, encoding) {
    this.#chunks.push(Buffer.from(toBytes(data, encoding)));
    return this;
  }
  digest(encoding) {
    const out = ops.hmac(this.#alg, this.#key, Buffer.concat(this.#chunks));
    const b = Buffer.from(out.buffer, out.byteOffset, out.byteLength);
    return encoding && encoding !== "buffer" ? b.toString(encoding) : b;
  }
}

function randomBytes(size, cb) {
  const u = ops.randomBytes(size);
  const b = Buffer.from(u.buffer, u.byteOffset, u.byteLength);
  if (typeof cb === "function") {
    process.nextTick(cb, null, b);
    return;
  }
  return b;
}

function randomInt(min, max, cb) {
  if (max === undefined || typeof max === "function") {
    cb = max;
    max = min;
    min = 0;
  }
  const range = max - min;
  if (!(range > 0)) throw new RangeError(`The value of "max" is out of range. It must be greater than the value of "min" (${min}).`);
  const buf = ops.randomBytes(6);
  let n = 0;
  for (const b of buf) n = n * 256 + b;
  const v = min + (n % range);
  if (typeof cb === "function") {
    process.nextTick(cb, null, v);
    return;
  }
  return v;
}

function timingSafeEqual(a, b) {
  if (a.byteLength !== b.byteLength) {
    const err = new RangeError("Input buffers must have the same byte length");
    err.code = "ERR_CRYPTO_TIMING_SAFE_EQUAL_LENGTH";
    throw err;
  }
  const x = toBytes(a);
  const y = toBytes(b);
  let diff = 0;
  for (let i = 0; i < x.length; i++) diff |= x[i] ^ y[i];
  return diff === 0;
}

const unsupported = (name) => () => {
  throw new Error(`crypto.${name} is not supported in pi-jsrt`);
};

const crypto = {
  createHash: (alg) => new Hash(alg),
  createHmac: (alg, key) => new Hmac(alg, key),
  hash: (alg, data, encoding = "hex") => new Hash(alg).update(data).digest(encoding),
  getHashes: () => hashes.slice(),
  randomBytes,
  pseudoRandomBytes: randomBytes,
  randomFillSync: (buf) => {
    ops.fillRandom(new Uint8Array(buf.buffer ?? buf, buf.byteOffset ?? 0, buf.byteLength));
    return buf;
  },
  randomFill: (buf, cb) => {
    crypto.randomFillSync(buf);
    process.nextTick(cb, null, buf);
  },
  randomInt,
  randomUUID: () => ops.randomUUID(),
  getRandomValues: (a) => ops.fillRandom(a),
  timingSafeEqual,
  webcrypto: internal.webcrypto,
  subtle: internal.webcrypto.subtle,
  constants: {},
  Hash,
  Hmac,
  createCipheriv: unsupported("createCipheriv"),
  createDecipheriv: unsupported("createDecipheriv"),
  createSign: unsupported("createSign"),
  createVerify: unsupported("createVerify"),
  generateKeyPairSync: unsupported("generateKeyPairSync"),
  pbkdf2Sync: unsupported("pbkdf2Sync"),
  scryptSync: unsupported("scryptSync"),
};
internal.defineBuiltin("crypto", () => crypto);
