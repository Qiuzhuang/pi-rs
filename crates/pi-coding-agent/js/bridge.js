// Internal glue between pi extensions (JavaScript, running in pi-jsrt) and the Rust extension
// runtime. Rust calls the exported functions; they build the `pi` and `ctx` objects extensions
// see and forward calls to Rust through the `__piHost.call` / `__piHost.callAsync` host
// functions. Values cross the boundary as JSON; JS functions become handle markers.

import { ref } from "jsrt:host";

const H = globalThis.__piHost;

/** Synchronous host call. */
export function call(op, payload) {
	return H.call(op, payload ?? {});
}

/** Asynchronous host call (returns a promise). */
export function callAsync(op, payload) {
	return H.callAsync(op, payload ?? {});
}

/** JSON `null` returned by Rust stands for `undefined` in the TS API. */
export function u(value) {
	return value === null ? undefined : value;
}

/** A JS function forwarding to a Rust callback registered under `id`. */
export function rustCallback(id) {
	if (id === null || id === undefined) return undefined;
	return (...args) => u(call("cb", { id, args }));
}

/** An async JS function forwarding to an async Rust callback registered under `id`. */
export function rustAsyncCallback(id) {
	if (id === null || id === undefined) return undefined;
	return async (...args) => u(await callAsync("cb.async", { id, args }));
}

// ---------------------------------------------------------------------------
// Abort signals
// ---------------------------------------------------------------------------

const controllers = new Map();

/** An AbortSignal driven by the Rust cancellation token `id`. */
export function signalFor(id, aborted) {
	if (id === null || id === undefined) return undefined;
	const controller = new AbortController();
	if (aborted) controller.abort();
	else controllers.set(id, controller);
	return controller.signal;
}

export function abortSignal(id) {
	const controller = controllers.get(id);
	controllers.delete(id);
	controller?.abort();
}

export function releaseSignal(id) {
	controllers.delete(id);
}

/** A Rust cancellation token id mirroring `signal` (JS → Rust). */
export function tokenFor(signal) {
	if (!signal) return undefined;
	if (signal.__piToken !== undefined) return signal.__piToken;
	const id = call("token.new");
	try {
		Object.defineProperty(signal, "__piToken", { value: id, enumerable: false });
	} catch {}
	if (signal.aborted) call("token.cancel", { id });
	else signal.addEventListener?.("abort", () => call("token.cancel", { id }), { once: true });
	return id;
}

// ---------------------------------------------------------------------------
// Components
// ---------------------------------------------------------------------------

const rustBases = new Set();

/** Register a class whose instances are plain proxies of Rust components (not subclasses). */
export function markRustBase(cls) {
	rustBases.add(cls);
}

/** Describe a component for Rust: a Rust-backed component id or a handle to the JS object. */
export function componentRef(component) {
	if (component === null || component === undefined) return null;
	if (component.__rid !== undefined && rustBases.has(component.constructor)) return { $piComponent: component.__rid };
	return {
		$piJsComponent: ref(component),
		rid: component.__rid ?? null,
		handlesInput: typeof component.handleInput === "function",
		focusable: "focused" in component,
		wantsKeyRelease: !!component.wantsKeyRelease,
		hasInvalidate: typeof component.invalidate === "function",
		hasDispose: typeof component.dispose === "function",
		hasMouse: typeof component.handleMouse === "function",
	};
}

/** `component.handleMouse(event)` for Rust: flags plus the dispatch and focus targets as component refs. */
export function invokeHandleMouse(component, event) {
	const result = component.handleMouse?.(event);
	if (!result) return null;
	const out = {
		handled: !!result.handled,
		capture: !!result.capture,
		focus: !!result.focus,
		render: typeof result.render === "boolean" ? result.render : null,
	};
	if (result.target) {
		const { component: target, originX, originY, width, height } = result.target;
		out.target = { component: componentRef(target), originX, originY, width, height };
	}
	if (result.focusTarget) out.focusTarget = componentRef(result.focusTarget);
	return out;
}

/** A JS object rendering a Rust component registered under `id`. */
export class RustComponent {
	constructor(id) {
		Object.defineProperty(this, "__rid", { value: id, enumerable: false });
	}
	render(width) {
		return call("rc", { id: this.__rid, m: "render", a: [width] }) ?? [];
	}
	handleMouse(event) {
		return call("rc", { id: this.__rid, m: "handleMouse", a: [event] }) ?? undefined;
	}
	handleInput(data) {
		call("rc", { id: this.__rid, m: "handleInput", a: [data] });
	}
	invalidate() {
		call("rc", { id: this.__rid, m: "invalidate", a: [] });
	}
	get focused() {
		return !!call("rc", { id: this.__rid, m: "getFocused", a: [] });
	}
	set focused(value) {
		call("rc", { id: this.__rid, m: "setFocused", a: [!!value] });
	}
}

markRustBase(RustComponent);

export function rustComponent(id) {
	return id === null || id === undefined ? undefined : new RustComponent(id);
}

// ---------------------------------------------------------------------------
// Theme, TUI and keybinding proxies
// ---------------------------------------------------------------------------

const themes = new Map();

