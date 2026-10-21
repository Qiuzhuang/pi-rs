// `@earendil-works/pi-coding-agent` for extensions running in pi-jsrt.
//
// Runtime values extensions use: tool factories backed by the Rust built-in tools, truncation
// helpers, theme helpers, UI components (DynamicBorder, BorderedLoader, CustomEditor), event
// type guards and config paths. Host-only classes (sessions, runtimes, modes) are exported as
// stubs that throw when constructed.

import {
	call,
	callAsync,
	makeContext,
	RustComponent,
	rustComponent,
	themeProxy,
	tokenFor,
	u,
	keybindingsProxy,
	markRustBase,
} from "pi-rs:bridge";
import { CancellableLoader, Container, Editor, Loader, Spacer, Text, getKeybindings } from "@earendil-works/pi-tui";

const host = (fn, ...args) => u(call("ca", { fn, args }));

// ---------------------------------------------------------------------------
// Config
// ---------------------------------------------------------------------------

const config = call("ca.config");
export const CONFIG_DIR_NAME = config.configDirName;
export const VERSION = config.version;
export const getAgentDir = () => call("ca.config").agentDir;
export const getDocsPath = () => config.docsPath;
export const getExamplesPath = () => config.examplesPath;
export const getPackageDir = () => config.packageDir;
export const getReadmePath = () => config.readmePath;

// ---------------------------------------------------------------------------
// Extension helpers
// ---------------------------------------------------------------------------

export function defineTool(tool) {
	return tool;
}

export function isToolCallEventType(toolName, event) {
	return event?.toolName === toolName;
}

export const isBashToolResult = (e) => e?.toolName === "bash";
export const isPowerShellToolResult = (e) => e?.toolName === "powershell";
export const isReadToolResult = (e) => e?.toolName === "read";
export const isEditToolResult = (e) => e?.toolName === "edit";
export const isWriteToolResult = (e) => e?.toolName === "write";
export const isGrepToolResult = (e) => e?.toolName === "grep";
export const isFindToolResult = (e) => e?.toolName === "find";
export const isLsToolResult = (e) => e?.toolName === "ls";

export function createSyntheticSourceInfo(path, options) {
	return {
		path,
		source: options.source,
		scope: options.scope ?? "temporary",
		origin: options.origin ?? "top-level",
		...(options.baseDir !== undefined ? { baseDir: options.baseDir } : {}),
	};
}

// ---------------------------------------------------------------------------
// Truncation and formatting (core/tools)
// ---------------------------------------------------------------------------

const constants = call("ca.constants");
export const DEFAULT_MAX_BYTES = constants.DEFAULT_MAX_BYTES;
export const DEFAULT_MAX_LINES = constants.DEFAULT_MAX_LINES;

export const formatSize = (bytes) => host("formatSize", bytes);
export const truncateHead = (content, options) => host("truncateHead", String(content), options ?? null);
export const truncateTail = (content, options) => host("truncateTail", String(content), options ?? null);
export const truncateLine = (line, maxChars) => host("truncateLine", String(line), maxChars ?? null);
export const formatReadLineRange = (...args) => host("formatReadLineRange", ...args);
export const parseFrontmatter = (content) => host("parseFrontmatter", String(content));
export const stripFrontmatter = (content) => host("stripFrontmatter", String(content));
export const convertToLlm = (messages) => host("convertToLlm", messages);
export const serializeConversation = (messages) => host("serializeConversation", messages);
export const estimateTokens = (message) => host("estimateTokens", message);
export const getLanguageFromPath = (path) => host("getLanguageFromPath", path);
export const highlightCode = (code, lang) => host("highlightCode", code, lang ?? null) ?? String(code).split("\n");
export const generateDiffString = (...args) => host("generateDiffString", ...args);
export const parseSkillBlock = (text) => host("parseSkillBlock", text);
export const VIRTUAL_MODEL_STATE_ENTRY = "pi.virtual-model-state";

