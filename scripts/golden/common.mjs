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
const { KeybindingsManager } = await ref("coding-agent/src/core/keybindings.ts");

// Interactive mode installs the app keybindings globally; key hints read them from there.
tui.setKeybindings(new KeybindingsManager({}));
// Interactive mode loads every highlight.js grammar right after startup; render with all of them.
const { loadAllHighlightLanguages } = await ref("coding-agent/src/utils/syntax-highlight.ts");
await loadAllHighlightLanguages();

/** Fixed terminal capabilities: truecolor, no inline images, no hyperlinks unless a case asks. */
export function setCaps(overrides = {}) {
	tui.setCapabilities({ images: null, trueColor: true, hyperlinks: false, ...overrides });
}

/** Select a theme; the theme's color mode follows the capabilities set before it is created. */
export function useTheme(name, caps) {
	setCaps(caps);
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
		const { widths, themes, out } = this.#start(name, options);
		for (const themeName of themes) {
			this.#prepare(themeName, options);
			out[themeName] = this.#render(build(input), widths);
		}
		this.#finish(name, kind, input, widths, options, out);
	}

	/** Like `add`, for builders that return a promise (components that load data asynchronously). */
	async addAsync(name, kind, input, build, options = {}) {
		const { widths, themes, out } = this.#start(name, options);
		for (const themeName of themes) {
			this.#prepare(themeName, options);
			out[themeName] = this.#render(await build(input), widths);
		}
		this.#finish(name, kind, input, widths, options, out);
	}

	#start(name, options) {
		if (this.names.has(name)) throw new Error(`duplicate case ${name}`);
		this.names.add(name);
		return { widths: options.widths ?? WIDTHS, themes: options.themes ?? THEMES, out: {} };
	}

	#prepare(themeName, options) {
		useTheme(themeName, options.caps);
	}

	#render(built, widths) {
		const render = typeof built === "function" ? built : (w) => built.render(w);
		const lines = widths.map((w) => render(w));
		if (built && typeof built.dispose === "function") built.dispose();
		return lines;
	}

	#finish(name, kind, input, widths, options, out) {
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
