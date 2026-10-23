// SDK classes of `@earendil-works/pi-coding-agent` for extensions running in pi-jsrt (virtual
// module `pi-rs:sdk`), backed by pi's Rust implementations: SettingsManager.

import { call, callAsync, u } from "pi-rs:bridge";
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

/** TS `SettingsManager` over a Rust settings manager (created with the static factories). */
export class SettingsManager {
	#id;

	constructor(token) {
		if (!token || typeof token !== "object" || !(SDK_OBJECT in token)) {
			throw new TypeError("SettingsManager is created with SettingsManager.create(), inMemory() or fromStorage()");
		}
		this.#id = token[SDK_OBJECT];
		settingsFinalizer?.register(this, this.#id);
	}

	/** @internal The Rust settings manager id (SDK options). */
	get __piSettingsId() {
		return this.#id;
	}

	static #wrap(id) {
		return new SettingsManager({ [SDK_OBJECT]: id });
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
