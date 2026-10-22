// Rust-backed parts of `@earendil-works/pi-ai` for extensions running in pi-jsrt (virtual module
// `pi-rs:ai-rust`): the built-in API implementations and providers of the Rust pi-ai crate as
// JavaScript objects, the global API / image registries (shared with pi, which reads them in
// Rust), the generated model catalog, env API keys and the compat `stream()` / `complete()` API.

import { call, callAsync, streamFromHost, tokenFor, u } from "pi-rs:bridge";
import { ref } from "jsrt:host";
import { createFauxCore } from "pi-rs:ai-faux";
import { createModels, normalizeContext } from "pi-rs:ai-core";

const ai = (fn, ...args) => u(call("ai", { fn, args }));

const RUST_CALLBACK_OPTIONS = new Set(["onPayload", "onResponse", "onProviderStreamEvent"]);

/** Request options for Rust: the signal as a token; supported callbacks stay functions. */
export function optionsToRust(options) {
	if (!options) return null;
	const out = { ...options };
	if (options.signal) out.signal = tokenFor(options.signal);
	for (const [key, value] of Object.entries(out)) {
		if (typeof value === "function" && !RUST_CALLBACK_OPTIONS.has(key)) delete out[key];
		if (value === undefined) delete out[key];
	}
	return out;
}

// ---------------------------------------------------------------------------
// Rust API implementations and providers as JavaScript objects
// ---------------------------------------------------------------------------

/** `ProviderStreams` backed by a Rust implementation registered under `desc.native`. */
function nativeStreams(desc) {
	const id = desc.native;
	const api = {
		stream: (model, context, options) =>
			streamFromHost("aip.p.stream", { id, method: "stream", model, context, options: optionsToRust(options) }),
		streamSimple: (model, context, options) =>
			streamFromHost("aip.p.stream", { id, method: "streamSimple", model, context, options: optionsToRust(options) }),
	};
	if (desc.fetchDeferred) {
		api.fetchDeferred = (model, handle, options) =>
			streamFromHost("aip.p.stream", { id, method: "fetchDeferred", model, handle, options: optionsToRust(options) });
	}
	if (desc.cancelDeferred) {
		api.cancelDeferred = async (model, handle, options) => {
			await callAsync("aip.p.cancelDeferred", { id, model, handle, options: optionsToRust(options) });
		};
	}
	Object.defineProperty(api, "__piNative", { value: id, enumerable: false });
	return api;
}

const builtinApi = (api) => () => nativeStreams(call("aip.api", { api }));

export const anthropicMessagesApi = builtinApi("anthropic-messages");
export const azureOpenAIResponsesApi = builtinApi("azure-openai-responses");
export const bedrockConverseStreamApi = builtinApi("bedrock-converse-stream");
export const googleGenerativeAIApi = builtinApi("google-generative-ai");
export const googleVertexApi = builtinApi("google-vertex");
export const mistralConversationsApi = builtinApi("mistral-conversations");
export const openAICodexResponsesApi = builtinApi("openai-codex-responses");
export const openAICompletionsApi = builtinApi("openai-completions");
export const openAIResponsesApi = builtinApi("openai-responses");
export const piMessagesApi = builtinApi("pi-messages");

function authCall(id, m, payload) {
	return callAsync("aip.p.auth", { id, m, ...payload }).then(u);
}

