// Golden renders of the selectors that take plain data: thinking, theme, show-images, trust,
// user-message (fork), oauth/login and settings.
import { Golden, ref } from "./common.mjs";

const C = "coding-agent/src/modes/interactive/components";
const { ThinkingSelectorComponent } = await ref(`${C}/thinking-selector.ts`);
const { ThemeSelectorComponent } = await ref(`${C}/theme-selector.ts`);
const { ShowImagesSelectorComponent } = await ref(`${C}/show-images-selector.ts`);
const { TrustSelectorComponent } = await ref(`${C}/trust-selector.ts`);
const { UserMessageSelectorComponent } = await ref(`${C}/user-message-selector.ts`);
const { OAuthSelectorComponent } = await ref(`${C}/oauth-selector.ts`);
const { SettingsSelectorComponent } = await ref(`${C}/settings-selector.ts`);

const g = new Golden("selectors.json");
const noop = () => {};

function feed(component, keys) {
	for (const key of keys ?? []) component.handleInput(key);
}

const DOWN = "\x1b[B";
const UP = "\x1b[A";
const ENTER = "\r";

// --- thinking -----------------------------------------------------------------------------------
const levels = ["off", "minimal", "low", "medium", "high", "xhigh"];
const buildThinking = (input) => {
	const c = new ThinkingSelectorComponent(
		input.current,
		input.levels,
		noop,
		noop,
		input.withDefault ? noop : undefined,
		input.defaultLevel,
	);
	feed(c, input.keys);
	return c;
};
g.add("thinking/basic", "thinking_selector", { current: "medium", levels }, buildThinking);
g.add("thinking/with-default", "thinking_selector", { current: "high", levels, withDefault: true, defaultLevel: "low" }, buildThinking);
g.add("thinking/browsed", "thinking_selector", { current: "off", levels: ["off", "low", "high"], keys: [DOWN, DOWN, DOWN] }, buildThinking);

// --- theme --------------------------------------------------------------------------------------
const buildTheme = (input) => {
	const c = new ThemeSelectorComponent(input.current, noop, noop, noop);
	feed(c.getSelectList ? c.getSelectList() : c, input.keys);
	return c;
};
g.add("theme/dark", "theme_selector", { current: "dark" }, buildTheme);
g.add("theme/light-browsed", "theme_selector", { current: "light", keys: [UP] }, buildTheme);

// --- show images --------------------------------------------------------------------------------
g.add("show-images/yes", "show_images_selector", { current: true }, (input) => new ShowImagesSelectorComponent(input.current, noop, noop));
g.add("show-images/no", "show_images_selector", { current: false }, (input) => new ShowImagesSelectorComponent(input.current, noop, noop));

// --- trust --------------------------------------------------------------------------------------
const buildTrust = (input) => {
	const c = new TrustSelectorComponent({
		cwd: input.cwd,
		savedDecision: input.savedDecision,
		projectTrusted: input.projectTrusted,
		onSelect: noop,
		onCancel: noop,
	});
	feed(c, input.keys);
	return c;
};
g.add("trust/none", "trust_selector", { cwd: "/home/golden/project", savedDecision: null, projectTrusted: false }, buildTrust);
g.add("trust/saved", "trust_selector", { cwd: "/home/golden/project", savedDecision: { path: "/home/golden/project", decision: true }, projectTrusted: true }, buildTrust);
g.add("trust/inherited", "trust_selector", { cwd: "/home/golden/project/nested", savedDecision: { path: "/home/golden", decision: false }, projectTrusted: false, keys: [DOWN] }, buildTrust);
g.add("trust/root", "trust_selector", { cwd: "/", savedDecision: null, projectTrusted: false }, buildTrust);

// --- user message selector (fork) ---------------------------------------------------------------
const buildUserMessages = (input) => {
	const c = new UserMessageSelectorComponent(input.messages, noop, noop, input.initial);
	feed(c.getMessageList(), input.keys);
	return c;
};
const userMessages = Array.from({ length: 14 }, (_, i) => ({
	id: `m${i}`,
	text: i % 4 === 0 ? `Message ${i}: a long prompt that keeps going so the preview needs truncation in narrow terminals\nwith a second line` : `Message ${i}`,
	timestamp: `2026-01-15T1${i % 10}:00:00.000Z`,
}));
g.add("user-messages/default", "user_message_selector", { messages: userMessages }, buildUserMessages);
g.add("user-messages/initial", "user_message_selector", { messages: userMessages, initial: "m2", keys: [DOWN] }, buildUserMessages);
g.add("user-messages/few", "user_message_selector", { messages: userMessages.slice(0, 3), keys: [UP] }, buildUserMessages);