const fileMutationQueues = new Map();
let registrationQueue = Promise.resolve();

/**
 * Serialize file mutation operations targeting the same file.
 * Operations for different files still run in parallel.
 */
export async function withFileMutationQueue(filePath, fn) {
	const registration = registrationQueue.then(async () => {
		const key = host("mutationQueueKey", String(filePath));
		const currentQueue = fileMutationQueues.get(key) ?? Promise.resolve();
		let releaseNext;
		const nextQueue = new Promise((resolve) => {
			releaseNext = resolve;
		});
		const chainedQueue = currentQueue.then(() => nextQueue);
		fileMutationQueues.set(key, chainedQueue);
		return { key, currentQueue, chainedQueue, releaseNext };
	});
	registrationQueue = registration.then(
		() => undefined,
		() => undefined,
	);
	const { key, currentQueue, chainedQueue, releaseNext } = await registration;
	await currentQueue;
	try {
		return await fn();
	} finally {
		releaseNext();
		if (fileMutationQueues.get(key) === chainedQueue) fileMutationQueues.delete(key);
	}
}

// ---------------------------------------------------------------------------
// Built-in tools (Rust implementations)
// ---------------------------------------------------------------------------

function builtinTool(kind, cwd, options, definition) {
	const d = call("builtin.create", { kind, cwd, options: options ?? null });
	const id = d.id;
	const tool = {
		name: d.name,
		label: d.label,
		description: d.description,
		parameters: d.parameters,
	};
	if (d.promptSnippet !== null && d.promptSnippet !== undefined) tool.promptSnippet = d.promptSnippet;
	if (d.promptGuidelines !== null && d.promptGuidelines !== undefined) tool.promptGuidelines = d.promptGuidelines;
	if (d.renderShell !== null && d.renderShell !== undefined) tool.renderShell = d.renderShell;
	if (d.executionMode !== null && d.executionMode !== undefined) tool.executionMode = d.executionMode;
	const execute = async (toolCallId, params, signal, onUpdate, ctx) =>
		await callAsync("builtin.execute", {
			id,
			toolCallId,
			params,
			signal: tokenFor(signal) ?? null,
			onUpdate: onUpdate ?? null,
			ctx: ctx?.__ctxId ?? null,
		});
	execute.__piRust = { tool: id, kind: "execute" };
	tool.execute = execute;
	if (definition) {
		if (d.hasPrepareArguments) {
			const prepareArguments = (args) => host("builtinPrepareArguments", id, args);
			prepareArguments.__piRust = { tool: id, kind: "prepareArguments" };
			tool.prepareArguments = prepareArguments;
		}
		if (d.hasRenderCall) {
			const renderCall = (args, theme, context) =>
				rustComponent(
					call("builtin.renderCall", { id, args, theme: theme?.__themeId ?? null, context: context?.__rustContext ?? null }),
				);
			renderCall.__piRust = { tool: id, kind: "renderCall" };
			tool.renderCall = renderCall;
		}
		if (d.hasRenderResult) {
			const renderResult = (result, options, theme, context) =>
				rustComponent(
					call("builtin.renderResult", {
						id,
						result,
						options,
						theme: theme?.__themeId ?? null,
						context: context?.__rustContext ?? null,
					}),
				);
			renderResult.__piRust = { tool: id, kind: "renderResult" };
			tool.renderResult = renderResult;
		}
	}
	return tool;
}

