// `@earendil-works/pi-tui` for extensions running in pi-jsrt.
//
// Components that extensions subclass or compose (Text, Box, Container, SelectList, ...) are
// JavaScript ports of the pi-tui TypeScript components, so they can call extension callbacks
// directly. Text measurement, key parsing and Markdown rendering call the Rust pi-tui crate
// through host functions. Input and Editor are backed by the Rust components.

import {
	call,
	componentRef,
	markRustBase,
	KeybindingsProxy,
	keybindingsProxy,
	RustComponent,
	rustCallback,
	tuiProxy,
	u,
} from "pi-rs:bridge";

const util = (fn, ...args) => call("tui.util", { fn, args });

// ---------------------------------------------------------------------------
// Constants and utilities
// ---------------------------------------------------------------------------

export const CURSOR_MARKER = "\x1b_pi:c\x07";
export const FAKE_CURSOR_START = "\x1b_pi:fc\x07";
export const FAKE_CURSOR_END = "\x1b_pi:/fc\x07";
export const VIEWPORT_TUI = Symbol.for("@earendil-works/pi-tui/viewport");

export function renderFakeCursor(text) {
	return `${FAKE_CURSOR_START}${text}${FAKE_CURSOR_END}`;
}

const widthCache = new Map();

export function visibleWidth(str) {
	if (str === undefined || str === null) return 0;
	str = String(str);
	if (str.length === 0) return 0;
	// Fast path: printable ASCII without escapes.
	let ascii = true;
	for (let i = 0; i < str.length; i++) {
		const c = str.charCodeAt(i);
		if (c < 0x20 || c > 0x7e) {
			ascii = false;
			break;
		}
	}
	if (ascii) return str.length;
	const cached = widthCache.get(str);
	if (cached !== undefined) return cached;
	const width = util("visibleWidth", str);
	if (widthCache.size > 2000) widthCache.clear();
	widthCache.set(str, width);
	return width;
}

export function truncateToWidth(text, maxWidth, ellipsis = "...", pad = false) {
	return util("truncateToWidth", String(text ?? ""), Math.floor(maxWidth), ellipsis, !!pad);
}

export function wrapTextWithAnsi(text, width) {
	return util("wrapTextWithAnsi", String(text ?? ""), Math.max(0, Math.floor(width)));
}

export function sliceByColumn(line, startCol, length, strict = false) {
	return util("sliceByColumn", String(line ?? ""), startCol, length, !!strict);
}

export function stripTerminalSequences(str) {
	return util("stripTerminalSequences", String(str ?? ""));
}

export function normalizeTerminalOutput(str) {
	return String(str ?? "");
}

export function applyBackgroundToLine(line, width, bgFn) {
	const paddingNeeded = Math.max(0, width - visibleWidth(line));
	return bgFn(line + " ".repeat(paddingNeeded));
}

export function flattenLines(_lines) {}

