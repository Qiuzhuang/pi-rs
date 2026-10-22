// Golden renders of the chat message components.
import { assistant, LONG_OUTPUT, MARKDOWN, THINKING } from "./corpus.mjs";
import { fakeTui, Golden, ref } from "./common.mjs";

const C = "coding-agent/src/modes/interactive/components";
const { AssistantMessageComponent } = await ref(`${C}/assistant-message.ts`);
const { UserMessageComponent } = await ref(`${C}/user-message.ts`);
const { BashExecutionComponent } = await ref(`${C}/bash-execution.ts`);
const { CustomMessageComponent } = await ref(`${C}/custom-message.ts`);
const { BranchSummaryMessageComponent } = await ref(`${C}/branch-summary-message.ts`);
const { CompactionSummaryMessageComponent } = await ref(`${C}/compaction-summary-message.ts`);
const { SkillInvocationMessageComponent } = await ref(`${C}/skill-invocation-message.ts`);

const g = new Golden("messages.json");

// --- assistant messages -------------------------------------------------------------------------
const buildAssistant = (input) => {
	const c = new AssistantMessageComponent(
		undefined,
		input.hideThinking ?? false,
		undefined,
		input.hiddenLabel ?? "Thinking...",
		input.outputPad ?? 1,
	);
	c.updateContent(input.message, input.streaming ?? false);
	return c;
};

for (const [key, text] of Object.entries(MARKDOWN)) {
	g.add(`assistant/${key}`, "assistant_message", { message: assistant([{ type: "text", text }]) }, buildAssistant);
}
g.add(
	"assistant/thinking+text",
	"assistant_message",
	{
		message: assistant([
			{ type: "thinking", thinking: THINKING.long },
			{ type: "text", text: MARKDOWN.paragraphs },
		]),
	},
	buildAssistant,
);
g.add(
	"assistant/thinking-hidden",
	"assistant_message",
	{
		hideThinking: true,
		message: assistant([
			{ type: "thinking", thinking: THINKING.short },
			{ type: "text", text: MARKDOWN.plain },
		]),
	},
	buildAssistant,
);
g.add(
	"assistant/thinking-hidden-custom-label",
	"assistant_message",
	{
		hideThinking: true,
		hiddenLabel: "Pondering…",
		message: assistant([{ type: "thinking", thinking: THINKING.short }]),
	},
	buildAssistant,
);
g.add(
	"assistant/thinking-runs",
	"assistant_message",
	{
		message: assistant([
			{ type: "thinking", thinking: "first block" },
			{ type: "thinking", thinking: "second block" },
			{ type: "text", text: "answer" },
			{ type: "thinking", thinking: "  " },
			{ type: "thinking", thinking: "late thought" },
		]),
	},
	buildAssistant,
);
g.add(
	"assistant/thinking-then-toolcall",
	"assistant_message",
	{
		message: assistant(
			[
				{ type: "thinking", thinking: THINKING.short },
				{ type: "toolCall", id: "call_1", name: "read", arguments: { path: "README.md" } },
			],
			{ stopReason: "toolUse" },
		),
	},
	buildAssistant,
);
g.add(
	"assistant/text-then-toolcall",
	"assistant_message",
	{
		message: assistant(
			[
				{ type: "text", text: "Let me check." },
				{ type: "toolCall", id: "call_1", name: "bash", arguments: { command: "ls" } },
			],
			{ stopReason: "toolUse" },
		),
	},
	buildAssistant,
);
g.add(
	"assistant/error",
	"assistant_message",
	{
		message: assistant([{ type: "text", text: "Partial answer" }], {
			stopReason: "error",
			errorMessage: "429 rate_limit_error: Too many requests",
		}),
	},
	buildAssistant,
);
g.add(
	"assistant/error-empty",
	"assistant_message",
	{ message: assistant([], { stopReason: "error" }) },
	buildAssistant,
);
g.add(
	"assistant/aborted",
	"assistant_message",
	{ message: assistant([{ type: "text", text: "I was saying" }], { stopReason: "aborted" }) },
	buildAssistant,
);
g.add(
	"assistant/aborted-custom",
	"assistant_message",
	{ message: assistant([], { stopReason: "aborted", errorMessage: "Interrupted by user" }) },
	buildAssistant,
);
g.add(
	"assistant/length",
	"assistant_message",
	{
		message: assistant(
			[
				{ type: "text", text: "Truncated text" },
				{ type: "toolCall", id: "c", name: "write", arguments: {} },
			],
			{ stopReason: "length" },
		),
	},
	buildAssistant,
);
g.add(
	"assistant/streaming-partial-code",
	"assistant_message",
	{ streaming: true, message: assistant([{ type: "text", text: "Working on it:\n\n```ts\nconst x = 1;\n" }]) },
	buildAssistant,
);
g.add(
	"assistant/outputpad-0",
	"assistant_message",
	{ outputPad: 0, message: assistant([{ type: "text", text: MARKDOWN.lists }]) },
	buildAssistant,
);
g.add(
	"assistant/outputpad-3",
	"assistant_message",
	{
		outputPad: 3,
		message: assistant([
			{ type: "thinking", thinking: THINKING.short },
			{ type: "text", text: MARKDOWN.code },
		]),
	},
	buildAssistant,
);
g.add("assistant/empty", "assistant_message", { message: assistant([]) }, buildAssistant);

