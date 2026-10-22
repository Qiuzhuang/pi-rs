// Golden captures of full InteractiveMode screens on a virtual terminal with the faux provider:
// the startup screen, and the screen after one streamed reply with a tool call.
//
// Each case records the live region the TUI renders (`ui.render(columns)`, with ANSI), then the text
// of the emulated terminal's whole buffer (scrollback and viewport, trailing blank lines dropped).
import fs from "node:fs";
import path from "node:path";
import { Golden, REF, ref } from "./common.mjs";

// Screens write session/settings state: keep it in a scratch agent dir. The cwd exists so tools can run.
export const SCREEN_ROOT = "/tmp/pi-golden-screens";
const AGENT_DIR = path.join(SCREEN_ROOT, "agent");
const CWD = path.join(SCREEN_ROOT, "project");
fs.rmSync(SCREEN_ROOT, { recursive: true, force: true });
fs.mkdirSync(AGENT_DIR, { recursive: true });
fs.mkdirSync(CWD, { recursive: true });
fs.writeFileSync(path.join(CWD, "README.md"), "# Golden project\n\nUsed by the golden screen tests.\n");
fs.writeFileSync(path.join(CWD, "main.ts"), 'console.log("hi");\n');
process.env.PI_CODING_AGENT_DIR = AGENT_DIR;
process.env.PI_SKIP_VERSION_CHECK = "1";

const { createHarness } = await import(`${REF}/packages/coding-agent/test/suite/harness.ts`);
const { VirtualTerminal } = await ref("tui/test/virtual-terminal.ts");
const { SessionManager } = await ref("coding-agent/src/core/session-manager.ts");
const { AgentSessionRuntime } = await ref("coding-agent/src/core/agent-session-runtime.ts");
const { InteractiveMode } = await ref("coding-agent/src/modes/interactive/interactive-mode.ts");
const { createLsTool } = await ref("coding-agent/src/core/tools/ls.ts");
const { fauxAssistantMessage, fauxToolCall, fauxText, fauxThinking } = await ref("ai/src/providers/faux.ts");

const g = new Golden("screens.json");

/**
 * The footer's input-token count and context percentage depend on the system prompt, which embeds
 * machine-specific paths (package docs, temp cwd). Both sides mask them.
 */
function normalizeUsage(line) {
	return line.replace(/↑[0-9.]+[kM]?/g, "↑#").replace(/[0-9.]+%\//g, "#%/");
}
const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));

async function waitFor(terminal, mode, predicate, what) {
	const deadline = Date.now() + 15000;
	for (;;) {
		mode.ui.requestRender();
		await sleep(50);
		const screen = terminal.getViewport().join("\n");
		if (predicate(screen)) return;
		if (Date.now() > deadline) throw new Error(`timed out waiting for ${what}:\n${screen}`);
	}
}

/**
 * input: { columns, rows, settings, steps: [{ responses } | { type } | { wait }] }
 * Output per width (only `columns`): the rendered tree lines, then a separator, then the viewport.
 */
