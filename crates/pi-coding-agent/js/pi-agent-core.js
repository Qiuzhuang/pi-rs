// `@earendil-works/pi-agent-core` for extensions running in pi-jsrt: the agent loop, the `Agent`
// class and the proxy stream, ported to JavaScript over the pi-ai glue (providers, streams and
// tool validation are backed by Rust). As in pi, whose SDK module installs pi-ai's `streamSimple`
// as the default stream function, the default here is the compat `streamSimple`.

import {
	EventStream,
	PI_ATTRIBUTION,
	createAssistantMessageEventStream,
	createInitialSystemMessage,
	getCurrentSystemMessage,
	getCurrentSystemPrompt,
	getCurrentTools,
	getToolStateChanges,
	mergeClientIdentity,
	normalizeContext,
	parseStreamingJson,
	toToolDeclaration,
	validateToolArguments,
	streamSimple,
} from "@earendil-works/pi-ai";

// ---------------------------------------------------------------------------
// stream-fn
// ---------------------------------------------------------------------------

let defaultStreamFn;
function setDefaultStreamFn(streamFn) {
  defaultStreamFn = streamFn;
}
function getDefaultStreamFn() {
  if (!defaultStreamFn) {
    throw new Error("No default stream function configured. Pass streamFn explicitly or call setDefaultStreamFn().");
  }
  return defaultStreamFn;
}

// ---------------------------------------------------------------------------
// agent-loop
// ---------------------------------------------------------------------------