export const createReadToolDefinition = (cwd, options) => builtinTool("read", cwd, options, true);
export const createBashToolDefinition = (cwd, options) => builtinTool("bash", cwd, options, true);
export const createEditToolDefinition = (cwd, options) => builtinTool("edit", cwd, options, true);
export const createWriteToolDefinition = (cwd, options) => builtinTool("write", cwd, options, true);
export const createGrepToolDefinition = (cwd, options) => builtinTool("grep", cwd, options, true);
export const createFindToolDefinition = (cwd, options) => builtinTool("find", cwd, options, true);
export const createLsToolDefinition = (cwd, options) => builtinTool("ls", cwd, options, true);
export const createPowerShellToolDefinition = (cwd, options) => builtinTool("powershell", cwd, options, true);
export const createReadTool = (cwd, options) => builtinTool("read", cwd, options, false);
export const createBashTool = (cwd, options) => builtinTool("bash", cwd, options, false);
export const createEditTool = (cwd, options) => builtinTool("edit", cwd, options, false);
export const createWriteTool = (cwd, options) => builtinTool("write", cwd, options, false);
export const createGrepTool = (cwd, options) => builtinTool("grep", cwd, options, false);
export const createFindTool = (cwd, options) => builtinTool("find", cwd, options, false);
export const createLsTool = (cwd, options) => builtinTool("ls", cwd, options, false);
export const createPowerShellTool = (cwd, options) => builtinTool("powershell", cwd, options, false);
export const createCodingTools = (cwd, options) => ["read", "bash", "edit", "write"].map((k) => builtinTool(k, cwd, options?.[k], false));
export const createReadOnlyTools = (cwd, options) => ["read", "grep", "find", "ls"].map((k) => builtinTool(k, cwd, options?.[k], false));

export function createLocalBashOperations(options) {
	return {
		exec: (command, cwd, execOptions) =>
			callAsync("ca.localBashExec", {
				command,
				cwd,
				shellPath: options?.shellPath ?? null,
				options: {
					onData: execOptions?.onData ?? null,
					signal: tokenFor(execOptions?.signal) ?? null,
					timeout: execOptions?.timeout ?? null,
					env: execOptions?.env ?? null,
				},
			}),
	};
}

export const createLocalPowerShellOperations = () => createLocalBashOperations({});

// ---------------------------------------------------------------------------
// Theme helpers
// ---------------------------------------------------------------------------

function currentTheme() {
	return themeProxy(call("ca.theme"));
}

export function getMarkdownTheme() {
	const theme = currentTheme();
	const md = {
		heading: (t) => theme.fg("mdHeading", theme.bold(t)),
		link: (t) => theme.fg("mdLink", t),
		linkUrl: (t) => theme.fg("mdLinkUrl", t),
		code: (t) => theme.fg("mdCode", t),
		codeBlock: (t) => theme.fg("mdCodeBlock", t),
		codeBlockBorder: (t) => theme.fg("mdCodeBlockBorder", t),
		quote: (t) => theme.fg("mdQuote", t),
		quoteBorder: (t) => theme.fg("mdQuoteBorder", t),
		hr: (t) => theme.fg("mdHr", t),
		listBullet: (t) => theme.fg("mdListBullet", t),
		bold: (t) => theme.bold(t),
		italic: (t) => theme.italic(t),
		underline: (t) => theme.underline(t),
		strikethrough: (t) => theme.strikethrough(t),
		highlightCode: (code, lang) => highlightCode(code, lang),
	};
	Object.defineProperty(md, "__mdTheme", { value: "default", enumerable: false });
	return md;
}

export function getSelectListTheme() {
	const theme = currentTheme();
	return {
		selectedPrefix: (t) => theme.fg("accent", t),
		selectedText: (t) => theme.fg("accent", t),
		description: (t) => theme.fg("muted", t),
		scrollInfo: (t) => theme.fg("muted", t),
		noMatch: (t) => theme.fg("muted", t),
	};
}

export function getSettingsListTheme() {
	const theme = currentTheme();
	return {
		label: (text, selected) => (selected ? theme.fg("accent", text) : text),
		value: (text, selected) => (selected ? theme.fg("accent", text) : theme.fg("muted", text)),
		description: (text) => theme.fg("dim", text),
		cursor: theme.fg("accent", "→ "),
		hint: (text) => theme.fg("dim", text),
	};
}

export function getEditorTheme() {
	const theme = currentTheme();
	return {
		borderColor: (t) => theme.fg("borderMuted", t),
		selectList: getSelectListTheme(),
	};
}