export const PUNCTUATION_REGEX = /[(){}[\]<>.,;:'"!?+\-=*/\\|&%^$#@~`]/;

export function isWhitespaceChar(char) {
	return /\s/.test(char);
}

export function isPunctuationChar(char) {
	return PUNCTUATION_REGEX.test(char);
}

export function matchesKey(data, keyId) {
	return !!util("matchesKey", String(data ?? ""), String(keyId ?? ""));
}

export function parseKey(data) {
	return u(util("parseKey", String(data ?? "")));
}

export function isKeyRelease(data) {
	return !!util("isKeyRelease", String(data ?? ""));
}

export function isKeyRepeat(data) {
	return !!util("isKeyRepeat", String(data ?? ""));
}

export function decodeKittyPrintable(data) {
	return u(util("decodeKittyPrintable", String(data ?? "")));
}

export function isKittyProtocolActive() {
	return !!util("isKittyProtocolActive");
}

export function setKittyProtocolActive(_active) {}

export const Key = {
	escape: "escape",
	esc: "esc",
	enter: "enter",
	return: "return",
	tab: "tab",
	space: "space",
	backspace: "backspace",
	delete: "delete",
	insert: "insert",
	clear: "clear",
	home: "home",
	end: "end",
	pageUp: "pageUp",
	pageDown: "pageDown",
	up: "up",
	down: "down",
	left: "left",
	right: "right",
	f1: "f1",
	f2: "f2",
	f3: "f3",
	f4: "f4",
	f5: "f5",
	f6: "f6",
	f7: "f7",
	f8: "f8",
	f9: "f9",
	f10: "f10",
	f11: "f11",
	f12: "f12",
	backtick: "`",
	hyphen: "-",
	equals: "=",
	leftbracket: "[",
	rightbracket: "]",
	backslash: "\\",
	semicolon: ";",
	quote: "'",
	comma: ",",
	period: ".",
	slash: "/",
	exclamation: "!",
	at: "@",
	hash: "#",
	dollar: "$",
	percent: "%",
	caret: "^",
	ampersand: "&",
	asterisk: "*",
	leftparen: "(",
	rightparen: ")",
	underscore: "_",
	plus: "+",
	pipe: "|",
	tilde: "~",
	leftbrace: "{",
	rightbrace: "}",
	colon: ":",
	lessthan: "<",
	greaterthan: ">",
	question: "?",
	ctrl: (key) => `ctrl+${key}`,
	shift: (key) => `shift+${key}`,
	alt: (key) => `alt+${key}`,
	super: (key) => `super+${key}`,
	ctrlShift: (key) => `ctrl+shift+${key}`,
	shiftCtrl: (key) => `shift+ctrl+${key}`,
	ctrlAlt: (key) => `ctrl+alt+${key}`,
	altCtrl: (key) => `alt+ctrl+${key}`,
	shiftAlt: (key) => `shift+alt+${key}`,
	altShift: (key) => `alt+shift+${key}`,
	ctrlSuper: (key) => `ctrl+super+${key}`,
	superCtrl: (key) => `super+ctrl+${key}`,
	shiftSuper: (key) => `shift+super+${key}`,
	superShift: (key) => `super+shift+${key}`,
	altSuper: (key) => `alt+super+${key}`,
	superAlt: (key) => `super+alt+${key}`,
	ctrlShiftAlt: (key) => `ctrl+shift+alt+${key}`,
	ctrlShiftSuper: (key) => `ctrl+shift+super+${key}`,
};

export function fuzzyMatch(query, text) {
	return util("fuzzyMatch", String(query ?? ""), String(text ?? ""));
}

export function fuzzyFilter(items, query, getText) {
	if (!query.trim()) return items;
	const tokens = query
		.trim()
		.split(/[\s/]+/)
		.filter((t) => t.length > 0);
	if (tokens.length === 0) return items;
	const results = [];
	for (const item of items) {
		const text = getText(item);
		let totalScore = 0;
		let allMatch = true;
		for (const token of tokens) {
			const match = fuzzyMatch(token, text);
			if (match.matches) totalScore += match.score;
			else {
				allMatch = false;
				break;
			}
		}
		if (allMatch) results.push({ item, totalScore });
	}
	results.sort((a, b) => a.totalScore - b.totalScore);
	return results.map((r) => r.item);
}

// ---------------------------------------------------------------------------
// Keybindings
// ---------------------------------------------------------------------------

export const TUI_KEYBINDINGS = {};

export function getKeybindings() {
	return keybindingsProxy(null);
}

export function setKeybindings(_keybindings) {}

export { KeybindingsProxy as KeybindingsManager };

// ---------------------------------------------------------------------------
// Components
// ---------------------------------------------------------------------------

export function isFocusable(component) {
	return component !== null && component !== undefined && "focused" in component;
}

export class Container {
	children = [];

	addChild(component) {
		this.children.push(component);
	}

	removeChild(component) {
		const index = this.children.indexOf(component);
		if (index !== -1) this.children.splice(index, 1);
	}

	clear() {
		this.children = [];
	}

	invalidate() {
		for (const child of this.children) child.invalidate?.();
	}

	render(width) {
		const lines = [];
		for (const child of this.children) {
			for (const line of child.render(width)) lines.push(line);
		}
		return lines;
	}
}

export class Text {
	constructor(text = "", paddingX = 1, paddingY = 1, customBgFn) {
		this.text = text;
		this.paddingX = paddingX;
		this.paddingY = paddingY;
		this.customBgFn = customBgFn;
	}

	setText(text) {
		this.text = text;
		this.cachedText = undefined;
		this.cachedWidth = undefined;
		this.cachedLines = undefined;
	}

	setCustomBgFn(customBgFn) {
		this.customBgFn = customBgFn;
		this.cachedText = undefined;
		this.cachedWidth = undefined;
		this.cachedLines = undefined;
	}

	setPaddingX(paddingX) {
		this.paddingX = paddingX;
		this.invalidate();
	}

	invalidate() {
		this.cachedText = undefined;
		this.cachedWidth = undefined;
		this.cachedLines = undefined;
	}

	render(width) {
		if (this.cachedLines && this.cachedText === this.text && this.cachedWidth === width) return this.cachedLines;
		if (!this.text || String(this.text).trim() === "") {
			const result = [];
			this.cachedText = this.text;
			this.cachedWidth = width;
			this.cachedLines = result;
			return result;
		}
		const normalizedText = String(this.text).replace(/\t/g, "   ");
		const paddingX = Math.min(this.paddingX, Math.max(0, Math.floor((width - 1) / 2)));
		const contentWidth = Math.max(1, width - paddingX * 2);
		const wrappedLines = wrapTextWithAnsi(normalizedText, contentWidth);
		const leftMargin = " ".repeat(paddingX);
		const rightMargin = " ".repeat(paddingX);
		const contentLines = [];
		for (const line of wrappedLines) {
			const lineWithMargins = leftMargin + line + rightMargin;
			if (this.customBgFn) contentLines.push(applyBackgroundToLine(lineWithMargins, width, this.customBgFn));
			else {
				const paddingNeeded = Math.max(0, width - visibleWidth(lineWithMargins));
				contentLines.push(lineWithMargins + " ".repeat(paddingNeeded));
			}
		}
		const emptyLine = " ".repeat(width);
		const emptyLines = [];
		for (let i = 0; i < this.paddingY; i++) {
			emptyLines.push(this.customBgFn ? applyBackgroundToLine(emptyLine, width, this.customBgFn) : emptyLine);
		}
		const result = [...emptyLines, ...contentLines, ...emptyLines];
		this.cachedText = this.text;
		this.cachedWidth = width;
		this.cachedLines = result;
		return result.length > 0 ? result : [""];
	}
}

export class TruncatedText {
	constructor(text, paddingX = 0, paddingY = 0) {
		this.text = text;
		this.paddingX = paddingX;
		this.paddingY = paddingY;
	}

	invalidate() {}

	render(width) {
		const result = [];
		const emptyLine = " ".repeat(width);
		for (let i = 0; i < this.paddingY; i++) result.push(emptyLine);
		const availableWidth = Math.max(1, width - this.paddingX * 2);
		let singleLineText = String(this.text);
		const newlineIndex = singleLineText.indexOf("\n");
		if (newlineIndex !== -1) singleLineText = singleLineText.substring(0, newlineIndex);
		const displayText = truncateToWidth(singleLineText, availableWidth);
		const leftPadding = " ".repeat(this.paddingX);
		const lineWithPadding = leftPadding + displayText + leftPadding;
		const paddingNeeded = Math.max(0, width - visibleWidth(lineWithPadding));
		result.push(lineWithPadding + " ".repeat(paddingNeeded));
		for (let i = 0; i < this.paddingY; i++) result.push(emptyLine);
		return result;
	}
}

export class Spacer {
	constructor(lines = 1) {
		this.lines = lines;
	}

	setLines(lines) {
		this.lines = lines;
	}

	invalidate() {}

	render(_width) {
		const result = [];
		for (let i = 0; i < this.lines; i++) result.push("");
		return result;
	}
}

export class Box {
	children = [];

	constructor(paddingX = 1, paddingY = 1, bgFn) {
		this.paddingX = paddingX;
		this.paddingY = paddingY;
		this.bgFn = bgFn;
		this.cache = undefined;
	}

	addChild(component) {
		this.children.push(component);
		this.cache = undefined;
	}

	removeChild(component) {
		const index = this.children.indexOf(component);
		if (index !== -1) {
			this.children.splice(index, 1);
			this.cache = undefined;
		}
	}

	clear() {
		this.children = [];
		this.cache = undefined;
	}

	setBgFn(bgFn) {
		this.bgFn = bgFn;
	}

	setPaddingX(paddingX) {
		this.paddingX = paddingX;
		this.cache = undefined;
	}

	invalidate() {
		this.cache = undefined;
		for (const child of this.children) child.invalidate?.();
	}

	render(width) {
		if (this.children.length === 0) return [];
		const contentWidth = Math.max(1, width - this.paddingX * 2);
		const leftPad = " ".repeat(this.paddingX);
		const childLines = [];
		for (const child of this.children) {
			for (const line of child.render(contentWidth)) childLines.push(line);
		}
		if (childLines.length === 0) return [];
		const bgSample = this.bgFn ? this.bgFn("test") : undefined;
		const cache = this.cache;
		if (
			cache &&
			cache.width === width &&
			cache.bgSample === bgSample &&
			cache.childLines.length === childLines.length &&
			cache.childLines.every((line, i) => line === childLines[i])
		) {
			return cache.lines;
		}
		const result = [];
		for (let i = 0; i < this.paddingY; i++) result.push(this.applyBg("", width));
		for (const line of childLines) result.push(this.applyBg(leftPad + line, width));
		for (let i = 0; i < this.paddingY; i++) result.push(this.applyBg("", width));
		this.cache = { childLines, width, bgSample, lines: result };
		return result;
	}

	applyBg(line, width) {
		const padNeeded = Math.max(0, width - visibleWidth(line));
		const padded = line + " ".repeat(padNeeded);
		return this.bgFn ? this.bgFn(padded) : padded;
	}
}

const DEFAULT_PRIMARY_COLUMN_WIDTH = 32;
const PRIMARY_COLUMN_GAP = 2;
const MIN_DESCRIPTION_WIDTH = 10;
const normalizeToSingleLine = (text) => text.replace(/[\r\n]+/g, " ").trim();
const clamp = (value, min, max) => Math.max(min, Math.min(value, max));

export class SelectList {
	constructor(items, maxVisible, theme, layout = {}) {
		this.items = items;
		this.filteredItems = items;
		this.selectedIndex = 0;
		this.maxVisible = maxVisible;
		this.theme = theme;
		this.layout = layout;
		this.onSelect = undefined;
		this.onCancel = undefined;
		this.onSelectionChange = undefined;
	}

	setFilter(filter) {
		this.filteredItems = this.items.filter((item) => item.value.toLowerCase().startsWith(filter.toLowerCase()));
		this.selectedIndex = 0;
	}

	setSelectedIndex(index) {
		this.selectedIndex = Math.max(0, Math.min(index, this.filteredItems.length - 1));
	}

	invalidate() {}

	render(width) {
		const lines = [];
		if (this.filteredItems.length === 0) {
			lines.push(this.theme.noMatch("  No matching commands"));
			return lines;
		}
		const primaryColumnWidth = this.getPrimaryColumnWidth();
		const { startIndex, endIndex } = this.getVisibleRange();
		for (let i = startIndex; i < endIndex; i++) {
			const item = this.filteredItems[i];
			if (!item) continue;
			const isSelected = i === this.selectedIndex;
			const descriptionSingleLine = item.description ? normalizeToSingleLine(item.description) : undefined;
			lines.push(this.renderItem(item, isSelected, width, descriptionSingleLine, primaryColumnWidth));
		}
		if (startIndex > 0 || endIndex < this.filteredItems.length) {
			const scrollText = `  (${this.selectedIndex + 1}/${this.filteredItems.length})`;
			lines.push(this.theme.scrollInfo(truncateToWidth(scrollText, width - 2, "")));
		}
		return lines;
	}

	handleInput(keyData) {
		const kb = getKeybindings();
		if (kb.matches(keyData, "tui.select.up")) {
			this.selectedIndex = this.selectedIndex === 0 ? this.filteredItems.length - 1 : this.selectedIndex - 1;
			this.notifySelectionChange();
		} else if (kb.matches(keyData, "tui.select.down")) {
			this.selectedIndex = this.selectedIndex === this.filteredItems.length - 1 ? 0 : this.selectedIndex + 1;
			this.notifySelectionChange();
		} else if (kb.matches(keyData, "tui.select.confirm")) {
			const selectedItem = this.filteredItems[this.selectedIndex];
			if (selectedItem && this.onSelect) this.onSelect(selectedItem);
		} else if (kb.matches(keyData, "tui.select.cancel")) {
			if (this.onCancel) this.onCancel();
		}
	}

	getVisibleRange() {
		const startIndex = Math.max(
			0,
			Math.min(this.selectedIndex - Math.floor(this.maxVisible / 2), this.filteredItems.length - this.maxVisible),
		);
		return { startIndex, endIndex: Math.min(startIndex + this.maxVisible, this.filteredItems.length) };
	}

	renderItem(item, isSelected, width, descriptionSingleLine, primaryColumnWidth) {
		const prefix = isSelected ? "→ " : "  ";
		const prefixWidth = visibleWidth(prefix);
		if (descriptionSingleLine && width > 40) {
			const effectivePrimaryColumnWidth = Math.max(1, Math.min(primaryColumnWidth, width - prefixWidth - 4));
			const maxPrimaryWidth = Math.max(1, effectivePrimaryColumnWidth - PRIMARY_COLUMN_GAP);
			const truncatedValue = this.truncatePrimary(item, isSelected, maxPrimaryWidth, effectivePrimaryColumnWidth);
			const truncatedValueWidth = visibleWidth(truncatedValue);
			const spacing = " ".repeat(Math.max(1, effectivePrimaryColumnWidth - truncatedValueWidth));
			const descriptionStart = prefixWidth + truncatedValueWidth + spacing.length;
			const remainingWidth = width - descriptionStart - 2;
			if (remainingWidth > MIN_DESCRIPTION_WIDTH) {
				const truncatedDesc = truncateToWidth(descriptionSingleLine, remainingWidth, "");
				if (isSelected) return this.theme.selectedText(`${prefix}${truncatedValue}${spacing}${truncatedDesc}`);
				return prefix + truncatedValue + this.theme.description(spacing + truncatedDesc);
			}
		}
		const maxWidth = width - prefixWidth - 2;
		const truncatedValue = this.truncatePrimary(item, isSelected, maxWidth, maxWidth);
		if (isSelected) return this.theme.selectedText(`${prefix}${truncatedValue}`);
		return prefix + truncatedValue;
	}

	getPrimaryColumnWidth() {
		const { min, max } = this.getPrimaryColumnBounds();
		const widestPrimary = this.filteredItems.reduce(
			(widest, item) => Math.max(widest, visibleWidth(this.getDisplayValue(item)) + PRIMARY_COLUMN_GAP),
			0,
		);
		return clamp(widestPrimary, min, max);
	}

	getPrimaryColumnBounds() {
		const rawMin =
			this.layout.minPrimaryColumnWidth ?? this.layout.maxPrimaryColumnWidth ?? DEFAULT_PRIMARY_COLUMN_WIDTH;
		const rawMax =
			this.layout.maxPrimaryColumnWidth ?? this.layout.minPrimaryColumnWidth ?? DEFAULT_PRIMARY_COLUMN_WIDTH;
		return { min: Math.max(1, Math.min(rawMin, rawMax)), max: Math.max(1, Math.max(rawMin, rawMax)) };
	}

	truncatePrimary(item, isSelected, maxWidth, columnWidth) {
		const displayValue = this.getDisplayValue(item);
		const truncatedValue = this.layout.truncatePrimary
			? this.layout.truncatePrimary({ text: displayValue, maxWidth, columnWidth, item, isSelected })
			: truncateToWidth(displayValue, maxWidth, "");
		return truncateToWidth(truncatedValue, maxWidth, "");
	}

	getDisplayValue(item) {
		return item.label || item.value;
	}

	notifySelectionChange() {
		const selectedItem = this.filteredItems[this.selectedIndex];
		if (selectedItem && this.onSelectionChange) this.onSelectionChange(selectedItem);
	}

	getSelectedItem() {
		return this.filteredItems[this.selectedIndex] || null;
	}
}

/** Single-line input backed by the Rust pi-tui `Input`. */
export class Input extends RustComponent {
	constructor(options = {}) {
		super(
			call("rc.new", {
				kind: "input",
				options: {
					prompt: options.prompt,
					placeholder: options.placeholder,
					placeholderStyle: options.placeholderStyle,
				},
			}),
		);
		this._onSubmit = undefined;
		this._onEscape = undefined;
	}
	get onSubmit() {
		return this._onSubmit;
	}
	set onSubmit(fn) {
		this._onSubmit = fn;
		call("rc", { id: this.__rid, m: "setOnSubmit", a: [fn ? (value) => this._onSubmit?.(value) : null] });
	}
	get onEscape() {
		return this._onEscape;
	}
	set onEscape(fn) {
		this._onEscape = fn;
		call("rc", { id: this.__rid, m: "setOnEscape", a: [fn ? () => this._onEscape?.() : null] });
	}
	getValue() {
		return call("rc", { id: this.__rid, m: "getValue", a: [] }) ?? "";
	}
	setValue(value) {
		call("rc", { id: this.__rid, m: "setValue", a: [String(value)] });
	}
}

/** Multi-line editor backed by the Rust pi-tui `Editor`. */
export class Editor extends RustComponent {
	constructor(tui, theme, options = {}, kind = "editor", keybindings) {
		super(
			call("rc.new", {
				kind,
				tui: tui?.__tuiId ?? null,
				keybindings: keybindings?.__kbId ?? null,
				theme: theme
					? {
							borderColor: theme.borderColor,
							selectList: theme.selectList,
						}
					: null,
				options: { paddingX: options?.paddingX, autocompleteMaxVisible: options?.autocompleteMaxVisible },
			}),
		);
		this.tui = tui;
		this.theme = theme;
		this._onSubmit = undefined;
		this._onChange = undefined;
	}
	get onSubmit() {
		return this._onSubmit;
	}
	set onSubmit(fn) {
		this._onSubmit = fn;
		call("rc", { id: this.__rid, m: "setOnSubmit", a: [fn ? (text) => this._onSubmit?.(text) : null] });
	}
	get onChange() {
		return this._onChange;
	}
	set onChange(fn) {
		this._onChange = fn;
		call("rc", { id: this.__rid, m: "setOnChange", a: [fn ? (text) => this._onChange?.(text) : null] });
	}
	get disableSubmit() {
		return !!call("rc", { id: this.__rid, m: "getDisableSubmit", a: [] });
	}
	set disableSubmit(value) {
		call("rc", { id: this.__rid, m: "setDisableSubmit", a: [!!value] });
	}
	get borderColor() {
		return this._borderColor ?? this.theme?.borderColor;
	}
	set borderColor(fn) {
		this._borderColor = fn;
		call("rc", { id: this.__rid, m: "setBorderColor", a: [fn] });
	}
	getText() {
		return call("rc", { id: this.__rid, m: "getText", a: [] }) ?? "";
	}
	getExpandedText() {
		return call("rc", { id: this.__rid, m: "getExpandedText", a: [] }) ?? "";
	}
	getLines() {
		return call("rc", { id: this.__rid, m: "getLines", a: [] }) ?? [];
	}
	getCursor() {
		return call("rc", { id: this.__rid, m: "getCursor", a: [] });
	}
	setText(text) {
		call("rc", { id: this.__rid, m: "setText", a: [String(text)] });
	}
	insertTextAtCursor(text) {
		call("rc", { id: this.__rid, m: "insertTextAtCursor", a: [String(text)] });
	}
	addToHistory(text) {
		call("rc", { id: this.__rid, m: "addToHistory", a: [String(text)] });
	}
	setPaddingX(padding) {
		call("rc", { id: this.__rid, m: "setPaddingX", a: [padding] });
	}
	getPaddingX() {
		return call("rc", { id: this.__rid, m: "getPaddingX", a: [] });
	}
	setAutocompleteMaxVisible(maxVisible) {
		call("rc", { id: this.__rid, m: "setAutocompleteMaxVisible", a: [maxVisible] });
	}
	getAutocompleteMaxVisible() {
		return call("rc", { id: this.__rid, m: "getAutocompleteMaxVisible", a: [] });
	}
	setAutocompleteProvider(provider) {
		call("rc", { id: this.__rid, m: "setAutocompleteProvider", a: [provider] });
	}
	isShowingAutocomplete() {
		return !!call("rc", { id: this.__rid, m: "isShowingAutocomplete", a: [] });
	}
}

markRustBase(Input);
markRustBase(Editor);

/** Markdown component: rendering is done by the Rust pi-tui Markdown renderer. */
export class Markdown {
	constructor(text, paddingX, paddingY, theme, defaultTextStyle, options) {
		this.text = text;
		this.paddingX = paddingX;
		this.paddingY = paddingY;
		this.theme = theme;
		this.defaultTextStyle = defaultTextStyle;
		this.options = options ? { ...options } : {};
	}

	setText(text) {
		this.text = text;
		this.invalidate();
	}

	invalidate() {
		this.cachedText = undefined;
		this.cachedWidth = undefined;
		this.cachedLines = undefined;
	}

	render(width) {
		if (this.cachedLines && this.cachedText === this.text && this.cachedWidth === width) return this.cachedLines;
		const lines = call("tui.markdown", {
			text: String(this.text ?? ""),
			paddingX: this.paddingX ?? 0,
			paddingY: this.paddingY ?? 0,
			theme: markdownThemeRef(this.theme),
			defaultTextStyle: this.defaultTextStyle ?? null,
			options: this.options,
			width,
		});
		this.cachedText = this.text;
		this.cachedWidth = width;
		this.cachedLines = lines;
		return lines;
	}
}

const markdownThemes = new WeakMap();

function markdownThemeRef(theme) {
	if (!theme) return null;
	if (theme.__mdTheme !== undefined) return { builtin: theme.__mdTheme };
	let id = markdownThemes.get(theme);
	if (id === undefined) {
		id = call("tui.markdownTheme", { theme });
		markdownThemes.set(theme, id);
	}
	return { id };
}

const DEFAULT_FRAMES = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];
const DEFAULT_INTERVAL_MS = 80;