const DEFAULT_AGENT_IDENTITY = { attribution: PI_ATTRIBUTION };
function agentLoop(prompts, context, config, signal, streamFn) {
  const stream = createAgentStream();
  void runAgentLoop(
    prompts,
    context,
    config,
    async (event) => {
      stream.push(event);
    },
    signal,
    streamFn
  ).then((messages) => {
    stream.end(messages);
  });
  return stream;
}
function agentLoopContinue(context, config, signal, streamFn) {
  if (context.messages.length === 0) {
    throw new Error("Cannot continue: no messages in context");
  }
  if (context.messages[context.messages.length - 1].role === "assistant") {
    throw new Error("Cannot continue from message role: assistant");
  }
  const stream = createAgentStream();
  void runAgentLoopContinue(
    context,
    config,
    async (event) => {
      stream.push(event);
    },
    signal,
    streamFn
  ).then((messages) => {
    stream.end(messages);
  });
  return stream;
}
async function runAgentLoop(prompts, context, config, emit, signal, streamFn) {
  const initialMessages = declareToolChanges(context, prompts);
  const newMessages = [...initialMessages];
  const currentContext = {
    ...context,
    messages: [...context.messages, ...initialMessages]
  };
  await emit({ type: "agent_start" });
  await emit({ type: "turn_start" });
  for (const message of initialMessages) {
    await emit({ type: "message_start", message });
    await emit({ type: "message_end", message });
  }
  await runLoop(currentContext, newMessages, config, signal, emit, streamFn ?? getDefaultStreamFn());
  return newMessages;
}
async function runAgentLoopContinue(context, config, emit, signal, streamFn) {
  if (context.messages.length === 0) {
    throw new Error("Cannot continue: no messages in context");
  }
  if (context.messages[context.messages.length - 1].role === "assistant") {
    throw new Error("Cannot continue from message role: assistant");
  }
  const newMessages = [];
  const currentContext = { ...context };
  await emit({ type: "agent_start" });
  await emit({ type: "turn_start" });
  await runLoop(currentContext, newMessages, config, signal, emit, streamFn ?? getDefaultStreamFn());
  return newMessages;
}
function createAgentStream() {
  return new EventStream(
    (event) => event.type === "agent_end",
    (event) => event.type === "agent_end" ? event.messages : []
  );
}
async function runLoop(initialContext, newMessages, initialConfig, signal, emit, streamFunction) {
  let currentContext = initialContext;
  let config = initialConfig;
  let lastCompletedTurn;
  let explicitContinuation = false;
  let pendingMessages = await config.getSteeringMessages?.() || [];
  while (true) {
    let hasMoreToolCalls = true;
    while (hasMoreToolCalls || pendingMessages.length > 0) {
      let preparedMessages = [];
      if (lastCompletedTurn) {
        const nextTurnSnapshot = await config.prepareNextTurn?.(lastCompletedTurn);
        if (nextTurnSnapshot) {
          currentContext = nextTurnSnapshot.context ?? currentContext;
          preparedMessages = nextTurnSnapshot.messages ?? [];
          config = {
            ...config,
            model: nextTurnSnapshot.model ?? config.model,
            reasoning: nextTurnSnapshot.thinkingLevel === void 0 ? config.reasoning : nextTurnSnapshot.thinkingLevel === "off" ? void 0 : nextTurnSnapshot.thinkingLevel
          };
        }
        if (pendingMessages.length === 0) {
          pendingMessages = await config.getSteeringMessages?.() || [];
        }
        await emit({ type: "turn_start" });
      }
      for (const message2 of declareToolChanges(currentContext, [...preparedMessages, ...pendingMessages])) {
        await emit({ type: "message_start", message: message2 });
        await emit({ type: "message_end", message: message2 });
        currentContext.messages.push(message2);
        newMessages.push(message2);
      }
      pendingMessages = [];
      const requestUpdate = await config.prepareRequest?.(
        {
          context: currentContext,
          model: config.model,
          thinkingLevel: config.reasoning ?? "off"
        },
        signal
      );
      if (requestUpdate) {
        currentContext = requestUpdate.context ?? currentContext;
        config = {
          ...config,
          model: requestUpdate.model ?? config.model,
          reasoning: requestUpdate.thinkingLevel === void 0 ? config.reasoning : requestUpdate.thinkingLevel === "off" ? void 0 : requestUpdate.thinkingLevel
        };
      }
      const message = await streamAssistantResponse(currentContext, config, signal, emit, streamFunction);
      newMessages.push(message);
      if (message.stopReason === "error" || message.stopReason === "aborted") {
        lastCompletedTurn = {
          message,
          toolResults: [],
          context: currentContext,
          newMessages
        };
        await config.finishTurn?.(lastCompletedTurn, signal);
        await emit({ type: "turn_end", message, toolResults: [] });
        await emit({ type: "agent_end", messages: newMessages });
        return;
      }
      const toolCalls = message.content.filter((c) => c.type === "toolCall");
      const toolResults = [];
      hasMoreToolCalls = false;
      if (toolCalls.length > 0) {
        const executedToolBatch = message.stopReason === "length" ? await failToolCallsFromTruncatedMessage(toolCalls, emit) : await executeToolCalls(currentContext, message, config, signal, emit);
        toolResults.push(...executedToolBatch.messages);
        hasMoreToolCalls = !executedToolBatch.terminate;
        for (const result of toolResults) {
          currentContext.messages.push(result);
          newMessages.push(result);
        }
      }
      lastCompletedTurn = {
        message,
        toolResults,
        context: currentContext,
        newMessages
      };
      const decision = await config.finishTurn?.(lastCompletedTurn, signal);
      await emit({ type: "turn_end", message, toolResults });
      if (decision?.action === "end") {
        await emit({ type: "agent_end", messages: newMessages });
        return;
      }
      explicitContinuation = decision?.action === "continue";
      pendingMessages = await config.getSteeringMessages?.() || [];
      if (hasMoreToolCalls || pendingMessages.length > 0) {
        explicitContinuation = false;
      }
    }
    const followUpMessages = await config.getFollowUpMessages?.() || [];
    if (followUpMessages.length > 0) {
      explicitContinuation = false;
      pendingMessages = followUpMessages;
      continue;
    }
    if (explicitContinuation) {
      explicitContinuation = false;
      continue;
    }
    break;
  }
  await emit({ type: "agent_end", messages: newMessages });
}
function declareToolChanges(context, pendingMessages) {
  let systemIndex = -1;
  for (let i = pendingMessages.length - 1; i >= 0; i--) {
    if (pendingMessages[i].role === "system") {
      systemIndex = i;
      break;
    }
  }
  const pending = pendingMessages[systemIndex];
  const baseline = pending ? pendingMessages.map(
    (message, index2) => index2 === systemIndex ? withToolChanges(pending, NO_CHANGES) : message
  ) : pendingMessages;
  const changes = getToolStateChanges(
    getCurrentTools([...context.messages, ...baseline]),
    (context.tools ?? []).map(toToolDeclaration)
  );
  const unchanged = changes.toolsAdded.length === 0 && changes.toolsRemoved.length === 0;
  if (pending) {
    if (unchanged && !pending.toolsAdded?.length && !pending.toolsRemoved?.length) return pendingMessages;
    return baseline.map((message, index2) => index2 === systemIndex ? withToolChanges(pending, changes) : message);
  }
  if (unchanged) return pendingMessages;
  const update = withToolChanges({ role: "system", content: "", timestamp: Date.now() }, changes);
  const insertIndex = pendingMessages.findIndex((message) => message.role !== "system");
  const index = insertIndex === -1 ? pendingMessages.length : insertIndex;
  return [...pendingMessages.slice(0, index), update, ...pendingMessages.slice(index)];
}
const NO_CHANGES = { toolsAdded: [], toolsRemoved: [] };
function withToolChanges(message, { toolsAdded, toolsRemoved }) {
  const { toolsAdded: _added, toolsRemoved: _removed, ...rest } = message;
  return {
    ...rest,
    ...toolsAdded.length > 0 ? { toolsAdded } : {},
    ...toolsRemoved.length > 0 ? { toolsRemoved } : {}
  };
}
async function streamAssistantResponse(context, config, signal, emit, streamFunction) {
  let messages = context.messages;
  if (config.transformContext) {
    messages = await config.transformContext(messages, signal);
  }
  const llmMessages = await config.convertToLlm(messages);
  const llmContext = normalizeContext({ messages: llmMessages });
  const resolvedApiKey = (config.getApiKey ? await config.getApiKey(config.model.provider) : void 0) || config.apiKey;
  const response = await streamFunction(config.model, llmContext, {
    ...config,
    apiKey: resolvedApiKey,
    identity: mergeClientIdentity(DEFAULT_AGENT_IDENTITY, config.identity),
    signal
  });
  const result = async () => Object.assign(await response.result(), { thinkingLevel: config.reasoning ?? "off" });
  let partialMessage = null;
  let addedPartial = false;
  for await (const event of response) {
    switch (event.type) {
      case "start":
        partialMessage = event.partial;
        context.messages.push(partialMessage);
        addedPartial = true;
        await emit({ type: "message_start", message: { ...partialMessage } });
        break;
      case "text_start":
      case "text_delta":
      case "text_end":
      case "thinking_start":
      case "thinking_delta":
      case "thinking_end":
      case "toolcall_start":
      case "toolcall_delta":
      case "toolcall_end":
        if (partialMessage) {
          partialMessage = event.partial;
          context.messages[context.messages.length - 1] = partialMessage;
          await emit({
            type: "message_update",
            assistantMessageEvent: event,
            message: { ...partialMessage }
          });
        }
        break;
      case "done":
      case "error": {
        const finalMessage2 = await result();
        if (addedPartial) {
          context.messages[context.messages.length - 1] = finalMessage2;
        } else {
          context.messages.push(finalMessage2);
        }
        if (!addedPartial) {
          await emit({ type: "message_start", message: { ...finalMessage2 } });
        }
        await emit({ type: "message_end", message: finalMessage2 });
        return finalMessage2;
      }
    }
  }
  const finalMessage = await result();
  if (addedPartial) {
    context.messages[context.messages.length - 1] = finalMessage;
  } else {
    context.messages.push(finalMessage);
    await emit({ type: "message_start", message: { ...finalMessage } });
  }
  await emit({ type: "message_end", message: finalMessage });
  return finalMessage;
}
async function failToolCallsFromTruncatedMessage(toolCalls, emit) {
  const messages = [];
  for (const toolCall of toolCalls) {
    await emit({
      type: "tool_execution_start",
      toolCallId: toolCall.id,
      toolName: toolCall.name,
      args: toolCall.arguments
    });
    const finalized = {
      toolCall,
      result: createErrorToolResult(
        `Tool call "${toolCall.name}" was not executed: the response hit the output token limit, so its arguments may be truncated. Re-issue the tool call with complete arguments.`
      ),
      isError: true
    };
    await emitToolExecutionEnd(finalized, emit);
    const toolResultMessage = createToolResultMessage(finalized);
    await emitToolResultMessage(toolResultMessage, emit);
    messages.push(toolResultMessage);
  }
  return { messages, terminate: false };
}
async function executeToolCalls(currentContext, assistantMessage, config, signal, emit) {
  const toolCalls = assistantMessage.content.filter((c) => c.type === "toolCall");
  const hasSequentialToolCall = toolCalls.some(
    (tc) => currentContext.tools?.find((t) => t.name === tc.name)?.executionMode === "sequential"
  );
  if (config.toolExecution === "sequential" || hasSequentialToolCall) {
    return executeToolCallsSequential(currentContext, assistantMessage, toolCalls, config, signal, emit);
  }
  return executeToolCallsParallel(currentContext, assistantMessage, toolCalls, config, signal, emit);
}
async function executeToolCallsSequential(currentContext, assistantMessage, toolCalls, config, signal, emit) {
  const finalizedCalls = [];
  const messages = [];
  for (const toolCall of toolCalls) {
    await emit({
      type: "tool_execution_start",
      toolCallId: toolCall.id,
      toolName: toolCall.name,
      args: toolCall.arguments
    });
    const preparation = await prepareToolCall(currentContext, assistantMessage, toolCall, config, signal);
    let finalized;
    if (preparation.kind === "immediate") {
      finalized = {
        toolCall,
        result: preparation.result,
        isError: preparation.isError
      };
    } else {
      const executed = await executePreparedToolCall(preparation, signal, emitToolExecutionUpdate(toolCall, emit));
      finalized = await finalizeExecutedToolCall(
        currentContext,
        assistantMessage,
        preparation,
        executed,
        config,
        signal
      );
    }
    await emitToolExecutionEnd(finalized, emit);
    const toolResultMessage = createToolResultMessage(finalized);
    await emitToolResultMessage(toolResultMessage, emit);
    finalizedCalls.push(finalized);
    messages.push(toolResultMessage);
    if (signal?.aborted) {
      break;
    }
  }
  return {
    messages,
    terminate: shouldTerminateToolBatch(finalizedCalls)
  };
}
async function executeToolCallsParallel(currentContext, assistantMessage, toolCalls, config, signal, emit) {
  const finalizedCalls = [];
  for (const toolCall of toolCalls) {
    await emit({
      type: "tool_execution_start",
      toolCallId: toolCall.id,
      toolName: toolCall.name,
      args: toolCall.arguments
    });
    const preparation = await prepareToolCall(currentContext, assistantMessage, toolCall, config, signal);
    if (preparation.kind === "immediate") {
      const finalized = {
        toolCall,
        result: preparation.result,
        isError: preparation.isError
      };
      await emitToolExecutionEnd(finalized, emit);
      finalizedCalls.push(finalized);
      if (signal?.aborted) {
        break;
      }
      continue;
    }
    finalizedCalls.push(async () => {
      if (signal?.aborted) {
        const finalized2 = {
          toolCall,
          result: createErrorToolResult("Operation aborted"),
          isError: true
        };
        await emitToolExecutionEnd(finalized2, emit);
        return finalized2;
      }
      const executed = await executePreparedToolCall(preparation, signal, emitToolExecutionUpdate(toolCall, emit));
      const finalized = await finalizeExecutedToolCall(
        currentContext,
        assistantMessage,
        preparation,
        executed,
        config,
        signal
      );
      await emitToolExecutionEnd(finalized, emit);
      return finalized;
    });
    if (signal?.aborted) {
      break;
    }
  }
  const orderedFinalizedCalls = await Promise.all(
    finalizedCalls.map((entry) => typeof entry === "function" ? entry() : Promise.resolve(entry))
  );
  const messages = [];
  for (const finalized of orderedFinalizedCalls) {
    const toolResultMessage = createToolResultMessage(finalized);
    await emitToolResultMessage(toolResultMessage, emit);
    messages.push(toolResultMessage);
  }
  return {
    messages,
    terminate: shouldTerminateToolBatch(orderedFinalizedCalls)
  };
}
function shouldTerminateToolBatch(finalizedCalls) {
  return finalizedCalls.length > 0 && finalizedCalls.every((finalized) => finalized.result.terminate === true);
}
function prepareToolCallArguments(tool, toolCall) {
  if (!tool.prepareArguments) {
    return toolCall;
  }
  const preparedArguments = tool.prepareArguments(toolCall.arguments);
  if (preparedArguments === toolCall.arguments) {
    return toolCall;
  }
  return {
    ...toolCall,
    arguments: preparedArguments
  };
}
async function prepareToolCall(currentContext, assistantMessage, toolCall, config, signal, tools = currentContext.tools ?? []) {
  const tool = tools.find((t) => t.name === toolCall.name);
  if (!tool) {
    return {
      kind: "immediate",
      result: createErrorToolResult(`Tool ${toolCall.name} not found`),
      isError: true
    };
  }
  try {
    const preparedToolCall = prepareToolCallArguments(tool, toolCall);
    const validatedArgs = validateToolArguments(tool, preparedToolCall);
    if (config.beforeToolCall) {
      const beforeResult = await config.beforeToolCall(
        {
          assistantMessage,
          toolCall,
          args: validatedArgs,
          context: currentContext
        },
        signal
      );
      if (signal?.aborted) {
        return {
          kind: "immediate",
          result: createErrorToolResult("Operation aborted"),
          isError: true
        };
      }
      if (beforeResult?.block) {
        const result = createErrorToolResult(beforeResult.reason || "Tool execution was blocked");
        if (beforeResult.terminate === true) {
          result.terminate = true;
        }
        return {
          kind: "immediate",
          result,
          isError: true
        };
      }
    }
    if (signal?.aborted) {
      return {
        kind: "immediate",
        result: createErrorToolResult("Operation aborted"),
        isError: true
      };
    }
    return {
      kind: "prepared",
      toolCall,
      tool,
      args: validatedArgs
    };
  } catch (error) {
    return {
      kind: "immediate",
      result: createErrorToolResult(error instanceof Error ? error.message : String(error)),
      isError: true
    };
  }
}
function emitToolExecutionUpdate(toolCall, emit) {
  return (partialResult) => emit({
    type: "tool_execution_update",
    toolCallId: toolCall.id,
    toolName: toolCall.name,
    args: toolCall.arguments,
    partialResult
  });
}
async function runToolCall(toolCall, options) {
  const { assistantMessage, context, signal } = options;
  const preparation = await prepareToolCall(context, assistantMessage, toolCall, options, signal, options.tools);
  if (preparation.kind === "immediate") {
    return { toolCall, result: preparation.result, isError: preparation.isError };
  }
  const executed = await executePreparedToolCall(preparation, signal, options.onUpdate ?? (() => {
  }));
  return finalizeExecutedToolCall(context, assistantMessage, preparation, executed, options, signal);
}
async function executePreparedToolCall(prepared, signal, onUpdate) {
  const updateEvents = [];
  let acceptingUpdates = true;
  const startedAt = performance.now();
  const elapsed = () => Math.round(performance.now() - startedAt);
  try {
    const result = await prepared.tool.execute(
      prepared.toolCall.id,
      prepared.args,
      signal,
      (partialResult) => {
        if (!acceptingUpdates) return;
        updateEvents.push(Promise.resolve(onUpdate(partialResult)));
      }
    );
    const durationMs = elapsed();
    acceptingUpdates = false;
    await Promise.all(updateEvents);
    return { result, isError: result.isError === true, durationMs };
  } catch (error) {
    const durationMs = elapsed();
    acceptingUpdates = false;
    await Promise.all(updateEvents);
    return {
      result: createErrorToolResult(error instanceof Error ? error.message : String(error)),
      isError: true,
      durationMs
    };
  } finally {
    acceptingUpdates = false;
  }
}
async function finalizeExecutedToolCall(currentContext, assistantMessage, prepared, executed, config, signal) {
  let result = executed.result;
  let isError = executed.isError;
  if (config.afterToolCall) {
    try {
      const afterResult = await config.afterToolCall(
        {
          assistantMessage,
          toolCall: prepared.toolCall,
          args: prepared.args,
          result,
          isError,
          context: currentContext
        },
        signal
      );
      if (afterResult) {
        const structuredContent = afterResult.structuredContent ?? (afterResult.content ? void 0 : result.structuredContent);
        result = {
          ...result,
          content: afterResult.content ?? result.content,
          details: afterResult.details ?? result.details,
          usage: afterResult.usage ?? result.usage,
          terminate: afterResult.terminate ?? result.terminate
        };
        if (structuredContent === void 0) delete result.structuredContent;
        else result.structuredContent = structuredContent;
        isError = afterResult.isError ?? isError;
      }
    } catch (error) {
      result = createErrorToolResult(error instanceof Error ? error.message : String(error));
      isError = true;
    }
  }
  return {
    toolCall: prepared.toolCall,
    result,
    isError,
    durationMs: executed.durationMs
  };
}
function createErrorToolResult(message) {
  return {
    content: [{ type: "text", text: message }],
    details: {}
  };
}
async function emitToolExecutionEnd(finalized, emit) {
  await emit({
    type: "tool_execution_end",
    toolCallId: finalized.toolCall.id,
    toolName: finalized.toolCall.name,
    result: finalized.result,
    isError: finalized.isError,
    ...finalized.durationMs === void 0 ? {} : { durationMs: finalized.durationMs }
  });
}
function createToolResultMessage(finalized) {
  return {
    role: "toolResult",
    toolCallId: finalized.toolCall.id,
    toolName: finalized.toolCall.name,
    // Untyped tools (JS extensions) can return results without content; normalize
    // so the null never enters session history or provider payloads.
    content: finalized.result.content ?? [],
    details: finalized.result.details,
    usage: finalized.result.usage,
    isError: finalized.isError,
    ...finalized.durationMs === void 0 ? {} : { durationMs: finalized.durationMs },
    timestamp: Date.now()
  };
}
async function emitToolResultMessage(toolResultMessage, emit) {
  await emit({ type: "message_start", message: toolResultMessage });
  await emit({ type: "message_end", message: toolResultMessage });
}

