#!/usr/bin/env node
// Regenerates crates/pi-coding-agent/js/vendor/typebox/ from the `typebox` npm package.
//
// Extensions running in pi-jsrt import `typebox`, `typebox/value` and `typebox/compile` (and the
// `@sinclair/typebox` aliases). pi ships the real library so tool schemas are byte-identical to
// TypeScript pi. The three entry points are bundled with esbuild code splitting; chunk imports are
// rewritten to `pi-rs:typebox/<chunk>` virtual-module specifiers.
//
// Usage: node scripts/vendor-typebox.mjs <dir containing node_modules with typebox and esbuild>
//   e.g. node scripts/vendor-typebox.mjs ../pi   (the TypeScript reference checkout)

import { execFileSync } from "node:child_process";
import { mkdirSync, mkdtempSync, readdirSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const root = resolve(process.argv[2] ?? ".");
const modules = join(root, "node_modules");
const pkg = JSON.parse(readFileSync(join(modules, "typebox", "package.json"), "utf8"));
const license = readFileSync(join(modules, "typebox", "license"), "utf8").trim();
const out = join(dirname(fileURLToPath(import.meta.url)), "..", "crates", "pi-coding-agent", "js", "vendor", "typebox");

const work = mkdtempSync(join(tmpdir(), "vendor-typebox-"));
const entries = { index: "typebox", value: "typebox/value", compile: "typebox/compile" };
for (const [name, specifier] of Object.entries(entries)) {
	writeFileSync(join(work, `${name}.mjs`), `export * from "${specifier}";\nexport { default } from "${specifier}";\n`);
}
const build = join(work, "out");
execFileSync(
	join(modules, ".bin", "esbuild"),
	[
		...Object.keys(entries).map((name) => join(work, `${name}.mjs`)),
		"--bundle",
		"--splitting",
		"--format=esm",
		"--platform=neutral",
		"--minify",
		"--legal-comments=none",
		"--chunk-names=chunk-[hash]",
		`--outdir=${build}`,
		"--log-level=warning",
	],
	{ env: { ...process.env, NODE_PATH: modules }, stdio: "inherit" },
);

// Stable names for the shared chunks: `chunk-<n>` in order of first use by the entry points.
const files = readdirSync(build).filter((f) => f.endsWith(".js"));
const chunkNames = new Map();
for (const name of [...Object.keys(entries).map((n) => `${n}.js`), ...files]) {
	const src = readFileSync(join(build, name), "utf8");
	for (const [, chunk] of src.matchAll(/(?:from|import)"\.\/(chunk-[A-Z0-9]+)\.js"/g)) {
		if (!chunkNames.has(chunk)) chunkNames.set(chunk, `chunk-${chunkNames.size + 1}`);
	}
}

rmSync(out, { recursive: true, force: true });
mkdirSync(out, { recursive: true });
const header = `// typebox ${pkg.version} (${Object.values(entries).join(", ")}), bundled by scripts/vendor-typebox.mjs.\n// Do not edit; regenerate instead.\n/*\n${license}\n*/\n`;
if (/"\.\/chunk-/.test(files.map((f) => readFileSync(join(build, f), "utf8")).join("").replace(/(?:from|import)"\.\/chunk-[A-Z0-9]+\.js"/g, ""))) {
	throw new Error("unexpected chunk reference in the esbuild output");
}
for (const file of files) {
	const base = file.replace(/\.js$/, "");
	const name = chunkNames.get(base) ?? base;
	const src = readFileSync(join(build, file), "utf8").replace(
		/(from|import)"\.\/(chunk-[A-Z0-9]+)\.js"/g,
		(_, keyword, chunk) => `${keyword}"pi-rs:typebox/${chunkNames.get(chunk)}"`,
	);
	writeFileSync(join(out, `${name}.js`), header + src);
}
rmSync(work, { recursive: true, force: true });
console.log(`typebox ${pkg.version} -> ${out}: ${readdirSync(out).join(", ")}`);
