// Golden renders of the interactive chrome: footer, status indicators, borders, editor, dialogs and
// the extension UI components.
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { CWD, fakeTui, Golden, ref, tui } from "./common.mjs";

const C = "coding-agent/src/modes/interactive/components";
const { FooterComponent } = await ref(`${C}/footer.ts`);
const { WorkingStatusIndicator, RetryStatusIndicator, CompactionStatusIndicator, BranchSummaryStatusIndicator, IdleStatus } =
	await ref(`${C}/status-indicator.ts`);
const { DynamicBorder } = await ref(`${C}/dynamic-border.ts`);
const { BorderedLoader } = await ref(`${C}/bordered-loader.ts`);
const { AuthUrlComponent } = await ref(`${C}/auth-url.ts`);
const { ExtensionSelectorComponent } = await ref(`${C}/extension-selector.ts`);
const { ExtensionInputComponent } = await ref(`${C}/extension-input.ts`);
const { ExtensionEditorComponent } = await ref(`${C}/extension-editor.ts`);
const { CustomEditor } = await ref(`${C}/custom-editor.ts`);
const { LoginDialogComponent } = await ref(`${C}/login-dialog.ts`);
const { EarendilAnnouncementComponent } = await ref(`${C}/earendil-announcement.ts`);
const { FirstTimeSetupComponent } = await ref(`${C}/first-time-setup.ts`);
const { piLogoLines, piWordmark } = await ref(`${C}/pi-logo.ts`);
const { keyHint, keyText, rawKeyHint } = await ref(`${C}/keybinding-hints.ts`);
const { KeybindingsManager } = await ref("coding-agent/src/core/keybindings.ts");
const { getEditorTheme, theme } = await ref("coding-agent/src/modes/interactive/theme/theme.ts");

const g = new Golden("chrome.json");
const noop = () => {};

/** A TUI stand-in with the terminal size the Rust tests use (80x24). */
function editorTui() {
	return { requestRender() {}, terminal: { columns: 80, rows: 24 } };
}

function feed(component, keys) {
	for (const key of keys ?? []) component.handleInput(key);
}

// --- footer -------------------------------------------------------------------------------------
const usage = (input, output, cacheRead, cacheWrite, cost) => ({
	input,
	output,
	cacheRead,
	cacheWrite,
	totalTokens: input + output + cacheRead + cacheWrite,
	cost: { input: 0, output: 0, cacheRead: 0, cacheWrite: 0, total: cost },
});
const entryBase = (id) => ({ id, parentId: null, timestamp: "2026-01-01T00:00:00.000Z" });
const assistantEntry = (id, u) => ({
	type: "message",
	...entryBase(id),
	message: {
		role: "assistant",
		content: [],
		api: "faux",
		provider: "faux",
		model: "m",
		usage: u,
		stopReason: "stop",
		timestamp: 0,
	},
});
const toolResultEntry = (id, u) => ({
	type: "message",
	...entryBase(id),
	message: { role: "toolResult", toolCallId: "t", toolName: "bash", content: [], isError: false, timestamp: 0, usage: u },
});
const branchEntry = (id, u) => ({ type: "branch_summary", ...entryBase(id), fromId: "x", summary: "s", usage: u });
const compactionEntry = (id, u) => ({
	type: "compaction",
	...entryBase(id),
	summary: "s",
	firstKeptEntryId: "x",
	tokensBefore: 1000,
	usage: u,
});

