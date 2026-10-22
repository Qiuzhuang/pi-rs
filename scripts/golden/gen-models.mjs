// Golden renders of the model selector (/model), the scoped-models selector (/scoped-models) and the
// resource config selector (pi config).
import { fakeTui, Golden, REF, ref } from "./common.mjs";

const C = "coding-agent/src/modes/interactive/components";
const { createHarness } = await import(`${REF}/packages/coding-agent/test/suite/harness.ts`);
const { ModelSelectorComponent } = await ref(`${C}/model-selector.ts`);
const { ScopedModelsSelectorComponent } = await ref(`${C}/scoped-models-selector.ts`);
const { ConfigSelectorComponent } = await ref(`${C}/config-selector.ts`);
const { SettingsManager, InMemorySettingsStorage } = await ref("coding-agent/src/core/settings-manager.ts");
const { getModels } = await ref("ai/src/compat.ts");

const g = new Golden("models.json");
const noop = () => {};
const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));
const DOWN = "\x1b[B";

function feed(component, keys) {
	for (const key of keys ?? []) component.handleInput(key);
}

// --- model selector -----------------------------------------------------------------------------
const fauxModels = [
	{ id: "faux-large", name: "Faux Large", reasoning: true, contextWindow: 200000 },
	{ id: "faux-small", name: "Faux Small", reasoning: false, contextWindow: 32000 },
	{ id: "faux-vision", name: "Faux Vision with a long display name", reasoning: true, input: ["text", "image"] },
	{ id: "faux-coder", name: "Faux Coder", reasoning: true },
];
const buildModelSelector = async (input) => {
	const harness = await createHarness({ models: input.models });
	const current = input.current ? harness.getModel(input.current) : undefined;
	const scoped = (input.scoped ?? []).map((id) => ({ model: harness.getModel(id), thinkingLevel: input.scopedThinking }));
	const selector = new ModelSelectorComponent(
		fakeTui(),
		current,
		harness.session.modelRuntime,
		scoped,
		noop,
		noop,
		input.search,
		input.withDefault ? noop : undefined,
		input.defaultModel ? { provider: current?.provider ?? "faux", id: input.defaultModel } : undefined,
	);
	if (input.focused) selector.focused = true;
	await sleep(input.settle ?? 100);
	feed(selector, input.keys);
	return {
		render: (w) => selector.render(w),
		dispose: () => {
			selector.dispose();
			harness.cleanup();
		},
	};
};
await g.addAsync("model/basic", "model_selector", { models: fauxModels, current: "faux-small" }, buildModelSelector);
await g.addAsync("model/with-default", "model_selector", { models: fauxModels, current: "faux-large", withDefault: true, defaultModel: "faux-coder" }, buildModelSelector);
await g.addAsync("model/search", "model_selector", { models: fauxModels, current: "faux-large", search: "cod" }, buildModelSelector);
await g.addAsync("model/typed", "model_selector", { models: fauxModels, current: "faux-large", focused: true, keys: ["v", "i"] }, buildModelSelector);
await g.addAsync("model/no-match", "model_selector", { models: fauxModels, current: "faux-large", search: "zzzz" }, buildModelSelector);
await g.addAsync("model/navigated", "model_selector", { models: fauxModels, current: "faux-large", keys: [DOWN, DOWN] }, buildModelSelector);
await g.addAsync("model/scoped", "model_selector", { models: fauxModels, current: "faux-large", scoped: ["faux-large", "faux-coder"], scopedThinking: "high" }, buildModelSelector);
await g.addAsync("model/scoped-toggled", "model_selector", { models: fauxModels, current: "faux-large", scoped: ["faux-large", "faux-coder"], keys: ["\t"] }, buildModelSelector);

// --- scoped models ------------------------------------------------------------------------------
const catalog = [...getModels("anthropic").slice(0, 4), ...getModels("openai").slice(0, 3)];
const buildScoped = (input) => {
	const c = new ScopedModelsSelectorComponent(
		{ allModels: input.allModels, enabledModelIds: input.enabled, refreshStatus: input.refreshStatus },
		{ onChange: noop, onPersist: noop, onCancel: noop },
	);
	if (input.focused) c.focused = true;
	feed(c, input.keys);
	return c;
};
const ids = catalog.map((m) => `${m.provider}/${m.id}`);
g.add("scoped/all-enabled", "scoped_models_selector", { allModels: catalog, enabled: null }, buildScoped);
g.add("scoped/some-enabled", "scoped_models_selector", { allModels: catalog, enabled: [ids[2], ids[0], ids[5]] }, buildScoped);
g.add("scoped/none-enabled", "scoped_models_selector", { allModels: catalog, enabled: [] }, buildScoped);
g.add("scoped/toggled", "scoped_models_selector", { allModels: catalog, enabled: null, keys: [DOWN, " ", DOWN, DOWN, " "] }, buildScoped);
g.add("scoped/search", "scoped_models_selector", { allModels: catalog, enabled: [ids[1]], focused: true, keys: ["o", "p", "u", "s"] }, buildScoped);
g.add("scoped/refresh-status", "scoped_models_selector", { allModels: catalog, enabled: null, refreshStatus: "Refreshing model catalogs…" }, buildScoped);

