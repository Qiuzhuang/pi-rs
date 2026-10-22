// Golden renders of the session tree selector (/tree) and the session selector (/resume).
import { assistant } from "./corpus.mjs";
import { Golden, ref } from "./common.mjs";

const C = "coding-agent/src/modes/interactive/components";
const { TreeSelectorComponent } = await ref(`${C}/tree-selector.ts`);
const { SessionSelectorComponent } = await ref(`${C}/session-selector.ts`);
const { KeybindingsManager } = await ref("coding-agent/src/core/keybindings.ts");

const g = new Golden("sessions.json");
const noop = () => {};
const DOWN = "\x1b[B";
const UP = "\x1b[A";

function feed(component, keys) {
	for (const key of keys ?? []) component.handleInput(key);
}

// --- tree ---------------------------------------------------------------------------------------
const ts = (i) => `2020-03-0${1 + (i % 9)}T10:${String(i % 60).padStart(2, "0")}:00.000Z`;
let counter = 0;
function entry(id, parentId, fields) {
	return { id, parentId, timestamp: ts(counter++), ...fields };
}
const user = (id, parent, text) => entry(id, parent, { type: "message", message: { role: "user", content: text, timestamp: 0 } });
const asst = (id, parent, content, extra) => entry(id, parent, { type: "message", message: assistant(content, extra) });
const toolResult = (id, parent, toolName, text, isError = false) =>
	entry(id, parent, {
		type: "message",
		message: { role: "toolResult", toolCallId: `tc-${id}`, toolName, content: [{ type: "text", text }], isError, timestamp: 0 },
	});

function node(e, children = [], extra = {}) {
	return { entry: e, children, ...extra };
}

function buildTree() {
	counter = 0;
	const u1 = user("u1", null, "Refactor the parser to support streaming input");
	const a1 = asst("a1", "u1", [{ type: "thinking", thinking: "plan" }, { type: "text", text: "I'll start by reading the parser." }]);
	const a2 = asst(
		"a2",
		"a1",
		[
			{ type: "toolCall", id: "tc1", name: "read", arguments: { path: "/home/golden/project/src/parser.ts", offset: 10, limit: 40 } },
			{ type: "toolCall", id: "tc2", name: "bash", arguments: { command: "npm test -- parser" } },
		],
		{ stopReason: "toolUse" },
	);
	const t1 = toolResult("t1", "a2", "read", "export function parse() {}");
	const t2 = toolResult("t2", "t1", "bash", "1 failing", true);
	const a3 = asst("a3", "t2", [
		{ type: "toolCall", id: "tc3", name: "edit", arguments: { path: "src/parser.ts", edits: [] } },
		{ type: "toolCall", id: "tc4", name: "grep", arguments: { pattern: "parse\\(", path: "src" } },
		{ type: "toolCall", id: "tc5", name: "find", arguments: { pattern: "*.test.ts" } },
		{ type: "toolCall", id: "tc6", name: "ls", arguments: { path: "src" } },
		{ type: "toolCall", id: "tc7", name: "write", arguments: { path: "notes.md", content: "x" } },
		{ type: "toolCall", id: "tc8", name: "custom_tool", arguments: { a: 1 } },
	], { stopReason: "toolUse" });
	const mc = entry("mc", "a3", { type: "model_change", provider: "openai", modelId: "gpt-5" });
	const tl = entry("tl", "mc", { type: "thinking_level_change", thinkingLevel: "high" });
	const comp = entry("comp", "tl", { type: "compaction", summary: "Summary of the work", firstKeptEntryId: "a3", tokensBefore: 54321 });
	const u2 = user("u2", "comp", "Now add tests for the streaming case, including a very long description that has to be truncated in the tree");
	const a4 = asst("a4", "u2", [{ type: "text", text: "Added three tests." }]);
	// A second branch from a1.
	const u3 = user("u3", "a1", "Actually, try a different approach with generators");
	const bs = entry("bs", "u3", { type: "branch_summary", fromId: "a4", summary: "Tried the streaming approach" });
	const a5 = asst("a5", "bs", [{ type: "text", text: "Using generators now." }], { stopReason: "error", errorMessage: "overloaded" });
	const cm = entry("cm", "a5", { type: "custom_message", customType: "reminder", content: "Remember the deadline", display: true });
	const cu = entry("cu", "cm", { type: "custom", customType: "bookmark", data: { x: 1 } });
	const si = entry("si", "cu", { type: "session_info", name: "generators" });
	const bash = entry("bx", "si", {
		type: "message",
		message: { role: "bashExecution", command: "git status", output: "clean", exitCode: 0, cancelled: false, truncated: false, timestamp: 0 },
	});
	const u4 = user("u4", "bx", "third branch leaf");

	return [
		node(u1, [
			node(a1, [
				node(a2, [
					node(t1, [
						node(t2, [
							node(a3, [
								node(mc, [node(tl, [node(comp, [node(u2, [node(a4, [], { label: "done", labelTimestamp: "2020-05-06T07:08:00.000Z" })])])])]),
							]),
						]),
					]),
				]),
				node(u3, [node(bs, [node(a5, [node(cm, [node(cu, [node(si, [node(bash, [node(u4)])])])])])])], { label: "alt approach" }),
			]),
		]),
	];
}