/** Theme proxy (TS `Theme`) backed by the Rust theme registered under `id`. */
export class ThemeProxy {
	constructor(id) {
		Object.defineProperty(this, "__themeId", { value: id, enumerable: false });
		Object.defineProperty(this, "__wrap", { value: new Map(), enumerable: false });
	}
	wrapper(method, arg) {
		const key = `${method}\u0000${arg ?? ""}`;
		let parts = this.__wrap.get(key);
		if (!parts) {
			const args = arg === undefined ? ["\u0001"] : [arg, "\u0001"];
			const sample = call("theme", { id: this.__themeId, m: method, a: args });
			const index = sample.indexOf("\u0001");
			parts = index === -1 ? null : [sample.slice(0, index), sample.slice(index + 1)];
			this.__wrap.set(key, parts);
		}
		return parts;
	}
	apply(method, arg, text) {
		const parts = this.wrapper(method, arg);
		if (parts) return parts[0] + text + parts[1];
		const args = arg === undefined ? [text] : [arg, text];
		return call("theme", { id: this.__themeId, m: method, a: args });
	}
	fg(color, text) {
		return this.apply("fg", color, String(text));
	}
	bg(color, text) {
		return this.apply("bg", color, String(text));
	}
	bold(text) {
		return this.apply("bold", undefined, String(text));
	}
	italic(text) {
		return this.apply("italic", undefined, String(text));
	}
	underline(text) {
		return this.apply("underline", undefined, String(text));
	}
	inverse(text) {
		return this.apply("inverse", undefined, String(text));
	}
	strikethrough(text) {
		return this.apply("strikethrough", undefined, String(text));
	}
	dim(text) {
		return `\x1b[2m${text}\x1b[22m`;
	}
	style(text, options) {
		return call("theme", { id: this.__themeId, m: "style", a: [text, options ?? {}] });
	}
	getFgAnsi(color) {
		return call("theme", { id: this.__themeId, m: "getFgAnsi", a: [color] });
	}
	getBgAnsi(color) {
		return call("theme", { id: this.__themeId, m: "getBgAnsi", a: [color] });
	}
	getColorMode() {
		return call("theme", { id: this.__themeId, m: "getColorMode", a: [] });
	}
	getThinkingBorderColor(level) {
		return (text) => this.apply("thinkingBorder", level, String(text));
	}
	getBashModeBorderColor() {
		return (text) => this.apply("bashBorder", undefined, String(text));
	}
	get name() {
		return u(call("theme", { id: this.__themeId, m: "name", a: [] }));
	}
	get sourcePath() {
		return u(call("theme", { id: this.__themeId, m: "sourcePath", a: [] }));
	}
	get appearance() {
		return call("theme", { id: this.__themeId, m: "appearance", a: [] });
	}
}

export function themeProxy(id) {
	if (id === null || id === undefined) return undefined;
	let proxy = themes.get(id);
	if (!proxy) {
		proxy = new ThemeProxy(id);
		themes.set(id, proxy);
	}
	return proxy;
}

/** The theme id of a theme proxy, or undefined for other values. */
export function themeId(theme) {
	return theme?.__themeId;
}

class OverlayHandleProxy {
	constructor(id) {
		this.__overlayId = id;
	}
	hide() {
		call("overlay", { id: this.__overlayId, m: "hide", a: [] });
	}
	setHidden(hidden) {
		call("overlay", { id: this.__overlayId, m: "setHidden", a: [!!hidden] });
	}
	isHidden() {
		return !!call("overlay", { id: this.__overlayId, m: "isHidden", a: [] });
	}
	focus() {
		call("overlay", { id: this.__overlayId, m: "focus", a: [] });
	}
	unfocus() {
		call("overlay", { id: this.__overlayId, m: "unfocus", a: [] });
	}
	isFocused() {
		return !!call("overlay", { id: this.__overlayId, m: "isFocused", a: [] });
	}
}

export function overlayProxy(id) {
	return id === null || id === undefined ? undefined : new OverlayHandleProxy(id);
}

/** TUI proxy (TS `TUI`) backed by the Rust TUI registered under `id`. */
export class TuiProxy {
	constructor(id) {
		Object.defineProperty(this, "__tuiId", { value: id, enumerable: false });
		const tuiId = id;
		this.terminal = {
			get columns() {
				return call("tui", { id: tuiId, m: "columns", a: [] });
			},
			get rows() {
				return call("tui", { id: tuiId, m: "rows", a: [] });
			},
			write(data) {
				call("tui", { id: tuiId, m: "write", a: [String(data)] });
			},
		};
	}
	/** TS `ViewportTUI[VIEWPORT_TUI]`: present (true) on fullscreen viewport TUIs. */
	get [Symbol.for("@earendil-works/pi-tui/viewport")]() {
		return call("tui", { id: this.__tuiId, m: "isViewport", a: [] }) === true ? true : undefined;
	}
	requestRender(force) {
		call("tui", { id: this.__tuiId, m: "requestRender", a: [!!force] });
	}
	invalidate() {
		call("tui", { id: this.__tuiId, m: "invalidate", a: [] });
	}
	setFocus(component) {
		call("tui", { id: this.__tuiId, m: "setFocus", a: [componentRef(component)] });
	}
	showOverlay(component, options) {
		return overlayProxy(call("tui", { id: this.__tuiId, m: "showOverlay", a: [componentRef(component), options ?? null] }));
	}
	hideOverlay() {
		call("tui", { id: this.__tuiId, m: "hideOverlay", a: [] });
	}
	hasOverlay() {
		return !!call("tui", { id: this.__tuiId, m: "hasOverlay", a: [] });
	}
	start() {
		call("tui", { id: this.__tuiId, m: "start", a: [] });
	}
	stop() {
		call("tui", { id: this.__tuiId, m: "stop", a: [] });
	}
	addChild() {}
	removeChild() {}
}

const tuis = new Map();

export function tuiProxy(id) {
	if (id === null || id === undefined) return undefined;
	let proxy = tuis.get(id);
	if (!proxy) {
		proxy = new TuiProxy(id);
		tuis.set(id, proxy);
	}
	return proxy;
}

/** Keybindings proxy (TS `KeybindingsManager`); `id` null is the global pi-tui manager. */
export class KeybindingsProxy {
	constructor(id) {
		Object.defineProperty(this, "__kbId", { value: id ?? null, enumerable: false });
	}
	matches(data, keybinding) {
		return !!call("kb", { id: this.__kbId, m: "matches", a: [data, keybinding] });
	}
	getKeys(keybinding) {
		return call("kb", { id: this.__kbId, m: "getKeys", a: [keybinding] }) ?? [];
	}
	getDefinition(keybinding) {
		return u(call("kb", { id: this.__kbId, m: "getDefinition", a: [keybinding] }));
	}
	getResolvedBindings() {
		return call("kb", { id: this.__kbId, m: "getResolvedBindings", a: [] }) ?? {};
	}
	getEffectiveConfig() {
		return call("kb", { id: this.__kbId, m: "getResolvedBindings", a: [] }) ?? {};
	}
	getConflicts() {
		return [];
	}
}

const keybindingProxies = new Map();