const buildFooter = (input) => {
	const session = {
		state: { model: input.model ?? undefined, thinkingLevel: input.thinkingLevel ?? "off" },
		sessionManager: {
			getEntries: () => input.entries,
			getEntryCount: () => input.entries.length,
			getSessionId: () => "golden-session",
			getLeafId: () => null,
			getSessionName: () => input.sessionName,
			getCwd: () => input.cwd,
		},
		getContextUsage: () => input.contextUsage ?? undefined,
		routedModel: input.routed ? { model: { id: input.routed.id }, thinkingLevel: input.routed.thinkingLevel } : undefined,
		modelRuntime: { isUsingSubscription: () => input.usingSubscription ?? false },
	};
	const footerData = {
		getGitBranch: () => input.branch ?? null,
		getExtensionStatuses: () => new Map(Object.entries(input.statuses ?? {})),
		getAvailableProviderCount: () => input.providerCount ?? 1,
		onBranchChange: () => () => {},
	};
	const footer = new FooterComponent(session, footerData);
	if (input.autoCompact === false) footer.setAutoCompactEnabled(false);
	return footer;
};
const model = (id, provider, reasoning = false, contextWindow = 200000) => ({ id, provider, reasoning, contextWindow });
const footerBase = {
	cwd: CWD,
	sessionName: undefined,
	model: model("claude-sonnet-4-5", "anthropic", true),
	thinkingLevel: "medium",
	entries: [],
	contextUsage: { tokens: 0, contextWindow: 200000, percent: 0 },
	branch: "main",
};
const footer = (name, extra) => g.add(`footer/${name}`, "footer", { ...footerBase, ...extra }, buildFooter);
footer("empty", {});
footer("no-model", { model: null, contextUsage: null });
footer("usage", {
	entries: [assistantEntry("a", usage(12345, 2345, 100000, 5000, 0.4567))],
	contextUsage: { tokens: 117345, contextWindow: 200000, percent: 58.67 },
});
footer("usage-warning", {
	entries: [assistantEntry("a", usage(1500, 999, 0, 0, 0.01)), toolResultEntry("t", usage(10, 10, 0, 0, 0.001))],
	contextUsage: { tokens: 150000, contextWindow: 200000, percent: 75.1 },
});
footer("usage-error", {
	entries: [assistantEntry("a", usage(2_500_000, 15_000_000, 0, 0, 12.5))],
	contextUsage: { tokens: 190000, contextWindow: 200000, percent: 95 },
	autoCompact: false,
});
footer("unknown-context", { contextUsage: { tokens: null, contextWindow: 1000000, percent: null } });
footer("branch-compaction-usage", {
	entries: [branchEntry("b", usage(500, 50, 0, 0, 0.002)), compactionEntry("c", usage(9000, 800, 0, 0, 0.02))],
});
footer("subscription", { usingSubscription: true, entries: [assistantEntry("a", usage(10, 20, 0, 0, 0))] });
footer("kimi", { model: model("kimi-k2", "kimi-coding"), thinkingLevel: "off" });
footer("non-reasoning", { model: model("gpt-4o", "openai"), thinkingLevel: "high" });
footer("thinking-off", { thinkingLevel: "off" });
footer("routed", { model: model("auto", "virtual", true), routed: { id: "claude-haiku-4-5", thinkingLevel: "low" } });
footer("multi-provider", { providerCount: 3 });
footer("session-name", { sessionName: "refactor parser", branch: "feature/a-really-long-branch-name-for-testing" });
footer("outside-home", { cwd: "/var/lib/project/with/a/very/long/path/that/needs/truncation/at/narrow/widths" });
footer("no-branch", { branch: null });
footer("statuses", { statuses: { zeta: "last", alpha: "first\tstatus\nwith newline", mid: "\x1b[32mgreen\x1b[39m  status" } });
footer("long-status", { statuses: { a: "x".repeat(200) } });
footer("long-model", { model: model("a-model-with-an-extremely-long-identifier-that-will-not-fit-in-the-footer", "openrouter", true), providerCount: 2 });

// --- status indicators --------------------------------------------------------------------------
const buildStatus = (input) => {
	const ui = fakeTui();
	let indicator;
	switch (input.type) {
		case "working":
			indicator = new WorkingStatusIndicator(ui, input.message, input.indicator);
			break;
		case "retry":
			indicator = new RetryStatusIndicator(ui, input.attempt, input.maxAttempts, input.delayMs);
			break;
		case "compaction":
			indicator = new CompactionStatusIndicator(ui, input.reason);
			break;
		case "branchSummary":
			indicator = new BranchSummaryStatusIndicator(ui);
			break;
		default:
			return (w) => new IdleStatus().render(w);
	}
	return (w) => {
		const lines = indicator.render(w);
		const out = [...lines, indicator.renderInBorder(w), indicator.renderSpinnerInBorder(w)];
		return out;
	};
};
g.add("status/working", "status_indicator", { type: "working", message: "Working..." }, buildStatus);
g.add("status/working-long", "status_indicator", { type: "working", message: "Reading a lot of files to understand the whole codebase before editing anything at all" }, buildStatus);
g.add("status/working-custom-frames", "status_indicator", { type: "working", message: "Thinking", indicator: { frames: ["\x1b[35m●\x1b[39m", "○"], intervalMs: 200 } }, buildStatus);
g.add("status/working-no-frames", "status_indicator", { type: "working", message: "Quiet", indicator: { frames: [] } }, buildStatus);
g.add("status/retry", "status_indicator", { type: "retry", attempt: 2, maxAttempts: 5, delayMs: 4500 }, buildStatus);
g.add("status/compaction-manual", "status_indicator", { type: "compaction", reason: "manual" }, buildStatus);
g.add("status/compaction-threshold", "status_indicator", { type: "compaction", reason: "threshold" }, buildStatus);
g.add("status/compaction-overflow", "status_indicator", { type: "compaction", reason: "overflow" }, buildStatus);
g.add("status/branch-summary", "status_indicator", { type: "branchSummary" }, buildStatus);
g.add("status/idle", "status_indicator", { type: "idle" }, buildStatus);