export class Loader extends Text {
	constructor(ui, spinnerColorFn, messageColorFn, message = "Loading...", indicator) {
		super("", 1, 0);
		this.frames = [...DEFAULT_FRAMES];
		this.intervalMs = DEFAULT_INTERVAL_MS;
		this.currentFrame = 0;
		this.intervalId = null;
		this.ui = ui;
		this.renderIndicatorVerbatim = false;
		this.spinnerColorFn = spinnerColorFn;
		this.messageColorFn = messageColorFn;
		this.message = message;
		this.setIndicator(indicator);
	}

	render(width) {
		return ["", ...super.render(width)];
	}

	start() {
		this.updateDisplay();
		this.restartAnimation();
	}

	stop() {
		if (this.intervalId) {
			clearInterval(this.intervalId);
			this.intervalId = null;
		}
	}

	setMessage(message) {
		this.message = message;
		this.updateDisplay();
	}

	invalidate() {
		super.invalidate();
		if (this.messageColorFn) this.updateDisplay();
	}

	setIndicator(indicator) {
		this.renderIndicatorVerbatim = indicator !== undefined;
		this.frames = indicator?.frames !== undefined ? [...indicator.frames] : [...DEFAULT_FRAMES];
		this.intervalMs = indicator?.intervalMs && indicator.intervalMs > 0 ? indicator.intervalMs : DEFAULT_INTERVAL_MS;
		this.currentFrame = 0;
		this.start();
	}