export function keybindingsProxy(id) {
	const key = id ?? null;
	let proxy = keybindingProxies.get(key);
	if (!proxy) {
		proxy = new KeybindingsProxy(key);
		keybindingProxies.set(key, proxy);
	}
	return proxy;
}

// ---------------------------------------------------------------------------
// Tool descriptions (pi.registerTool)
// ---------------------------------------------------------------------------

/** Replace functions implemented in Rust (built-in tools and renderers) by their markers. */
export function rustOr(fn) {
	if (typeof fn === "function" && fn.__piRust !== undefined) return { $piRust: fn.__piRust };
	return fn;
}

export function describeTool(tool) {
	if (!tool || typeof tool !== "object") throw new Error("registerTool() expects a tool definition object");
	return {
		name: tool.name,
		label: tool.label,
		description: tool.description,
		promptSnippet: tool.promptSnippet,
		promptGuidelines: tool.promptGuidelines,
		parameters: tool.parameters,
		constrainedSampling: tool.constrainedSampling,
		renderShell: tool.renderShell,
		prepareArguments: rustOr(tool.prepareArguments),
		outputSchema: tool.outputSchema,
		exposure: tool.exposure,
		namespace: tool.namespace,
		reachesTools: tool.reachesTools,
		annotations: tool.annotations,
		defaultActive: tool.defaultActive,
		prepareLoadout: tool.prepareLoadout,
		executionMode: tool.executionMode,
		execute: rustOr(tool.execute),
		renderCall: rustOr(tool.renderCall),
		renderResult: rustOr(tool.renderResult),
	};
}

// ---------------------------------------------------------------------------
// The `pi` object (ExtensionAPI)
// ---------------------------------------------------------------------------

function provideArgs(value) {
	return value === undefined ? null : value;
}

export function createPi(api) {
	const pi = {
		on(event, handler) {
			if (typeof handler !== "function") throw new Error(`pi.on("${event}") expects a handler function`);
			const id = call("pi.on", { api, event, handler });
			return () => call("unsub", { id });
		},
		registerTool(tool) {
			call("pi.registerTool", { api, tool: describeTool(tool) });
		},
		registerCommand(name, options) {
			if (typeof name !== "string" || name.length === 0) {
				call("pi.registerCommand", { api, name: "", options: {} });
			}
			if (typeof options?.handler !== "function") {
				throw new Error(
					`Command "/${name}" registered by extension "${call("pi.path", { api })}" must define handler().`,
				);
			}
			call("pi.registerCommand", {
				api,
				name,
				options: {
					description: options.description,
					handler: options.handler,
					getArgumentCompletions: options.getArgumentCompletions,
				},
			});
		},
		registerShortcut(shortcut, options) {
			call("pi.registerShortcut", {
				api,
				shortcut,
				options: { description: options?.description, handler: options?.handler },
			});
		},
		registerFlag(name, options) {
			call("pi.registerFlag", { api, name, options: options ?? {} });
		},
		getFlag(name) {
			return u(call("pi.getFlag", { api, name }));
		},
		registerMessageRenderer(customType, renderer) {
			call("pi.registerMessageRenderer", { api, customType, renderer });
		},
		registerMarkdownTransformer(transformer) {
			call("pi.registerMarkdownTransformer", { api, transformer });
		},
		registerEntryRenderer(customType, renderer) {
			call("pi.registerEntryRenderer", { api, customType, renderer });
		},
		registerToolRenderer(resolver) {
			call("pi.registerToolRenderer", { api, resolver });
		},
		sendMessage(message, options) {
			call("pi.sendMessage", { api, message, options: provideArgs(options) });
		},
		sendUserMessage(content, options) {
			call("pi.sendUserMessage", { api, content, options: provideArgs(options) });
		},
		appendEntry(customType, data) {
			call("pi.appendEntry", { api, customType, data: provideArgs(data) });
		},
		setSessionName(name) {
			call("pi.setSessionName", { api, name });
		},
		getSessionName() {
			return u(call("pi.getSessionName", { api }));
		},
		setLabel(entryId, label) {
			call("pi.setLabel", { api, entryId, label: provideArgs(label) });
		},
		exec(command, args, options) {
			return callAsync("pi.exec", {
				api,
				command,
				args: args ?? [],
				options: { cwd: options?.cwd, timeout: options?.timeout, signal: tokenFor(options?.signal) },
			});
		},
		getActiveTools() {
			return call("pi.getActiveTools", { api });
		},
		getAllTools() {
			return call("pi.getAllTools", { api });
		},
		getSettings() {
			return call("pi.getSettings", { api });
		},
		setActiveTools(toolNames) {
			call("pi.setActiveTools", { api, toolNames });
		},
		getCommands() {
			return call("pi.getCommands", { api });
		},
		setModel(model) {
			return callAsync("pi.setModel", { api, model });
		},
		getThinkingLevel() {
			return call("pi.getThinkingLevel", { api });
		},
		setThinkingLevel(level) {
			call("pi.setThinkingLevel", { api, level });
		},
		registerProvider(providerOrName, config) {
			if (typeof providerOrName === "string") {
				if (!config) throw new Error("Provider config is required when registering by name");
				call("pi.registerProvider", { api, name: providerOrName, config: describeProviderConfig(config) });
				return;
			}
			call("pi.registerNativeProvider", { api, provider: describeNativeProvider(providerOrName) });
		},
		unregisterProvider(name) {
			call("pi.unregisterProvider", { api, name });
		},
		registerMcpServer(name, config) {
			call("pi.registerMcpServer", { api, name, config });
		},
		unregisterMcpServer(name) {
			call("pi.unregisterMcpServer", { api, name });
		},
		getMcpServers() {
			return call("pi.getMcpServers", { api });
		},
		registerVirtualModel(model) {
			call("pi.registerVirtualModel", { api, model });
		},
		unregisterVirtualModel(provider, id) {
			call("pi.unregisterVirtualModel", { api, provider, id });
		},
		events: {
			emit(channel, data) {
				call("pi.events.emit", { api, channel, data: provideArgs(data) });
			},
			on(channel, handler) {
				const id = call("pi.events.on", { api, channel, handler });
				return () => call("unsub", { id });
			},
		},
	};
	return pi;
}

function refs(map) {
	if (!map || typeof map !== "object") return map;
	return Object.fromEntries(Object.entries(map).map(([key, value]) => [key, value ? ref(value) : value]));
}