/** A Rust `Provider` (built-in factories) as a JavaScript `Provider`. */
class RustProvider {
	constructor(desc) {
		const id = desc.native;
		Object.defineProperty(this, "__piNative", { value: id, enumerable: false });
		this.id = desc.id;
		this.name = desc.name;
		if (desc.baseUrl !== undefined && desc.baseUrl !== null) this.baseUrl = desc.baseUrl;
		if (desc.headers !== undefined && desc.headers !== null) this.headers = desc.headers;
		const auth = {};
		const apiKey = desc.auth?.apiKey;
		if (apiKey) {
			const method = {
				name: apiKey.name,
				resolve: ({ ctx, credential, signal }) =>
					authCall(id, "apiKey.resolve", { ctx: ref(ctx), credential: credential ?? null, signal: tokenFor(signal) }),
			};
			if (apiKey.check) {
				method.check = ({ ctx, credential, signal }) =>
					authCall(id, "apiKey.check", { ctx: ref(ctx), credential: credential ?? null, signal: tokenFor(signal) });
			}
			if (apiKey.login) {
				method.login = (interaction) =>
					authCall(id, "apiKey.login", { interaction: ref(interaction), signal: tokenFor(interaction.signal) });
			}
			auth.apiKey = method;
		}
		const oauth = desc.auth?.oauth;
		if (oauth) {
			auth.oauth = {
				name: oauth.name,
				isSubscription: oauth.isSubscription ?? undefined,
				loginLabel: oauth.loginLabel ?? undefined,
				login: (interaction, options) =>
					authCall(id, "oauth.login", {
						interaction: ref(interaction),
						signal: tokenFor(interaction.signal),
						options: options ? { getDeviceId: options.getDeviceId, agentName: options.agentName } : null,
					}),
				refresh: (credential, signal) => authCall(id, "oauth.refresh", { credential, signal: tokenFor(signal) }),
				toAuth: (credential) => authCall(id, "oauth.toAuth", { credential }),
			};
		}
		this.auth = auth;
		if (desc.refresh) {
			this.refreshModels = async (context) => {
				await callAsync("aip.p.refreshModels", {
					id,
					context: {
						credential: context.credential ?? null,
						stored: context.stored ?? null,
						publish: context.publish,
						allowNetwork: !!context.allowNetwork,
						force: context.force ?? null,
						signal: tokenFor(context.signal),
						baseUrl: context.baseUrl ?? null,
					},
				});
			};
		}
		if (desc.fetchDeferred) {
			this.fetchDeferred = (model, handle, options) =>
				streamFromHost("aip.p.stream", { id, method: "fetchDeferred", model, handle, options: optionsToRust(options) });
		}
		if (desc.cancelDeferred) {
			this.cancelDeferred = async (model, handle, options) => {
				await callAsync("aip.p.cancelDeferred", { id, model, handle, options: optionsToRust(options) });
			};
		}
		if (desc.images) {
			this.generateImages = async (model, context, options) =>
				await callAsync("aip.p.generateImages", { id, model, context, options: optionsToRust(options) });
		}
		if (desc.classify) {
			this.classify = async (model, context, options) =>
				await callAsync("aip.p.classify", { id, model, context, options: optionsToRust(options) });
		}
	}

	getModels() {
		return call("aip.p.getModels", { id: this.__piNative }) ?? [];
	}

	getAllModels() {
		return call("aip.p.getAllModels", { id: this.__piNative }) ?? [];
	}

	filterModels(models, credential) {
		return call("aip.p.filterModels", { id: this.__piNative, models, credential: credential ?? null });
	}

	filterAllModels(models, credential) {
		return call("aip.p.filterAllModels", { id: this.__piNative, models, credential: credential ?? null });
	}

	stream(model, context, options) {
		return streamFromHost("aip.p.stream", {
			id: this.__piNative,
			method: "stream",
			model,
			context,
			options: optionsToRust(options),
		});
	}

	streamSimple(model, context, options) {
		return streamFromHost("aip.p.stream", {
			id: this.__piNative,
			method: "streamSimple",
			model,
			context,
			options: optionsToRust(options),
		});
	}
}

// ---------------------------------------------------------------------------
// providers/all.ts
// ---------------------------------------------------------------------------

export const getBuiltinModel = (provider, modelId) => ai("getModel", provider, modelId);
export const getBuiltinImageModel = (provider, modelId) => ai("getBuiltinImageModel", provider, modelId);
export const getBuiltinClassifierModel = (provider, modelId) => ai("getBuiltinClassifierModel", provider, modelId);
export const getBuiltinProviders = () => ai("getProviders") ?? [];
export const getBuiltinModelDataGeneratedAt = () => ai("getBuiltinModelDataGeneratedAt");
export const getBuiltinModels = (provider) => ai("getModels", provider) ?? [];
export const getBuiltinImageModels = (provider) => ai("getBuiltinImageModels", provider) ?? [];
export const getBuiltinClassifierModels = (provider) => ai("getBuiltinClassifierModels", provider) ?? [];
export const getAllBuiltinModels = (provider) => ai("getAllBuiltinModels", provider) ?? [];

/** All built-in providers, freshly constructed. */
export function builtinProviders() {
	return (call("aip.builtinProviders") ?? []).map((desc) => new RustProvider(desc));
}

/** The Radius gateway provider. */
export function radiusProvider(options = {}) {
	return new RustProvider(call("aip.radiusProvider", { options }));
}

/** A `Models` collection with every built-in provider registered. */
export function builtinModels(options) {
	const models = createModels(options);
	for (const provider of builtinProviders()) models.setProvider(provider);
	return models;
}

// ---------------------------------------------------------------------------
// env-api-keys.ts, image-models.ts
// ---------------------------------------------------------------------------