	restartAnimation() {
		this.stop();
		if (this.frames.length <= 1) return;
		this.intervalId = setInterval(() => {
			this.currentFrame = (this.currentFrame + 1) % this.frames.length;
			this.updateDisplay();
		}, this.intervalMs);
		this.intervalId?.unref?.();
	}

	getRenderedIndicator() {
		const frame = this.frames[this.currentFrame] ?? "";
		return this.renderIndicatorVerbatim ? frame : this.spinnerColorFn(frame);
	}

	updateDisplay() {
		const renderedFrame = this.getRenderedIndicator();
		const indicator = renderedFrame.length > 0 ? `${renderedFrame} ` : "";
		this.setText(`${indicator}${this.messageColorFn(this.message)}`);
		if (this.ui) this.ui.requestRender();
	}
}

export class CancellableLoader extends Loader {
	constructor(...args) {
		super(...args);
		this.abortController = new AbortController();
		this.onAbort = undefined;
	}

	get signal() {
		return this.abortController.signal;
	}

	get aborted() {
		return this.abortController.signal.aborted;
	}

	handleInput(data) {
		if (getKeybindings().matches(data, "tui.select.cancel")) {
			this.abortController.abort();
			this.onAbort?.();
		}
	}

	dispose() {
		this.stop();
	}
}