function describeProviderConfig(config) {
	const out = { ...config };
	if (config.images) out.images = refs(config.images);
	if (config.classifiers) out.classifiers = refs(config.classifiers);
	if (config.oauth) {
		out.oauth = {
			name: config.oauth.name,
			isSubscription: config.oauth.isSubscription,
			usesCallbackServer: config.oauth.usesCallbackServer,
			login: config.oauth.login,
			refreshToken: config.oauth.refreshToken,
			getApiKey: config.oauth.getApiKey,
			modifyModels: config.oauth.modifyModels,
		};
	}
	return out;
}

/** Describe an auth object (`ProviderAuth`) for Rust: names, capabilities and object handles. */
export function describeAuth(auth) {
	const apiKey = auth?.apiKey;
	const oauth = auth?.oauth;
	return {
		apiKey: apiKey
			? {
					name: apiKey.name,
					login: typeof apiKey.login === "function",
					check: typeof apiKey.check === "function",
					object: ref(apiKey),
				}
			: null,
		oauth: oauth
			? {
					name: oauth.name,
					isSubscription: oauth.isSubscription,
					loginLabel: oauth.loginLabel,
					object: ref(oauth),
				}
			: null,
	};
}

/** Describe a pi-ai `Provider` object for `pi.registerProvider(provider)`. */
function describeNativeProvider(provider) {
	if (!provider || typeof provider !== "object") throw new Error("registerProvider() expects a provider object");
	if (provider.__piNative !== undefined) return { native: provider.__piNative };
	const has = (name) => typeof provider[name] === "function";
	return {
		id: provider.id,
		name: provider.name,
		baseUrl: provider.baseUrl,
		headers: provider.headers,
		auth: describeAuth(provider.auth),
		object: ref(provider),
		methods: {
			getAllModels: has("getAllModels"),
			refreshModels: has("refreshModels"),
			filterModels: has("filterModels"),
			filterAllModels: has("filterAllModels"),
			fetchDeferred: has("fetchDeferred"),
			cancelDeferred: has("cancelDeferred"),
			generateImages: has("generateImages"),
			classify: has("classify"),
		},
	};
}

// ---------------------------------------------------------------------------
// Contexts
// ---------------------------------------------------------------------------

function makeUi(ctx) {
	const ui = (m, ...a) => call("ui", { ctx, m, a });
	const uiAsync = (m, ...a) => callAsync("ui.async", { ctx, m, a });
	const dialogOptions = (opts) => (opts ? { timeout: opts.timeout, signal: tokenFor(opts.signal) } : null);
	let editorFactory;
	return {
		select: async (title, options, opts) => u(await uiAsync("select", title, options ?? [], dialogOptions(opts))),
		confirm: async (title, message, opts) => !!(await uiAsync("confirm", title, message ?? "", dialogOptions(opts))),
		input: async (title, placeholder, opts) =>
			u(await uiAsync("input", title, placeholder ?? null, dialogOptions(opts))),
		editor: async (title, prefill) => u(await uiAsync("editor", title, prefill ?? null)),
		notify: (message, type) => void ui("notify", String(message), type ?? "info"),
		onTerminalInput: (handler) => {
			const id = ui("onTerminalInput", handler);
			return () => call("unsub", { id });
		},
		setStatus: (key, text) => void ui("setStatus", key, text ?? null),
		setWorkingMessage: (message) => void ui("setWorkingMessage", message ?? null),
		setWorkingVisible: (visible) => void ui("setWorkingVisible", !!visible),
		setWorkingIndicator: (options) => void ui("setWorkingIndicator", options ?? null),
		setHiddenThinkingLabel: (label) => void ui("setHiddenThinkingLabel", label ?? null),
		setWidget: (key, content, options) => void ui("setWidget", key, content ?? null, options ?? {}),
		setFooter: (factory) => void ui("setFooter", factory ?? null),
		setHeader: (factory) => void ui("setHeader", factory ?? null),
		setTitle: (title) => void ui("setTitle", String(title)),
		custom: async (factory, options) => {
			const overlayOptions = options?.overlayOptions;
			return u(
				await uiAsync("custom", factory, {
					overlay: options?.overlay,
					overlayOptions: overlayOptions ?? null,
					onHandle: options?.onHandle ?? null,
				}),
			);
		},
		pasteToEditor: (text) => void ui("pasteToEditor", String(text)),
		setEditorText: (text) => void ui("setEditorText", String(text)),
		getEditorText: () => ui("getEditorText") ?? "",
		addAutocompleteProvider: (factory) => void ui("addAutocompleteProvider", factory),
		setEditorComponent: (factory) => {
			editorFactory = factory ?? undefined;
			ui("setEditorComponent", factory ?? null);
		},
		getEditorComponent: () => (ui("getEditorComponent") ? editorFactory : undefined),
		get theme() {
			return themeProxy(ui("theme"));
		},
		getAllThemes: () => ui("getAllThemes") ?? [],
		getTheme: (name) => themeProxy(ui("getTheme", name)),
		setTheme: (theme) => ui("setTheme", typeof theme === "string" ? theme : { $themeId: themeId(theme) }),
		getToolsExpanded: () => !!ui("getToolsExpanded"),
		setToolsExpanded: (expanded) => void ui("setToolsExpanded", !!expanded),
	};
}

function makeSessionManager(ctx) {
	const sm = (m, ...a) => u(call("sm", { ctx, m, a }));
	return new Proxy(
		{},
		{
			get(_target, prop) {
				if (typeof prop !== "string" || prop === "then") return undefined;
				return (...args) => sm(prop, ...args);
			},
		},
	);
}

function makeModelRegistry(ctx) {
	const mr = (m, ...a) => u(call("mr", { ctx, m, a }));
	const mrAsync = async (m, ...a) => u(await callAsync("mr.async", { ctx, m, a }));
	const asyncMethods = new Set([
		"refresh",
		"getApiKeyAndHeaders",
		"complete",
		"getProviderAuth",
		"getApiKeyForProvider",
		"getAvailableOfType",
		"classify",
		"generateImages",
	]);
	return new Proxy(
		{},
		{
			get(_target, prop) {
				if (typeof prop !== "string" || prop === "then") return undefined;
				if (prop === "stream" || prop === "streamSimple") {
					return (model, context, options) => streamFromHost("mr.stream", { ctx, m: prop, a: [model, context, options ?? null] });
				}
				if (asyncMethods.has(prop)) return (...args) => mrAsync(prop, ...stripSignals(args));
				return (...args) => mr(prop, ...args);
			},
		},
	);
}

