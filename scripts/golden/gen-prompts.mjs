// Golden text the model sees: the system prompt, the built-in tool declarations, the compaction and
// branch summary requests, and the CLI help. Written to `prompts.json`; compared by
// `crates/pi-coding-agent/tests/golden_prompts.rs`.
//
// Case shape: `{ name, kind, input, out }` (no themes or widths).
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";

const REF = process.env.PI_TS_REF;
if (!REF) throw new Error("PI_TS_REF must point at the TypeScript pi checkout");
const here = path.dirname(fileURLToPath(import.meta.url));
const OUT_DIR = process.env.GOLDEN_OUT ?? path.resolve(here, "../../crates/pi-coding-agent/tests/fixtures/golden");

/** Package directory the docs paths in the prompt resolve under (the Rust test pins the same). */
const PACKAGE_DIR = "/home/golden/pi-package";
process.env.PI_PACKAGE_DIR = PACKAGE_DIR;
const CWD = "/home/golden/project";

const ref = (rel) => import(pathToFileURL(path.join(REF, "packages", rel)).href);

const { buildSystemPrompt, buildSystemPromptSections } = await ref("coding-agent/src/core/system-prompt.ts");
const { formatSkillsForPrompt } = await ref("coding-agent/src/core/skills.ts");
const { createAllToolDefinitions } = await ref("coding-agent/src/core/tools/index.ts");
const compaction = await ref("coding-agent/src/core/compaction/index.ts");
const { convertToLlm } = await ref("coding-agent/src/core/messages.ts");
const { printHelp } = await ref("coding-agent/src/cli/args.ts");

const cases = [];
const names = new Set();
function add(name, kind, input, out) {
	if (names.has(name)) throw new Error(`duplicate case ${name}`);
	names.add(name);
	cases.push({ name, kind, input, out });
}

// ---------------------------------------------------------------------------------------------
// Tools

const definitions = createAllToolDefinitions(CWD);
const toolSnippets = {};
const toolGuidelines = {};
for (const [name, def] of Object.entries(definitions)) {
	const out = JSON.parse(
		JSON.stringify({
			name: def.name,
			label: def.label,
			description: def.description,
			promptSnippet: def.promptSnippet,
			promptGuidelines: def.promptGuidelines,
			parameters: def.parameters,
			outputSchema: def.outputSchema,
			annotations: def.annotations,
			executionMode: def.executionMode,
			exposure: def.exposure,
			defaultActive: def.defaultActive,
			renderShell: def.renderShell,
		}),
	);
	add(`tool/${name}`, "tool_definition", { toolName: name, cwd: CWD }, out);
	if (def.promptSnippet) toolSnippets[name] = def.promptSnippet;
	if (def.promptGuidelines) toolGuidelines[name] = def.promptGuidelines;
}

// ---------------------------------------------------------------------------------------------
// System prompt

const skill = (name, description, extra = {}) => ({
	name,
	description,
	filePath: `/home/golden/.pi/agent/skills/${name}/SKILL.md`,
	baseDir: `/home/golden/.pi/agent/skills/${name}`,
	sourceInfo: { path: `/home/golden/.pi/agent/skills/${name}/SKILL.md`, source: "local", scope: "user", origin: "top-level" },
	disableModelInvocation: false,
	...extra,
});
const skills = [
	skill("pdf-tools", "Extract text & tables from <PDF> files, \"quoted\" and 'single'."),
	skill("hidden-skill", "Only via /skill:hidden-skill", { disableModelInvocation: true }),
	skill("git-helper", "Helps with git."),
];
const contextFiles = [
	{ path: "/home/golden/.pi/agent/AGENTS.md", content: "Global instructions." },
	{ path: `${CWD}/AGENTS.md`, content: "# Project\n\nUse tabs.\n" },
];

function prompt(name, input) {
	add(`system_prompt/${name}`, "system_prompt", input, {
		text: buildSystemPrompt(input),
		sections: input.forceSystemPrompt === undefined ? buildSystemPromptSections(input) : null,
	});
}