export class SettingsList {
	constructor(items, maxVisible, theme, onChange, onCancel, options = {}) {
		this.items = items;
		this.filteredItems = items;
		this.maxVisible = maxVisible;
		this.theme = theme;
		this.onChange = onChange;
		this.onCancel = onCancel;
		this.selectedIndex = 0;
		this.searchEnabled = options.enableSearch ?? false;
		if (this.searchEnabled) this.searchInput = new Input();
		this.submenuComponent = null;
		this.submenuItemIndex = null;
		this.navigateAfterClose = null;
	}

	updateValue(id, newValue) {
		const item = this.items.find((i) => i.id === id);
		if (item) item.currentValue = newValue;
	}

	selectItem(id) {
		const items = this.searchEnabled ? this.filteredItems : this.items;
		const index = items.findIndex((i) => i.id === id);
		if (index !== -1) this.selectedIndex = index;
	}

	invalidate() {
		this.submenuComponent?.invalidate?.();
	}

	render(width) {
		if (this.submenuComponent) return this.submenuComponent.render(width);
		return this.renderMainList(width);
	}

	renderMainList(width) {
		const lines = [];
		if (this.searchEnabled && this.searchInput) {
			lines.push(...this.searchInput.render(width));
			lines.push("");
		}
		if (this.items.length === 0) {
			lines.push(this.theme.hint("  No settings available"));
			if (this.searchEnabled) this.addHintLine(lines, width);
			return lines;
		}
		const displayItems = this.getDisplayItems();
		if (displayItems.length === 0) {
			lines.push(truncateToWidth(this.theme.hint("  No matching settings"), width));
			this.addHintLine(lines, width);
			return lines;
		}
		const { startIndex, endIndex } = this.getVisibleRange(displayItems);
		const maxLabelWidth = Math.min(36, Math.max(...this.items.map((item) => visibleWidth(item.label))));
		for (let i = startIndex; i < endIndex; i++) {
			const item = displayItems[i];
			if (!item) continue;
			const isSelected = i === this.selectedIndex;
			const prefix = isSelected ? this.theme.cursor : "  ";
			const prefixWidth = visibleWidth(prefix);
			const labelPadded = item.label + " ".repeat(Math.max(0, maxLabelWidth - visibleWidth(item.label)));
			const labelText = this.theme.label(labelPadded, isSelected);
			const separator = "  ";
			const usedWidth = prefixWidth + maxLabelWidth + visibleWidth(separator);
			const valueMaxWidth = width - usedWidth - 2;
			const valueText = this.theme.value(truncateToWidth(item.currentValue, valueMaxWidth, ""), isSelected);
			lines.push(truncateToWidth(prefix + labelText + separator + valueText, width));
		}
		if (startIndex > 0 || endIndex < displayItems.length) {
			const scrollText = `  (${this.selectedIndex + 1}/${displayItems.length})`;
			lines.push(this.theme.hint(truncateToWidth(scrollText, width - 2, "")));
		}
		const selectedItem = displayItems[this.selectedIndex];
		if (selectedItem?.description) {
			lines.push("");
			for (const line of wrapTextWithAnsi(selectedItem.description, width - 4)) {
				lines.push(this.theme.description(`  ${line}`));
			}
		}
		this.addHintLine(lines, width);
		return lines;
	}