function stripSignals(args) {
	return args.map((arg) =>
		arg && typeof arg === "object" && !Array.isArray(arg) && arg.signal
			? { ...arg, signal: tokenFor(arg.signal) }
			: arg,
	);
}

/** An AssistantMessageEventStream fed by a Rust stream (`op` returns a stream id). */
export function streamFromHost(op, payload) {
	const stream = new globalThis.__piAi.AssistantMessageEventStream();
	(async () => {
		try {
			const id = await callAsync(op, payload);
			for (;;) {
				const event = await callAsync("stream.next", { id });
				if (event === null) break;
				stream.push(event);
			}
			stream.end();
		} catch (error) {
			stream.push({ type: "error", reason: "error", error: { role: "assistant", content: [], stopReason: "error", errorMessage: String(error?.message ?? error) } });
			stream.end();
		}
	})();
	return stream;
}

const contexts = new Map();

/** The ctx object for the Rust context `id` (`kind`: base, tool, command, replaced, trust). */
export function makeContext(id, kind) {
	const key = `${kind}:${id}`;
	const cached = contexts.get(key);
	if (cached) return cached;
	const get = (prop) => u(call("ctx.get", { ctx: id, prop }));
	const invoke = (m, ...a) => u(call("ctx.call", { ctx: id, m, a }));
	const invokeAsync = async (m, ...a) => u(await callAsync("ctx.async", { ctx: id, m, a }));
	let ui;
	let sessionManager;
	let modelRegistry;
	const ctx = {
		get __ctxId() {
			return id;
		},
		get ui() {
			get("assertActive");
			ui ??= makeUi(id);
			return ui;
		},
		get mode() {
			return get("mode");
		},
		get hasUI() {
			return get("hasUI");
		},
		get cwd() {
			return get("cwd");
		},
	};
	if (kind !== "trust") {
		Object.defineProperties(ctx, {
			sessionManager: {
				get() {
					get("assertActive");
					sessionManager ??= makeSessionManager(id);
					return sessionManager;
				},
				enumerable: true,
			},
			modelRegistry: {
				get() {
					get("assertActive");
					modelRegistry ??= makeModelRegistry(id);
					return modelRegistry;
				},
				enumerable: true,
			},
			model: { get: () => get("model"), enumerable: true },
			scopedModels: { get: () => get("scopedModels") ?? [], enumerable: true },
			thinkingLevel: { get: () => get("thinkingLevel"), enumerable: true },
			signal: {
				get() {
					const token = get("signal");
					return token ? signalFor(token.id, token.aborted) : undefined;
				},
				enumerable: true,
			},
		});
		Object.assign(ctx, {
			isIdle: () => !!invoke("isIdle"),
			isProjectTrusted: () => !!invoke("isProjectTrusted"),
			abort: () => void invoke("abort"),
			hasPendingMessages: () => !!invoke("hasPendingMessages"),
			shutdown: () => void invoke("shutdown"),
			getContextUsage: () => invoke("getContextUsage"),
			compact: (options) =>
				void invoke("compact", {
					customInstructions: options?.customInstructions,
					onComplete: options?.onComplete,
					onError: options?.onError,
				}),
			getSystemPrompt: () => invoke("getSystemPrompt") ?? "",
		});
	}
	if (kind === "tool") {
		Object.defineProperty(ctx, "tools", { get: () => get("tools") ?? [], enumerable: true });
		ctx.executeTool = async (name, args, options) => {
			const updateId = options?.onUpdate ? options.onUpdate : null;
			return await invokeAsync("executeTool", name, args ?? {}, {
				signal: tokenFor(options?.signal),
				onUpdate: updateId,
			});
		};
	}
	if (kind === "command" || kind === "replaced") {
		const withSessionOption = (options) =>
			options
				? {
						...options,
						withSession: options.withSession ?? null,
						setup: options.setup ?? null,
					}
				: null;
		Object.assign(ctx, {
			getSystemPromptOptions: () => invoke("getSystemPromptOptions"),
			waitForIdle: () => invokeAsync("waitForIdle"),
			newSession: (options) => invokeAsync("newSession", withSessionOption(options)),
			fork: (entryId, options) => invokeAsync("fork", entryId, withSessionOption(options)),
			navigateTree: (targetId, options) => invokeAsync("navigateTree", targetId, options ?? null),
			switchSession: (sessionPath, options) => invokeAsync("switchSession", sessionPath, withSessionOption(options)),
			reload: () => invokeAsync("reload"),
		});
	}
	if (kind === "replaced") {
		Object.assign(ctx, {
			sendMessage: (message, options) => invokeAsync("sendMessage", message, options ?? null),
			sendUserMessage: (content, options) => invokeAsync("sendUserMessage", content, options ?? null),
		});
	}
	contexts.set(key, ctx);
	return ctx;
}

export function releaseContext(id) {
	for (const kind of ["base", "tool", "command", "replaced", "trust"]) contexts.delete(`${kind}:${id}`);
}

// ---------------------------------------------------------------------------
// Entry points used by Rust
// ---------------------------------------------------------------------------

/** Import an extension module and return its factory (jiti `{ default: true }` semantics). */
export function factoryFromModule(mod) {
	const factory = mod && typeof mod === "object" && "default" in mod ? mod.default : mod;
	return typeof factory === "function" ? ref(factory) : null;
}

export async function runFactory(factory, api) {
	await factory(createPi(api));
}

function isEqualJson(a, b) {
	try {
		return JSON.stringify(a) === JSON.stringify(b);
	} catch {
		return false;
	}
}