prompt("defaults", { cwd: CWD });
prompt("default_tools", { cwd: CWD, toolSnippets, toolGuidelines });
prompt("all_tools", {
	cwd: CWD,
	selectedTools: ["read", "bash", "edit", "write", "grep", "find", "ls"],
	toolSnippets,
	toolGuidelines,
	contextFiles,
	skills,
});
prompt("read_only", { cwd: CWD, selectedTools: ["read", "grep", "find", "ls"], toolSnippets, toolGuidelines, skills });
prompt("bash_only_skills", { cwd: CWD, selectedTools: ["bash"], toolSnippets, toolGuidelines, skills });
prompt("powershell", { cwd: "C:\\Users\\golden\\project", selectedTools: ["read", "powershell", "edit", "write"], toolSnippets, toolGuidelines });
prompt("bash_and_powershell", { cwd: CWD, selectedTools: ["bash", "powershell"], toolSnippets, toolGuidelines });
prompt("no_tools", { cwd: CWD, selectedTools: [], skills, contextFiles });
prompt("custom_prompt", {
	cwd: CWD,
	customPrompt: "You are a careful reviewer.",
	appendSystemPrompt: "Always answer in English.",
	toolSnippets,
	toolGuidelines,
	contextFiles,
	skills,
});
prompt("guidelines_and_sections", {
	cwd: CWD,
	selectedTools: ["read", "bash", "custom_tool"],
	toolSnippets: { ...toolSnippets, custom_tool: "Run the custom thing" },
	toolGuidelines: { ...toolGuidelines, custom_tool: ["Prefer custom_tool for custom things.", "  Be concise in your responses  "] },
	promptGuidelines: ["Extra guideline.", "Extra guideline.", "   ", "Show file paths clearly when working with files"],
	appendSystemPrompt: "Appended.",
	sections: { memory: "Remember the user prefers tabs.", empty: "", cwd: "overridden cwd" },
});
prompt("hidden_tools", {
	cwd: CWD,
	selectedTools: ["read", "bash", "codemode"],
	hiddenTools: ["read", "bash"],
	toolSnippets: { ...toolSnippets, codemode: "Run a script that calls tools" },
	toolGuidelines,
	skills,
});
prompt("forced", { cwd: CWD, forceSystemPrompt: "Exactly this prompt.", toolSnippets, skills });

for (const tool of ["read", "bash", "indirect"]) {
	add(`skills/${tool}`, "format_skills", { skills, fileReadTool: tool }, { text: formatSkillsForPrompt(skills, tool) });
}
add("skills/none_visible", "format_skills", { skills: [skills[1]], fileReadTool: "read" }, { text: formatSkillsForPrompt([skills[1]], "read") });

// ---------------------------------------------------------------------------------------------
// Compaction and branch summaries

const usage = { input: 10, output: 5, cacheRead: 0, cacheWrite: 0, totalTokens: 15, cost: { input: 0, output: 0, cacheRead: 0, cacheWrite: 0, total: 0 } };
const model = {
	id: "summarizer",
	name: "Summarizer",
	api: "anthropic-messages",
	provider: "anthropic",
	baseUrl: "https://api.anthropic.com",
	reasoning: true,
	input: ["text"],
	cost: { input: 0, output: 0, cacheRead: 0, cacheWrite: 0 },
	contextWindow: 200000,
	maxTokens: 8192,
};
const longOutput = `${"0123456789".repeat(250)}tail`;
const messages = [
	{ role: "user", content: "Fix the bug in src/app.ts", timestamp: 1 },
	{
		role: "user",
		content: [
			{ type: "text", text: "Here is a screenshot:" },
			{ type: "image", data: "aGVsbG8=", mimeType: "image/png" },
			{ type: "text", text: " and more text" },
		],
		timestamp: 2,
	},
	{
		role: "assistant",
		content: [
			{ type: "thinking", thinking: "Let me look at the file." },
			{ type: "text", text: "I'll read the file." },
			{ type: "toolCall", id: "call-1", name: "read", arguments: { path: "src/app.ts", offset: 10, limit: 20 } },
			{ type: "toolCall", id: "call-2", name: "bash", arguments: { command: "rg \"needle\" -n", timeout: 30 } },
		],
		api: "anthropic-messages",
		provider: "anthropic",
		model: "claude",
		usage,
		stopReason: "toolUse",
		timestamp: 3,
	},
	{ role: "toolResult", toolCallId: "call-1", toolName: "read", content: [{ type: "text", text: longOutput }], isError: false, timestamp: 4 },
	{ role: "toolResult", toolCallId: "call-2", toolName: "bash", content: [{ type: "text", text: "src/app.ts:12: needle" }], isError: true, timestamp: 5 },
	{
		role: "assistant",
		content: [
			{ type: "text", text: "Found it." },
			{ type: "text", text: "Editing now." },
			{ type: "toolCall", id: "call-3", name: "edit", arguments: { path: "src/app.ts", edits: [{ oldText: "a", newText: "b" }] } },
		],
		api: "anthropic-messages",
		provider: "anthropic",
		model: "claude",
		usage,
		stopReason: "toolUse",
		timestamp: 6,
	},
	{ role: "toolResult", toolCallId: "call-3", toolName: "edit", content: [{ type: "text", text: "Edited." }], isError: false, timestamp: 7 },
	{ role: "bashExecution", command: "npm test", output: "1 failing", exitCode: 1, cancelled: false, truncated: true, fullOutputPath: "/tmp/out.log", timestamp: 8 },
	{ role: "bashExecution", command: "ls", output: "", exitCode: 0, cancelled: false, truncated: false, timestamp: 9 },
	{ role: "bashExecution", command: "secret", output: "x", exitCode: 0, cancelled: false, truncated: false, timestamp: 10, excludeFromContext: true },
	{ role: "bashExecution", command: "sleep 100", output: "", exitCode: undefined, cancelled: true, truncated: false, timestamp: 11 },
	{ role: "custom", customType: "note", content: "A custom note.", display: true, timestamp: 12 },
	{ role: "branchSummary", summary: "Explored another approach.", fromId: "abc", timestamp: 13 },
	{ role: "compactionSummary", summary: "Earlier work summarized.", tokensBefore: 1234, timestamp: 14 },
	{ role: "assistant", content: [{ type: "text", text: "Done. All tests pass." }], api: "anthropic-messages", provider: "anthropic", model: "claude", usage, stopReason: "stop", timestamp: 15 },
];