	handleInput(data) {
		if (this.submenuComponent) {
			this.submenuComponent.handleInput?.(data);
			return;
		}
		const kb = getKeybindings();
		const displayItems = this.getDisplayItems();
		if (kb.matches(data, "tui.select.up")) {
			if (displayItems.length === 0) return;
			this.selectedIndex = this.selectedIndex === 0 ? displayItems.length - 1 : this.selectedIndex - 1;
		} else if (kb.matches(data, "tui.select.down")) {
			if (displayItems.length === 0) return;
			this.selectedIndex = this.selectedIndex === displayItems.length - 1 ? 0 : this.selectedIndex + 1;
		} else if (
			kb.matches(data, "tui.select.confirm") ||
			(data === " " && (!this.searchEnabled || this.searchInput?.getValue().length === 0))
		) {
			this.activateItem();
		} else if (kb.matches(data, "tui.select.cancel")) {
			this.onCancel();
		} else if (this.searchEnabled && this.searchInput) {
			this.searchInput.handleInput(data);
			this.applyFilter(this.searchInput.getValue());
		}
	}

	getDisplayItems() {
		return this.searchEnabled ? this.filteredItems : this.items;
	}

	getVisibleRange(displayItems) {
		const startIndex = Math.max(
			0,
			Math.min(this.selectedIndex - Math.floor(this.maxVisible / 2), displayItems.length - this.maxVisible),
		);
		return { startIndex, endIndex: Math.min(startIndex + this.maxVisible, displayItems.length) };
	}

