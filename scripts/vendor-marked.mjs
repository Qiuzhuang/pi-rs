#!/usr/bin/env node
// Regenerates crates/pi-coding-agent/js/vendor/marked.js from the `marked` npm package (the version
// pi-tui depends on). pi-tui re-exports `Marked` to extensions; pi-rs serves it as the virtual
// module `pi-rs:marked`.
//
// Usage: node scripts/vendor-marked.mjs <dir containing node_modules/marked>

import { readFileSync, writeFileSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const root = resolve(process.argv[2] ?? ".");
const dir = join(root, "node_modules", "marked");
const version = JSON.parse(readFileSync(join(dir, "package.json"), "utf8")).version;
const license = readFileSync(join(dir, "LICENSE"), "utf8").trim();
const source = readFileSync(join(dir, "lib", "marked.esm.js"), "utf8").replace(/\n\/\/# sourceMappingURL=.*\s*$/, "\n");
const out = join(dirname(fileURLToPath(import.meta.url)), "..", "crates", "pi-coding-agent", "js", "vendor", "marked.js");
writeFileSync(out, `// marked ${version} (lib/marked.esm.js), copied by scripts/vendor-marked.mjs. Do not edit.\n/*\n${license}\n*/\n${source}`);
console.log(`marked ${version} -> ${out}`);