// --- config selector ----------------------------------------------------------------------------
const AGENT = "/home/golden/.pi/agent";
const PROJECT = "/home/golden/project";
const meta = (source, scope, origin, extra = {}) => ({ source, scope, origin, ...extra });
const resolvedGlobal = {
	extensions: [
		{ path: "builtin:mcp", enabled: true, metadata: meta("builtin", "user", "top-level") },
		{ path: "builtin:llama.cpp", enabled: false, metadata: meta("builtin", "user", "top-level") },
		{ path: `${AGENT}/extensions/notify.ts`, enabled: true, metadata: meta("auto", "user", "top-level") },
		{ path: `${AGENT}/extensions/git-tools/index.ts`, enabled: true, metadata: meta("auto", "user", "top-level") },
		{ path: `${AGENT}/npm/node_modules/pi-tools/extensions/bar.ts`, enabled: true, metadata: meta("npm:pi-tools", "user", "package", { packageRoot: `${AGENT}/npm/node_modules/pi-tools` }) },
	],
	skills: [
		{ path: `${AGENT}/skills/pdf/SKILL.md`, enabled: true, metadata: meta("auto", "user", "top-level") },
		{ path: `${AGENT}/npm/node_modules/pi-tools/skills/review/SKILL.md`, enabled: false, metadata: meta("npm:pi-tools", "user", "package") },
	],
	prompts: [{ path: `${AGENT}/prompts/fix.md`, enabled: true, metadata: meta("auto", "user", "top-level") }],
	themes: [{ path: `${AGENT}/themes/nord.json`, enabled: true, metadata: meta("auto", "user", "top-level") }],
};
const resolvedProject = {
	extensions: [
		...resolvedGlobal.extensions,
		{ path: `${PROJECT}/.pi/extensions/local.ts`, enabled: true, metadata: meta("auto", "project", "top-level") },
	],
	skills: resolvedGlobal.skills,
	prompts: [],
	themes: [],
};
const buildConfig = (input) => {
	const storage = new InMemorySettingsStorage();
	storage.withLock("global", () => JSON.stringify(input.globalSettings ?? {}));
	const settingsManager = SettingsManager.fromStorage(storage, { projectTrusted: true });
	const c = new ConfigSelectorComponent(
		{ global: input.global, project: input.project },
		settingsManager,
		PROJECT,
		AGENT,
		noop,
		noop,
		noop,
		input.height ?? 24,
		input.scope,
		input.projectMode ?? true,
	);
	if (input.focused) c.focused = true;
	feed(c.getResourceList(), input.keys);
	return c;
};
g.add("config/global", "config_selector", { global: resolvedGlobal, project: resolvedProject, scope: "global" }, buildConfig);
g.add("config/project", "config_selector", { global: resolvedGlobal, project: resolvedProject, scope: "project" }, buildConfig);
g.add("config/no-project-mode", "config_selector", { global: resolvedGlobal, project: resolvedProject, scope: "global", projectMode: false }, buildConfig);
g.add("config/toggled", "config_selector", { global: resolvedGlobal, project: resolvedProject, scope: "global", keys: [DOWN, " ", DOWN, DOWN] }, buildConfig);
g.add("config/switch-mode", "config_selector", { global: resolvedGlobal, project: resolvedProject, scope: "global", keys: ["\t"] }, buildConfig);
g.add("config/search", "config_selector", { global: resolvedGlobal, project: resolvedProject, scope: "global", focused: true, keys: ["p", "d", "f"] }, buildConfig);
g.add("config/small-height", "config_selector", { global: resolvedGlobal, project: resolvedProject, scope: "global", height: 10, keys: Array(6).fill(DOWN) }, buildConfig);

g.write();
process.exit(0);