const TREE = buildTree();
const buildTreeSelector = (input) => {
	const c = new TreeSelectorComponent(structuredClone(input.tree), input.leaf, input.height ?? 24, noop, noop, noop, input.selected, input.filter);
	if (input.focused) c.focused = true;
	feed(c, input.keys);
	return c;
};
g.add("tree/default", "tree_selector", { tree: TREE, leaf: "a4" }, buildTreeSelector);
g.add("tree/other-leaf", "tree_selector", { tree: TREE, leaf: "u4" }, buildTreeSelector);
g.add("tree/selected-initial", "tree_selector", { tree: TREE, leaf: "a4", selected: "u3" }, buildTreeSelector);
g.add("tree/filter-all", "tree_selector", { tree: TREE, leaf: "a4", filter: "all", height: 60 }, buildTreeSelector);
g.add("tree/filter-no-tools", "tree_selector", { tree: TREE, leaf: "a4", filter: "no-tools" }, buildTreeSelector);
g.add("tree/filter-user-only", "tree_selector", { tree: TREE, leaf: "a4", filter: "user-only" }, buildTreeSelector);
g.add("tree/filter-labeled", "tree_selector", { tree: TREE, leaf: "a4", filter: "labeled-only" }, buildTreeSelector);
g.add("tree/navigate", "tree_selector", { tree: TREE, leaf: "a4", keys: [UP, UP, UP] }, buildTreeSelector);
g.add("tree/search", "tree_selector", { tree: TREE, leaf: "a4", keys: ["t", "e", "s", "t"] }, buildTreeSelector);
g.add("tree/cycle-filter", "tree_selector", { tree: TREE, leaf: "a4", keys: ["\x0f"] }, buildTreeSelector);
g.add("tree/label-timestamps", "tree_selector", { tree: TREE, leaf: "a4", filter: "all", height: 60, keys: ["T"] }, buildTreeSelector);
g.add("tree/fold", "tree_selector", { tree: TREE, leaf: "a4", keys: [UP, UP, "\x1b[1;5D"] }, buildTreeSelector);
g.add("tree/label-edit", "tree_selector", { tree: TREE, leaf: "a4", focused: true, keys: ["L", "x", "y"] }, buildTreeSelector);
g.add("tree/small-height", "tree_selector", { tree: TREE, leaf: "a4", height: 6 }, buildTreeSelector);

