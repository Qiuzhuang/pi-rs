// Golden renders of the pi-tui base components with pi's themes: Markdown over a broad corpus,
// SelectList, SettingsList, Input, Box/Text/TruncatedText, Loader, Image fallback; plus a 256-color
// pass over a few coding-agent components.
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { assistant, MARKDOWN, TS_FILE } from "./corpus.mjs";
import { CWD, fakeTui, Golden, ref, tui } from "./common.mjs";

const here = path.dirname(fileURLToPath(import.meta.url));
const MD_CORPUS = JSON.parse(fs.readFileSync(path.join(here, "markdown-corpus.json"), "utf8"));
const { getMarkdownTheme, getSelectListTheme, getSettingsListTheme, theme } = await ref(
	"coding-agent/src/modes/interactive/theme/theme.ts",
);
const { AssistantMessageComponent } = await ref("coding-agent/src/modes/interactive/components/assistant-message.ts");
const { UserMessageComponent } = await ref("coding-agent/src/modes/interactive/components/user-message.ts");
const { ToolExecutionComponent } = await ref("coding-agent/src/modes/interactive/components/tool-execution.ts");
const { withBuiltInRenderers } = await ref("coding-agent/src/core/tools/renderers/index.ts");
const { Markdown, SelectList, SettingsList, Input, Box, Text, TruncatedText, Loader, CancellableLoader, Image, Spacer } = tui;

const g = new Golden("tui.json");
const DOWN = "\x1b[B";
const UP = "\x1b[A";

function feed(component, keys) {
	for (const key of keys ?? []) component.handleInput(key);
}

// --- markdown -----------------------------------------------------------------------------------
const STYLES = {
	none: undefined,
	thinking: { color: (t) => theme.fg("thinkingText", t), italic: true },
	user: { color: (t) => theme.fg("userMessageText", t), bgColor: (t) => theme.bg("userMessageBg", t) },
};
const buildMarkdown = (input) =>
	new Markdown(input.text, input.paddingX ?? 1, input.paddingY ?? 0, getMarkdownTheme(), STYLES[input.style ?? "none"], {
		preserveOrderedListMarkers: input.preserve ?? false,
		preserveBackslashEscapes: input.preserve ?? false,
	});
MD_CORPUS.forEach((text, i) => {
	const style = ["none", "none", "thinking", "user"][i % 4];
	g.add(`markdown/corpus-${String(i).padStart(3, "0")}`, "markdown", { text, style, preserve: style === "user" }, buildMarkdown);
});
g.add("markdown/padded", "markdown", { text: MARKDOWN.mixed, paddingX: 3, paddingY: 2 }, buildMarkdown);
g.add("markdown/empty", "markdown", { text: "" }, buildMarkdown);

// --- select list --------------------------------------------------------------------------------
const selectItems = [
	{ value: "a", label: "alpha", description: "The first letter of the Greek alphabet" },
	{ value: "b", label: "beta", description: "Second" },
	{ value: "c", label: "gamma-with-a-long-label-name", description: "A description that is long enough to be truncated at narrow widths" },
	{ value: "d", label: "delta" },
	{ value: "e", label: "epsilon", description: "Fifth" },
	{ value: "f", label: "zeta", description: "Sixth" },
	{ value: "g", label: "eta 日本語", description: "Seventh 👍" },
];
const buildSelect = (input) => {
	const list = new SelectList(input.items, input.maxVisible ?? 5, getSelectListTheme(), input.layout ?? {});
	if (input.filter !== undefined) list.setFilter(input.filter);
	if (input.selected !== undefined) list.setSelectedIndex(input.selected);
	feed(list, input.keys);
	return list;
};
g.add("select/basic", "select_list", { items: selectItems }, buildSelect);
g.add("select/scrolled", "select_list", { items: selectItems, keys: [DOWN, DOWN, DOWN, DOWN, DOWN] }, buildSelect);
g.add("select/wrap-up", "select_list", { items: selectItems, keys: [UP] }, buildSelect);
g.add("select/filtered", "select_list", { items: selectItems, filter: "ta" }, buildSelect);
g.add("select/no-match", "select_list", { items: selectItems, filter: "zzz" }, buildSelect);
g.add("select/all-visible", "select_list", { items: selectItems.slice(0, 3), maxVisible: 10, selected: 2 }, buildSelect);
g.add("select/primary-width", "select_list", { items: selectItems, layout: { minPrimaryColumnWidth: 12, maxPrimaryColumnWidth: 16 } }, buildSelect);

// --- settings list ------------------------------------------------------------------------------
const settingItems = [
	{ id: "auto", label: "Auto-compact", description: "Compact the context automatically", currentValue: "true", values: ["true", "false"] },
	{ id: "mode", label: "Steering mode", description: "How queued messages are delivered", currentValue: "one-at-a-time", values: ["all", "one-at-a-time"] },
	{ id: "theme", label: "Theme", currentValue: "dark", values: ["dark", "light"] },
	{ id: "width", label: "Image width", description: "Width of inline images in cells", currentValue: "60", values: ["40", "60", "80", "120"] },
	{ id: "long", label: "A setting with a long label that wraps", description: "Description ".repeat(12), currentValue: "a very long current value", values: ["a very long current value", "b"] },
	{ id: "x1", label: "Extra one", currentValue: "1", values: ["1", "2"] },
	{ id: "x2", label: "Extra two", currentValue: "2", values: ["1", "2"] },
];
const buildSettings = (input) => {
	const list = new SettingsList(input.items, input.maxVisible ?? 5, getSettingsListTheme(), () => {}, () => {}, { enableSearch: input.search ?? false });
	feed(list, input.keys);
	return list;
};
g.add("settings-list/basic", "settings_list", { items: settingItems }, buildSettings);
g.add("settings-list/cycled", "settings_list", { items: settingItems, keys: [DOWN, "\r", " "] }, buildSettings);
g.add("settings-list/scrolled", "settings_list", { items: settingItems, keys: [DOWN, DOWN, DOWN, DOWN, DOWN, DOWN] }, buildSettings);
g.add("settings-list/search", "settings_list", { items: settingItems, search: true, keys: ["e", "x"] }, buildSettings);
g.add("settings-list/search-empty", "settings_list", { items: settingItems, search: true }, buildSettings);
g.add("settings-list/long-selected", "settings_list", { items: settingItems, keys: [DOWN, DOWN, DOWN, DOWN] }, buildSettings);

