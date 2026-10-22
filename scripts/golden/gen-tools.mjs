// Golden renders of tool executions (ToolExecutionComponent with the built-in renderers) and of
// the diff renderer.
import { LONG_OUTPUT, PY_FILE, TS_FILE } from "./corpus.mjs";
import { CWD, fakeTui, Golden, ref } from "./common.mjs";

const { ToolExecutionComponent } = await ref("coding-agent/src/modes/interactive/components/tool-execution.ts");
const { withBuiltInRenderers } = await ref("coding-agent/src/core/tools/renderers/index.ts");
const { generateDiffString } = await ref("coding-agent/src/core/tools/edit-diff.ts");
const { renderDiff } = await ref("coding-agent/src/modes/interactive/components/diff.ts");

const g = new Golden("tools.json");

/**
 * input: { toolName, args, definition: "builtin" | "none" | "bare", options, steps }
 * steps: "argsComplete" | "expand" | "collapse" | { result, partial } | { args }
 */
const buildTool = (input) => {
	let definition;
	if (input.definition === "builtin") definition = withBuiltInRenderers(input.toolName, undefined);
	else if (input.definition === "bare") definition = withBuiltInRenderers(input.toolName, {});
	const c = new ToolExecutionComponent(
		input.toolName,
		"call-1",
		input.args,
		input.options ?? {},
		definition,
		fakeTui(),
		CWD,
	);
	for (const step of input.steps ?? []) {
		if (step === "argsComplete") c.setArgsComplete();
		else if (step === "expand") c.setExpanded(true);
		else if (step === "collapse") c.setExpanded(false);
		else if (step.args) c.updateArgs(step.args);
		else if (step.result) c.updateResult(step.result, step.partial ?? false);
	}
	return c;
};

const text = (t) => [{ type: "text", text: t }];
const ok = (t, details, extra = {}) => ({ result: { content: text(t), details, isError: false, ...extra } });
const err = (t, details) => ({ result: { content: text(t), details, isError: true } });
const partial = (t, details) => ({ result: { content: t === undefined ? [] : text(t), details, isError: false }, partial: true });

function tool(name, toolName, args, steps, extra = {}) {
	g.add(`${toolName}/${name}`, "tool_execution", { toolName, args, definition: "builtin", steps, ...extra }, buildTool);
}

const truncLines = {
	content: "",
	truncated: true,
	truncatedBy: "lines",
	totalLines: 4000,
	totalBytes: 90000,
	outputLines: 2000,
	outputBytes: 45000,
	lastLinePartial: false,
	firstLineExceedsLimit: false,
	maxLines: 2000,
	maxBytes: 51200,
};
const truncBytes = { ...truncLines, truncatedBy: "bytes", outputLines: 812 };

// --- read ---------------------------------------------------------------------------------------
tool("pending", "read", { path: "src/config.ts" }, []);
tool("pending-range", "read", { path: "src/config.ts", offset: 10, limit: 20 }, []);
tool("pending-offset-only", "read", { path: "src/config.ts", offset: "5" }, []);
tool("home-path", "read", { path: "/home/golden/notes/todo.md" }, ["argsComplete", ok("todo")]);
tool("collapsed-success", "read", { path: "src/config.ts" }, ["argsComplete", ok(TS_FILE)]);
tool("expanded-ts", "read", { path: "src/config.ts" }, ["argsComplete", ok(TS_FILE), "expand"]);
tool("expanded-py", "read", { path: "main.py" }, ["argsComplete", ok(PY_FILE), "expand"]);
tool("expanded-plain", "read", { path: "notes.txt" }, ["argsComplete", ok("plain\ttabbed\nsecond line\n\n\n"), "expand"]);
tool("expanded-long", "read", { path: "log.txt" }, ["argsComplete", ok(LONG_OUTPUT), "expand"]);
tool("error", "read", { path: "missing.ts" }, ["argsComplete", err("ENOENT: no such file or directory, open '/home/golden/project/missing.ts'")]);
tool(
	"truncated-lines",
	"read",
	{ path: "big.ts" },
	["argsComplete", ok("const a = 1;\nconst b = 2;", { truncation: truncLines }), "expand"],
);
tool(
	"truncated-bytes",
	"read",
	{ path: "big.txt" },
	["argsComplete", ok("aaa", { truncation: truncBytes }), "expand"],
);
tool(
	"first-line-exceeds",
	"read",
	{ path: "min.js" },
	["argsComplete", ok("", { truncation: { ...truncBytes, firstLineExceedsLimit: true } }), "expand"],
);
tool("agents-md-compact", "read", { path: "AGENTS.md" }, ["argsComplete", ok("# Agents")]);
tool("skill-compact", "read", { path: "/home/golden/.pi/agent/skills/pdf/SKILL.md", offset: 1, limit: 40 }, ["argsComplete", ok("skill")]);
tool("invalid-path", "read", { path: 42 }, []);
tool("legacy-file-path", "read", { file_path: "README.md" }, []);
tool(
	"image-result",
	"read",
	{ path: "logo.png" },
	[
		"argsComplete",
		{
			result: {
				content: [
					{ type: "text", text: "Read image file [image/png]" },
					{ type: "image", data: "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mNk+M9QDwADhgGAWjR9awAAAABJRU5ErkJggg==", mimeType: "image/png" },
				],
				isError: false,
			},
		},
		"expand",
	],
);
g.add(
	"read/hyperlinks",
	"tool_execution",
	{ toolName: "read", args: { path: "src/a b.ts" }, definition: "builtin", steps: [] },
	buildTool,
	{ caps: { hyperlinks: true } },
);
tool("outputpad-0", "read", { path: "src/config.ts" }, ["argsComplete", ok(TS_FILE), "expand"], { options: { outputPad: 0 } });