export const ANTHROPIC_AUTH_TOKEN_ENV = "ANTHROPIC_AUTH_TOKEN";
export const ANTHROPIC_OAUTH_TOKEN_ENV = "ANTHROPIC_OAUTH_TOKEN";
export const ANTHROPIC_API_KEY_ENV = "ANTHROPIC_API_KEY";
export const ANTHROPIC_FEDERATION_RULE_ID_ENV = "ANTHROPIC_FEDERATION_RULE_ID";
export const ANTHROPIC_ORGANIZATION_ID_ENV = "ANTHROPIC_ORGANIZATION_ID";
export const ANTHROPIC_SERVICE_ACCOUNT_ID_ENV = "ANTHROPIC_SERVICE_ACCOUNT_ID";
export const ANTHROPIC_IDENTITY_TOKEN_FILE_ENV = "ANTHROPIC_IDENTITY_TOKEN_FILE";
export const ANTHROPIC_WORKSPACE_ID_ENV = "ANTHROPIC_WORKSPACE_ID";

export const findEnvKeys = (provider, env) => ai("findEnvKeys", provider, env ?? null);
export const getEnvApiKey = (provider, env) => ai("getEnvApiKey", provider, env ?? null);

/** @deprecated Static catalog read. */
export const getImageModel = (provider, modelId) => ai("getImageModel", provider, modelId);
/** @deprecated Static catalog read. */
export const getImageProviders = () => ai("getImageProviders") ?? [];
/** @deprecated Static catalog read. */
export const getImageModels = (provider) => ai("getImageModels", provider) ?? [];

// ---------------------------------------------------------------------------
// compat.ts: the global API registry (shared with pi) and stream()/complete()
// ---------------------------------------------------------------------------

/** @deprecated Static catalog read. */
export const getModel = getBuiltinModel;
/** @deprecated Static catalog read. */
export const getModels = getBuiltinModels;
/** @deprecated Static catalog read. */
export const getProviders = getBuiltinProviders;

/** API providers registered from this JavaScript runtime (also registered in pi's registry). */
const jsApiProviders = new Map();

function wrapStream(api, stream) {
	return (model, context, options) => {
		if (model.api !== api) throw new Error(`Mismatched api: ${model.api} expected ${api}`);
		return stream(model, context, options);
	};
}

function describeRegisteredStreams(provider) {
	const streams = { stream: provider.stream, streamSimple: provider.streamSimple };
	return { object: ref(streams), fetchDeferred: false, cancelDeferred: false };
}

export function registerApiProvider(provider, sourceId) {
	jsApiProviders.set(provider.api, {
		provider: {
			api: provider.api,
			stream: wrapStream(provider.api, provider.stream),
			streamSimple: wrapStream(provider.api, provider.streamSimple),
		},
		sourceId,
	});
	call("aip.compat.register", {
		api: provider.api,
		streams: describeRegisteredStreams(provider),
		sourceId: sourceId ?? null,
	});
}

function rustApiProvider(entry) {
	if (!entry) return undefined;
	const streams = nativeStreams(entry);
	return { api: entry.api, stream: streams.stream, streamSimple: streams.streamSimple };
}

export function getApiProvider(api) {
	return jsApiProviders.get(api)?.provider ?? rustApiProvider(call("aip.compat.get", { api }));
}

export function getApiProviders() {
	return (call("aip.compat.list") ?? []).map((entry) => jsApiProviders.get(entry.api)?.provider ?? rustApiProvider(entry));
}

export function unregisterApiProviders(sourceId) {
	for (const [api, entry] of jsApiProviders.entries()) {
		if (entry.sourceId === sourceId) jsApiProviders.delete(api);
	}
	call("aip.compat.unregister", { sourceId });
}

export function registerFauxProvider(options = {}) {
	const core = createFauxCore(options);
	const sourceId = `faux-provider-${Math.random().toString(36).slice(2, 10)}`;
	registerApiProvider({ api: core.api, stream: core.stream, streamSimple: core.streamSimple }, sourceId);
	return {
		api: core.api,
		models: core.models,
		getModel: core.getModel,
		state: core.state,
		setResponses: core.setResponses,
		appendResponses: core.appendResponses,
		getPendingResponseCount: core.getPendingResponseCount,
		unregister() {
			unregisterApiProviders(sourceId);
		},
	};
}

/** Registers the builtin API implementations without clobbering existing entries. */
export function registerBuiltInApiProviders() {
	call("aip.compat.registerBuiltins");
}

export function resetApiProviders() {
	jsApiProviders.clear();
	call("aip.compat.reset");
}

const AMBIENT_AUTH_MARKER = "<authenticated>";

function hasExplicitApiKey(apiKey) {
	return typeof apiKey === "string" && apiKey.trim().length > 0;
}