// --- borders, loaders, hints --------------------------------------------------------------------
g.add("border/default", "dynamic_border", {}, () => new DynamicBorder());
g.add("bordered-loader/cancellable", "bordered_loader", { message: "Fetching models...", cancellable: true }, (input) =>
	new BorderedLoader(fakeTui(), theme, input.message, { cancellable: input.cancellable }),
);
g.add("bordered-loader/plain", "bordered_loader", { message: "Saving", cancellable: false }, (input) =>
	new BorderedLoader(fakeTui(), theme, input.message, { cancellable: input.cancellable }),
);
g.add("auth-url/default", "auth_url", { url: "https://auth.example.com/oauth/authorize?client_id=pi&redirect_uri=http%3A%2F%2Flocalhost%3A1455%2Fcallback&scope=openid%20profile" }, (input) =>
	new AuthUrlComponent(fakeTui(), input.url),
);
const hints = [
	["keyHint", "app.tools.expand", "to expand"],
	["keyHint", "app.interrupt", "to interrupt"],
	["keyHint", "tui.select.confirm", "select"],
	["keyHint", "tui.select.cancel", "cancel"],
	["keyHint", "app.message.copy", "to copy"],
	["keyHint", "tui.input.newLine", "newline"],
	["keyHint", "app.editor.external", "external editor"],
	["keyHint", "app.thinking.cycle", "thinking"],
	["keyHint", "app.model.cycleForward", "model"],
	["keyText", "app.clear"],
	["keyText", "app.exit"],
	["keyText", "app.session.tree"],
	["rawKeyHint", "↑↓", "navigate"],
];
g.add("hints/all", "key_hints", { hints }, (input) => () =>
	input.hints.map(([fn, a, b]) => (fn === "keyHint" ? keyHint(a, b) : fn === "keyText" ? keyText(a) : rawKeyHint(a, b))),
	{ widths: [80] },
);
g.add("logo/lines", "pi_logo", {}, () => () => [...piLogoLines(), piWordmark()], { widths: [80] });
g.add("announcement/earendil", "earendil_announcement", {}, () => new EarendilAnnouncementComponent());

// --- extension UI -------------------------------------------------------------------------------
const buildSelector = (input) => {
	const c = new ExtensionSelectorComponent(input.title, input.options, noop, noop, { description: input.description });
	feed(c, input.keys);
	return c;
};
const opts = ["Allow once", "Allow always", "Deny", "Deny and tell the model why it was denied in a long explanation"];
g.add("ext-selector/basic", "extension_selector", { title: "Run `rm -rf build`?", options: opts }, buildSelector);
g.add("ext-selector/description", "extension_selector", { title: "Pick one", options: opts, description: "The tool wants to delete files outside the project directory." }, buildSelector);
g.add("ext-selector/navigated", "extension_selector", { title: "Pick", options: opts, keys: ["\x1b[B", "j", "j", "\x1b[A"] }, buildSelector);
g.add("ext-selector/bottom-clamped", "extension_selector", { title: "Pick", options: ["a", "b"], keys: ["\x1b[B", "\x1b[B", "\x1b[B"] }, buildSelector);

const buildInput = (input) => {
	const c = new ExtensionInputComponent(input.title, undefined, noop, noop, { initialValue: input.initialValue, description: input.description });
	if (input.focused) c.focused = true;
	feed(c, input.keys);
	return c;
};
g.add("ext-input/empty", "extension_input", { title: "Commit message" }, buildInput);
g.add("ext-input/initial", "extension_input", { title: "Name", initialValue: "golden-value", description: "Used for the branch name." }, buildInput);
g.add("ext-input/typed-focused", "extension_input", { title: "Name", focused: true, keys: ["h", "e", "l", "l", "o", " ", "w", "\x1b[D", "\x1b[D"] }, buildInput);
g.add("ext-input/long", "extension_input", { title: "Long", initialValue: "a very long value ".repeat(8), focused: true }, buildInput);

const buildExtEditor = (input) => {
	const kb = new KeybindingsManager({});
	const c = new ExtensionEditorComponent(editorTui(), kb, input.title, input.prefill, noop, noop, { description: input.description }, "vi");
	if (input.focused) c.focused = true;
	feed(c, input.keys);
	return c;
};
g.add("ext-editor/empty", "extension_editor", { title: "Edit the plan" }, buildExtEditor);
g.add("ext-editor/prefill", "extension_editor", { title: "Edit", prefill: "line one\nline two\n\nline four is longer than the others and wraps at narrow widths", description: "Describe it." }, buildExtEditor);
g.add("ext-editor/focused-typed", "extension_editor", { title: "Edit", focused: true, keys: ["a", "b", "\x1b[13;2u", "c"] }, buildExtEditor);

