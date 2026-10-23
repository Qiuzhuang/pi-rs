// SDK classes of `@earendil-works/pi-coding-agent` for extensions running in pi-jsrt (virtual
// module `pi-rs:sdk`), backed by pi's Rust implementations: SettingsManager.

import {
	call,
	callAsync,
	describeNativeProvider,
	describeProviderConfig,
	describeTool,
	ModelRegistry,
	rustComponent,
	SessionManager,
	streamFromHost,
	tokenFor,
	u,
} from "pi-rs:bridge";
import { ref } from "jsrt:host";

const SDK_OBJECT = Symbol("pi-rs.sdkObject");

function finalizer(op) {
	return typeof FinalizationRegistry === "function"
		? new FinalizationRegistry((id) => {
				try {
					call(op, { m: "release", id });
				} catch {}
			})
		: undefined;
}

// ---------------------------------------------------------------------------
// SettingsManager
// ---------------------------------------------------------------------------

const settingsFinalizer = finalizer("settings.static");
const hostSettings = new Map();

/** TS `SettingsManager` over a Rust settings manager (created with the static factories). */
export class SettingsManager {
	#id;

	constructor(token) {
		if (!token || typeof token !== "object" || !(SDK_OBJECT in token)) {
			throw new TypeError("SettingsManager is created with SettingsManager.create(), inMemory() or fromStorage()");
		}
		this.#id = token[SDK_OBJECT];
		if (!token.host) settingsFinalizer?.register(this, this.#id);
	}

	/** @internal The Rust settings manager id (SDK options). */
	get __piSettingsId() {
		return this.#id;
	}

	static #wrap(id) {
		return new SettingsManager({ [SDK_OBJECT]: id });
	}

	/** @internal A host settings manager (`session.settingsManager`), one object per manager. */
	static __host(id) {
		let manager = hostSettings.get(id);
		if (!manager) {
			manager = new SettingsManager({ [SDK_OBJECT]: id, host: true });
			hostSettings.set(id, manager);
		}
		return manager;
	}