// ---------------------------------------------------------------------------
// agent
// ---------------------------------------------------------------------------

function defaultConvertToLlm(messages) {
  return messages.filter(
    (message) => message.role === "system" || message.role === "user" || message.role === "assistant" || message.role === "toolResult"
  );
}
const EMPTY_USAGE = {
  input: 0,
  output: 0,
  cacheRead: 0,
  cacheWrite: 0,
  totalTokens: 0,
  cost: { input: 0, output: 0, cacheRead: 0, cacheWrite: 0, total: 0 }
};
const DEFAULT_MODEL = {
  id: "unknown",
  name: "unknown",
  api: "unknown",
  provider: "unknown",
  baseUrl: "",
  reasoning: false,
  input: [],
  cost: { input: 0, output: 0, cacheRead: 0, cacheWrite: 0 },
  contextWindow: 0,
  maxTokens: 0
};
function createMutableAgentState(initialState) {
  let tools = initialState?.tools?.slice() ?? [];
  let messages = initialState?.messages?.slice() ?? [];
  const initialMessage = createInitialSystemMessage(initialState?.systemPrompt, tools.map(toToolDeclaration));
  if (messages[0]?.role !== "system" && initialMessage) messages.unshift(initialMessage);
  return {
    get systemPrompt() {
      return getCurrentSystemPrompt(messages);
    },
    model: initialState?.model ?? DEFAULT_MODEL,
    thinkingLevel: initialState?.thinkingLevel ?? "off",
    get tools() {
      return tools;
    },
    set tools(nextTools) {
      tools = nextTools.slice();
    },
    get messages() {
      return messages;
    },
    set messages(nextMessages) {
      messages = nextMessages.slice();
    },
    isStreaming: false,
    streamingMessage: void 0,
    pendingToolCalls: /* @__PURE__ */ new Set(),
    errorMessage: void 0
  };
}
class PendingMessageQueue {
  messages = [];
  mode;
  constructor(mode) {
    this.mode = mode;
  }
  enqueue(message) {
    this.messages.push(message);
  }
  hasItems() {
    return this.messages.length > 0;
  }
  peek() {
    if (this.mode === "all") return this.messages.slice();
    const first = this.messages[0];
    return first ? [first] : [];
  }
  drain() {
    const drained = this.peek();
    this.messages = this.messages.slice(drained.length);
    return drained;
  }
  clear() {
    this.messages = [];
  }
}
class Agent {
  _state;
  listeners = /* @__PURE__ */ new Set();
  steeringQueue;
  followUpQueue;
  convertToLlm;
  transformContext;
  streamFunction;
  getApiKey;
  onPayload;
  onResponse;
  onProviderStreamEvent;
  beforeToolCall;
  afterToolCall;
  finishTurn;
  prepareRequest;
  prepareNextTurn;
  prepareNextTurnWithContext;
  activeRun;
  /** Session identifier forwarded to providers for cache-aware backends. */
  sessionId;
  /** Optional per-level thinking token budgets forwarded to the stream function. */
  thinkingBudgets;
  /** Preferred transport forwarded to the stream function. */
  transport;
  /** Optional cap for provider-requested retry delays. */
  maxRetryDelayMs;
  /** Tool execution strategy for assistant messages that contain multiple tool calls. */
  toolExecution;
  /** Request identity forwarded to the stream function, over `DEFAULT_AGENT_IDENTITY`. */
  identity;
  constructor(options) {
    const runtimeOptions = options ?? {};
    this._state = createMutableAgentState(runtimeOptions.initialState);
    this.convertToLlm = runtimeOptions.convertToLlm ?? defaultConvertToLlm;
    this.transformContext = runtimeOptions.transformContext;
    this.streamFunction = runtimeOptions.streamFn ?? getDefaultStreamFn();
    this.getApiKey = runtimeOptions.getApiKey;
    this.onPayload = runtimeOptions.onPayload;
    this.onResponse = runtimeOptions.onResponse;
    this.onProviderStreamEvent = runtimeOptions.onProviderStreamEvent;
    this.beforeToolCall = runtimeOptions.beforeToolCall;
    this.afterToolCall = runtimeOptions.afterToolCall;
    this.finishTurn = runtimeOptions.finishTurn;
    this.prepareRequest = runtimeOptions.prepareRequest;
    this.prepareNextTurn = runtimeOptions.prepareNextTurn;
    this.prepareNextTurnWithContext = runtimeOptions.prepareNextTurnWithContext;
    this.steeringQueue = new PendingMessageQueue(runtimeOptions.steeringMode ?? "one-at-a-time");
    this.followUpQueue = new PendingMessageQueue(runtimeOptions.followUpMode ?? "one-at-a-time");
    this.sessionId = runtimeOptions.sessionId;
    this.thinkingBudgets = runtimeOptions.thinkingBudgets;
    this.transport = runtimeOptions.transport ?? "auto";
    this.maxRetryDelayMs = runtimeOptions.maxRetryDelayMs;
    this.toolExecution = runtimeOptions.toolExecution ?? "parallel";
    this.identity = runtimeOptions.identity;
  }
  /**
   * Subscribe to agent lifecycle events.
   *
   * Listener promises are awaited in subscription order and are included in
   * the current run's settlement. Listeners also receive the active abort
   * signal for the current run.
   *
   * `agent_end` is the final emitted event for a run, but the agent does not
   * become idle until all awaited listeners for that event have settled.
   */
  subscribe(listener) {
    this.listeners.add(listener);
    return () => this.listeners.delete(listener);
  }
  /**
   * Current agent state.
   *
   * Assigning `state.tools` or `state.messages` copies the provided top-level array.
   */
  get state() {
    return this._state;
  }
  /** Controls how queued steering messages are drained. */
  set steeringMode(mode) {
    this.steeringQueue.mode = mode;
  }
  get steeringMode() {
    return this.steeringQueue.mode;
  }
  /** Controls how queued follow-up messages are drained. */
  set followUpMode(mode) {
    this.followUpQueue.mode = mode;
  }
  get followUpMode() {
    return this.followUpQueue.mode;
  }
  /** Queue a message to be injected after the current assistant turn finishes. */
  steer(message) {
    this.steeringQueue.enqueue(message);
  }
  /** Queue a message to run only after the agent would otherwise stop. */
  followUp(message) {
    this.followUpQueue.enqueue(message);
  }
  /** Remove all queued steering messages. */
  clearSteeringQueue() {
    this.steeringQueue.clear();
  }
  /** Remove all queued follow-up messages. */
  clearFollowUpQueue() {
    this.followUpQueue.clear();
  }
  /** Remove all queued steering and follow-up messages. */
  clearAllQueues() {
    this.clearSteeringQueue();
    this.clearFollowUpQueue();
  }
  /** Returns true when either queue still contains pending messages. */
  hasQueuedMessages() {
    return this.steeringQueue.hasItems() || this.followUpQueue.hasItems();
  }
  /** Preview the messages selected for the next turn without consuming them. */
  peekQueuedMessages() {
    const steering = this.steeringQueue.peek();
    return steering.length > 0 ? steering : this.followUpQueue.peek();
  }
  /** Active abort signal for the current run, if any. */
  get signal() {
    return this.activeRun?.abortController.signal;
  }
  /** Abort the current run, if one is active. */
  abort() {
    this.activeRun?.abortController.abort();
  }
  /**
   * Resolve when the current run and all awaited event listeners have finished.
   *
   * This resolves after `agent_end` listeners settle.
   */
  waitForIdle() {
    return this.activeRun?.promise ?? Promise.resolve();
  }
  /** Clear conversation state and queues while retaining the replayed prompt/tool baseline. */
  reset() {
    if (this.activeRun) {
      throw new Error("Agent is already processing. Wait for completion before resetting.");
    }
    const baseline = getCurrentSystemMessage(this._state.messages);
    this._state.messages = baseline ? [baseline] : [];
    this._state.isStreaming = false;
    this._state.streamingMessage = void 0;
    this._state.pendingToolCalls = /* @__PURE__ */ new Set();
    this._state.errorMessage = void 0;
    this.clearFollowUpQueue();
    this.clearSteeringQueue();
  }
  async prompt(input, images) {
    if (this.activeRun) {
      throw new Error(
        "Agent is already processing a prompt. Use steer() or followUp() to queue messages, or wait for completion."
      );
    }
    const messages = this.normalizePromptInput(input, images);
    await this.runPromptMessages(messages);
  }
  /** Continue from the current transcript. The last message must be a user or tool-result message. */
  async continue() {
    if (this.activeRun) {
      throw new Error("Agent is already processing. Wait for completion before continuing.");
    }
    const lastMessage = this._state.messages[this._state.messages.length - 1];
    if (!lastMessage || this._state.messages.every((message) => message.role === "system")) {
      throw new Error("No messages to continue from");
    }
    if (lastMessage.role === "assistant") {
      const queuedSteering = this.steeringQueue.drain();
      if (queuedSteering.length > 0) {
        await this.runPromptMessages(queuedSteering, { skipInitialSteeringPoll: true });
        return;
      }
      const queuedFollowUps = this.followUpQueue.drain();
      if (queuedFollowUps.length > 0) {
        await this.runPromptMessages(queuedFollowUps);
        return;
      }
      throw new Error("Cannot continue from message role: assistant");
    }
    await this.runContinuation();
  }
  normalizePromptInput(input, images) {
    if (Array.isArray(input)) {
      return input;
    }
    if (typeof input !== "string") {
      return [input];
    }
    const content = [{ type: "text", text: input }];
    if (images && images.length > 0) {
      content.push(...images);
    }
    return [{ role: "user", content, timestamp: Date.now() }];
  }
  async runPromptMessages(messages, options = {}) {
    await this.runWithLifecycle(async (signal) => {
      await runAgentLoop(
        messages,
        this.createContextSnapshot(),
        this.createLoopConfig(options),
        (event) => this.processEvents(event),
        signal,
        this.streamFunction
      );
    });
  }
  async runContinuation() {
    await this.runWithLifecycle(async (signal) => {
      await runAgentLoopContinue(
        this.createContextSnapshot(),
        this.createLoopConfig(),
        (event) => this.processEvents(event),
        signal,
        this.streamFunction
      );
    });
  }
  createContextSnapshot() {
    return {
      messages: this._state.messages.slice(),
      tools: this._state.tools.slice()
    };
  }
  createLoopConfig(options = {}) {
    let skipInitialSteeringPoll = options.skipInitialSteeringPoll === true;
    return {
      model: this._state.model,
      reasoning: this._state.thinkingLevel === "off" ? void 0 : this._state.thinkingLevel,
      sessionId: this.sessionId,
      onPayload: this.onPayload,
      onResponse: this.onResponse,
      onProviderStreamEvent: this.onProviderStreamEvent,
      transport: this.transport,
      thinkingBudgets: this.thinkingBudgets,
      maxRetryDelayMs: this.maxRetryDelayMs,
      identity: this.identity,
      toolExecution: this.toolExecution,
      beforeToolCall: this.beforeToolCall,
      afterToolCall: this.afterToolCall,
      finishTurn: this.finishTurn,
      prepareRequest: this.prepareRequest,
      prepareNextTurn: this.prepareNextTurnWithContext || this.prepareNextTurn ? async (context) => {
        if (this.prepareNextTurnWithContext) {
          return await this.prepareNextTurnWithContext(context, this.signal);
        }
        return await this.prepareNextTurn?.(this.signal);
      } : void 0,
      convertToLlm: this.convertToLlm,
      transformContext: this.transformContext,
      getApiKey: this.getApiKey,
      getSteeringMessages: async () => {
        if (skipInitialSteeringPoll) {
          skipInitialSteeringPoll = false;
          return [];
        }
        return this.steeringQueue.drain();
      },
      getFollowUpMessages: async () => this.followUpQueue.drain()
    };
  }
  async runWithLifecycle(executor) {
    if (this.activeRun) {
      throw new Error("Agent is already processing.");
    }
    const abortController = new AbortController();
    let resolvePromise = () => {
    };
    const promise = new Promise((resolve) => {
      resolvePromise = resolve;
    });
    this.activeRun = { promise, resolve: resolvePromise, abortController };
    this._state.isStreaming = true;
    this._state.streamingMessage = void 0;
    this._state.errorMessage = void 0;
    try {
      await executor(abortController.signal);
    } catch (error) {
      await this.handleRunFailure(error, abortController.signal.aborted);
    } finally {
      this.finishRun();
    }
  }
  async handleRunFailure(error, aborted) {
    const failureMessage = {
      role: "assistant",
      content: [{ type: "text", text: "" }],
      api: this._state.model.api,
      provider: this._state.model.provider,
      model: this._state.model.id,
      usage: EMPTY_USAGE,
      stopReason: aborted ? "aborted" : "error",
      errorMessage: error instanceof Error ? error.message : String(error),
      timestamp: Date.now()
    };
    await this.processEvents({ type: "message_start", message: failureMessage });
    await this.processEvents({ type: "message_end", message: failureMessage });
    await this.processEvents({ type: "turn_end", message: failureMessage, toolResults: [] });
    await this.processEvents({ type: "agent_end", messages: [failureMessage] });
  }
  finishRun() {
    this._state.isStreaming = false;
    this._state.streamingMessage = void 0;
    this._state.pendingToolCalls = /* @__PURE__ */ new Set();
    this.activeRun?.resolve();
    this.activeRun = void 0;
  }
  /**
   * Reduce internal state for a loop event, then await listeners.
   *
   * `agent_end` only means no further loop events will be emitted. The run is
   * considered idle later, after all awaited listeners for `agent_end` finish
   * and `finishRun()` clears runtime-owned state.
   */
  async processEvents(event) {
    switch (event.type) {
      case "message_start":
        this._state.streamingMessage = event.message;
        break;
      case "message_update":
        this._state.streamingMessage = event.message;
        break;
      case "message_end":
        this._state.streamingMessage = void 0;
        this._state.messages.push(event.message);
        break;
      case "tool_execution_start": {
        const pendingToolCalls = new Set(this._state.pendingToolCalls);
        pendingToolCalls.add(event.toolCallId);
        this._state.pendingToolCalls = pendingToolCalls;
        break;
      }
      case "tool_execution_end": {
        const pendingToolCalls = new Set(this._state.pendingToolCalls);
        pendingToolCalls.delete(event.toolCallId);
        this._state.pendingToolCalls = pendingToolCalls;
        break;
      }
      case "turn_end":
        if (event.message.role === "assistant" && event.message.errorMessage) {
          this._state.errorMessage = event.message.errorMessage;
        }
        break;
      case "agent_end":
        this._state.streamingMessage = void 0;
        break;
    }
    const signal = this.activeRun?.abortController.signal;
    if (!signal) {
      throw new Error("Agent listener invoked outside active run");
    }
    for (const listener of this.listeners) {
      await listener(event, signal);
    }
  }
}

