// Node module-resolution hook that maps the pi workspace packages
// (`@earendil-works/pi-tui`, `@earendil-works/pi-ai/compat`, ...) to their TypeScript sources in
// the reference checkout, mirroring the aliases in the reference's `vitest.base.ts`.
//
// Used by `scripts/golden/run.sh` via `node --import ./register.mjs`.
import fs from "node:fs";
import path from "node:path";
import { pathToFileURL } from "node:url";

const REF = process.env.PI_TS_REF;
if (!REF) throw new Error("PI_TS_REF must point at the TypeScript pi checkout");

const PACKAGE_DIRS = {
	"pi-tui": "tui",
	"pi-ai": "ai",
	"pi-agent-core": "agent",
	"pi-mcp": "mcp",
	"pi-telemetry": "telemetry",
	"pi-codemode": "codemode",
	"pi-coding-agent": "coding-agent",
	chord: "chord",
};

function resolveWorkspace(specifier) {
	const match = /^@(?:earendil-works|mariozechner)\/([^/]+)(?:\/(.+))?$/.exec(specifier);
	if (!match) return undefined;
	const dir = PACKAGE_DIRS[match[1]];
	if (!dir) return undefined;
	const src = path.join(REF, "packages", dir, "src");
	const sub = match[2] ?? "index";
	for (const candidate of [`${sub}.ts`, path.join(sub, "index.ts"), sub]) {
		const file = path.join(src, candidate);
		if (fs.existsSync(file) && fs.statSync(file).isFile()) return pathToFileURL(file).href;
	}
	return undefined;
}

export async function resolve(specifier, context, nextResolve) {
	const mapped = resolveWorkspace(specifier);
	if (mapped) return { url: mapped, shortCircuit: true };
	return nextResolve(specifier, context);
}