// --- main prompt editor -------------------------------------------------------------------------
const buildEditor = (input) => {
	const kb = new KeybindingsManager({});
	const editor = new CustomEditor(editorTui(), getEditorTheme(), kb, {
		embedWorkingStatus: input.embed ?? false,
		paddingX: input.paddingX,
	});
	if (input.text) editor.setText(input.text);
	if (input.focused) editor.focused = true;
	feed(editor, input.keys);
	if (input.status) editor.setWorkingStatusIndicator(new WorkingStatusIndicator(fakeTui(), input.status));
	return editor;
};
g.add("editor/empty", "custom_editor", {}, buildEditor);
g.add("editor/empty-focused", "custom_editor", { focused: true }, buildEditor);
g.add("editor/text", "custom_editor", { text: "fix the bug in parser.ts", focused: true }, buildEditor);
g.add("editor/multiline", "custom_editor", { text: "first line\nsecond line\n\nfourth line with more words that will wrap when narrow", focused: true }, buildEditor);
g.add("editor/padding", "custom_editor", { text: "padded", paddingX: 2, focused: true }, buildEditor);
g.add("editor/cursor-moved", "custom_editor", { text: "hello world", focused: true, keys: ["\x1b[D", "\x1b[D", "\x1b[D", "\x01"] }, buildEditor);
g.add("editor/unicode", "custom_editor", { text: "日本語 テキスト 👍 emoji", focused: true }, buildEditor);
g.add("editor/many-lines", "custom_editor", { text: Array.from({ length: 30 }, (_, i) => `line ${i + 1}`).join("\n"), focused: true }, buildEditor);
g.add("editor/embedded-status", "custom_editor", { embed: true, status: "Working...", text: "queued message", focused: true }, buildEditor);
g.add("editor/embedded-status-overflow", "custom_editor", { embed: true, status: "Working...", text: Array.from({ length: 30 }, (_, i) => `line ${i + 1}`).join("\n"), focused: true }, buildEditor);
g.add("editor/status-not-embedded", "custom_editor", { embed: false, status: "Working...", text: "x" }, buildEditor);

// --- login dialog -------------------------------------------------------------------------------
const buildLogin = (input) => {
	const c = new LoginDialogComponent(fakeTui(), input.provider, noop, input.providerName, input.title);
	for (const step of input.steps ?? []) {
		if (step.deviceCode) c.showDeviceCode(step.deviceCode);
		if (step.details) c.showDetails(step.details);
		if (step.info) c.showInfo(step.info.message, step.info.links, step.info.closeHint);
		if (step.waiting) c.showWaiting(step.waiting);
		if (step.progress) c.showProgress(step.progress);
		if (step.prompt) void c.showPrompt(step.prompt.message, step.prompt.placeholder).catch(noop);
		if (step.manual) void c.showManualInput(step.manual).catch(noop);
		if (step.keys) feed(c, step.keys);
	}
	return c;
};
g.add("login/initial", "login_dialog", { provider: "anthropic" }, buildLogin);
g.add("login/device-code", "login_dialog", {
	provider: "github-copilot",
	providerName: "GitHub Copilot",
	steps: [{ deviceCode: { userCode: "ABCD-1234", verificationUri: "https://github.com/login/device" } }, { waiting: "Waiting for authorization..." }],
}, buildLogin);
g.add("login/prompt", "login_dialog", {
	provider: "custom",
	title: "Sign in to Custom",
	steps: [{ details: ["Step 1: open the site", "Step 2: copy the key"] }, { prompt: { message: "Paste your API key:", placeholder: "sk-..." } }, { keys: ["s", "k", "-", "1"] }],
}, buildLogin);
g.add("login/manual", "login_dialog", { provider: "openai", steps: [{ progress: "Starting callback server..." }, { manual: "Paste the redirect URL:" }] }, buildLogin);
g.add("login/info", "login_dialog", {
	provider: "radius",
	steps: [{ info: { message: "Your subscription is managed on the web.", links: [{ label: "Dashboard", url: "https://example.com/dash" }, { url: "https://example.com/help" }], closeHint: true } }],
}, buildLogin);

// --- first-time setup ---------------------------------------------------------------------------
const buildSetup = (input) => {
	const c = new FirstTimeSetupComponent({ onThemePreview: noop, onSubmit: noop, onCancel: noop });
	feed(c, input.keys);
	return c;
};
g.add("setup/initial", "first_time_setup", {}, buildSetup);
g.add("setup/theme-moved", "first_time_setup", { keys: ["\x1b[B", "\x1b[B"] }, buildSetup);
g.add("setup/analytics-step", "first_time_setup", { keys: ["\x1b[B", "\r"] }, buildSetup);
g.add("setup/analytics-moved", "first_time_setup", { keys: ["\r", "\x1b[B"] }, buildSetup);

g.write();
process.exit(0);