// ---------------------------------------------------------------------------
// proxy
// ---------------------------------------------------------------------------

function buildProxyRequestOptions(options) {
  return {
    temperature: options.temperature,
    samplingParams: options.samplingParams,
    maxTokens: options.maxTokens,
    reasoning: options.reasoning,
    cacheRetention: options.cacheRetention,
    sessionId: options.sessionId,
    headers: options.headers,
    metadata: options.metadata,
    transport: options.transport,
    thinkingBudgets: options.thinkingBudgets,
    maxRetryDelayMs: options.maxRetryDelayMs
  };
}
function streamProxy(model, context, options) {
  const stream = createAssistantMessageEventStream();
  (async () => {
    const partial = {
      role: "assistant",
      stopReason: "pending",
      content: [],
      api: model.api,
      provider: model.provider,
      model: model.id,
      usage: {
        input: 0,
        output: 0,
        cacheRead: 0,
        cacheWrite: 0,
        totalTokens: 0,
        cost: { input: 0, output: 0, cacheRead: 0, cacheWrite: 0, total: 0 }
      },
      timestamp: Date.now()
    };
    let reader;
    const abortHandler = () => {
      if (reader) {
        reader.cancel("Request aborted by user").catch(() => {
        });
      }
    };
    if (options.signal) {
      options.signal.addEventListener("abort", abortHandler);
    }
    try {
      const response = await fetch(`${options.proxyUrl}/api/stream`, {
        method: "POST",
        headers: {
          Authorization: `Bearer ${options.authToken}`,
          "Content-Type": "application/json"
        },
        body: JSON.stringify({
          model,
          context,
          options: buildProxyRequestOptions(options)
        }),
        signal: options.signal
      });
      if (!response.ok) {
        let errorMessage = `Proxy error: ${response.status} ${response.statusText}`;
        try {
          const errorData = await response.json();
          if (errorData.error) {
            errorMessage = `Proxy error: ${errorData.error}`;
          }
        } catch {
        }
        throw new Error(errorMessage);
      }
      reader = response.body.getReader();
      const decoder = new TextDecoder();
      let buffer = "";
      let sawTerminalEvent = false;
      const processLine = (line) => {
        if (!line.startsWith("data: ")) return;
        const data = line.slice(6).trim();
        if (!data) return;
        const proxyEvent = JSON.parse(data);
        const event = processProxyEvent(proxyEvent, partial);
        if (event) {
          if (event.type === "done" || event.type === "error") sawTerminalEvent = true;
          stream.push(event);
        }
      };
      while (true) {
        const { done, value } = await reader.read();
        if (done) break;
        if (options.signal?.aborted) {
          throw new Error("Request aborted by user");
        }
        buffer += decoder.decode(value, { stream: true });
        const lines = buffer.split("\n");
        buffer = lines.pop() || "";
        for (const line of lines) {
          processLine(line);
        }
      }
      if (options.signal?.aborted) {
        throw new Error("Request aborted by user");
      }
      buffer += decoder.decode();
      if (buffer) {
        processLine(buffer);
      }
      if (!sawTerminalEvent) {
        partial.stopReason = "error";
        partial.errorMessage = "Connection closed by proxy server before the response completed";
        stream.push({
          type: "error",
          reason: "error",
          error: partial
        });
      }
      stream.end();
    } catch (error) {
      const errorMessage = error instanceof Error ? error.message : String(error);
      const reason = options.signal?.aborted ? "aborted" : "error";
      partial.stopReason = reason;
      partial.errorMessage = errorMessage;
      stream.push({
        type: "error",
        reason,
        error: partial
      });
      stream.end();
    } finally {
      if (options.signal) {
        options.signal.removeEventListener("abort", abortHandler);
      }
    }
  })();
  return stream;
}
function processProxyEvent(proxyEvent, partial) {
  switch (proxyEvent.type) {
    case "start":
      return { type: "start", partial };
    case "text_start":
      partial.content[proxyEvent.contentIndex] = { type: "text", text: "" };
      return { type: "text_start", contentIndex: proxyEvent.contentIndex, partial };
    case "text_delta": {
      const content = partial.content[proxyEvent.contentIndex];
      if (content?.type === "text") {
        content.text += proxyEvent.delta;
        return {
          type: "text_delta",
          contentIndex: proxyEvent.contentIndex,
          delta: proxyEvent.delta,
          partial
        };
      }
      throw new Error("Received text_delta for non-text content");
    }
    case "text_end": {
      const content = partial.content[proxyEvent.contentIndex];
      if (content?.type === "text") {
        content.textSignature = proxyEvent.contentSignature;
        return {
          type: "text_end",
          contentIndex: proxyEvent.contentIndex,
          content: content.text,
          partial
        };
      }
      throw new Error("Received text_end for non-text content");
    }
    case "thinking_start":
      partial.content[proxyEvent.contentIndex] = { type: "thinking", thinking: "" };
      return { type: "thinking_start", contentIndex: proxyEvent.contentIndex, partial };
    case "thinking_delta": {
      const content = partial.content[proxyEvent.contentIndex];
      if (content?.type === "thinking") {
        content.thinking += proxyEvent.delta;
        return {
          type: "thinking_delta",
          contentIndex: proxyEvent.contentIndex,
          delta: proxyEvent.delta,
          partial
        };
      }
      throw new Error("Received thinking_delta for non-thinking content");
    }
    case "thinking_end": {
      const content = partial.content[proxyEvent.contentIndex];
      if (content?.type === "thinking") {
        content.thinkingSignature = proxyEvent.contentSignature;
        return {
          type: "thinking_end",
          contentIndex: proxyEvent.contentIndex,
          content: content.thinking,
          partial
        };
      }
      throw new Error("Received thinking_end for non-thinking content");
    }
    case "toolcall_start":
      partial.content[proxyEvent.contentIndex] = {
        type: "toolCall",
        id: proxyEvent.id,
        name: proxyEvent.toolName,
        arguments: {},
        partialJson: ""
      };
      return { type: "toolcall_start", contentIndex: proxyEvent.contentIndex, partial };
    case "toolcall_delta": {
      const content = partial.content[proxyEvent.contentIndex];
      if (content?.type === "toolCall") {
        content.partialJson += proxyEvent.delta;
        content.arguments = parseStreamingJson(content.partialJson) || {};
        partial.content[proxyEvent.contentIndex] = { ...content };
        return {
          type: "toolcall_delta",
          contentIndex: proxyEvent.contentIndex,
          delta: proxyEvent.delta,
          partial
        };
      }
      throw new Error("Received toolcall_delta for non-toolCall content");
    }
    case "toolcall_end": {
      const content = partial.content[proxyEvent.contentIndex];
      if (content?.type === "toolCall") {
        Object.assign(content, proxyEvent.toolCall);
        delete content.partialJson;
        return {
          type: "toolcall_end",
          contentIndex: proxyEvent.contentIndex,
          toolCall: content,
          partial
        };
      }
      return void 0;
    }
    case "done":
      partial.stopReason = proxyEvent.reason;
      partial.usage = proxyEvent.usage;
      if (proxyEvent.providerThinkingLevel !== void 0) {
        partial.providerThinkingLevel = proxyEvent.providerThinkingLevel;
      }
      return { type: "done", reason: proxyEvent.reason, message: partial };
    case "error":
      partial.stopReason = proxyEvent.reason;
      partial.errorMessage = proxyEvent.errorMessage;
      partial.usage = proxyEvent.usage;
      if (proxyEvent.providerThinkingLevel !== void 0) {
        partial.providerThinkingLevel = proxyEvent.providerThinkingLevel;
      }
      return { type: "error", reason: proxyEvent.reason, error: partial };
    default: {
      const _exhaustiveCheck = proxyEvent;
      console.warn(`Unhandled proxy event type: ${proxyEvent.type}`);
      return void 0;
    }
  }
}

setDefaultStreamFn(streamSimple);

export {
	setDefaultStreamFn,
	DEFAULT_AGENT_IDENTITY,
	agentLoop,
	agentLoopContinue,
	runAgentLoop,
	runAgentLoopContinue,
	runToolCall,
	Agent,
	streamProxy,
};