// --- input --------------------------------------------------------------------------------------
const buildInput = (input) => {
	const c = new Input(input.placeholder ? { placeholder: input.placeholder, placeholderStyle: (t) => theme.fg("dim", t) } : {});
	if (input.value) c.setValue(input.value);
	if (input.focused) c.focused = true;
	feed(c, input.keys);
	return c;
};
g.add("input/empty", "input", {}, buildInput);
g.add("input/placeholder-focused", "input", { placeholder: "Type to search…", focused: true }, buildInput);
g.add("input/value", "input", { value: "hello world", focused: true }, buildInput);
g.add("input/cursor-middle", "input", { value: "hello world", focused: true, keys: ["\x1b[D", "\x1b[D", "\x1b[D"] }, buildInput);
g.add("input/long-scrolled", "input", { value: "x".repeat(30) + " the end of a long input value that scrolls " + "y".repeat(60), focused: true }, buildInput);
g.add("input/wide-chars", "input", { value: "日本語テキスト👍 mixed", focused: true, keys: ["\x01", "\x1b[C", "\x1b[C"] }, buildInput);

// --- box / text / truncated ---------------------------------------------------------------------
g.add("text/padded-bg", "text", { text: "Some text that wraps across lines when the width is narrow enough to need it.", px: 2, py: 1, bg: "toolSuccessBg" }, (input) =>
	new Text(input.text, input.px, input.py, (t) => theme.bg(input.bg, t)),
);
g.add("text/styled", "text", { text: `${"\x1b[1mbold\x1b[22m"} and \x1b[32mgreen\x1b[39m and a tab\there`, px: 1, py: 0 }, (input) => new Text(input.text, input.px, input.py));
g.add("text/empty", "text", { text: "", px: 1, py: 1 }, (input) => new Text(input.text, input.px, input.py));
g.add("box/nested", "box", { bg: "customMessageBg" }, (input) => {
	const box = new Box(2, 1, (t) => theme.bg(input.bg, t));
	box.addChild(new Text(theme.fg("customMessageLabel", "\x1b[1m[label]\x1b[22m"), 0, 0));
	box.addChild(new Spacer(1));
	box.addChild(new Text("Inner text that is long enough to wrap inside the box at forty columns.", 0, 0));
	return box;
});
g.add("truncated/long", "truncated_text", { text: `${theme.fg("accent", "accent")} ${"word ".repeat(40)}`, px: 1, py: 0 }, (input) =>
	new TruncatedText(input.text, input.px, input.py),
);
g.add("truncated/multiline", "truncated_text", { text: "first line\nsecond line", px: 0, py: 1 }, (input) => new TruncatedText(input.text, input.px, input.py));

// --- loaders ------------------------------------------------------------------------------------
g.add("loader/default", "loader", { message: "Loading..." }, (input) => new Loader(fakeTui(), (t) => theme.fg("accent", t), (t) => theme.fg("muted", t), input.message));
g.add("loader/cancellable", "cancellable_loader", { message: "Connecting to the server, please wait" }, (input) =>
	new CancellableLoader(fakeTui(), (t) => theme.fg("accent", t), (t) => theme.fg("muted", t), input.message),
);

// --- image fallback -----------------------------------------------------------------------------
const PNG_1x1 = "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mNk+M9QDwADhgGAWjR9awAAAABJRU5ErkJggg==";
g.add("image/fallback", "image", { data: PNG_1x1, mime: "image/png", filename: "dot.png" }, (input) =>
	new Image(input.data, input.mime, { fallbackColor: (t) => theme.fg("muted", t) }, { filename: input.filename }),
);
g.add("image/fallback-unknown", "image", { data: "AAAA", mime: "image/webp" }, (input) =>
	new Image(input.data, input.mime, { fallbackColor: (t) => theme.fg("muted", t) }, {}),
);

// --- 256-color terminals ------------------------------------------------------------------------
const caps256 = { trueColor: false };
g.add(
	"256/assistant",
	"assistant_message",
	{ message: assistant([{ type: "thinking", thinking: "plan" }, { type: "text", text: MARKDOWN.mixed }]) },
	(input) => new AssistantMessageComponent(input.message),
	{ caps: caps256 },
);
g.add("256/user", "user_message", { text: MARKDOWN.paragraphs }, (input) => new UserMessageComponent(input.text), { caps: caps256 });
g.add(
	"256/read-expanded",
	"tool_execution",
	{ toolName: "read", args: { path: "src/config.ts" }, result: TS_FILE },
	(input) => {
		const c = new ToolExecutionComponent(input.toolName, "call-1", input.args, {}, withBuiltInRenderers(input.toolName, undefined), fakeTui(), CWD);
		c.updateResult({ content: [{ type: "text", text: input.result }], isError: false }, false);
		c.setExpanded(true);
		return c;
	},
	{ caps: caps256 },
);
g.add("256/markdown-code", "markdown", { text: MARKDOWN.code }, buildMarkdown, { caps: caps256 });

g.write();
process.exit(0);