function withEnvApiKey(model, options) {
	if (hasExplicitApiKey(options?.apiKey)) return options;
	const apiKey = getEnvApiKey(model.provider, options?.env);
	if (!apiKey || apiKey === AMBIENT_AUTH_MARKER) return options;
	return { ...options, apiKey };
}

export function stream(model, context, options) {
	const registered = jsApiProviders.get(model.api);
	if (registered) return registered.provider.stream(model, normalizeContext(context), withEnvApiKey(model, options));
	return streamFromHost("aip.compat.stream", { simple: false, model, context, options: optionsToRust(options) });
}

export async function complete(model, context, options) {
	return stream(model, context, options).result();
}

export function streamSimple(model, context, options) {
	const registered = jsApiProviders.get(model.api);
	if (registered) {
		return registered.provider.streamSimple(model, normalizeContext(context), withEnvApiKey(model, options));
	}
	return streamFromHost("aip.compat.stream", { simple: true, model, context, options: optionsToRust(options) });
}

export async function completeSimple(model, context, options) {
	return streamSimple(model, context, options).result();
}

// ---------------------------------------------------------------------------
// legacy-api-aliases.ts
// ---------------------------------------------------------------------------

const anthropicMessagesStreams = anthropicMessagesApi();
const azureOpenAIResponsesStreams = azureOpenAIResponsesApi();
const googleGenerativeAIStreams = googleGenerativeAIApi();
const googleVertexStreams = googleVertexApi();
const mistralConversationsStreams = mistralConversationsApi();
const openAICodexResponsesStreams = openAICodexResponsesApi();
const openAICompletionsStreams = openAICompletionsApi();
const openAIResponsesStreams = openAIResponsesApi();

export const streamAnthropic = anthropicMessagesStreams.stream;
export const streamSimpleAnthropic = anthropicMessagesStreams.streamSimple;
export const streamAzureOpenAIResponses = azureOpenAIResponsesStreams.stream;
export const streamSimpleAzureOpenAIResponses = azureOpenAIResponsesStreams.streamSimple;
export const streamGoogle = googleGenerativeAIStreams.stream;
export const streamSimpleGoogle = googleGenerativeAIStreams.streamSimple;
export const streamGoogleVertex = googleVertexStreams.stream;
export const streamSimpleGoogleVertex = googleVertexStreams.streamSimple;
export const streamMistral = mistralConversationsStreams.stream;
export const streamSimpleMistral = mistralConversationsStreams.streamSimple;
export const streamOpenAICodexResponses = openAICodexResponsesStreams.stream;
export const streamSimpleOpenAICodexResponses = openAICodexResponsesStreams.streamSimple;
export const streamOpenAICompletions = openAICompletionsStreams.stream;
export const streamSimpleOpenAICompletions = openAICompletionsStreams.streamSimple;
export const streamOpenAIResponses = openAIResponsesStreams.stream;
export const streamSimpleOpenAIResponses = openAIResponsesStreams.streamSimple;

// ---------------------------------------------------------------------------
// images-api-registry.ts, images.ts, providers/images/register-builtins.ts
// ---------------------------------------------------------------------------

const jsImagesProviders = new Map();

function wrapGenerateImages(api, generateImages) {
	return (model, context, options) => {
		if (model.api !== api) throw new Error(`Mismatched api: ${model.api} expected ${api}`);
		return generateImages(model, context, options);
	};
}

export function registerImagesApiProvider(provider, sourceId) {
	jsImagesProviders.set(provider.api, {
		provider: { api: provider.api, generateImages: wrapGenerateImages(provider.api, provider.generateImages) },
		sourceId,
	});
	call("aip.images.register", {
		api: provider.api,
		object: ref({ generateImages: provider.generateImages }),
		sourceId: sourceId ?? null,
	});
}

export function getImagesApiProvider(api) {
	const registered = jsImagesProviders.get(api);
	if (registered) return registered.provider;
	if (!call("aip.images.has", { api })) return undefined;
	return {
		api,
		generateImages: async (model, context, options) =>
			await callAsync("aip.images.generate", { model, context, options: optionsToRust(options) }),
	};
}

/** Global image generation dispatched on `model.api` through the images api registry. */
export async function generateImages(model, context, options) {
	const provider = getImagesApiProvider(model.api);
	if (!provider) throw new Error(`No API provider registered for api: ${model.api}`);
	return provider.generateImages(model, context, options);
}

/** OpenRouter image generation (the built-in `openrouter-images` API). */
export const generateImagesOpenRouter = async (model, context, options) =>
	await callAsync("aip.images.openrouter", { model, context, options: optionsToRust(options) });

export function registerBuiltInImagesApiProviders() {
	call("aip.images.registerBuiltins");
}
