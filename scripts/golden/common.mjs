// Shared setup for the golden generators.
//
// Every generator renders components of the TypeScript reference with a fixed environment and
// writes `{ cases: [{ name, kind, input, widths, out: { dark: [[line...] per width], light: ... } }] }`
// to `crates/pi-coding-agent/tests/fixtures/golden/<file>.json`. The Rust tests in
// `crates/pi-coding-agent/tests/golden_ui.rs` rebuild each case from `kind` + `input` and compare
// the rendered lines byte for byte.
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";

export const REF = process.env.PI_TS_REF;
if (!REF) throw new Error("PI_TS_REF must point at the TypeScript pi checkout");

const here = path.dirname(fileURLToPath(import.meta.url));
export const OUT_DIR = process.env.GOLDEN_OUT ?? path.resolve(here, "../../crates/pi-coding-agent/tests/fixtures/golden");

/** Fixed environment shared with the Rust side (see `golden_ui.rs`). */
export const HOME = "/home/golden";
export const CWD = "/home/golden/project";
export const WIDTHS = [40, 80, 120];
export const THEMES = ["dark", "light"];
/** 2026-01-15T12:00:00Z, used wherever components read the clock. */
export const NOW = 1768478400000;

/** Import a module of the reference by its path below `packages/`. */
export function ref(rel) {
	return import(pathToFileURL(path.join(REF, "packages", rel)).href);
}

const tui = await ref("tui/src/index.ts");
const themeModule = await ref("coding-agent/src/modes/interactive/theme/theme.ts");

/** Fixed terminal capabilities: truecolor, no inline images, no hyperlinks unless a case asks. */
export function setCaps(overrides = {}) {
	tui.setCapabilities({ images: null, trueColor: true, hyperlinks: false, ...overrides });
}

export function useTheme(name) {
	setCaps();
	themeModule.initTheme(name, false);
}

export class Golden {
	constructor(file) {
		this.file = file;
		this.cases = [];
		this.names = new Set();
	}

	/**
	 * Render `build(input)` for every theme and width. `build` returns a component, or a function
	 * `(width) => string[]` for cases that render something other than one component.
	 * `options.widths` / `options.themes` override the defaults.
	 */
	add(name, kind, input, build, options = {}) {
		if (this.names.has(name)) throw new Error(`duplicate case ${name}`);
		this.names.add(name);
		const widths = options.widths ?? WIDTHS;
		const themes = options.themes ?? THEMES;
		const out = {};
		for (const themeName of themes) {
			useTheme(themeName);
			if (options.caps) setCaps(options.caps);
			const built = build(input);
			const render = typeof built === "function" ? built : (w) => built.render(w);
			out[themeName] = widths.map((w) => render(w));
			if (built && typeof built.dispose === "function") built.dispose();
		}
		this.cases.push({ name, kind, input, widths, ...(options.caps ? { caps: options.caps } : {}), out });
	}

	write() {
		fs.mkdirSync(OUT_DIR, { recursive: true });
		const file = path.join(OUT_DIR, this.file);
		fs.writeFileSync(file, `${JSON.stringify({ cases: this.cases }, null, 1)}\n`);
		console.log(`${this.file}: ${this.cases.length} cases`);
	}
}

/** A TUI stand-in for components that only call `requestRender`. */
export function fakeTui() {
	return { requestRender() {}, terminal: { columns: 80, rows: 24 } };
}

export { tui, themeModule };