// --- oauth / login ------------------------------------------------------------------------------
const providers = [
	{ id: "anthropic", name: "Anthropic (Claude Pro/Max)", authType: "oauth", status: { type: "oauth", source: "OAuth" } },
	{ id: "openai-codex", name: "ChatGPT Plus/Pro (Codex)", authType: "oauth", subscription: true },
	{ id: "github-copilot", name: "GitHub Copilot", authType: "oauth", subscription: false, status: { type: "api_key", source: "GITHUB_TOKEN" } },
	{ id: "anthropic", name: "Anthropic", authType: "api_key", status: { type: "api_key", source: "ANTHROPIC_API_KEY" } },
	{ id: "openai", name: "OpenAI", authType: "api_key" },
	{ id: "google", name: "Google Gemini", authType: "api_key", status: { type: "api_key", source: "models.json" } },
	{ id: "groq", name: "Groq", authType: "api_key", status: { type: "oauth" } },
];
const buildOAuth = (input) => {
	const c = new OAuthSelectorComponent(input.mode, input.providers, noop, noop, input.search);
	feed(c, input.keys);
	return c;
};
g.add("oauth/login", "oauth_selector", { mode: "login", providers }, buildOAuth);
g.add("oauth/login-browsed", "oauth_selector", { mode: "login", providers, keys: [DOWN, DOWN, DOWN] }, buildOAuth);
g.add("oauth/login-search", "oauth_selector", { mode: "login", providers, search: "open" }, buildOAuth);
g.add("oauth/login-typed", "oauth_selector", { mode: "login", providers, keys: ["g", "o"] }, buildOAuth);
g.add("oauth/logout", "oauth_selector", { mode: "logout", providers: providers.filter((p) => p.status) }, buildOAuth);
g.add("oauth/api-keys-only", "oauth_selector", { mode: "login", providers: providers.filter((p) => p.authType === "api_key") }, buildOAuth);
g.add("oauth/empty", "oauth_selector", { mode: "logout", providers: [] }, buildOAuth);

// --- settings -----------------------------------------------------------------------------------
const settingsConfig = {
	autoCompact: true,
	defaultModel: "anthropic/claude-sonnet-4-5",
	availableDefaultModels: [],
	showImages: true,
	imageWidthCells: 60,
	autoResizeImages: true,
	blockImages: false,
	enableSkillCommands: true,
	steeringMode: "one-at-a-time",
	followUpMode: "one-at-a-time",
	transport: "sse",
	sessionShareDestination: "auto",
	httpIdleTimeoutMs: 300000,
	cacheWarmingMode: "off",
	thinkingLevel: "medium",
	availableThinkingLevels: ["off", "minimal", "low", "medium", "high"],
	modelThinkingLevels: {},
	currentTheme: "dark",
	terminalTheme: "dark",
	availableThemes: ["dark", "light"],
	hideThinkingBlock: false,
	mermaidRenderingMode: "streaming",
	showCacheMissNotices: false,
	collapseChangelog: false,
	enableInstallTelemetry: true,
	doubleEscapeAction: "tree",
	treeFilterMode: "default",
	showHardwareCursor: false,
	editorPaddingX: 0,
	outputPad: 1,
	autocompleteMaxVisible: 5,
	quietStartup: false,
	defaultProjectTrust: "ask",
	clearOnShrink: false,
	showTerminalProgress: true,
	tuiMode: "regular",
	fullscreenExitOutput: "transcript",
	fullscreenScrollbar: "auto",
	fullscreenCopyOnSelect: true,
	fullscreenWheelScrollLines: 7,
	warnings: {},
};
const callbacks = new Proxy({}, { get: () => noop });
const buildSettings = (input) => {
	const c = new SettingsSelectorComponent({ ...settingsConfig, ...input.config }, callbacks);
	feed(c.getSettingsList(), input.keys);
	return c;
};
g.add("settings/initial", "settings_selector", { config: {} }, buildSettings);
g.add("settings/scrolled", "settings_selector", { config: {}, keys: Array(12).fill(DOWN) }, buildSettings);
g.add("settings/search", "settings_selector", { config: {}, keys: ["t", "h", "e", "m", "e"] }, buildSettings);
g.add("settings/search-none", "settings_selector", { config: {}, keys: ["z", "z", "q"] }, buildSettings);
g.add("settings/toggle-auto-compact", "settings_selector", { config: {}, keys: [ENTER] }, buildSettings);
g.add("settings/thinking-submenu", "settings_selector", { config: {}, keys: ["t", "h", "i", "n", "k", "i", "n", "g", " ", "l", ENTER] }, buildSettings);
g.add("settings/theme-submenu", "settings_selector", { config: {}, keys: ["t", "h", "e", "m", "e", ENTER] }, buildSettings);
g.add("settings/light", "settings_selector", { config: { currentTheme: "light", terminalTheme: "light", tuiMode: "fullscreen", quietStartup: true } }, buildSettings);
g.add("settings/wrap-up", "settings_selector", { config: {}, keys: [UP] }, buildSettings);

g.write();
process.exit(0);