	activateItem() {
		const item = this.getDisplayItems()[this.selectedIndex];
		if (!item) return;
		if (item.submenu) {
			this.submenuItemIndex = this.selectedIndex;
			this.submenuComponent = item.submenu(item.currentValue, (selectedValue, options) => {
				if (selectedValue !== undefined) {
					item.currentValue = selectedValue;
					this.onChange(item.id, selectedValue);
				}
				if (options?.navigateTo) this.navigateAfterClose = options.navigateTo;
				this.closeSubmenu();
			});
		} else if (item.values && item.values.length > 0) {
			const currentIndex = item.values.indexOf(item.currentValue);
			const nextIndex = (currentIndex + 1) % item.values.length;
			const newValue = item.values[nextIndex];
			item.currentValue = newValue;
			this.onChange(item.id, newValue);
		}
	}

	closeSubmenu() {
		this.submenuComponent = null;
		if (this.navigateAfterClose !== null) {
			const id = this.navigateAfterClose;
			this.navigateAfterClose = null;
			this.submenuItemIndex = null;
			this.selectItem(id);
			this.activateItem();
		} else if (this.submenuItemIndex !== null) {
			this.selectedIndex = this.submenuItemIndex;
			this.submenuItemIndex = null;
		}
	}

	applyFilter(query) {
		this.filteredItems = fuzzyFilter(this.items, query, (item) => item.label);
		this.selectedIndex = 0;
	}

