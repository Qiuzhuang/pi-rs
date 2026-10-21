// `@earendil-works/pi-ai` (and `/compat`) for extensions running in pi-jsrt. Event streams,
// uuids and cost math are JavaScript; model catalog lookups and streaming call the Rust pi-ai
// crate through host functions.

import { call, callAsync, streamFromHost, tokenFor, u } from "pi-rs:bridge";
import { Type } from "typebox";

export { Type };

const ai = (fn, ...args) => u(call("ai", { fn, args }));

/** String enum schema (`{ type: "string", enum: [...] }`), as pi-ai's typebox helper. */
export function StringEnum(values, options = {}) {
	return Type.Unsafe({ type: "string", enum: [...values], ...options });
}

class FifoQueue {
	incoming = [];
	outgoing = [];
	get length() {
		return this.incoming.length + this.outgoing.length;
	}
	enqueue(value) {
		this.incoming.push(value);
	}
	dequeue() {
		if (this.outgoing.length === 0) {
			while (this.incoming.length > 0) this.outgoing.push(this.incoming.pop());
		}
		return this.outgoing.pop();
	}
}

export class EventStream {
	constructor(isComplete, extractResult) {
		this.queue = new FifoQueue();
		this.waiting = new FifoQueue();
		this.done = false;
		this.isComplete = isComplete;
		this.extractResult = extractResult;
		this.finalResultPromise = new Promise((resolve) => {
			this.resolveFinalResult = resolve;
		});
	}

	push(event) {
		if (this.done) return;
		if (this.isComplete(event)) {
			this.done = true;
			this.resolveFinalResult(this.extractResult(event));
		}
		const waiter = this.waiting.dequeue();
		if (waiter) waiter({ value: event, done: false });
		else this.queue.enqueue(event);
	}

	end(result) {
		this.done = true;
		if (result !== undefined) this.resolveFinalResult(result);
		while (this.waiting.length > 0) this.waiting.dequeue()({ value: undefined, done: true });
	}

	async *[Symbol.asyncIterator]() {
		while (true) {
			if (this.queue.length > 0) yield this.queue.dequeue();
			else if (this.done) return;
			else {
				const result = await new Promise((resolve) => this.waiting.enqueue(resolve));
				if (result.done) return;
				yield result.value;
			}
		}
	}

	result() {
		return this.finalResultPromise;
	}
}

export class AssistantMessageEventStream extends EventStream {
	constructor() {
		super(
			(event) => event.type === "done" || event.type === "error",
			(event) => {
				if (event.type === "done") return event.message;
				if (event.type === "error") return event.error;
				throw new Error("Unexpected event type for final result");
			},
		);
		this.startedAt = Date.now();
		this.startedAtMonotonic = performance.now();
	}

	push(event) {
		if (event.type === "done") this.time(event.message);
		else if (event.type === "error") this.time(event.error);
		super.push(event);
	}

	end(result) {
		if (result !== undefined) this.time(result);
		super.end(result);
	}

	time(message) {
		if (!message || this.done || message.durationMs !== undefined || message.timestamp < this.startedAt) return;
		message.durationMs = Math.max(0, Math.round(performance.now() - this.startedAtMonotonic));
	}
}

globalThis.__piAi = { AssistantMessageEventStream };

export function createAssistantMessageEventStream() {
	return new AssistantMessageEventStream();
}

const MAX_UUID_V7_TIMESTAMP = 0xffffffffffff;
let lastOrdinaryTimestamp = -1;
let sequence;

export function uuidv7(timestampMs) {
	const requestedTimestamp = timestampMs ?? Date.now();
	if (!Number.isInteger(requestedTimestamp) || requestedTimestamp < 0 || requestedTimestamp > MAX_UUID_V7_TIMESTAMP) {
		throw new RangeError(`UUIDv7 timestamp must be an integer between 0 and ${MAX_UUID_V7_TIMESTAMP}`);
	}
	const effectiveTimestamp = timestampMs === undefined ? Math.max(requestedTimestamp, lastOrdinaryTimestamp) : timestampMs;
	if (timestampMs === undefined) lastOrdinaryTimestamp = effectiveTimestamp;
	const bytes = new Uint8Array(16);
	globalThis.crypto.getRandomValues(bytes);
	if (sequence === undefined) {
		sequence = bytes[1] * 2 ** 32 + bytes[2] * 2 ** 24 + bytes[3] * 2 ** 16 + bytes[4] * 2 ** 8 + bytes[5];
	} else {
		sequence++;
	}
	let ts = effectiveTimestamp;
	for (let index = 5; index >= 0; index--) {
		bytes[index] = ts % 256;
		ts = Math.floor(ts / 256);
	}
	const seq = sequence;
	bytes[6] = 0x70 | (Math.floor(seq / 2 ** 37) & 0x0f);
	bytes[7] = Math.floor(seq / 2 ** 29) & 0xff;
	bytes[8] = 0x80 | (Math.floor(seq / 2 ** 23) & 0x3f);
	bytes[9] = Math.floor(seq / 2 ** 15) & 0xff;
	bytes[10] = Math.floor(seq / 2 ** 7) & 0xff;
	bytes[11] = ((seq % 128) << 1) | (bytes[11] & 0x01);
	const hex = Array.from(bytes, (byte) => byte.toString(16).padStart(2, "0"));
	return `${hex.slice(0, 4).join("")}-${hex.slice(4, 6).join("")}-${hex.slice(6, 8).join("")}-${hex.slice(8, 10).join("")}-${hex.slice(10).join("")}`;
}