// --- user messages ------------------------------------------------------------------------------
const buildUser = (input) => new UserMessageComponent(input.text, undefined, input.outputPad ?? 1);
const userTexts = {
	plain: "fix the failing test in src/parser.ts",
	multiline: "Please do the following:\n1. read the file\n2. fix the bug\n\nThanks!",
	markdown: "Can you explain `foo()` and **why** it fails?\n\n```js\nfoo(1, 2)\n```",
	escapes: "1\\. keep this \\*literal\\* text",
	long: "This is a very long user prompt that goes on and on to make sure the wrapping inside the user message background works at all widths, including the narrow 40 column one.",
	unicode: "日本語のテキスト and emoji 👍 mixed",
	table: MARKDOWN.table,
	empty: "",
};
for (const [key, text] of Object.entries(userTexts)) {
	g.add(`user/${key}`, "user_message", { text }, buildUser);
}
g.add("user/outputpad-0", "user_message", { text: userTexts.multiline, outputPad: 0 }, buildUser);
g.add("user/outputpad-2", "user_message", { text: userTexts.markdown, outputPad: 2 }, buildUser);

// --- bash execution (user `!` commands) ---------------------------------------------------------
const buildBash = (input) => {
	const c = new BashExecutionComponent(input.command, fakeTui(), input.excludeFromContext ?? false, input.outputPad ?? 1);
	for (const chunk of input.chunks ?? []) c.appendOutput(chunk);
	if (input.complete) {
		const { exitCode, cancelled, truncation, fullOutputPath } = input.complete;
		c.setComplete(exitCode ?? undefined, cancelled ?? false, truncation, fullOutputPath);
	}
	if (input.expanded) c.setExpanded(true);
	return c;
};
g.add("bash/running", "bash_execution", { command: "npm test", chunks: ["running tests...\n"] }, buildBash);
g.add("bash/running-empty", "bash_execution", { command: "sleep 5" }, buildBash);
g.add(
	"bash/complete",
	"bash_execution",
	{ command: "ls -la", chunks: ["total 8\n", "drwxr-xr-x  2 user user 4096 a\n-rw-r--r--  1 user user  120 b.txt\n"], complete: { exitCode: 0 } },
	buildBash,
);
g.add(
	"bash/error-exit",
	"bash_execution",
	{ command: "false", chunks: ["something failed\n"], complete: { exitCode: 2 } },
	buildBash,
);
g.add("bash/cancelled", "bash_execution", { command: "yes", chunks: ["y\ny\ny"], complete: { exitCode: undefined, cancelled: true } }, buildBash);
g.add("bash/long-collapsed", "bash_execution", { command: "seq", chunks: [LONG_OUTPUT], complete: { exitCode: 0 } }, buildBash);
g.add(
	"bash/long-expanded",
	"bash_execution",
	{ command: "seq", chunks: [LONG_OUTPUT], complete: { exitCode: 0 }, expanded: true },
	buildBash,
);
g.add(
	"bash/wide-lines",
	"bash_execution",
	{ command: "cat wide.txt", chunks: [`${"w".repeat(150)}\nshort\n${"v".repeat(95)}`], complete: { exitCode: 0 } },
	buildBash,
);
g.add(
	"bash/ansi-crlf",
	"bash_execution",
	{ command: "colors", chunks: ["\x1b[31mred\x1b[0m line\r\n", "next\rover\n"], complete: { exitCode: 0 } },
	buildBash,
);
g.add(
	"bash/excluded",
	"bash_execution",
	{ command: "git status", excludeFromContext: true, chunks: ["On branch main\n"], complete: { exitCode: 0 } },
	buildBash,
);
g.add(
	"bash/truncated-full-output",
	"bash_execution",
	{
		command: "big",
		chunks: ["tail of output\n"],
		complete: {
			exitCode: 0,
			truncation: {
				content: "tail of output",
				truncated: true,
				truncatedBy: "lines",
				totalLines: 5000,
				totalBytes: 100000,
				outputLines: 2000,
				outputBytes: 40000,
				lastLinePartial: false,
				firstLineExceedsLimit: false,
				maxLines: 2000,
				maxBytes: 51200,
			},
			fullOutputPath: "/tmp/pi-bash-1234.log",
		},
	},
	buildBash,
);
g.add("bash/outputpad-0", "bash_execution", { command: "echo hi", chunks: ["hi\n"], complete: { exitCode: 0 }, outputPad: 0 }, buildBash);