export const initTheme = () => {};

export class Theme {
	constructor() {
		throw new Error("Theme instances are provided by pi (ctx.ui.theme)");
	}
}

function formatKeyPart(part, options) {
	const displayPart = call("ca.platform") === "darwin" && part.toLowerCase() === "alt" ? "option" : part;
	return options.capitalize ? displayPart.charAt(0).toUpperCase() + displayPart.slice(1) : displayPart;
}

export function formatKeyText(key, options = {}) {
	return key
		.split("/")
		.map((k) =>
			k
				.split("+")
				.map((part) => formatKeyPart(part, options))
				.join("+"),
		)
		.join("/");
}

function formatKeys(keys, options = {}) {
	if (keys.length === 0) return "";
	return formatKeyText(keys.join("/"), options);
}

export function keyText(keybinding) {
	return formatKeys(getKeybindings().getKeys(keybinding));
}

export function keyDisplayText(keybinding) {
	return formatKeys(getKeybindings().getKeys(keybinding), { capitalize: true });
}

export function keyHint(keybinding, description) {
	const theme = currentTheme();
	return theme.fg("dim", keyText(keybinding)) + theme.fg("muted", ` ${description}`);
}

export function rawKeyHint(key, description) {
	const theme = currentTheme();
	return theme.fg("dim", formatKeyText(key)) + theme.fg("muted", ` ${description}`);
}

// ---------------------------------------------------------------------------
// Components
// ---------------------------------------------------------------------------

export class DynamicBorder {
	constructor(color) {
		this.color = color ?? ((str) => currentTheme().fg("border", str));
	}

	invalidate() {}

	render(width) {
		return [this.color("─".repeat(Math.max(1, width)))];
	}
}

export class BorderedLoader extends Container {
	constructor(tui, theme, message, options) {
		super();
		this.cancellable = options?.cancellable ?? true;
		const borderColor = (s) => theme.fg("border", s);
		this.addChild(new DynamicBorder(borderColor));
		if (this.cancellable) {
			this.loader = new CancellableLoader(
				tui,
				(s) => theme.fg("accent", s),
				(s) => theme.fg("muted", s),
				message,
			);
		} else {
			this.signalController = new AbortController();
			this.loader = new Loader(
				tui,
				(s) => theme.fg("accent", s),
				(s) => theme.fg("muted", s),
				message,
			);
		}
		this.addChild(this.loader);
		if (this.cancellable) {
			this.addChild(new Spacer(1));
			this.addChild(new Text(keyHint("tui.select.cancel", "cancel"), 1, 0));
		}
		this.addChild(new Spacer(1));
		this.addChild(new DynamicBorder(borderColor));
	}

	get signal() {
		if (this.cancellable) return this.loader.signal;
		return this.signalController?.signal ?? new AbortController().signal;
	}

	set onAbort(fn) {
		if (this.cancellable) this.loader.onAbort = fn;
	}

	handleInput(data) {
		if (this.cancellable) this.loader.handleInput(data);
	}

	dispose() {
		if (typeof this.loader.dispose === "function") this.loader.dispose();
		else if (typeof this.loader.stop === "function") this.loader.stop();
	}
}

/** Editor with app-level keybindings, backed by the Rust `CustomEditor`. */
export class CustomEditor extends Editor {
	constructor(tui, theme, keybindings, options) {
		super(tui, theme, options ?? {}, "customEditor", keybindings);
		this.keybindings = keybindings;
		this.embedWorkingStatus = options?.embedWorkingStatus ?? false;
		const id = this.__rid;
		const handlers = new Map();
		const originalSet = handlers.set.bind(handlers);
		const originalDelete = handlers.delete.bind(handlers);
		handlers.set = (action, handler) => {
			originalSet(action, handler);
			call("rc", { id, m: "setActionHandler", a: [action, handler] });
			return handlers;
		};
		handlers.delete = (action) => {
			call("rc", { id, m: "setActionHandler", a: [action, null] });
			return originalDelete(action);
		};
		this.actionHandlers = handlers;
		for (const name of ["onEscape", "onCtrlD", "onPasteImage", "onExtensionShortcut"]) {
			let value;
			Object.defineProperty(this, name, {
				get: () => value,
				set: (fn) => {
					value = fn;
					call("rc", { id, m: "set_" + name, a: [fn ?? null] });
				},
				enumerable: true,
				configurable: true,
			});
		}
	}