/** Invoke an event handler. In-place mutations of the event are returned as replacements. */
export async function invokeHandler(handler, event, ctxId, kind, signalId) {
	const ctx = makeContext(ctxId, kind);
	if (signalId !== null && signalId !== undefined) event.signal = signalFor(signalId.id, signalId.aborted);
	switch (event.type) {
		case "tool_call": {
			const before = JSON.stringify(event.input);
			const result = await handler(event, ctx);
			const changed = JSON.stringify(event.input) !== before;
			if (!changed) return result ?? null;
			return { ...(result ?? {}), input: event.input };
		}
		case "before_provider_headers": {
			await handler(event, ctx);
			return { headers: event.headers };
		}
		case "before_agent_start": {
			const before = JSON.stringify(event.systemPromptOptions);
			const result = await handler(event, ctx);
			if (isEqualJson(JSON.parse(before), event.systemPromptOptions)) return result ?? null;
			return { ...(result ?? {}), systemPromptOptions: event.systemPromptOptions };
		}
		case "context": {
			const snapshot = event.messages.slice();
			const result = await handler(event, ctx);
			if (result?.messages) return result;
			const same = snapshot.length === event.messages.length && snapshot.every((m, i) => m === event.messages[i]);
			return same ? (result ?? null) : { ...(result ?? {}), messages: event.messages };
		}
		case "turn_end":
		case "agent_before_settle": {
			const before = JSON.stringify(event.entries);
			let result;
			try {
				result = await handler(event, ctx);
			} catch (error) {
				if (JSON.stringify(event.entries) === before) throw error;
				// Shared mutations made before the throw are kept (TS mutates the shared array).
				return {
					entries: event.entries,
					$error: { message: String(error?.message ?? error), stack: error?.stack ?? null },
				};
			}
			if (result?.entries !== undefined || JSON.stringify(event.entries) === before) return result ?? null;
			return { ...(result ?? {}), entries: event.entries };
		}
		case "user_bash": {
			const result = await handler(event, ctx);
			if (result === undefined || result === null) return null;
			if (typeof result !== "object") return { $invalid: true };
			const hasOperations = result.operations !== undefined;
			const hasResult = result.result !== undefined;
			if (hasOperations === hasResult) return { $invalid: true };
			if (hasOperations) {
				if (typeof result.operations?.exec !== "function") return { $invalid: true };
				return { operations: { exec: result.operations.exec } };
			}
			return { result: result.result };
		}
		default:
			return (await handler(event, ctx)) ?? null;
	}
}

export async function invokeProjectTrust(handler, event, ctxId) {
	return (await handler(event, makeContext(ctxId, "trust"))) ?? null;
}

export async function invokeCommand(handler, args, ctxId, kind) {
	await handler(args, makeContext(ctxId, kind ?? "command"));
}

export async function invokeShortcut(handler, ctxId) {
	await handler(makeContext(ctxId, "base"));
}

export async function invokeWithSession(callback, ctxId) {
	await callback(makeContext(ctxId, "replaced"));
}

export async function invokeEventBus(handler, data) {
	await handler(data);
}

/** Execute a JS tool. `update` is a Rust callback id for `onUpdate`. */
export async function executeTool(execute, toolCallId, params, signal, update, ctxId) {
	const onUpdate = update === null || update === undefined ? undefined : rustCallback(update);
	const ctx = ctxId === null || ctxId === undefined ? undefined : makeContext(ctxId, "tool");
	const result = await execute(toolCallId, params, signalFor(signal?.id, signal?.aborted), onUpdate, ctx);
	return result ?? null;
}

function renderContext(context) {
	if (!context) return context;
	const { $rid, ...rest } = context;
	const out = { ...rest };
	out.invalidate = rustCallback(context.invalidate) ?? (() => {});
	out.lastComponent = context.lastComponent ?? undefined;
	out.state = context.state ?? {};
	if (out.durationMs === null) delete out.durationMs;
	Object.defineProperty(out, "__rustContext", { value: $rid, enumerable: false });
	return out;
}

export function renderCall(fn, args, theme, context) {
	return componentRef(fn(args, themeProxy(theme), renderContext(context)));
}

export function renderResult(fn, result, options, theme, context) {
	return componentRef(fn(result, options, themeProxy(theme), renderContext(context)));
}

export function renderMessage(fn, message, options, theme) {
	return componentRef(fn(message, options, themeProxy(theme)));
}

/** Call a component factory `(tui, theme, keybindings, done)` and describe the component. */
export async function makeComponent(factory, tui, theme, keybindings, done, extra) {
	const args = [tuiProxy(tui), themeProxy(theme)];
	if (extra === "footer") args.push(footerDataProxy(keybindings));
	else if (keybindings !== null && keybindings !== undefined) args.push(keybindingsProxy(keybindings));
	if (done !== null && done !== undefined) args.push(rustCallback(done));
	return componentRef(await factory(...args));
}

export function makeComponentSync(factory, tui, theme, keybindings, extra) {
	const args = [tuiProxy(tui), themeProxy(theme)];
	if (extra === "footer") args.push(footerDataProxy(keybindings));
	else if (keybindings !== null && keybindings !== undefined) args.push(keybindingsProxy(keybindings));
	const component = factory(...args);
	if (component && typeof component.then === "function") {
		throw new Error("component factories must return a component synchronously");
	}
	return componentRef(component);
}

function footerDataProxy(id) {
	const fd = (m, ...a) => u(call("footer", { id, m, a }));
	return {
		getGitBranch: () => fd("getGitBranch") ?? null,
		getExtensionStatuses: () => new Map(Object.entries(fd("getExtensionStatuses") ?? {})),
		getAvailableProviderCount: () => fd("getAvailableProviderCount") ?? 0,
		onBranchChange: (callback) => {
			const id2 = fd("onBranchChange", callback);
			return () => call("unsub", { id: id2 });
		},
	};
}

export function callMethod(target, method, args) {
	return target[method](...(args ?? []));
}

export async function callMethodAsync(target, method, args) {
	return await target[method](...(args ?? []));
}

export function callFunction(fn, args) {
	return fn(...(args ?? []));
}

export async function callFunctionAsync(fn, args) {
	return await fn(...(args ?? []));
}