export function calculateCost(model, usage) {
	const inputTokens = usage.input + usage.cacheRead + usage.cacheWrite;
	let rates = model.cost;
	let matchedThreshold = -1;
	for (const tier of model.cost.tiers ?? []) {
		if (inputTokens > tier.inputTokensAbove && tier.inputTokensAbove > matchedThreshold) {
			rates = tier;
			matchedThreshold = tier.inputTokensAbove;
		}
	}
	const longWrite = usage.cacheWrite1h ?? 0;
	const shortWrite = usage.cacheWrite - longWrite;
	usage.cost.input = (rates.input / 1000000) * usage.input;
	usage.cost.output = (rates.output / 1000000) * usage.output;
	usage.cost.cacheRead = (rates.cacheRead / 1000000) * usage.cacheRead;
	usage.cost.cacheWrite = (rates.cacheWrite * shortWrite + rates.input * 2 * longWrite) / 1000000;
	usage.cost.total = usage.cost.input + usage.cost.output + usage.cost.cacheRead + usage.cost.cacheWrite;
	return usage.cost;
}

const EXTENDED_THINKING_LEVELS = ["off", "minimal", "low", "medium", "high", "xhigh", "max"];

export function getSupportedThinkingLevels(model) {
	if (!model.reasoning) return ["off"];
	return EXTENDED_THINKING_LEVELS.filter((level) => {
		const mapped = model.thinkingLevelMap?.[level];
		if (mapped === null) return false;
		if (level === "xhigh" || level === "max") return mapped !== undefined;
		return true;
	});
}

export function supportsXhigh(model) {
	return getSupportedThinkingLevels(model).includes("xhigh");
}

export function modelsAreEqual(a, b) {
	return !!a && !!b && a.provider === b.provider && a.id === b.id;
}

export const getModel = (provider, modelId) => ai("getModel", provider, modelId);
export const getBuiltinModel = getModel;
export const getModels = (provider) => ai("getModels", provider) ?? [];
export const getBuiltinModels = getModels;
export const getProviders = () => ai("getProviders") ?? [];
export const getBuiltinProviders = getProviders;
export const getEnvApiKey = (provider) => ai("getEnvApiKey", provider);

function streamOptions(options) {
	if (!options) return null;
	const out = { ...options };
	if (options.signal) out.signal = tokenFor(options.signal);
	for (const [key, value] of Object.entries(out)) if (typeof value === "function") delete out[key];
	return out;
}

export function stream(model, context, options) {
	return streamFromHost("ai.stream", { simple: false, model, context, options: streamOptions(options) });
}

export function streamSimple(model, context, options) {
	return streamFromHost("ai.stream", { simple: true, model, context, options: streamOptions(options) });
}

export async function complete(model, context, options) {
	return await stream(model, context, options).result();
}

export async function completeSimple(model, context, options) {
	return await streamSimple(model, context, options).result();
}

// Transcript helpers (Rust implementations).
export const getCurrentSystemMessage = (messages) => ai("getCurrentSystemMessage", messages);
export const getCurrentSystemPrompt = (messages) => ai("getCurrentSystemPrompt", messages) ?? "";
export const getCurrentTools = (messages) => ai("getCurrentTools", messages) ?? [];
export const collapseSystemMessages = (messages) => ai("collapseSystemMessages", messages) ?? messages;
export const contentText = (content) => ai("contentText", content) ?? "";
export const getSystemMessageText = (message) => ai("getSystemMessageText", message) ?? "";
export const isContextOverflow = (message, contextWindow) => !!ai("isContextOverflow", message, contextWindow ?? null);
export const parseStreamingJson = (text) => ai("parseStreamingJson", text) ?? {};
export const validateToolCall = (tools, toolCall) => ai("validateToolCall", tools, toolCall);
export const validateToolArguments = (tool, toolCall) => ai("validateToolArguments", tool, toolCall);

export const PI_AI_VERSION = call("version");

function unavailable(name) {
	return (..._args) => {
		throw new Error(`${name} is not available to extensions in pi-rs`);
	};
}

// Compat API provider registry: custom APIs register through pi.registerProvider({ streamSimple }).
export const registerApiProvider = unavailable("registerApiProvider");
export const getApiProvider = () => undefined;
export const getApiProviders = () => [];
export const unregisterApiProviders = () => {};
export const resetApiProviders = () => {};
export const registerBuiltInApiProviders = () => {};
export const registerFauxProvider = unavailable("registerFauxProvider");

function apiShim(api) {
	return {
		api,
		stream: (model, context, options) => stream({ ...model, api }, context, options),
		streamSimple: (model, context, options) => streamSimple({ ...model, api }, context, options),
	};
}

export const anthropicMessagesApi = apiShim("anthropic-messages");
export const openAIResponsesApi = apiShim("openai-responses");
export const openAICompletionsApi = apiShim("openai-completions");
export const openAICodexResponsesApi = apiShim("openai-codex-responses");
export const azureOpenAIResponsesApi = apiShim("azure-openai-responses");
export const googleGenerativeAIApi = apiShim("google-generative-ai");
export const googleVertexApi = apiShim("google-vertex");
export const mistralConversationsApi = apiShim("mistral-conversations");
export const bedrockConverseStreamApi = apiShim("bedrock-converse-stream");

export default {
	Type,
	StringEnum,
	EventStream,
	AssistantMessageEventStream,
	createAssistantMessageEventStream,
	uuidv7,
	calculateCost,
	getModel,
	getModels,
	getProviders,
	stream,
	streamSimple,
	complete,
	completeSimple,
};

export { callAsync as __callAsync };