	onAction(action, handler) {
		this.actionHandlers.set(action, handler);
	}

	setWorkingStatusIndicator(_indicator) {}
}

markRustBase(CustomEditor);

function unavailableClass(name) {
	return class {
		constructor() {
			throw new Error(`${name} is not available to extensions in pi-rs`);
		}
	};
}

function unavailableFn(name) {
	return () => {
		throw new Error(`${name} is not available to extensions in pi-rs`);
	};
}

export const AgentSession = unavailableClass("AgentSession");
export const AgentSessionRuntime = unavailableClass("AgentSessionRuntime");
export const ExtensionRunner = unavailableClass("ExtensionRunner");
export const ModelRegistry = unavailableClass("ModelRegistry");
export const ModelRuntime = unavailableClass("ModelRuntime");
export const CredentialSynchronizationError = class extends Error {};
export const DefaultPackageManager = unavailableClass("DefaultPackageManager");
export const DefaultResourceLoader = unavailableClass("DefaultResourceLoader");
export const SessionManager = unavailableClass("SessionManager");
export const SettingsManager = unavailableClass("SettingsManager");
export const ProjectTrustStore = unavailableClass("ProjectTrustStore");
export const InteractiveMode = unavailableClass("InteractiveMode");
export const RpcClient = unavailableClass("RpcClient");
export const ArminComponent = unavailableClass("ArminComponent");
export const AssistantMessageComponent = unavailableClass("AssistantMessageComponent");
export const BashExecutionComponent = unavailableClass("BashExecutionComponent");
export const BranchSummaryMessageComponent = unavailableClass("BranchSummaryMessageComponent");
export const CompactionSummaryMessageComponent = unavailableClass("CompactionSummaryMessageComponent");
export const CustomMessageComponent = unavailableClass("CustomMessageComponent");
export const ExtensionEditorComponent = unavailableClass("ExtensionEditorComponent");
export const ExtensionInputComponent = unavailableClass("ExtensionInputComponent");
export const ExtensionSelectorComponent = unavailableClass("ExtensionSelectorComponent");
export const FooterComponent = unavailableClass("FooterComponent");
export const LoginDialogComponent = unavailableClass("LoginDialogComponent");
export const ModelSelectorComponent = unavailableClass("ModelSelectorComponent");
export const OAuthSelectorComponent = unavailableClass("OAuthSelectorComponent");
export const SessionSelectorComponent = unavailableClass("SessionSelectorComponent");
export const SettingsSelectorComponent = unavailableClass("SettingsSelectorComponent");
export const ShowImagesSelectorComponent = unavailableClass("ShowImagesSelectorComponent");
export const SkillInvocationMessageComponent = unavailableClass("SkillInvocationMessageComponent");
export const ThemeSelectorComponent = unavailableClass("ThemeSelectorComponent");
export const ThinkingSelectorComponent = unavailableClass("ThinkingSelectorComponent");
export const ToolExecutionComponent = unavailableClass("ToolExecutionComponent");
export const TreeSelectorComponent = unavailableClass("TreeSelectorComponent");
export const UserMessageComponent = unavailableClass("UserMessageComponent");
export const UserMessageSelectorComponent = unavailableClass("UserMessageSelectorComponent");