	addHintLine(lines, width) {
		lines.push("");
		lines.push(
			truncateToWidth(
				this.theme.hint(
					this.searchEnabled
						? "  Type to search · Enter/Space to change · Esc to cancel"
						: "  Enter/Space to change · Esc to cancel",
				),
				width,
			),
		);
	}
}

export class Image {
	constructor(base64Data, mimeType, theme, options = {}, dimensions) {
		this.base64Data = base64Data;
		this.mimeType = mimeType;
		this.theme = theme;
		this.options = options;
		this.dimensions = dimensions;
		this.imageId = options.imageId;
	}

	getImageId() {
		return this.imageId;
	}

	invalidate() {
		this.cachedLines = undefined;
		this.cachedWidth = undefined;
	}

	render(width) {
		if (this.cachedLines && this.cachedWidth === width) return this.cachedLines;
		const fallback = util("imageFallback", this.mimeType, this.dimensions ?? null, this.options.filename ?? null);
		const color = this.theme?.fallbackColor ?? ((s) => s);
		this.cachedLines = [truncateToWidth(color(fallback), width)];
		this.cachedWidth = width;
		return this.cachedLines;
	}
}

export class HStack extends Container {
	render(width) {
		const columns = this.children.map((child) => child.render(Math.max(1, Math.floor(width / Math.max(1, this.children.length)))));
		const height = Math.max(0, ...columns.map((c) => c.length));
		const colWidth = Math.max(1, Math.floor(width / Math.max(1, this.children.length)));
		const lines = [];
		for (let row = 0; row < height; row++) {
			lines.push(columns.map((c) => truncateToWidth(c[row] ?? "", colWidth, "", true)).join(""));
		}
		return lines;
	}
}

export class VStack extends Container {}

export class ScrollView extends Container {
	constructor(child) {
		super();
		if (child) this.addChild(child);
	}
}

export class MouseRegion extends Container {
	constructor(child) {
		super();
		if (child) this.addChild(child);
	}
}

// ---------------------------------------------------------------------------
// Host-only classes (not available to extensions in pi-rs)
// ---------------------------------------------------------------------------

function unavailable(name) {
	return class {
		constructor() {
			throw new Error(`${name} is not available to extensions in pi-rs`);
		}
	};
}

export const TUI = unavailable("TUI");
export const TuiMainScreen = unavailable("TuiMainScreen");
export const ProcessTerminal = unavailable("ProcessTerminal");
export const StdinBuffer = unavailable("StdinBuffer");
export const CombinedAutocompleteProvider = unavailable("CombinedAutocompleteProvider");
export const Marked = unavailable("Marked");

export function setImageTranscoder(_transcoder) {}
export function isAppleTerminalSession() {
	return false;
}
export function getNativeClipboard() {
	return undefined;
}
export function renderLatex(text) {
	return String(text);
}
export function formatProgramStatus() {
	return "";
}
export function styleText(text) {
	return String(text);
}
export function styleTextWithAnsi(text) {
	return String(text);
}

export { tuiProxy as __tuiProxy, componentRef as __componentRef, rustCallback as __rustCallback };