// --- session selector ---------------------------------------------------------------------------
const MIN = 60_000;
const HOUR = 60 * MIN;
const DAY = 24 * HOUR;
const sessionSpecs = [
	{ id: "s1", name: "parser refactor", ago: 30 * MIN + 5000, messages: 42, first: "Refactor the parser to support streaming input" },
	{ id: "s2", ago: 5 * HOUR + 30 * MIN, messages: 3, first: "What does this repo do?" },
	{ id: "s3", ago: 3 * DAY + 12 * HOUR, messages: 128, first: "Implement the OAuth flow for the new provider and add tests", parent: "s1" },
	{ id: "s4", name: "release notes", ago: 17 * DAY, messages: 7, first: "Write release notes for 1.2", cwd: "/home/golden/other" },
	{ id: "s5", ago: 100 * DAY, messages: 1, first: "hello" },
	{ id: "s6", ago: 800 * DAY, messages: 9, first: "Old session with a first message that is long enough to need truncation at forty columns" },
];
function sessionInfo(spec, now) {
	const path = `/home/golden/.pi/agent/sessions/--home-golden-project--/${spec.id}.jsonl`;
	return {
		path,
		id: spec.id,
		cwd: spec.cwd ?? "/home/golden/project",
		name: spec.name,
		parentSessionPath: spec.parent ? `/home/golden/.pi/agent/sessions/--home-golden-project--/${spec.parent}.jsonl` : undefined,
		created: new Date(now - spec.ago - HOUR),
		modified: new Date(now - spec.ago),
		messageCount: spec.messages,
		firstMessage: spec.first,
		allMessagesText: spec.first,
	};
}

/** `input.sessions` are specs with ages relative to the time of rendering (the Rust side does the same). */
const buildSessionSelector = async (input) => {
	const now = Date.now();
	const current = input.sessions.filter((s) => !s.cwd).map((s) => sessionInfo(s, now));
	const all = input.sessions.map((s) => sessionInfo(s, now));
	const c = new SessionSelectorComponent(
		async () => current,
		async () => all,
		noop,
		noop,
		noop,
		noop,
		{
			showRenameHint: input.renameHint ?? true,
			keybindings: new KeybindingsManager({}),
			renameSession: input.rename ? async () => {} : undefined,
		},
		input.currentPath,
	);
	await new Promise((resolve) => setTimeout(resolve, 20));
	if (input.focused) c.focused = true;
	for (const key of input.keys ?? []) {
		c.handleInput(key);
		await new Promise((resolve) => setTimeout(resolve, 20));
	}
	return c;
};
await g.addAsync("sessions/default", "session_selector", { sessions: sessionSpecs }, buildSessionSelector);
await g.addAsync("sessions/no-rename-hint", "session_selector", { sessions: sessionSpecs, renameHint: false }, buildSessionSelector);
await g.addAsync("sessions/current-marked", "session_selector", { sessions: sessionSpecs, currentPath: "/home/golden/.pi/agent/sessions/--home-golden-project--/s2.jsonl" }, buildSessionSelector);
await g.addAsync("sessions/all-scope", "session_selector", { sessions: sessionSpecs, keys: ["\t"] }, buildSessionSelector);
await g.addAsync("sessions/sort-toggle", "session_selector", { sessions: sessionSpecs, keys: ["\x13"] }, buildSessionSelector);
await g.addAsync("sessions/sort-toggle-twice", "session_selector", { sessions: sessionSpecs, keys: ["\x13", "\x13"] }, buildSessionSelector);
await g.addAsync("sessions/named-only", "session_selector", { sessions: sessionSpecs, keys: ["\x0e"] }, buildSessionSelector);
await g.addAsync("sessions/show-path", "session_selector", { sessions: sessionSpecs, keys: ["\x10"] }, buildSessionSelector);
await g.addAsync("sessions/search", "session_selector", { sessions: sessionSpecs, focused: true, keys: ["o", "a", "u", "t", "h"] }, buildSessionSelector);
await g.addAsync("sessions/navigate", "session_selector", { sessions: sessionSpecs, keys: [DOWN, DOWN] }, buildSessionSelector);
await g.addAsync("sessions/empty", "session_selector", { sessions: [] }, buildSessionSelector);
await g.addAsync("sessions/rename-mode", "session_selector", { sessions: sessionSpecs, rename: true, focused: true, keys: ["\x12", "x"] }, buildSessionSelector);
await g.addAsync("sessions/delete-confirm", "session_selector", { sessions: sessionSpecs, keys: [DOWN, "\x04"] }, buildSessionSelector);

g.write();
process.exit(0);