export const createAgentSession = unavailableFn("createAgentSession");
export const createAgentSessionFromServices = unavailableFn("createAgentSessionFromServices");
export const createAgentSessionRuntime = unavailableFn("createAgentSessionRuntime");
export const createAgentSessionServices = unavailableFn("createAgentSessionServices");
export const createExtensionRuntime = unavailableFn("createExtensionRuntime");
export const discoverAndLoadExtensions = unavailableFn("discoverAndLoadExtensions");
export const wrapRegisteredTool = unavailableFn("wrapRegisteredTool");
export const wrapRegisteredTools = unavailableFn("wrapRegisteredTools");
export const createCodemodeExtension = unavailableFn("createCodemodeExtension");
export const createMcpExtension = unavailableFn("createMcpExtension");
export const createToolSearchExtension = unavailableFn("createToolSearchExtension");
export const createEventBus = unavailableFn("createEventBus");
export const main = unavailableFn("main");
export const runPrintMode = unavailableFn("runPrintMode");
export const runRpcMode = unavailableFn("runRpcMode");
export const parseArgs = unavailableFn("parseArgs");
export const readStoredCredential = unavailableFn("readStoredCredential");
export const resolveCliModel = unavailableFn("resolveCliModel");
export const resolveModelScopeWithDiagnostics = unavailableFn("resolveModelScopeWithDiagnostics");
export const loadProjectContextFiles = unavailableFn("loadProjectContextFiles");
export const loadSkills = unavailableFn("loadSkills");
export const loadSkillsFromDir = unavailableFn("loadSkillsFromDir");
export const formatSkillsForPrompt = unavailableFn("formatSkillsForPrompt");
export const hasTrustRequiringProjectResources = unavailableFn("hasTrustRequiringProjectResources");
export const copyToClipboard = (text) => callAsync("ca.copyToClipboard", { text: String(text) });
export const convertToPng = unavailableFn("convertToPng");
export const resizeImage = unavailableFn("resizeImage");
export const formatDimensionNote = unavailableFn("formatDimensionNote");
export const detectSupportedImageMimeTypeFromFile = unavailableFn("detectSupportedImageMimeTypeFromFile");
export const getShellConfig = unavailableFn("getShellConfig");
export const getPowerShellConfig = unavailableFn("getPowerShellConfig");
export const renderDiff = unavailableFn("renderDiff");
export const truncateToVisualLines = unavailableFn("truncateToVisualLines");
export const generateUnifiedPatch = unavailableFn("generateUnifiedPatch");
export const calculateContextTokens = unavailableFn("calculateContextTokens");
export const collectEntriesForBranchSummary = unavailableFn("collectEntriesForBranchSummary");
export const compact = unavailableFn("compact");
export const DEFAULT_COMPACTION_SETTINGS = { enabled: true, reserveTokens: 16384, keepRecentTokens: 20000 };
export const findCutPoint = unavailableFn("findCutPoint");
export const findTurnStartIndex = unavailableFn("findTurnStartIndex");
export const generateBranchSummary = unavailableFn("generateBranchSummary");
export const generateSummary = unavailableFn("generateSummary");
export const generateSummaryWithUsage = unavailableFn("generateSummaryWithUsage");
export const getLastAssistantUsage = unavailableFn("getLastAssistantUsage");
export const prepareBranchEntries = unavailableFn("prepareBranchEntries");
export const shouldCompact = unavailableFn("shouldCompact");
export const buildContextEntries = unavailableFn("buildContextEntries");
export const buildSessionContext = unavailableFn("buildSessionContext");
export const buildSessionProjection = unavailableFn("buildSessionProjection");
export const CURRENT_SESSION_VERSION = 3;
export const getLatestCompactionEntry = unavailableFn("getLatestCompactionEntry");
export const migrateSessionEntries = unavailableFn("migrateSessionEntries");
export const parseSessionEntries = unavailableFn("parseSessionEntries");
export const sessionEntryToContextMessages = unavailableFn("sessionEntryToContextMessages");

export { makeContext as __makeContext, keybindingsProxy as __keybindingsProxy, RustComponent as __RustComponent };