// --- bash ---------------------------------------------------------------------------------------
tool("pending-empty-command", "bash", {}, []);
tool("pending", "bash", { command: "npm test" }, ["argsComplete"]);
tool("timeout", "bash", { command: "sleep 100", timeout: 30 }, ["argsComplete"]);
tool("partial-output", "bash", { command: "npm test" }, ["argsComplete", partial("PASS a.test.ts\nPASS b.test.ts")]);
tool("success", "bash", { command: "ls" }, ["argsComplete", ok("README.md\nsrc\npackage.json")]);
tool("success-duration", "bash", { command: "make" }, ["argsComplete", ok("done", undefined, { durationMs: 4200 })]);
tool("long-duration", "bash", { command: "make all" }, ["argsComplete", ok("built", undefined, { durationMs: 7384900 })]);
tool("long-collapsed", "bash", { command: "seq 30" }, ["argsComplete", ok(LONG_OUTPUT)]);
tool("long-expanded", "bash", { command: "seq 30" }, ["argsComplete", ok(LONG_OUTPUT), "expand"]);
tool("wide-collapsed", "bash", { command: "cat wide" }, ["argsComplete", ok(`${"a".repeat(130)}\n${"b".repeat(90)}\nend`)]);
tool("error", "bash", { command: "false" }, ["argsComplete", err("oops\n\nCommand exited with code 1")]);
tool("empty-output", "bash", { command: "true" }, ["argsComplete", ok("")]);
tool("ansi-output", "bash", { command: "color" }, ["argsComplete", ok("\x1b[32mgreen\x1b[0m and \x1b[1mbold\x1b[0m\r\nnext")]);
tool(
	"truncated-full-output",
	"bash",
	{ command: "generate" },
	[
		"argsComplete",
		ok("line-3999\nline-4000\n\n[Showing lines 2001-4000 of 4000. Full output: /tmp/pi-bash-abc.log]", {
			truncation: truncLines,
			fullOutputPath: "/tmp/pi-bash-abc.log",
		}),
	],
);
tool(
	"truncated-bytes",
	"bash",
	{ command: "generate" },
	["argsComplete", ok("tail", { truncation: truncBytes }), "expand"],
);
tool("multiline-command", "bash", { command: "for i in 1 2 3; do\n  echo $i\ndone" }, ["argsComplete", ok("1\n2\n3")]);
tool("invalid-command", "bash", { command: ["ls"] }, []);
tool("outputpad-2", "bash", { command: "echo hi" }, ["argsComplete", ok("hi")], { options: { outputPad: 2 } });

