// node:dns — lookup()/promises.lookup() through getaddrinfo (tokio), resolve4/resolve6 on top of
// it. Other record types need a DNS client pi-jsrt does not have: they fail with ENOTIMP.

const net = internal.requireBuiltin("net");

const ADDRCONFIG = 32;
const V4MAPPED = 8;
const ALL = 16;
const errorCodes = {
  NODATA: "ENODATA", FORMERR: "EFORMERR", SERVFAIL: "ESERVFAIL", NOTFOUND: "ENOTFOUND", NOTIMP: "ENOTIMP",
  REFUSED: "EREFUSED", BADQUERY: "EBADQUERY", BADNAME: "EBADNAME", BADFAMILY: "EBADFAMILY", BADRESP: "EBADRESP",
  CONNREFUSED: "ECONNREFUSED", TIMEOUT: "ETIMEOUT", EOF: "EOF", FILE: "EFILE", NOMEM: "ENOMEM",
  DESTRUCTION: "EDESTRUCTION", BADSTR: "EBADSTR", BADFLAGS: "EBADFLAGS", NONAME: "ENONAME", BADHINTS: "EBADHINTS",
  NOTINITIALIZED: "ENOTINITIALIZED", LOADIPHLPAPI: "ELOADIPHLPAPI", ADDRGETNETWORKPARAMS: "EADDRGETNETWORKPARAMS",
  CANCELLED: "ECANCELLED",
};

let defaultOrder = "verbatim";

function lookupError(err, hostname, syscall = "getaddrinfo") {
  const code = err?.code ?? "ENOTFOUND";
  return Object.assign(new Error(`${syscall} ${code} ${hostname}`), { code, errno: code === "ENOTFOUND" ? -3008 : undefined, syscall, hostname });
}

async function lookupAll(hostname, options) {
  const family = options.family === "IPv4" ? 4 : options.family === "IPv6" ? 6 : Number(options.family ?? 0);
  const ipFamily = net.isIP(hostname);
  let list;
  if (ipFamily) list = [{ address: hostname, family: ipFamily }];
  else {
    try {
      list = await ops.dnsLookup(String(hostname));
    } catch (err) {
      throw lookupError(err, hostname);
    }
  }
  if (family === 4 || family === 6) list = list.filter((a) => a.family === family);
  const order = options.order ?? (options.verbatim === false ? "ipv4first" : options.verbatim === true ? "verbatim" : defaultOrder);
  if (order === "ipv4first") list = [...list.filter((a) => a.family === 4), ...list.filter((a) => a.family === 6)];
  else if (order === "ipv6first") list = [...list.filter((a) => a.family === 6), ...list.filter((a) => a.family === 4)];
  if (list.length === 0) throw lookupError({ code: "ENOTFOUND" }, hostname);
  return list;
}

function lookup(hostname, options, callback) {
  if (typeof options === "function") {
    callback = options;
    options = {};
  } else if (typeof options === "number") {
    options = { family: options };
  }
  options = options ?? {};
  if (typeof callback !== "function") {
    throw Object.assign(new TypeError(`The "callback" argument must be of type function. Received ${typeof callback}`), { code: "ERR_INVALID_ARG_TYPE" });
  }
  if (!hostname) {
    process.nextTick(callback, null, options.all ? [] : null, options.all ? undefined : options.family === 6 ? 6 : 4);
    return {};
  }
  lookupAll(hostname, options).then(
    (list) => (options.all ? callback(null, list) : callback(null, list[0].address, list[0].family)),
    (err) => callback(err),
  );
  return {};
}

function notImplemented(syscall, hostname) {
  return Object.assign(new Error(`${syscall} ENOTIMP ${hostname}`), { code: "ENOTIMP", syscall, hostname });
}

async function resolveFamily(hostname, family, options) {
  const list = await lookupAll(hostname, { family }).catch((err) => {
    throw Object.assign(new Error(`query${family === 4 ? "A" : "AAAA"} ENOTFOUND ${hostname}`), { code: "ENOTFOUND", syscall: `query${family === 4 ? "A" : "AAAA"}`, hostname });
  });
  return options?.ttl ? list.map((a) => ({ address: a.address, ttl: 0 })) : list.map((a) => a.address);
}

const promiseResolvers = {
  resolve4: (hostname, options) => resolveFamily(hostname, 4, options),
  resolve6: (hostname, options) => resolveFamily(hostname, 6, options),
  resolve(hostname, rrtype = "A") {
    if (rrtype === "A") return resolveFamily(hostname, 4);
    if (rrtype === "AAAA") return resolveFamily(hostname, 6);
    return Promise.reject(notImplemented(`query${rrtype}`, hostname));
  },
  reverse: (ip) => Promise.reject(notImplemented("getHostByAddr", ip)),
  lookupService: (address, port) => Promise.reject(notImplemented("getnameinfo", address)),
};
for (const type of ["Any", "Caa", "Cname", "Mx", "Naptr", "Ns", "Ptr", "Soa", "Srv", "Tlsa", "Txt"]) {
  promiseResolvers[`resolve${type}`] = (hostname) => Promise.reject(notImplemented(`query${type.toUpperCase()}`, hostname));
}

function servers() {
  try {
    const text = internal.requireBuiltin("fs").readFileSync("/etc/resolv.conf", "utf8");
    return text.split("\n").map((l) => /^\s*nameserver\s+(\S+)/.exec(l)?.[1]).filter(Boolean);
  } catch {
    return [];
  }
}

const callbackify = (fn) => (...args) => {
  const cb = args.pop();
  fn(...args).then((v) => cb(null, v), (err) => cb(err));
};

class PromisesResolver {
  constructor(options) {
    this._servers = undefined;
    this.options = options;
  }
  getServers() {
    return this._servers ?? servers();
  }
  setServers(list) {
    this._servers = [...list];
  }
  setLocalAddress() {}
  cancel() {}
}
Object.assign(PromisesResolver.prototype, promiseResolvers);

class Resolver extends PromisesResolver {}
for (const [name, fn] of Object.entries(promiseResolvers)) Resolver.prototype[name] = callbackify(fn);

const promises = {
  ...promiseResolvers,
  lookup: (hostname, options = {}) => {
    if (typeof options === "number") options = { family: options };
    return lookupAll(hostname, options).then((list) => (options.all ? list : list[0]));
  },
  Resolver: PromisesResolver,
  getServers: servers,
  setServers() {},
  getDefaultResultOrder: () => defaultOrder,
  setDefaultResultOrder: (order) => {
    defaultOrder = order;
  },
  ...errorCodes,
};

const dns = {
  lookup,
  lookupService: callbackify(promiseResolvers.lookupService),
  Resolver,
  getDefaultResultOrder: () => defaultOrder,
  setDefaultResultOrder: (order) => {
    defaultOrder = order;
  },
  setServers() {},
  getServers: servers,
  ADDRCONFIG,
  ALL,
  V4MAPPED,
  ...errorCodes,
  promises,
};
for (const [name, fn] of Object.entries(promiseResolvers)) if (!(name in dns)) dns[name] = callbackify(fn);

internal.defineBuiltin("dns", () => dns);
internal.defineBuiltin("dns/promises", () => promises);