	static create(cwd, agentDir, options) {
		return SettingsManager.#wrap(
			call("settings.static", { m: "create", cwd: cwd ?? process.cwd(), agentDir: agentDir ?? null, options: options ?? null }),
		);
	}

	static fromStorage(storage, options) {
		return SettingsManager.#wrap(call("settings.static", { m: "fromStorage", storage: ref(storage), options: options ?? null }));
	}

	static inMemory(settings, options) {
		return SettingsManager.#wrap(call("settings.static", { m: "inMemory", settings: settings ?? {}, options: options ?? null }));
	}

	#call(m, ...a) {
		return call("settings", { id: this.#id, m, a: a.map((v) => (v === undefined ? null : v)) });
	}

	async reload() {
		await callAsync("settings.async", { id: this.#id, m: "reload", a: [] });
	}

	async flush() {
		await callAsync("settings.async", { id: this.#id, m: "flush", a: [] });
	}

	drainErrors() {
		return (this.#call("drainErrors") ?? []).map((e) => ({ ...e, error: new Error(e.error?.message ?? "") }));
	}

	getSettings(...args) {
		return u(this.#call("getSettings", ...args));
	}
	getGlobalSettings(...args) {
		return u(this.#call("getGlobalSettings", ...args));
	}
	getProjectSettings(...args) {
		return u(this.#call("getProjectSettings", ...args));
	}
	isProjectTrusted(...args) {
		return u(this.#call("isProjectTrusted", ...args));
	}
	setProjectTrusted(...args) {
		this.#call("setProjectTrusted", ...args);
	}
	applyOverrides(...args) {
		this.#call("applyOverrides", ...args);
	}
	getLastChangelogVersion(...args) {
		return u(this.#call("getLastChangelogVersion", ...args));
	}
	setLastChangelogVersion(...args) {
		this.#call("setLastChangelogVersion", ...args);
	}
	getSessionDir(...args) {
		return u(this.#call("getSessionDir", ...args));
	}
	getDefaultProvider(...args) {
		return u(this.#call("getDefaultProvider", ...args));
	}
	getDefaultModel(...args) {
		return u(this.#call("getDefaultModel", ...args));
	}
	setDefaultProvider(...args) {
		this.#call("setDefaultProvider", ...args);
	}
	setDefaultModel(...args) {
		this.#call("setDefaultModel", ...args);
	}
	setDefaultModelAndProvider(...args) {
		this.#call("setDefaultModelAndProvider", ...args);
	}
	getSteeringMode(...args) {
		return u(this.#call("getSteeringMode", ...args));
	}
	setSteeringMode(...args) {
		this.#call("setSteeringMode", ...args);
	}
	getFollowUpMode(...args) {
		return u(this.#call("getFollowUpMode", ...args));
	}
	setFollowUpMode(...args) {
		this.#call("setFollowUpMode", ...args);
	}
	getThemeSetting(...args) {
		return u(this.#call("getThemeSetting", ...args));
	}
	getTheme(...args) {
		return u(this.#call("getTheme", ...args));
	}
	setTheme(...args) {
		this.#call("setTheme", ...args);
	}
	getDefaultThinkingLevel(...args) {
		return u(this.#call("getDefaultThinkingLevel", ...args));
	}
	setDefaultThinkingLevel(...args) {
		this.#call("setDefaultThinkingLevel", ...args);
	}
	getModelThinkingLevel(...args) {
		return u(this.#call("getModelThinkingLevel", ...args));
	}
	getAllModelThinkingLevels(...args) {
		return u(this.#call("getAllModelThinkingLevels", ...args));
	}
	setModelThinkingLevel(...args) {
		this.#call("setModelThinkingLevel", ...args);
	}
	removeModelThinkingLevel(...args) {
		this.#call("removeModelThinkingLevel", ...args);
	}
	getTransport(...args) {
		return u(this.#call("getTransport", ...args));
	}
	setTransport(...args) {
		this.#call("setTransport", ...args);
	}
	getSessionShareDestination(...args) {
		return u(this.#call("getSessionShareDestination", ...args));
	}
	setSessionShareDestination(...args) {
		this.#call("setSessionShareDestination", ...args);
	}
	getCompactionEnabled(...args) {
		return u(this.#call("getCompactionEnabled", ...args));
	}
	setCompactionEnabled(...args) {
		this.#call("setCompactionEnabled", ...args);
	}
	getCompactionReserveTokens(...args) {
		return u(this.#call("getCompactionReserveTokens", ...args));
	}
	getCompactionKeepRecentTokens(...args) {
		return u(this.#call("getCompactionKeepRecentTokens", ...args));
	}
	getCompactionSettings(...args) {
		return u(this.#call("getCompactionSettings", ...args));
	}
	getBranchSummarySettings(...args) {
		return u(this.#call("getBranchSummarySettings", ...args));
	}
	getBranchSummarySkipPrompt(...args) {
		return u(this.#call("getBranchSummarySkipPrompt", ...args));
	}
	getRetryEnabled(...args) {
		return u(this.#call("getRetryEnabled", ...args));
	}
	setRetryEnabled(...args) {
		this.#call("setRetryEnabled", ...args);
	}
	getRetrySettings(...args) {
		return u(this.#call("getRetrySettings", ...args));
	}
	getHttpIdleTimeoutMs(...args) {
		return u(this.#call("getHttpIdleTimeoutMs", ...args));
	}
	setHttpIdleTimeoutMs(...args) {
		this.#call("setHttpIdleTimeoutMs", ...args);
	}
	getCacheWarmingMode(...args) {
		return u(this.#call("getCacheWarmingMode", ...args));
	}
	setCacheWarmingMode(...args) {
		this.#call("setCacheWarmingMode", ...args);
	}
	getProviderRetrySettings(...args) {
		return u(this.#call("getProviderRetrySettings", ...args));
	}
	getWebSocketConnectTimeoutMs(...args) {
		return u(this.#call("getWebSocketConnectTimeoutMs", ...args));
	}
	getHideThinkingBlock(...args) {
		return u(this.#call("getHideThinkingBlock", ...args));
	}
	getShowCacheMissNotices(...args) {
		return u(this.#call("getShowCacheMissNotices", ...args));
	}
	getExternalEditorCommand(...args) {
		return u(this.#call("getExternalEditorCommand", ...args));
	}
	setHideThinkingBlock(...args) {
		this.#call("setHideThinkingBlock", ...args);
	}
	setShowCacheMissNotices(...args) {
		this.#call("setShowCacheMissNotices", ...args);
	}
	getShellPath(...args) {
		return u(this.#call("getShellPath", ...args));
	}
	setShellPath(...args) {
		this.#call("setShellPath", ...args);
	}
	getQuietStartup(...args) {
		return u(this.#call("getQuietStartup", ...args));
	}
	setQuietStartup(...args) {
		this.#call("setQuietStartup", ...args);
	}
	getDefaultProjectTrust(...args) {
		return u(this.#call("getDefaultProjectTrust", ...args));
	}
	setDefaultProjectTrust(...args) {
		this.#call("setDefaultProjectTrust", ...args);
	}
	getShellCommandPrefix(...args) {
		return u(this.#call("getShellCommandPrefix", ...args));
	}
	setShellCommandPrefix(...args) {
		this.#call("setShellCommandPrefix", ...args);
	}
	getNpmCommand(...args) {
		return u(this.#call("getNpmCommand", ...args));
	}
	setNpmCommand(...args) {
		this.#call("setNpmCommand", ...args);
	}
	getCollapseChangelog(...args) {
		return u(this.#call("getCollapseChangelog", ...args));
	}
	setCollapseChangelog(...args) {
		this.#call("setCollapseChangelog", ...args);
	}
	getEnableInstallTelemetry(...args) {
		return u(this.#call("getEnableInstallTelemetry", ...args));
	}
	setEnableInstallTelemetry(...args) {
		this.#call("setEnableInstallTelemetry", ...args);
	}
	getEnableAnalytics(...args) {
		return u(this.#call("getEnableAnalytics", ...args));
	}
	getTrackingId(...args) {
		return u(this.#call("getTrackingId", ...args));
	}
	setEnableAnalytics(...args) {
		this.#call("setEnableAnalytics", ...args);
	}
	getOrCreateDeviceId(...args) {
		return u(this.#call("getOrCreateDeviceId", ...args));
	}
	getPackages(...args) {
		return u(this.#call("getPackages", ...args));
	}
	setPackages(...args) {
		this.#call("setPackages", ...args);
	}
	setProjectPackages(...args) {
		this.#call("setProjectPackages", ...args);
	}
	getExtensionPaths(...args) {
		return u(this.#call("getExtensionPaths", ...args));
	}
	setExtensionPaths(...args) {
		this.#call("setExtensionPaths", ...args);
	}
	setProjectExtensionPaths(...args) {
		this.#call("setProjectExtensionPaths", ...args);
	}
	getSkillPaths(...args) {
		return u(this.#call("getSkillPaths", ...args));
	}
	setSkillPaths(...args) {
		this.#call("setSkillPaths", ...args);
	}
	setProjectSkillPaths(...args) {
		this.#call("setProjectSkillPaths", ...args);
	}
	getPromptTemplatePaths(...args) {
		return u(this.#call("getPromptTemplatePaths", ...args));
	}
	setPromptTemplatePaths(...args) {
		this.#call("setPromptTemplatePaths", ...args);
	}
	setProjectPromptTemplatePaths(...args) {
		this.#call("setProjectPromptTemplatePaths", ...args);
	}
	getThemePaths(...args) {
		return u(this.#call("getThemePaths", ...args));
	}
	setThemePaths(...args) {
		this.#call("setThemePaths", ...args);
	}
	setProjectThemePaths(...args) {
		this.#call("setProjectThemePaths", ...args);
	}
	getEnableSkillCommands(...args) {
		return u(this.#call("getEnableSkillCommands", ...args));
	}
	setEnableSkillCommands(...args) {
		this.#call("setEnableSkillCommands", ...args);
	}
	getThinkingBudgets(...args) {
		return u(this.#call("getThinkingBudgets", ...args));
	}
	getTerminalCapabilityOverrides(...args) {
		return u(this.#call("getTerminalCapabilityOverrides", ...args));
	}
	getShowImages(...args) {
		return u(this.#call("getShowImages", ...args));
	}
	setShowImages(...args) {
		this.#call("setShowImages", ...args);
	}
	getImageWidthCells(...args) {
		return u(this.#call("getImageWidthCells", ...args));
	}
	setImageWidthCells(...args) {
		this.#call("setImageWidthCells", ...args);
	}
	getClearOnShrink(...args) {
		return u(this.#call("getClearOnShrink", ...args));
	}
	setClearOnShrink(...args) {
		this.#call("setClearOnShrink", ...args);
	}
	getShowTerminalProgress(...args) {
		return u(this.#call("getShowTerminalProgress", ...args));
	}
	setShowTerminalProgress(...args) {
		this.#call("setShowTerminalProgress", ...args);
	}
	getTuiMode(...args) {
		return u(this.#call("getTuiMode", ...args));
	}
	setTuiMode(...args) {
		this.#call("setTuiMode", ...args);
	}
	getFullscreenExitOutput(...args) {
		return u(this.#call("getFullscreenExitOutput", ...args));
	}
	setFullscreenExitOutput(...args) {
		this.#call("setFullscreenExitOutput", ...args);
	}
	getFullscreenScrollbar(...args) {
		return u(this.#call("getFullscreenScrollbar", ...args));
	}
	setFullscreenScrollbar(...args) {
		this.#call("setFullscreenScrollbar", ...args);
	}
	getFullscreenCopyOnSelect(...args) {
		return u(this.#call("getFullscreenCopyOnSelect", ...args));
	}
	setFullscreenCopyOnSelect(...args) {
		this.#call("setFullscreenCopyOnSelect", ...args);
	}
	getFullscreenWheelScrollLines(...args) {
		return u(this.#call("getFullscreenWheelScrollLines", ...args));
	}
	setFullscreenWheelScrollLines(...args) {
		this.#call("setFullscreenWheelScrollLines", ...args);
	}
	getImageAutoResize(...args) {
		return u(this.#call("getImageAutoResize", ...args));
	}
	setImageAutoResize(...args) {
		this.#call("setImageAutoResize", ...args);
	}
	getBlockImages(...args) {
		return u(this.#call("getBlockImages", ...args));
	}
	setBlockImages(...args) {
		this.#call("setBlockImages", ...args);
	}
	getEnabledModels(...args) {
		return u(this.#call("getEnabledModels", ...args));
	}
	getDefaultTools(...args) {
		return u(this.#call("getDefaultTools", ...args));
	}
	setEnabledModels(...args) {
		this.#call("setEnabledModels", ...args);
	}
	getDoubleEscapeAction(...args) {
		return u(this.#call("getDoubleEscapeAction", ...args));
	}
	setDoubleEscapeAction(...args) {
		this.#call("setDoubleEscapeAction", ...args);
	}
	getTreeFilterMode(...args) {
		return u(this.#call("getTreeFilterMode", ...args));
	}
	setTreeFilterMode(...args) {
		this.#call("setTreeFilterMode", ...args);
	}
	getShowHardwareCursor(...args) {
		return u(this.#call("getShowHardwareCursor", ...args));
	}
	setShowHardwareCursor(...args) {
		this.#call("setShowHardwareCursor", ...args);
	}
	getEditorPaddingX(...args) {
		return u(this.#call("getEditorPaddingX", ...args));
	}
	setEditorPaddingX(...args) {
		this.#call("setEditorPaddingX", ...args);
	}
	getOutputPad(...args) {
		return u(this.#call("getOutputPad", ...args));
	}
	setOutputPad(...args) {
		this.#call("setOutputPad", ...args);
	}
	getAutocompleteMaxVisible(...args) {
		return u(this.#call("getAutocompleteMaxVisible", ...args));
	}
	setAutocompleteMaxVisible(...args) {
		this.#call("setAutocompleteMaxVisible", ...args);
	}
	getCodeBlockIndent(...args) {
		return u(this.#call("getCodeBlockIndent", ...args));
	}
	getMermaidRenderingMode(...args) {
		return u(this.#call("getMermaidRenderingMode", ...args));
	}
	setMermaidRenderingMode(...args) {
		this.#call("setMermaidRenderingMode", ...args);
	}
	getWarnings(...args) {
		return u(this.#call("getWarnings", ...args));
	}
	setWarnings(...args) {
		this.#call("setWarnings", ...args);
	}
}

// ---------------------------------------------------------------------------
// ModelRuntime
// ---------------------------------------------------------------------------

const runtimeFinalizer =
	typeof FinalizationRegistry === "function"
		? new FinalizationRegistry((id) => {
				try {
					call("rt", { runtime: id, m: "release", a: [] });
				} catch {}
			})
		: undefined;

const signalOption = (options) =>
	options && typeof options === "object" ? { ...options, signal: tokenFor(options.signal) } : (options ?? null);
const providerFromHost = (value) => globalThis.__piAiRust.providerFromHost(value);

/** TS `ModelRuntime` over a Rust model runtime (`ModelRuntime.create()`, `ctx.modelRegistry.runtime`). */
export class ModelRuntime {
	#id;

	constructor(token) {
		if (!token || typeof token !== "object" || !(SDK_OBJECT in token)) {
			throw new TypeError("ModelRuntime is created with ModelRuntime.create()");
		}
		this.#id = token[SDK_OBJECT];
		if (token.owned) runtimeFinalizer?.register(this, this.#id);
	}

	/** @internal The Rust runtime id (SDK options). */
	get __piRuntimeId() {
		return this.#id;
	}

	static async create(options = {}) {
		const id = await callAsync("rt.async", { m: "create", a: [{ ...options, signal: tokenFor(options.signal) }] });
		const runtime = new ModelRuntime({ [SDK_OBJECT]: id, owned: true });
		rememberRuntime(id, runtime);
		return runtime;
	}

	#call(m, ...a) {
		return u(call("rt", { runtime: this.#id, m, a: a.map((v) => (v === undefined ? null : v)) }));
	}

	async #async(m, ...a) {
		return u(await callAsync("rt.async", { runtime: this.#id, m, a: a.map((v) => (v === undefined ? null : v)) }));
	}

	#stream(m, model, second, options) {
		return streamFromHost("rt.stream", { runtime: this.#id, m, a: [model, second, signalOption(options)] });
	}

	getProviders() {
		return (this.#call("getProviders") ?? []).map(providerFromHost);
	}
	getProvider(providerId) {
		return providerFromHost(this.#call("getProvider", providerId));
	}
	getModels(providerId) {
		return this.#call("getModels", providerId) ?? [];
	}
	getModel(providerId, modelId) {
		return this.#call("getModel", providerId, modelId);
	}
	getModelsOfType(type, providerId) {
		return this.#call("getModelsOfType", type, providerId) ?? [];
	}
	getModelOfType(type, providerId, modelId) {
		return this.#call("getModelOfType", type, providerId, modelId);
	}
	getAllModels(providerId) {
		return this.#call("getAllModels", providerId) ?? [];
	}
	async getAvailableOfType(type, providerId, options) {
		return (await this.#async("getAvailableOfType", type, providerId, signalOption(options))) ?? [];
	}
	async getAllAvailable(providerId, options) {
		return (await this.#async("getAllAvailable", providerId, signalOption(options))) ?? [];
	}
	checkAuth(providerId, options) {
		return this.#async("checkAuth", providerId, signalOption(options));
	}
	async getAvailable(providerId, options) {
		return (await this.#async("getAvailable", providerId, signalOption(options))) ?? [];
	}
	getAvailableSnapshot() {
		return this.#call("getAvailableSnapshot") ?? [];
	}
	getError() {
		return this.#call("getError");
	}
	getRegisteredProviderConfig(providerId) {
		return globalThis.__piAiRust.providerConfigFromHost(providerId, this.#call("getRegisteredProviderConfig", providerId));
	}
	getRegisteredProviderIds() {
		return this.#call("getRegisteredProviderIds") ?? [];
	}
	getRegisteredNativeProvider(providerId) {
		return providerFromHost(this.#call("getRegisteredNativeProvider", providerId));
	}
	getCompatibilityRequestConfig(model) {
		return this.#call("getCompatibilityRequestConfig", model);
	}
	isUsingOAuth(providerId) {
		return !!this.#call("isUsingOAuth", providerId);
	}
	isUsingSubscription(providerId) {
		return !!this.#call("isUsingSubscription", providerId);
	}
	hasConfiguredAuth(providerId) {
		return !!this.#call("hasConfiguredAuth", providerId);
	}
	getAuth(providerOrModel, overrides) {
		return this.#async("getAuth", providerOrModel, signalOption(overrides));
	}
	async setRuntimeApiKey(providerId, apiKey, options) {
		await this.#async("setRuntimeApiKey", providerId, apiKey, signalOption(options));
	}
	async removeRuntimeApiKey(providerId, options) {
		await this.#async("removeRuntimeApiKey", providerId, signalOption(options));
	}
	async listCredentials(options) {
		return (await this.#async("listCredentials", signalOption(options))) ?? [];
	}
	getProviderAuthStatus(providerId) {
		return this.#call("getProviderAuthStatus", providerId);
	}
	stream(model, context, options) {
		return this.#stream("stream", model, context, options);
	}
	complete(model, context, options) {
		return this.#async("complete", model, context, signalOption(options));
	}
	streamSimple(model, context, options) {
		return this.#stream("streamSimple", model, context, options);
	}
	completeSimple(model, context, options) {
		return this.#async("completeSimple", model, context, signalOption(options));
	}
	streamDeferred(model, handle, options) {
		return this.#stream("streamDeferred", model, handle, options);
	}
	fetchDeferred(model, handle, options) {
		return this.#async("fetchDeferred", model, handle, signalOption(options));
	}
	async cancelDeferred(model, handle, options) {
		await this.#async("cancelDeferred", model, handle, signalOption(options));
	}
	generateImages(model, context, options) {
		return this.#async("generateImages", model, context, signalOption(options));
	}
	classify(model, context, options) {
		return this.#async("classify", model, context, signalOption(options));
	}
	async login(providerId, type, interaction, options) {
		return u(
			await callAsync("rt.async", {
				runtime: this.#id,
				m: "login",
				a: [providerId, type, ref(interaction), options ?? null],
				signal: tokenFor(interaction?.signal),
			}),
		);
	}
	async logout(providerId, options) {
		await this.#async("logout", providerId, signalOption(options));
	}
	async refresh(options) {
		const result = await this.#async("refresh", signalOption(options ?? {}));
		return {
			aborted: !!result?.aborted,
			errors: new Map(Object.entries(result?.errors ?? {}).map(([provider, message]) => [provider, new Error(message)])),
		};
	}
	registerNativeProvider(provider) {
		this.#call("registerNativeProvider", describeNativeProvider(provider));
	}
	registerProvider(providerId, config) {
		this.#call("registerProvider", providerId, describeProviderConfig(config));
	}
	unregisterProvider(providerId) {
		this.#call("unregisterProvider", providerId);
	}
	registerVirtualModel(definition) {
		this.#call("registerVirtualModel", definition);
	}
	unregisterVirtualModel(providerId, id) {
		this.#call("unregisterVirtualModel", providerId, id);
	}
	resolveModel(model, messages, options) {
		return this.#async("resolveModel", model, messages, signalOption(options));
	}
	getPhysicalModel(providerId, modelId) {
		return this.#call("getPhysicalModel", providerId, modelId);
	}
}

/** Runtimes by id, so `registry.runtime` is one object per runtime (created ones held weakly). */
const knownRuntimes = new Map();

function rememberRuntime(id, runtime) {
	knownRuntimes.set(id, typeof WeakRef === "function" ? new WeakRef(runtime) : runtime);
}

function runtimeFor(id) {
	const known = knownRuntimes.get(id);
	let runtime = known instanceof ModelRuntime ? known : known?.deref?.();
	if (!runtime) {
		// Host runtimes (`ctx.modelRegistry.runtime`) live as long as pi does: keep them strongly.
		runtime = new ModelRuntime({ [SDK_OBJECT]: id });
		knownRuntimes.set(id, runtime);
	}
	return runtime;
}

globalThis.__piSdk = { runtimeFor, extensionsResultFromHost };

export { ModelRegistry };

// ---------------------------------------------------------------------------
// Rust tools as JavaScript tool definitions
// ---------------------------------------------------------------------------

/**
 * A tool definition from Rust (`describe_definition`): the extension's own object, or a tool
 * object whose functions call the Rust tool. `withRenderers` adds prepareArguments/renderers.
 */
export function toolFromHost(d, withRenderers = true) {
	if (!d) return undefined;
	if (d.object !== undefined) return d.object;
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
	if (d.defaultActive !== undefined) tool.defaultActive = d.defaultActive;
	if (d.exposure !== undefined) tool.exposure = d.exposure;
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
	if (withRenderers) {
		if (d.hasPrepareArguments) {
			const prepareArguments = (args) => u(call("ca", { fn: "builtinPrepareArguments", args: [id, args] }));
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

// ---------------------------------------------------------------------------
// Built-in extension factories (Rust implementations)
// ---------------------------------------------------------------------------

function builtinExtension(name, options) {
	const factory = async (pi) => {
		if (!pi || typeof pi.__piApiId !== "number") {
			throw new Error(`The built-in ${name} extension factory expects the pi object of an extension`);
		}
		await callAsync("builtin.ext.run", { api: pi.__piApiId, name, options: options ?? null });
	};
	Object.defineProperty(factory, "__piBuiltinExtension", { value: { name, options: options ?? null } });
	return factory;
}

export const createCodemodeExtension = (options) => builtinExtension("codemode", options);
export const createToolSearchExtension = () => builtinExtension("tool-search");
export const createMcpExtension = (options) => {
	const data = {};
	if (options?.logPath !== undefined) data.logPath = options.logPath;
	return builtinExtension("mcp", data);
};

/** An extension factory for Rust: built-in markers stay markers; functions become handles. */
function describeFactory(factory) {
	const builtin = factory?.__piBuiltinExtension;
	if (builtin) return { $piBuiltinExtension: builtin.name, options: builtin.options };
	if (typeof factory !== "function") throw new TypeError("extension factory must be a function");
	return factory;
}

// ---------------------------------------------------------------------------
// DefaultResourceLoader
// ---------------------------------------------------------------------------

function mapOf(record, value = (v) => v) {
	return new Map(Object.entries(record ?? {}).map(([k, v]) => [k, value(v)]));
}

/** A `LoadExtensionsResult` from Rust (`extensions_result_to_js`). */
export function extensionsResultFromHost(result) {
	const extensions = (result?.extensions ?? []).map((e) => {
		const extension = {
			path: e.path,
			resolvedPath: e.resolvedPath,
			sourceInfo: e.sourceInfo,
			handlers: mapOf(e.handlers, (count) => Array.from({ length: count }, () => () => undefined)),
			tools: mapOf(e.tools),
			messageRenderers: new Map((e.messageRenderers ?? []).map((name) => [name, () => undefined])),
			commands: mapOf(e.commands),
			flags: mapOf(e.flags),
			shortcuts: mapOf(e.shortcuts),
		};
		if (e.hidden !== undefined) extension.hidden = e.hidden;
		if (e.replaceable !== undefined) extension.replaceable = e.replaceable;
		Object.defineProperty(extension, "__piIndex", { value: e.$index, enumerable: false });
		return extension;
	});
	const out = { extensions, errors: result?.errors ?? [], runtime: {} };
	if (result?.warnings) out.warnings = result.warnings;
	return out;
}

const loaderFinalizer = finalizer("rl");

/** TS `DefaultResourceLoader` over the Rust loader. */
export class DefaultResourceLoader {
	#id;

	constructor(options = {}) {
		const o = { ...options };
		if (o.settingsManager !== undefined) o.settingsManager = o.settingsManager?.__piSettingsId ?? null;
		if (o.eventBus !== undefined) delete o.eventBus;
		if (o.extensionFactories) {
			o.extensionFactories = o.extensionFactories.map((entry) =>
				typeof entry === "function"
					? describeFactory(entry)
					: { ...entry, factory: describeFactory(entry.factory) },
			);
		}
		for (const key of Object.keys(o)) if (o[key] === undefined) delete o[key];
		this.#id = call("rl", { m: "new", a: [o] });
		loaderFinalizer?.register(this, this.#id);
	}

	/** @internal The Rust loader id (SDK options). */
	get __piLoaderId() {
		return this.#id;
	}

	#call(m, ...a) {
		return u(call("rl", { id: this.#id, m, a }));
	}

	async reload() {
		await callAsync("rl.async", { id: this.#id });
	}
	getExtensions() {
		return extensionsResultFromHost(this.#call("getExtensions"));
	}
	getSkills() {
		return this.#call("getSkills");
	}
	getPrompts() {
		return this.#call("getPrompts");
	}
	getThemes() {
		return this.#call("getThemes");
	}
	getAgentsFiles() {
		return this.#call("getAgentsFiles");
	}
	getSystemPrompt() {
		return this.#call("getSystemPrompt");
	}
	getAppendSystemPrompt() {
		return this.#call("getAppendSystemPrompt") ?? [];
	}
	extendResources(paths) {
		this.#call("extendResources", paths ?? {});
	}
}

// ---------------------------------------------------------------------------
// createAgentSession and AgentSession
// ---------------------------------------------------------------------------

const sessionFinalizer = finalizer("as");

/** `session.agent`: the parts of pi-agent-core's Agent that extensions reach through a session. */
class SessionAgent {
	#session;
	constructor(session) {
		this.#session = session;
	}
	get state() {
		return this.#session.state;
	}
	get beforeToolCall() {
		return hookFromHost(this.#session.__piCall("agent.getBeforeToolCall"));
	}
	set beforeToolCall(fn) {
		this.#session.__piCall("agent.setBeforeToolCall", typeof fn === "function" ? fn : null);
	}
	get afterToolCall() {
		return hookFromHost(this.#session.__piCall("agent.getAfterToolCall"));
	}
	set afterToolCall(fn) {
		this.#session.__piCall("agent.setAfterToolCall", typeof fn === "function" ? fn : null);
	}
}

function hookFromHost(value) {
	if (!value) return undefined;
	if (value.object !== undefined) return value.object;
	const hook = value.native;
	return async (context, signal) => {
		// The Rust context of the hook call in progress travels as `$hookContext` (non-enumerable).
		const ctx = context && context.$hookContext !== undefined ? { ...context, $hookContext: context.$hookContext } : context;
		return u(await callAsync("as.async", { id: value.session, m: "agent.callHook", a: [hook, ctx], signal: tokenFor(signal) }));
	};
}

/** TS `AgentSession` over a Rust agent session (`createAgentSession()`). */
export class AgentSession {
	#id;
	#loader;
	#agent;

	constructor(token) {
		if (!token || typeof token !== "object" || !(SDK_OBJECT in token)) {
			throw new TypeError("AgentSession is created with createAgentSession()");
		}
		this.#id = token[SDK_OBJECT];
		this.#loader = token.loader;
		sessionFinalizer?.register(this, this.#id);
	}

	/** @internal */
	__piCall(m, ...a) {
		return u(call("as", { id: this.#id, m, a: a.map((v) => (v === undefined ? null : v)) }));
	}

	async #async(m, ...a) {
		return u(await callAsync("as.async", { id: this.#id, m, a: a.map((v) => (v === undefined ? null : v)) }));
	}

	get agent() {
		this.#agent ??= new SessionAgent(this);
		return this.#agent;
	}
	get sessionManager() {
		return globalThis.__piSessionManagerFor(this.__piCall("sessionManager"));
	}
	get settingsManager() {
		return SettingsManager.__host(this.__piCall("settingsManager"));
	}
	get modelRuntime() {
		return runtimeFor(this.__piCall("modelRuntime"));
	}
	get resourceLoader() {
		return this.#loader;
	}
	subscribe(listener) {
		const id = this.__piCall("subscribe", listener);
		return () => call("unsub", { id });
	}
	dispose() {
		this.__piCall("dispose");
	}
	refreshContext() {
		this.__piCall("refreshContext");
	}
	get state() {
		const state = this.__piCall("state");
		return state ? { ...state, pendingToolCalls: new Set(state.pendingToolCalls ?? []) } : state;
	}
	get cacheWarmingStatus() {
		return this.__piCall("cacheWarmingStatus");
	}
	setCacheWarmingMode(mode) {
		this.__piCall("setCacheWarmingMode", mode);
	}
	get model() {
		return this.__piCall("model");
	}
	get thinkingLevel() {
		return this.__piCall("thinkingLevel");
	}
	get routedModel() {
		return this.__piCall("routedModel");
	}
	get isStreaming() {
		return !!this.__piCall("isStreaming");
	}
	get isIdle() {
		return !!this.__piCall("isIdle");
	}
	get systemPrompt() {
		return this.__piCall("systemPrompt") ?? "";
	}
	get retryAttempt() {
		return this.__piCall("retryAttempt") ?? 0;
	}
	getActiveToolNames() {
		return this.__piCall("getActiveToolNames") ?? [];
	}
	getCallableToolNames() {
		return this.__piCall("getCallableToolNames") ?? [];
	}
	getAllTools() {
		return this.__piCall("getAllTools") ?? [];
	}
	getToolDefinition(name) {
		return toolFromHost(this.__piCall("getToolDefinition", name));
	}
	setActiveToolsByName(toolNames) {
		this.__piCall("setActiveToolsByName", toolNames ?? []);
	}
	get isCompacting() {
		return !!this.__piCall("isCompacting");
	}
	get messages() {
		return this.__piCall("messages") ?? [];
	}
	get steeringMode() {
		return this.__piCall("steeringMode");
	}
	get followUpMode() {
		return this.__piCall("followUpMode");
	}
	get sessionFile() {
		return this.__piCall("sessionFile");
	}
	get sessionId() {
		return this.__piCall("sessionId");
	}
	get sessionName() {
		return this.__piCall("sessionName");
	}
	get scopedModels() {
		return this.__piCall("scopedModels") ?? [];
	}
	setScopedModels(scopedModels) {
		this.__piCall("setScopedModels", scopedModels ?? []);
	}
	get promptTemplates() {
		return this.__piCall("promptTemplates") ?? [];
	}
	async prompt(text, options) {
		await this.#async("prompt", text, options ?? {});
	}
	steer(text, images, options) {
		return this.#async("steer", text, images, options);
	}
	followUp(text, images, options) {
		return this.#async("followUp", text, images, options);
	}
	async sendCustomMessage(message, options) {
		await this.#async("sendCustomMessage", message, options);
	}
	async sendUserMessage(content, options) {
		await this.#async("sendUserMessage", content, options);
	}
	clearQueue() {
		return this.__piCall("clearQueue");
	}
	get pendingMessageCount() {
		return this.__piCall("pendingMessageCount") ?? 0;
	}
	getSteeringMessages() {
		return this.__piCall("getSteeringMessages") ?? [];
	}
	getFollowUpMessages() {
		return this.__piCall("getFollowUpMessages") ?? [];
	}
	async abort() {
		await this.#async("abort");
	}
	async waitForIdle() {
		await this.#async("waitForIdle");
	}
	async setModel(model, options) {
		await this.#async("setModel", model, options);
	}
	cycleModel(direction, options) {
		return this.#async("cycleModel", direction, options);
	}
	setThinkingLevel(level, options) {
		this.__piCall("setThinkingLevel", level, options);
	}
	cycleThinkingLevel(options) {
		return this.__piCall("cycleThinkingLevel", options);
	}
	getAvailableThinkingLevels() {
		return this.__piCall("getAvailableThinkingLevels") ?? [];
	}
	supportsThinking() {
		return !!this.__piCall("supportsThinking");
	}
	setSteeringMode(mode) {
		this.__piCall("setSteeringMode", mode);
	}
	setFollowUpMode(mode) {
		this.__piCall("setFollowUpMode", mode);
	}
	compact(customInstructions) {
		return this.#async("compact", customInstructions);
	}
	abortCompaction() {
		this.__piCall("abortCompaction");
	}
	abortBranchSummary() {
		this.__piCall("abortBranchSummary");
	}
	setAutoCompactionEnabled(enabled) {
		this.__piCall("setAutoCompactionEnabled", !!enabled);
	}
	get autoCompactionEnabled() {
		return !!this.__piCall("autoCompactionEnabled");
	}
	async bindExtensions(bindings = {}) {
		await this.#async("bindExtensions", {
			mode: bindings.mode,
			onError: bindings.onError,
			shutdownHandler: bindings.shutdownHandler,
			abortHandler: bindings.abortHandler,
		});
	}
	async reload() {
		await this.#async("reload");
	}
	abortRetry() {
		this.__piCall("abortRetry");
	}
	get isRetrying() {
		return !!this.__piCall("isRetrying");
	}
	get autoRetryEnabled() {
		return !!this.__piCall("autoRetryEnabled");
	}
	setAutoRetryEnabled(enabled) {
		this.__piCall("setAutoRetryEnabled", !!enabled);
	}
	recordBashResult(command, result, options) {
		this.__piCall("recordBashResult", command, result, options);
	}
	abortBash() {
		this.__piCall("abortBash");
	}
	get isBashRunning() {
		return !!this.__piCall("isBashRunning");
	}
	get hasPendingBashMessages() {
		return !!this.__piCall("hasPendingBashMessages");
	}
	setSessionName(name) {
		this.__piCall("setSessionName", name);
	}
	navigateTree(targetId, options) {
		return this.#async("navigateTree", targetId, options);
	}
	getUserMessagesForForking() {
		return this.__piCall("getUserMessagesForForking") ?? [];
	}
	getSessionStats() {
		return this.__piCall("getSessionStats");
	}
	getContextUsage() {
		return this.__piCall("getContextUsage");
	}
	exportToHtml(outputPath, options) {
		return this.#async("exportToHtml", outputPath, options);
	}
	exportToJsonl(outputPath) {
		return this.__piCall("exportToJsonl", outputPath);
	}
	getLastAssistantText() {
		return this.__piCall("getLastAssistantText");
	}
	hasExtensionHandlers(eventType) {
		return !!this.__piCall("hasExtensionHandlers", eventType);
	}
}

/** TS `createAgentSession(options)`: a Rust AgentSession driven from JavaScript. */
export async function createAgentSession(options = {}) {
	const o = {};
	for (const key of ["cwd", "agentDir", "model", "thinkingLevel", "scopedModels", "noTools", "tools", "excludeTools", "sessionStartEvent"]) {
		if (options[key] !== undefined) o[key] = options[key];
	}
	if (options.modelRuntime !== undefined) o.modelRuntime = options.modelRuntime?.__piRuntimeId ?? null;
	if (options.modelRegistry !== undefined && o.modelRuntime === undefined) o.modelRegistry = options.modelRegistry?.__piRegistryTarget ?? null;
	if (options.customTools) o.customTools = options.customTools.map(describeTool);
	if (options.resourceLoader !== undefined) {
		const id = options.resourceLoader?.__piLoaderId;
		if (id === undefined) throw new Error("createAgentSession() in pi-rs takes a DefaultResourceLoader as resourceLoader");
		o.resourceLoader = id;
	}
	if (options.sessionManager !== undefined) o.sessionManager = options.sessionManager?.__piSessionManagerId ?? null;
	if (options.settingsManager !== undefined) o.settingsManager = options.settingsManager?.__piSettingsId ?? null;
	const result = await callAsync("as.async", { m: "create", a: [o] });
	const session = new AgentSession({ [SDK_OBJECT]: result.session, loader: options.resourceLoader });
	const out = { session, extensionsResult: extensionsResultFromHost(result.extensionsResult) };
	if (result.modelFallbackMessage !== undefined) out.modelFallbackMessage = result.modelFallbackMessage;
	return out;
}