async function buildScreen(input, themeName) {
	const harness = await createHarness({
		settings: { theme: themeName, ...input.settings },
		sessionManager: SessionManager.inMemory(CWD),
		tools: [createLsTool(CWD)],
	});
	const services = {
		cwd: CWD,
		agentDir: AGENT_DIR,
		modelRuntime: harness.session.modelRuntime,
		settingsManager: harness.settingsManager,
		resourceLoader: harness.session.resourceLoader,
		diagnostics: [],
	};
	const runtime = new AgentSessionRuntime(harness.session, services, async () => {
		throw new Error("session replacement is not supported in golden screens");
	});
	const terminal = new VirtualTerminal(input.columns, input.rows);
	const mode = new InteractiveMode(runtime, { terminal });
	await mode.init();
	await waitFor(terminal, mode, (s) => s.includes("─"), "the editor");
	let running;
	for (const step of input.steps ?? []) {
		if (step.responses) harness.setResponses(step.responses);
		if (step.type !== undefined) {
			if (!running) {
				running = mode.run().catch(() => {});
				// Let the main loop reach its first getUserInput().
				await sleep(200);
			}
			terminal.sendInput(step.type);
		}
		if (step.wait) await waitFor(terminal, mode, (s) => s.includes(step.wait), step.wait);
		if (step.idle) {
			const deadline = Date.now() + 15000;
			while (harness.session.isStreaming && Date.now() < deadline) await sleep(20);
		}
	}
	await sleep(100);
	mode.ui.requestRender();
	await terminal.flush?.();
	await sleep(50);
	const lines = mode.ui.render(input.columns);
	const buffer = terminal.getScrollBuffer().map((l) => l.trimEnd());
	while (buffer.length > 0 && buffer[buffer.length - 1] === "") buffer.pop();
	mode.stop?.();
	harness.cleanup();
	return () => [...lines, "\u0000terminal", ...buffer].map(normalizeUsage);
}

async function screen(name, input) {
	const out = {};
	for (const themeName of ["dark", "light"]) {
		out[themeName] = [(await buildScreen(input, themeName))()];
	}
	g.cases.push({ name, kind: "screen", input, widths: [input.columns], out });
}

const reply = [
	fauxAssistantMessage([fauxThinking("Let me look at the files."), fauxToolCall("ls", { path: "." }, { id: "call_ls" })], {
		stopReason: "toolUse",
		timestamp: 0,
	}),
	fauxAssistantMessage(
		[fauxText("The project has two files:\n\n| File | Purpose |\n|---|---|\n| `README.md` | docs |\n| `main.ts` | entry point |")],
		{ timestamp: 0 },
	),
];

await screen("startup/80x24", { columns: 80, rows: 24, settings: {} });
await screen("startup/120x30-quiet", { columns: 120, rows: 30, settings: { quietStartup: true } });
await screen("reply/80x30", {
	columns: 80,
	rows: 30,
	settings: { quietStartup: true },
	steps: [{ responses: reply }, { type: "list the files" }, { type: "\r" }, { wait: "entry point" }, { idle: true }],
});
await screen("reply/120x40", {
	columns: 120,
	rows: 40,
	settings: {},
	steps: [{ responses: reply }, { type: "list the files" }, { type: "\r" }, { wait: "entry point" }, { idle: true }],
});

await screen("startup/expanded-help", {
	columns: 100,
	rows: 40,
	settings: {},
	steps: [{ type: "\x0f" }, { wait: "to queue follow-up" }],
});
await screen("error-reply/80x24", {
	columns: 80,
	rows: 24,
	settings: { quietStartup: true },
	steps: [
		{ responses: [fauxAssistantMessage([], { stopReason: "error", errorMessage: "529 overloaded_error: Overloaded", timestamp: 0 })] },
		{ type: "hello" },
		{ type: "\r" },
		{ wait: "Overloaded" },
		{ idle: true },
	],
});
await screen("slash-autocomplete/80x24", {
	columns: 80,
	rows: 24,
	settings: { quietStartup: true },
	steps: [{ type: "/" }, { type: "m" }, { wait: "model" }],
});
await screen("bash-command/80x24", {
	columns: 80,
	rows: 24,
	settings: { quietStartup: true },
	steps: [{ type: "!" }, { type: "e" }, { type: "c" }, { type: "h" }, { type: "o" }, { type: " " }, { type: "h" }, { type: "i" }, { type: "\r" }, { wait: "$ echo hi" }, { idle: true }],
});
await screen("tools-expanded/100x40", {
	columns: 100,
	rows: 40,
	settings: { quietStartup: true },
	steps: [{ responses: reply }, { type: "list the files" }, { type: "\r" }, { wait: "entry point" }, { idle: true }, { type: "\x0f" }],
});

g.write();
fs.rmSync(SCREEN_ROOT, { recursive: true, force: true });
process.exit(0);