add("compaction/serialize_conversation", "serialize_conversation", { messages }, { text: compaction.serializeConversation(convertToLlm(messages)) });
add(
	"compaction/format_file_operations",
	"format_file_operations",
	{ readFiles: ["a.ts", "b.ts"], modifiedFiles: ["c.ts"] },
	{ text: compaction.formatFileOperations(["a.ts", "b.ts"], ["c.ts"]) },
);

/** A stream function that records each request and answers with `reply`. */
function recorder(reply = "SUMMARY") {
	const calls = [];
	const streamFn = async (_model, context, options) => {
		calls.push({
			context: JSON.parse(JSON.stringify(context, (key, value) => (key === "timestamp" ? undefined : value))),
			reasoning: options.reasoning ?? null,
			cacheRetention: options.cacheRetention ?? null,
		});
		return {
			result: async () => ({
				role: "assistant",
				content: [{ type: "text", text: `${reply} ${calls.length}` }],
				api: model.api,
				provider: model.provider,
				model: model.id,
				usage,
				stopReason: "stop",
				timestamp: 0,
			}),
		};
	};
	return { calls, streamFn };
}

async function summary(name, input) {
	const { calls, streamFn } = recorder();
	const result = await compaction.generateSummaryWithUsage(
		messages,
		model,
		undefined,
		"key",
		undefined,
		undefined,
		input.customInstructions,
		input.previousSummary,
		input.thinkingLevel,
		streamFn,
		undefined,
		undefined,
		undefined,
		"session-1",
	);
	add(`compaction/${name}`, "generate_summary", input, { calls, text: result.text });
}
await summary("summary", {});
await summary("summary_update_custom", { previousSummary: "## Goal\nOld goal.", customInstructions: "Focus on tests.", thinkingLevel: "high" });

async function compactCase(name, input) {
	const { calls, streamFn } = recorder();
	const fileOps = compaction.createFileOps();
	for (const f of input.read ?? []) fileOps.read.add(f);
	for (const f of input.edited ?? []) fileOps.edited.add(f);
	for (const f of input.written ?? []) fileOps.written.add(f);
	const preparation = {
		firstKeptEntryId: "keep-1",
		messagesToSummarize: input.history ? messages.slice(0, 7) : [],
		turnPrefixMessages: input.split ? messages.slice(7) : [],
		isSplitTurn: !!input.split,
		tokensBefore: 5000,
		previousSummary: input.previousSummary,
		fileOps,
		settings: { enabled: true, reserveTokens: 16384, keepRecentTokens: 20000 },
	};
	const result = await compaction.compact(
		preparation,
		model,
		"key",
		undefined,
		input.customInstructions,
		undefined,
		input.thinkingLevel,
		streamFn,
	);
	add(`compaction/${name}`, "compact", input, { calls, result: JSON.parse(JSON.stringify({ ...result, usage: undefined })) });
}
await compactCase("compact", { history: true, read: ["src/app.ts", "README.md"], edited: ["src/app.ts"], written: ["new.ts"] });
await compactCase("compact_split", { history: true, split: true, customInstructions: "Keep error text.", read: ["x.ts"] });
await compactCase("compact_split_prefix_only", { split: true, previousSummary: "Prior summary." });