/** Drain a JS AssistantMessageEventStream (from a custom `streamSimple`) into Rust. */
export async function pumpStream(stream, push) {
	const pushEvent = rustCallback(push);
	try {
		const iterable = await stream;
		for await (const event of iterable) pushEvent(event);
	} catch (error) {
		pushEvent({
			type: "error",
			reason: "error",
			error: {
				role: "assistant",
				content: [],
				api: "unknown",
				provider: "unknown",
				model: "unknown",
				usage: {
					input: 0,
					output: 0,
					cacheRead: 0,
					cacheWrite: 0,
					totalTokens: 0,
					cost: { input: 0, output: 0, cacheRead: 0, cacheWrite: 0, total: 0 },
				},
				stopReason: "error",
				errorMessage: String(error?.message ?? error),
				timestamp: Date.now(),
			},
		});
	}
	return null;
}

/** Request options from Rust (`native::options_to_js`): markers become a signal and callbacks. */
export function streamOptionsFromRust(options) {
	if (!options || typeof options !== "object") return options ?? undefined;
	const { $signal, $onPayload, $onResponse, $onProviderStreamEvent, ...rest } = options;
	const out = { ...rest };
	if ($signal) out.signal = signalFor($signal.id, $signal.aborted);
	if ($onPayload !== undefined && $onPayload !== null) out.onPayload = rustAsyncCallback($onPayload);
	if ($onResponse !== undefined && $onResponse !== null) out.onResponse = rustAsyncCallback($onResponse);
	if ($onProviderStreamEvent !== undefined && $onProviderStreamEvent !== null) {
		out.onProviderStreamEvent = rustAsyncCallback($onProviderStreamEvent);
	}
	return out;
}

/** Call a stream function (`target`, or `target[method]`) with Rust options; returns the stream. */
export function invokeStream(target, method, args, options) {
	const all = [...(args ?? []), streamOptionsFromRust(options)];
	return ref(method ? target[method](...all) : target(...all));
}

/** Call `target[method](...args, options)` where the last argument is Rust stream options. */
export async function invokeWithRustOptions(target, method, args) {
	const all = [...(args ?? [])];
	if (all.length > 0) all[all.length - 1] = streamOptionsFromRust(all[all.length - 1]);
	return (await target[method](...all)) ?? null;
}

/** A Rust `AuthContext` as a JavaScript auth context. */
function authContextProxy(id) {
	return {
		env: async (name) => u(await callAsync("aip.ctx", { id, m: "env", a: [name] })),
		fileExists: async (path) => !!(await callAsync("aip.ctx", { id, m: "fileExists", a: [path] })),
	};
}

/** A Rust login interaction as a JavaScript `ProviderAuthInteraction`. */
function interactionProxy(value) {
	const id = value.$interaction;
	return {
		signal: signalFor(value.$signal?.id, value.$signal?.aborted) ?? new AbortController().signal,
		prompt: async (prompt) => {
			const { signal, ...rest } = prompt ?? {};
			return await callAsync("aip.interaction.prompt", { id, prompt: { ...rest, signal: tokenFor(signal) ?? null } });
		},
		notify: (event) => void call("aip.interaction.notify", { id, event }),
	};
}

/** Call a method of a JavaScript `ApiKeyAuth` / `OAuthAuth` with Rust arguments. */
export async function invokeAuthMethod(auth, method, payload) {
	const signal = payload.$signal ? signalFor(payload.$signal.id, payload.$signal.aborted) : undefined;
	switch (method) {
		case "resolve":
		case "check":
			return (
				(await auth[method]({
					ctx: authContextProxy(payload.ctx.$authContext),
					credential: payload.credential ?? undefined,
					signal: signal ?? new AbortController().signal,
				})) ?? null
			);
		case "login": {
			const interaction = interactionProxy(payload.interaction);
			if (payload.options === null || payload.options === undefined) return (await auth.login(interaction)) ?? null;
			const { $getDeviceId, ...options } = payload.options;
			if ($getDeviceId !== undefined && $getDeviceId !== null) options.getDeviceId = rustCallback($getDeviceId);
			return (await auth.login(interaction, options)) ?? null;
		}
		case "refresh":
			return (await auth.refresh(payload.credential, signal ?? new AbortController().signal)) ?? null;
		case "toAuth":
			return (await auth.toAuth(payload.credential)) ?? null;
		default:
			throw new Error(`Unknown auth method ${method}`);
	}
}

/** Prompt through a JavaScript login interaction for a Rust login flow. */
export async function invokeInteractionPrompt(interaction, prompt) {
	const { $signal, ...rest } = prompt;
	if ($signal) rest.signal = signalFor($signal.id, $signal.aborted);
	return await interaction.prompt(rest);
}

/** `RefreshModelsContext` from Rust (`native::refresh_context_to_js`). */
function refreshContextFromRust(context) {
	const { $signal, $publish, ...rest } = context;
	return {
		...rest,
		credential: rest.credential ?? undefined,
		stored: rest.stored ?? undefined,
		signal: signalFor($signal.id, $signal.aborted),
		publish: async (publication) => !!(await callAsync("cb.async", { id: $publish, args: [publication] })),
	};
}

export async function invokeRefreshModels(provider, context) {
	await provider.refreshModels(refreshContextFromRust(context));
	return null;
}

export async function invokeRefreshModelsFn(refreshModels, context) {
	return (await refreshModels(refreshContextFromRust(context))) ?? [];
}

export function invokeFilter(provider, method, args) {
	const [models, credential] = args;
	return provider[method](models, credential ?? undefined) ?? null;
}

/** Call a JavaScript `context.publish` for a Rust provider's model refresh. */
export async function invokePublish(publish, value) {
	const publication = {};
	if ("persist" in value) publication.persist = value.persist;
	if (value.$update !== undefined && value.$update !== null) publication.update = rustCallback(value.$update);
	return !!(await publish(publication));
}

/** pi's session teardown runs JavaScript session-resource cleanups too. */
export function cleanupJsSessionResources(sessionId) {
	globalThis.__piSessionCleanup?.(sessionId ?? undefined);
	return null;
}

/** Resolve a tool renderer: `next` returns the precomputed renderers (Rust ids). */
export function resolveToolRenderers(resolver, toolName, next) {
	const nextRenderers = next ? describeRustRenderers(next) : undefined;
	const result = resolver(toolName, () => nextRenderers);
	if (result === undefined || result === null) return null;
	return {
		renderShell: result.renderShell,
		renderCall: rustOr(result.renderCall),
		renderResult: rustOr(result.renderResult),
	};
}