// --- edit ---------------------------------------------------------------------------------------
const before = "function greet(name) {\n\tconsole.log('Hello ' + name);\n\treturn true;\n}\n\nmodule.exports = greet;\n";
const after = "function greet(name, greeting = 'Hello') {\n\tconsole.log(`${greeting} ${name}`);\n\treturn true;\n}\n\nmodule.exports = greet;\n";
const diff1 = generateDiffString(before, after);
const longBefore = Array.from({ length: 40 }, (_, i) => `line ${i + 1}`).join("\n");
const longAfter = longBefore.replace("line 5\n", "line five\n").replace("line 30\n", "line 30\ninserted a\ninserted b\n").replace("line 38\n", "");
const diff2 = generateDiffString(longBefore, longAfter);
const editArgs = { path: "src/greet.js", edits: [{ oldText: "a", newText: "b" }] };
tool("pending", "edit", editArgs, []);
tool("success", "edit", editArgs, [ok("Successfully replaced 1 block(s) in src/greet.js.", { diff: diff1.diff, firstChangedLine: diff1.firstChangedLine })]);
tool("success-multi-hunk", "edit", { path: "lines.txt", edits: [{ oldText: "x", newText: "y" }] }, [ok("ok", { diff: diff2.diff, firstChangedLine: diff2.firstChangedLine })]);
tool("error", "edit", editArgs, [err("Could not find the exact text in src/greet.js. The old text must match exactly including all whitespace and newlines.")]);
tool("legacy-args", "edit", { file_path: "a.txt", oldText: "old", newText: "new" }, [ok("ok", { diff: generateDiffString("old\n", "new\n").diff, firstChangedLine: 1 })]);
tool("no-path", "edit", {}, []);
tool("outputpad-0", "edit", editArgs, [ok("ok", { diff: diff1.diff, firstChangedLine: 1 })], { options: { outputPad: 0 } });

// --- write --------------------------------------------------------------------------------------
tool("streaming-partial", "write", { path: "src/config.ts", content: TS_FILE.slice(0, 120) }, []);
tool("streaming-steps", "write", { path: "src/config.ts", content: "" }, [
	{ args: { path: "src/config.ts", content: "import" } },
	{ args: { path: "src/config.ts", content: "import { readFile }" } },
	{ args: { path: "src/config.ts", content: TS_FILE.slice(0, 80) } },
]);
tool("complete-collapsed", "write", { path: "src/config.ts", content: TS_FILE }, ["argsComplete", ok("Successfully wrote 400 bytes")]);
tool("complete-expanded", "write", { path: "src/config.ts", content: TS_FILE }, ["argsComplete", ok("ok"), "expand"]);
tool("plain-text", "write", { path: "notes.txt", content: "first\n\tsecond\r\nthird\n\n" }, ["argsComplete", ok("ok")]);
tool("python", "write", { path: "main.py", content: PY_FILE }, ["argsComplete", ok("ok")]);
tool("error", "write", { path: "/etc/passwd", content: "x" }, ["argsComplete", err("EACCES: permission denied")]);
tool("invalid-content", "write", { path: "a.ts", content: 5 }, ["argsComplete"]);
tool("empty-content", "write", { path: "empty.ts", content: "" }, ["argsComplete", ok("ok")]);

// --- grep / find / ls ---------------------------------------------------------------------------
const grepOut = Array.from({ length: 22 }, (_, i) => `src/file${i}.ts:${i + 3}: const value${i} = compute(${i});`).join("\n");
tool("pending", "grep", { pattern: "TODO", path: "src", glob: "*.ts", limit: 50 }, []);
tool("default-path", "grep", { pattern: "fn main" }, [ok("src/main.rs:1: fn main() {")]);
tool("many-collapsed", "grep", { pattern: "value" }, [ok(grepOut)]);
tool("many-expanded", "grep", { pattern: "value" }, [ok(grepOut), "expand"]);
tool("truncated", "grep", { pattern: "x" }, [ok("a.ts:1: x", { matchLimitReached: 100, truncation: truncBytes, linesTruncated: true })]);
tool("no-matches", "grep", { pattern: "nothing" }, [ok("No matches found")]);
tool("error", "grep", { pattern: "(" }, [err("regex parse error: unclosed group")]);
tool("invalid-pattern", "grep", { pattern: 7, path: null }, []);