const entries = messages.map((message, i) => ({
	type: "message",
	id: `e${i}`,
	parentId: i === 0 ? null : `e${i - 1}`,
	timestamp: "2026-01-15T12:00:00.000Z",
	message,
}));
entries.push({
	type: "branch_summary",
	id: "bs",
	parentId: "e14",
	timestamp: "2026-01-15T12:00:00.000Z",
	fromId: "x",
	summary: "Nested branch summary.",
});
async function branch(name, input) {
	const { calls, streamFn } = recorder("BRANCH");
	const result = await compaction.generateBranchSummary(input.empty ? [] : entries, {
		model,
		apiKey: "key",
		signal: new AbortController().signal,
		customInstructions: input.customInstructions,
		replaceInstructions: input.replaceInstructions,
		streamFn,
	});
	add(`branch_summary/${name}`, "branch_summary", input, {
		calls,
		result: JSON.parse(JSON.stringify({ ...result, usage: undefined })),
	});
}
await branch("default", {});
await branch("custom", { customInstructions: "Mention the tests." });
await branch("replace", { customInstructions: "Only list files.", replaceInstructions: true });
await branch("empty", { empty: true });

// ---------------------------------------------------------------------------------------------
// CLI help

function captureHelp(flags) {
	const lines = [];
	const log = console.log;
	console.log = (...args) => lines.push(args.join(" "));
	try {
		printHelp(flags);
	} finally {
		console.log = log;
	}
	return lines.join("\n");
}
add("help/default", "help", { flags: null }, { text: captureHelp(undefined) });
const flags = [
	{ name: "plan", type: "boolean", description: "Start in plan mode", extensionPath: "/ext/plan.ts" },
	{ name: "profile", type: "string", extensionPath: "/ext/profile.ts" },
];
add("help/extension_flags", "help", { flags }, { text: captureHelp(flags) });

// ---------------------------------------------------------------------------------------------
// HTML export (the page is large: compare its SHA-256)

const { exportFromFile } = await ref("coding-agent/src/core/export-html/index.ts");
const { createHash } = await import("node:crypto");
const os = await import("node:os");
const sessionLines = [
	{ type: "session", version: 3, id: "019a1b2c-3d4e-7f60-8a9b-0c1d2e3f4a5b", timestamp: "2026-01-15T12:00:00.000Z", cwd: CWD },
	...entries.map((entry) => ({ ...entry, futureField: entry.id === "e1" ? { kept: true } : undefined })),
	{ type: "future_kind", id: "fk", parentId: "bs", timestamp: "2026-01-15T12:00:00.000Z", payload: "<b>&</b>" },
];
for (const theme of ["dark", "light"]) {
	const tmp = fs.mkdtempSync(path.join(os.tmpdir(), "pi-golden-export-"));
	const input = path.join(tmp, "session.jsonl");
	fs.writeFileSync(input, sessionLines.map((l) => `${JSON.stringify(l)}\n`).join(""));
	// The export templates resolve under the package directory: use the reference's own.
	delete process.env.PI_PACKAGE_DIR;
	const output = await exportFromFile(input, { outputPath: path.join(tmp, "out.html"), themeName: theme });
	process.env.PI_PACKAGE_DIR = PACKAGE_DIR;
	const html = fs.readFileSync(output);
	add(`export_html/${theme}`, "export_html", { lines: sessionLines, themeName: theme }, {
		sha256: createHash("sha256").update(html).digest("hex"),
		length: html.length,
	});
	fs.rmSync(tmp, { recursive: true, force: true });
}

// ---------------------------------------------------------------------------------------------

fs.mkdirSync(OUT_DIR, { recursive: true });
fs.writeFileSync(path.join(OUT_DIR, "prompts.json"), `${JSON.stringify({ cases }, null, 1)}\n`);
console.log(`prompts.json: ${cases.length} cases`);