// --- custom / summary / skill messages ----------------------------------------------------------
const buildCustom = (input) => {
	const c = new CustomMessageComponent(input.message, undefined, undefined, input.outputPad ?? 1);
	if (input.expanded) c.setExpanded(true);
	return c;
};
g.add(
	"custom/string",
	"custom_message",
	{ message: { role: "custom", customType: "reminder", content: "Remember to **run the tests**.", display: true, timestamp: 0 } },
	buildCustom,
);
g.add(
	"custom/blocks",
	"custom_message",
	{
		message: {
			role: "custom",
			customType: "status-report",
			content: [
				{ type: "text", text: "First block" },
				{ type: "image", data: "AAAA", mimeType: "image/png" },
				{ type: "text", text: MARKDOWN.lists },
			],
			display: true,
			timestamp: 0,
		},
	},
	buildCustom,
);

const buildBranch = (input) => {
	const c = new BranchSummaryMessageComponent(input.message, undefined, input.outputPad ?? 1);
	if (input.expanded) c.setExpanded(true);
	return c;
};
const branchMessage = {
	role: "branchSummary",
	summary: "Explored an alternative approach using `serde_json::Value`.\n\n- tried A\n- reverted B",
	fromId: "abc123",
	timestamp: 0,
};
g.add("branch/collapsed", "branch_summary", { message: branchMessage }, buildBranch);
g.add("branch/expanded", "branch_summary", { message: branchMessage, expanded: true }, buildBranch);

const buildCompaction = (input) => {
	const c = new CompactionSummaryMessageComponent(input.message, undefined, input.outputPad ?? 1);
	if (input.expanded) c.setExpanded(true);
	return c;
};
const compactionMessage = {
	role: "compactionSummary",
	summary: "## Goal\n\nPort the parser.\n\n## Progress\n\n1. Read files\n2. Wrote tests",
	tokensBefore: 123456,
	timestamp: 0,
};
g.add("compaction/collapsed", "compaction_summary", { message: compactionMessage }, buildCompaction);
g.add("compaction/expanded", "compaction_summary", { message: compactionMessage, expanded: true }, buildCompaction);
g.add("compaction/outputpad-0", "compaction_summary", { message: compactionMessage, outputPad: 0 }, buildCompaction);

const buildSkill = (input) => {
	const c = new SkillInvocationMessageComponent(input.block, undefined, input.outputPad ?? 1);
	if (input.expanded) c.setExpanded(true);
	return c;
};
const skillBlock = {
	name: "pdf-tools",
	location: "/home/golden/.pi/agent/skills/pdf-tools/SKILL.md",
	content: "# PDF tools\n\nUse `pdftotext` to extract text.\n\n```bash\npdftotext in.pdf out.txt\n```",
	userMessage: "extract the text",
};
g.add("skill/collapsed", "skill_invocation", { block: skillBlock }, buildSkill);
g.add("skill/expanded", "skill_invocation", { block: skillBlock, expanded: true }, buildSkill);

g.write();
process.exit(0);