const findOut = Array.from({ length: 25 }, (_, i) => `src/components/component-${i}.tsx`).join("\n");
tool("pending", "find", { pattern: "*.tsx", path: "src", limit: 100 }, []);
tool("results", "find", { pattern: "*.tsx" }, [ok("src/a.tsx\nsrc/b.tsx")]);
tool("many-collapsed", "find", { pattern: "*.tsx" }, [ok(findOut)]);
tool("many-expanded", "find", { pattern: "*.tsx" }, [ok(findOut), "expand"]);
tool("truncated", "find", { pattern: "*" }, [ok("a\nb", { resultLimitReached: 1000, truncation: truncBytes })]);
tool("home-path", "find", { pattern: "*.md", path: "/home/golden/docs" }, [ok("/home/golden/docs/a.md")]);

const lsOut = Array.from({ length: 26 }, (_, i) => (i % 3 === 0 ? `dir${i}/` : `file${i}.txt`)).join("\n");
tool("pending", "ls", {}, []);
tool("pending-limit", "ls", { path: "src", limit: 10 }, []);
tool("results", "ls", { path: "." }, [ok(".git/\nREADME.md\nsrc/")]);
tool("many-collapsed", "ls", { path: "." }, [ok(lsOut)]);
tool("many-expanded", "ls", { path: "." }, [ok(lsOut), "expand"]);
tool("truncated", "ls", { path: "." }, [ok("a", { entryLimitReached: 500, truncation: truncBytes })]);
tool("error", "ls", { path: "nope" }, [err("Path not found: nope")]);

// --- generic / unknown tools --------------------------------------------------------------------
g.add(
	"generic/no-definition",
	"tool_execution",
	{ toolName: "mcp_search", args: { query: "rust tui", limit: 5 }, definition: "none", steps: [ok("result one\nresult two")] },
	buildTool,
);
g.add(
	"generic/no-definition-pending",
	"tool_execution",
	{ toolName: "mcp_search", args: { query: "rust tui" }, definition: "none", steps: [] },
	buildTool,
);
g.add(
	"generic/no-definition-error",
	"tool_execution",
	{ toolName: "mcp_search", args: {}, definition: "none", steps: [err("Tool mcp_search not found")] },
	buildTool,
);
g.add(
	"generic/bare-collapsed",
	"tool_execution",
	{
		toolName: "deploy",
		args: { target: "production", force: true, regions: ["eu-west-1", "us-east-1"], note: "a long note ".repeat(10) },
		definition: "bare",
		steps: [ok(LONG_OUTPUT)],
	},
	buildTool,
);
g.add(
	"generic/bare-expanded",
	"tool_execution",
	{
		toolName: "deploy",
		args: { target: "production", script: "line1\n\tline2", nested: { a: [1, 2] } },
		definition: "bare",
		steps: [ok(LONG_OUTPUT), "expand"],
	},
	buildTool,
);
g.add(
	"generic/bare-no-args",
	"tool_execution",
	{ toolName: "ping", args: {}, definition: "bare", steps: [ok("pong")] },
	buildTool,
);
g.add(
	"generic/bare-image",
	"tool_execution",
	{
		toolName: "screenshot",
		args: {},
		definition: "bare",
		steps: [{ result: { content: [{ type: "image", data: "AAAA", mimeType: "image/jpeg" }], isError: false } }],
	},
	buildTool,
);

// --- diff renderer ------------------------------------------------------------------------------
const buildDiff = (input) => () => renderDiff(input.diff, input.filePath ? { filePath: input.filePath } : undefined).split("\n");
g.add("diff/simple", "render_diff", { diff: diff1.diff }, buildDiff, { widths: [80] });
g.add("diff/multi-hunk", "render_diff", { diff: diff2.diff }, buildDiff, { widths: [80] });
g.add(
	"diff/word-level",
	"render_diff",
	{ diff: generateDiffString("const a = foo(bar, baz);\n", "const a = foo(qux, baz);\n").diff },
	buildDiff,
	{ widths: [80] },
);
g.add(
	"diff/multiple-changed-lines",
	"render_diff",
	{ diff: generateDiffString("a\nb\nc\nd\n", "a\nB\nC\nd\n").diff },
	buildDiff,
	{ widths: [80] },
);
g.add(
	"diff/tabs",
	"render_diff",
	{ diff: generateDiffString("\tindented\nx\n", "\t\tindented more\nx\n").diff, filePath: "a.go" },
	buildDiff,
	{ widths: [80] },
);

g.write();
process.exit(0);