function describeRustRenderers(next) {
	const out = { renderShell: next.renderShell ?? undefined };
	if (next.renderCall !== null && next.renderCall !== undefined) {
		const id = next.renderCall;
		const fn = (args, theme, context) => rustComponent(call("renderer.call", { id, args, theme: themeId(theme), context: renderContextForRust(context) }));
		fn.__piRust = { renderer: id };
		out.renderCall = fn;
	}
	if (next.renderResult !== null && next.renderResult !== undefined) {
		const id = next.renderResult;
		const fn = (result, options, theme, context) =>
			rustComponent(call("renderer.result", { id, result, options, theme: themeId(theme), context: renderContextForRust(context) }));
		fn.__piRust = { renderer: id };
		out.renderResult = fn;
	}
	return out;
}

export function renderContextForRust(context) {
	return context?.__rustContext ?? null;
}

// ---------------------------------------------------------------------------
// Small helpers used by Rust
// ---------------------------------------------------------------------------

export function setProp(target, key, value) {
	target[key] = value;
	return null;
}

export function setPropCallback(target, key, id) {
	target[key] = id === null || id === undefined ? undefined : rustCallback(id);
	return null;
}

export function callMethodIfExists(target, method, args) {
	return typeof target?.[method] === "function" ? (target[method](...(args ?? [])) ?? null) : null;
}

/**
 * Copy the interactive mode's app-level handlers onto a JS `CustomEditor` (duck-typed on an
 * `actionHandlers` Map, like TS `setCustomEditorComponent`). Returns whether it is one.
 */
export function adoptAppHandlers(editor, handlers, actions) {
	if (!editor || !(editor.actionHandlers instanceof Map)) return false;
	for (const name of ["onEscape", "onCtrlD", "onPasteImage", "onExtensionShortcut"]) {
		if (!editor[name] && handlers[name] !== null && handlers[name] !== undefined) editor[name] = rustCallback(handlers[name]);
	}
	for (const [action, id] of actions) editor.actionHandlers.set(action, rustCallback(id));
	return true;
}

export function newState() {
	return ref({});
}

export function statIsDirectory(stat) {
	if (stat && typeof stat.isDirectory === "function") return !!stat.isDirectory();
	return !!stat?.isDirectory;
}

function styleFns(ids) {
	const out = {};
	for (const [key, id] of Object.entries(ids ?? {})) out[key] = rustCallback(id);
	return out;
}

export function makeEditorComponent(factory, tui, editorTheme, keybindings) {
	const theme = {
		borderColor: rustCallback(editorTheme.borderColor),
		selectList: styleFns(editorTheme.selectList),
	};
	return componentRef(factory(tuiProxy(tui), theme, keybindingsProxy(keybindings)));
}

/** A JS view of a Rust autocomplete provider registered under `id`. */
export function autocompleteProxy(id) {
	return {
		async getSuggestions(lines, cursorLine, cursorCol, options) {
			return u(await callAsync("ac.async", { id, m: "getSuggestions", a: [lines, cursorLine, cursorCol, { force: !!options?.force }] }));
		},
		applyCompletion(lines, cursorLine, cursorCol, item, prefix) {
			return call("ac", { id, m: "applyCompletion", a: [lines, cursorLine, cursorCol, item, prefix] });
		},
		shouldTriggerFileCompletion(lines, cursorLine, cursorCol) {
			return !!call("ac", { id, m: "shouldTriggerFileCompletion", a: [lines, cursorLine, cursorCol] });
		},
		get triggerCharacters() {
			return call("ac", { id, m: "triggerCharacters", a: [] }) ?? [];
		},
		$piAutocomplete: id,
	};
}

export function makeAutocompleteProvider(factory, currentId) {
	return ref(factory(autocompleteProxy(currentId)));
}

export async function invokeWithOverlay(callback, id) {
	await callback(overlayProxy(id));
}

export function invokePrepareLoadout(fn, snapshot) {
	const loadout = {
		declared: snapshot.declared,
		callable: snapshot.callable,
		registered: snapshot.registered,
		getExposure: (name) => snapshot.exposures[name] ?? "direct",
		getNamespace: (name) => snapshot.namespaces[name],
		getPromptGuidelines: (name) => snapshot.guidelines[name] ?? [],
	};
	return fn(loadout) ?? null;
}

export async function invokeBashExec(exec, command, cwd, onData, signal, timeout, env) {
	const dataCallback = rustCallback(onData);
	const result = await exec(command, cwd, {
		onData: (data) => dataCallback(typeof data === "string" ? data : Array.from(data ?? [])),
		signal: signalFor(signal?.id, signal?.aborted),
		timeout: timeout ?? undefined,
		env: env ?? undefined,
	});
	return result ?? { exitCode: null };
}

export async function invokeSetup(setup, id) {
	await setup(makeSessionManager(id));
}

export async function invokeOAuthLogin(login, callbacks) {
	return await login({
		onAuth: rustCallback(callbacks.onAuth),
		onDeviceCode: rustCallback(callbacks.onDeviceCode),
		onProgress: rustCallback(callbacks.onProgress),
		onPrompt: rustAsyncCallback(callbacks.onPrompt),
		onSelect: rustAsyncCallback(callbacks.onSelect),
		onManualCodeInput: rustAsyncCallback(callbacks.onManualCodeInput),
		signal: callbacks.signal ? signalFor(callbacks.signal.id, callbacks.signal.aborted) : undefined,
	});
}

export async function invokeWithSignal(fn, args, signal) {
	const all = [...(args ?? [])];
	if (signal) {
		const s = signalFor(signal.id, signal.aborted);
		if (all.length > 0 && all[all.length - 1] && typeof all[all.length - 1] === "object") all[all.length - 1].signal = s;
		else all.push(s);
	}
	return (await fn(...all)) ?? null;
}

export async function invokeRoute(route, request, ctxId) {
	const { $signal, ...rest } = request;
	if ($signal) rest.signal = signalFor($signal.id, $signal.aborted);
	return await route(rest, makeContext(ctxId, "base"));
}
