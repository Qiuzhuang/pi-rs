// MessageEvent, MessageChannel/MessagePort and BroadcastChannel (in-process: there are no
// workers), and `navigator`. Loaded on first access to one of these globals.

const { Event, EventTarget } = internal;

class MessageEvent extends Event {
  #data;
  #origin;
  #lastEventId;
  #source;
  #ports;
  constructor(type, init = {}) {
    super(type, init);
    this.#data = init?.data === undefined ? null : init.data;
    this.#origin = init?.origin === undefined ? "" : String(init.origin);
    this.#lastEventId = init?.lastEventId === undefined ? "" : String(init.lastEventId);
    this.#source = init?.source ?? null;
    this.#ports = Object.freeze([...(init?.ports ?? [])]);
  }
  get data() {
    return this.#data;
  }
  get origin() {
    return this.#origin;
  }
  get lastEventId() {
    return this.#lastEventId;
  }
  get source() {
    return this.#source;
  }
  get ports() {
    return this.#ports;
  }
}

function defineHandler(proto, name) {
  const key = Symbol(name);
  Object.defineProperty(proto, `on${name}`, {
    configurable: true,
    enumerable: true,
    get() {
      return this[key] ?? null;
    },
    set(fn) {
      if (this[key]) this.removeEventListener(name, this[key]);
      this[key] = typeof fn === "function" ? fn : null;
      if (this[key]) {
        this.addEventListener(name, this[key]);
        if (name === "message") this.start?.();
      }
    },
  });
}

class MessagePort extends EventTarget {
  #peer = null;
  #queue = [];
  #started = false;
  #closed = false;
  #scheduled = false;
  #refed = true;
  constructor(token) {
    if (token !== kCreate) throw new TypeError("Illegal constructor");
    super();
  }
  static _entangle(a, b) {
    a.#peer = b;
    b.#peer = a;
  }
  postMessage(message, transfer) {
    if (this.#closed || !this.#peer) return;
    const options = Array.isArray(transfer) ? { transfer } : transfer;
    const data = structuredClone(message, options?.transfer ? { transfer: options.transfer.filter((t) => !(t instanceof MessagePort)) } : undefined);
    const ports = (options?.transfer ?? []).filter((t) => t instanceof MessagePort);
    this.#peer._enqueue(data, ports);
  }
  _enqueue(data, ports) {
    if (this.#closed) return;
    this.#queue.push([data, ports]);
    this._schedule();
  }
  _schedule() {
    if (!this.#started || this.#scheduled || this.#queue.length === 0) return;
    this.#scheduled = true;
    setImmediate(() => {
      this.#scheduled = false;
      while (this.#started && !this.#closed && this.#queue.length) {
        const [data, ports] = this.#queue.shift();
        this.dispatchEvent(new MessageEvent("message", { data, ports }));
      }
    });
  }
  start() {
    this.#started = true;
    this._schedule();
  }
  close() {
    if (this.#closed) return;
    this.#closed = true;
    const peer = this.#peer;
    this.#peer = null;
    if (peer) peer.close();
    queueMicrotask(() => this.dispatchEvent(new Event("close")));
  }
  ref() {
    this.#refed = true;
    return this;
  }
  unref() {
    this.#refed = false;
    return this;
  }
  hasRef() {
    return this.#refed;
  }
  // Node's EventEmitter-style API on ports.
  on(name, fn) {
    if (name === "message") {
      const wrapped = (e) => fn(e.data);
      fn[kWrapped] = wrapped;
      this.addEventListener(name, wrapped);
      this.start();
    } else {
      this.addEventListener(name, fn);
    }
    return this;
  }
  once(name, fn) {
    const wrapped = name === "message" ? (e) => fn(e.data) : fn;
    this.addEventListener(name, wrapped, { once: true });
    if (name === "message") this.start();
    return this;
  }
  off(name, fn) {
    this.removeEventListener(name, fn[kWrapped] ?? fn);
    return this;
  }
  removeListener(name, fn) {
    return this.off(name, fn);
  }
  addListener(name, fn) {
    return this.on(name, fn);
  }
  get [Symbol.toStringTag]() {
    return "MessagePort";
  }
}
const kCreate = Symbol("create");
const kWrapped = Symbol("wrapped");
defineHandler(MessagePort.prototype, "message");
defineHandler(MessagePort.prototype, "messageerror");

class MessageChannel {
  #port1;
  #port2;
  constructor() {
    this.#port1 = new MessagePort(kCreate);
    this.#port2 = new MessagePort(kCreate);
    MessagePort._entangle(this.#port1, this.#port2);
  }
  get port1() {
    return this.#port1;
  }
  get port2() {
    return this.#port2;
  }
}

const channels = new Map(); // name -> Set<BroadcastChannel>

class BroadcastChannel extends EventTarget {
  #name;
  #closed = false;
  constructor(name) {
    if (arguments.length === 0) throw new TypeError('The "name" argument must be specified');
    super();
    this.#name = String(name);
    if (!channels.has(this.#name)) channels.set(this.#name, new Set());
    channels.get(this.#name).add(this);
  }
  get name() {
    return this.#name;
  }
  postMessage(message) {
    if (this.#closed) throw new DOMException("BroadcastChannel is closed.", "InvalidStateError");
    for (const other of channels.get(this.#name) ?? []) {
      if (other === this) continue;
      const data = structuredClone(message);
      setImmediate(() => {
        if (!other.#closed) other.dispatchEvent(new MessageEvent("message", { data }));
      });
    }
  }
  close() {
    if (this.#closed) return;
    this.#closed = true;
    channels.get(this.#name)?.delete(this);
  }
  ref() {
    return this;
  }
  unref() {
    return this;
  }
}
defineHandler(BroadcastChannel.prototype, "message");
defineHandler(BroadcastChannel.prototype, "messageerror");

class Navigator {
  constructor(token) {
    if (token !== kCreate) throw new TypeError("Illegal constructor");
  }
  get hardwareConcurrency() {
    return internal.requireBuiltin("os").availableParallelism?.() ?? internal.requireBuiltin("os").cpus().length;
  }
  get language() {
    return "en-US";
  }
  get languages() {
    return ["en-US"];
  }
  get platform() {
    return process.platform;
  }
  get userAgent() {
    return `Node.js/${process.version.slice(1).split(".")[0]}`;
  }
  get [Symbol.toStringTag]() {
    return "Navigator";
  }
}

internal.messaging = { MessageEvent, MessagePort, MessageChannel, BroadcastChannel };
globalThis.MessageEvent = MessageEvent;
globalThis.MessagePort = MessagePort;
globalThis.MessageChannel = MessageChannel;
globalThis.BroadcastChannel = BroadcastChannel;
globalThis.Navigator = Navigator;
globalThis.navigator = new Navigator(kCreate);
