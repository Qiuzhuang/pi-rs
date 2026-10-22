// Dump highlight.js 10.7.3 language definitions as JSON graphs (object identity preserved).
// Usage: node dump.js <hljs package lib dir> <out.json>
const fs = require("fs");
const path = require("path");
const libDir = process.argv[2];
const out = process.argv[3];
const hljs = require(path.join(libDir, "core.js"));

const mathematicaSource = fs.readFileSync(path.join(libDir, "languages/mathematica.js"), "utf8");
const symbolsMatch = /const SYSTEM_SYMBOLS = (\[[\s\S]*?\]);/.exec(mathematicaSource);
const SYSTEM_SYMBOLS = eval(symbolsMatch[1]);

function identifyFn(fn) {
  const src = fn.toString();
  if (src.includes("m.index !== 0")) return { $fn: "shebang" };
  if (src.includes("resp.data._beginMatch = m[1]")) return { $fn: "end_same_as_begin_begin" };
  if (src.includes("resp.data._beginMatch !== m[1]")) return { $fn: "end_same_as_begin_end" };
  if (src.includes("hasClosingTag")) return { $fn: "jsx_is_truly_opening_tag" };
  if (src.includes("SYSTEM_SYMBOLS_SET")) return { $fn: "in_set", set: SYSTEM_SYMBOLS };
  if (src.includes("mode.beforeMatch")) return { $fn: "before_match_extension" };
  throw new Error("unknown callback: " + src);
}

function dumpLanguage(name, def) {
  const lang = def(hljs);
  const ids = new Map();
  const nodes = [];
  function enc(v) {
    if (v === null || v === undefined) return null;
    if (v instanceof RegExp) return { $re: v.source, flags: v.flags };
    if (typeof v === "function") return identifyFn(v);
    if (Array.isArray(v)) return v.map(enc);
    if (typeof v === "object") {
      if (ids.has(v)) return { $ref: ids.get(v) };
      const id = nodes.length;
      ids.set(v, id);
      nodes.push(null);
      const o = {};
      if (Object.isFrozen(v)) o.$frozen = true;
      for (const k in v) {
        if (k === "rawDefinition") continue;
        o[k] = enc(v[k]);
      }
      nodes[id] = o;
      return { $ref: id };
    }
    return v;
  }
  const root = enc(lang);
  return { name, root, nodes };
}

const files = fs.readdirSync(path.join(libDir, "languages")).filter((f) => f.endsWith(".js"));
// Registration order of lib/index.js.
const indexSource = fs.readFileSync(path.join(libDir, "index.js"), "utf8");
const order = [...indexSource.matchAll(/registerLanguage\('([^']+)', require\('\.\/languages\/([^']+)'\)\)/g)].map(
  (m) => [m[1], m[2]],
);
const result = [];
for (const [name, file] of order) {
  const def = require(path.join(libDir, "languages", file));
  result.push(dumpLanguage(name, def));
}
fs.writeFileSync(out, JSON.stringify(result));
console.log(order.length, "languages", files.length, "files");
