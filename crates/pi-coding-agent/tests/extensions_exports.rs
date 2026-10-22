//! Runtime exports of `@earendil-works/pi-coding-agent` that extensions call directly, backed by
//! the Rust ports: compaction and session-tree helpers, skills, diffs, shell config, CLI parsing,
//! image helpers and the event bus.

mod extensions_support;

use extensions_support::*;
use serde_json::{Value, json};

const EXTENSION: &str = r##"
import * as ca from "@earendil-works/pi-coding-agent";
import * as tui from "@earendil-works/pi-tui";
import { Agent, agentLoop, DEFAULT_AGENT_IDENTITY } from "@earendil-works/pi-agent-core";
import { registerFauxProvider, fauxAssistantMessage, fauxToolCall, Type } from "@earendil-works/pi-ai";

export default async function (pi) {
	const out = {};
	const user = (id, parentId, text) => ({
		type: "message", id, parentId, timestamp: "2026-01-01T00:00:00.000Z",
		message: { role: "user", content: text, timestamp: 1 },
	});
	const assistant = (id, parentId, text, total) => ({
		type: "message", id, parentId, timestamp: "2026-01-01T00:00:00.000Z",
		message: {
			role: "assistant", content: [{ type: "text", text }], api: "faux", provider: "faux", model: "m",
			usage: { input: 1, output: 2, cacheRead: 0, cacheWrite: 0, totalTokens: total,
				cost: { input: 0, output: 0, cacheRead: 0, cacheWrite: 0, total: 0 } },
			stopReason: "stop", timestamp: 2,
		},
	});
	const entries = [user("a", null, "hi"), assistant("b", "a", "hello", 42), user("c", "b", "again"), user("d", "a", "branch")];
	out.lastUsage = ca.getLastAssistantUsage(entries)?.totalTokens;
	out.contextTokens = ca.calculateContextTokens({ input: 3, output: 4, cacheRead: 1, cacheWrite: 0, totalTokens: 0, cost: { input: 0, output: 0, cacheRead: 0, cacheWrite: 0, total: 0 } });
	out.shouldCompact = ca.shouldCompact(9000, 10000, { enabled: true, reserveTokens: 2000, keepRecentTokens: 1 });
	out.contextLeaf = ca.buildSessionContext(entries, "c").messages.map((m) => m.role);
	out.contextNull = ca.buildSessionContext(entries, null).messages.length;
	out.contextDefault = ca.buildSessionContext(entries).messages.length;
	out.turnStart = ca.findTurnStartIndex(entries, 1, 0);
	out.latestCompaction = ca.getLatestCompactionEntry(entries);
	const parsed = ca.parseSessionEntries('{"type":"session","id":"s","version":1,"timestamp":"t","cwd":"/"}\n' + JSON.stringify(entries[0]) + "\n");
	out.parsed = parsed.map((e) => e.type);
	const migrate = parsed.slice();
	ca.migrateSessionEntries(migrate);
	out.migratedVersion = migrate[0].version;
	const tree = { getBranch: (id) => { const path = []; for (let e = entries.find((x) => x.id === id); e; e = entries.find((x) => x.id === e.parentId)) path.unshift(e); return path; },
		getEntry: (id) => entries.find((e) => e.id === id) };
	const collected = ca.collectEntriesForBranchSummary(tree, "c", "d");
	out.collected = [collected.entries.map((e) => e.id), collected.commonAncestorId];
	out.patch = ca.generateUnifiedPatch("f.txt", "a\nb\n", "a\nc\n");
	out.visual = ca.truncateToVisualLines("1\n2\n3\n4", 2, 80).skippedCount;
	out.diff = ca.renderDiff("+1 added").includes("added");
	out.skills = ca.formatSkillsForPrompt([]);
	out.args = (() => { const a = ca.parseArgs(["--model", "x", "hello", "--zz"]); return [a.model, a.unknownFlags instanceof Map, a.messages]; })();
	out.shell = typeof ca.getShellConfig().shell;
	out.dimension = ca.formatDimensionNote({ data: "", mimeType: "image/png", originalWidth: 200, originalHeight: 100, width: 100, height: 50, wasResized: true });
	out.png = await ca.convertToPng("AAAA", "image/png");
	const bus = ca.createEventBus();
	const seen = [];
	const off = bus.on("ch", (d) => seen.push(d));
	bus.emit("ch", 1);
	off();
	bus.emit("ch", 2);
	await Promise.resolve();
	out.bus = seen;
	const provider = new tui.CombinedAutocompleteProvider([{ name: "model", description: "Pick" }, { name: "mode" }], process.cwd());
	const suggestions = await provider.getSuggestions(["/mo"], 0, 3, { signal: new AbortController().signal });
	const stdin = new tui.StdinBuffer({ timeout: 10 });
	const sequences = [];
	stdin.on("data", (seq) => sequences.push(seq));
	stdin.process("a\x1b[Ab");
	stdin.destroy?.();
	const km = new tui.KeybindingsManager(tui.TUI_KEYBINDINGS, { "tui.select.up": "k", "tui.select.down": "k" });
	out.theme = (() => {
		try {
			const theme = new ca.Theme({ accent: "#ff0000" }, {}, "truecolor", { name: "mine" });
			return theme.fg("accent", "x");
		} catch (e) {
			return "error: " + e.message;
		}
	})();
	out.tuiMore = {
		selectUp: tui.TUI_KEYBINDINGS["tui.select.up"].defaultKeys,
		km: [km.matches("k", "tui.select.up"), km.getConflicts().length, km.getResolvedBindings()["tui.select.up"]],
		latex: tui.renderLatex("x^2"),
		status: tui.formatProgramStatus({ state: "working" }),
		styled: tui.styleText("a", { bold: true }, "truecolor"),
		apple: tui.isAppleTerminalSession(),
	};
	out.tuiExtra = {
		marked: new tui.Marked().lexer("# Title\n\ntext").map((t) => t.type).filter((t) => t !== "space"),
		suggestions: suggestions?.items.map((i) => i.value),
		sequences,
	};
	out.tui = {
		hex: tui.colorToHex(tui.parseColor("#ff8800")),
		indexed: tui.parseColor(196),
		mixKind: tui.mixColors(tui.rgbColor(0, 0, 0), tui.rgbColor(255, 255, 255), 0.5, "srgb").kind,
		fg: tui.foregroundAnsi(tui.rgbColor(1, 2, 3), "truecolor"),
		png: tui.getPngDimensions("iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mNkYPhfDwAChwGA60e6kgAAAABJRU5ErkJggg=="),
		bad: tui.getPngDimensions("AAAA") === undefined,
		link: tui.hyperlink("x", "https://e.x").includes("\x1b]8;;https://e.x"),
		viewport: tui.isViewportTUI({}),
		rows: tui.calculateImageRows({ widthPx: 100, heightPx: 100 }, 10, { widthPx: 10, heightPx: 20 }),
		image: (() => {
			const png = "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mNkYPhfDwAChwGA60e6kgAAAABJRU5ErkJggg==";
			tui.setCapabilities({ images: "kitty", trueColor: true, hyperlinks: true });
			const image = new tui.Image(png, "image/png", { fallbackColor: (s) => s });
			const kitty = image.render(40);
			tui.setCapabilities({ images: null, trueColor: true, hyperlinks: true });
			const fallback = new tui.Image(png, "image/png", { fallbackColor: (s) => `<${s}>` }, { filename: "a.png" }).render(40);
			tui.resetCapabilitiesCache();
			return [kitty[0].startsWith("\x1b_G"), image.getImageId() !== undefined, fallback[0].startsWith("<[Image")];
		})(),
		invalid: (() => { try { tui.rgbColor(Infinity, 0, 0); return false; } catch { return true; } })(),
	};

	// pi-agent-core: the Agent runs a tool-calling turn against the faux provider through the
	// default stream function (pi-ai's compat `streamSimple`).
	const faux = registerFauxProvider();
	faux.setResponses([fauxAssistantMessage([fauxToolCall("echo", { text: "ping" })]), fauxAssistantMessage("done")]);
	const echo = {
		name: "echo", label: "Echo", description: "Echo text",
		parameters: Type.Object({ text: Type.String() }),
		execute: async (_id, params) => ({ content: [{ type: "text", text: `echo ${params.text}` }], details: {} }),
	};
	const agent = new Agent({ initialState: { model: faux.getModel(), systemPrompt: "Be brief.", tools: [echo] } });
	const events = [];
	agent.subscribe((e) => events.push(e.type));
	await agent.prompt("hi");
	out.agent = agent.state.messages.map((m) => m.role + ":" + (typeof m.content === "string" ? m.content : m.content.map((c) => c.text ?? c.name).join("")));
	out.agentEvents = [events[0], events.at(-1), events.includes("tool_execution_end")];
	out.identity = DEFAULT_AGENT_IDENTITY.attribution !== undefined && typeof agentLoop === "function";
	faux.unregister?.();
	pi.registerTool({
		name: "exports",
		label: "exports",
		description: JSON.stringify(out),
		parameters: { type: "object", properties: {} },
		execute: async () => ({ content: [] }),
	});
}
"##;

#[tokio::test(flavor = "multi_thread")]
async fn coding_agent_helpers_are_backed_by_rust() {
    let dir = ExtDir::new();
    dir.write("exports.ts", EXTENSION);
    let result = dir.discover().await;
    assert!(result.errors.is_empty(), "{:#?}", result.errors);
    let extension = result.extensions[0].read();
    let out: Value = serde_json::from_str(&extension.tools["exports"].definition.description).unwrap();

    assert_eq!(out["lastUsage"], json!(42));
    assert_eq!(out["contextTokens"], json!(8));
    assert_eq!(out["shouldCompact"], json!(true));
    assert_eq!(out["contextLeaf"], json!(["user", "assistant", "user"]));
    assert_eq!(out["contextNull"], json!(0));
    // Default leaf: the last entry ("d", a branch off "a").
    assert_eq!(out["contextDefault"], json!(2));
    assert_eq!(out["turnStart"], json!(0));
    assert_eq!(out["latestCompaction"], Value::Null);
    assert_eq!(out["parsed"], json!(["session", "message"]));
    assert_eq!(out["migratedVersion"], json!(3));
    assert_eq!(out["collected"], json!([["b", "c"], "a"]));
    assert!(out["patch"].as_str().unwrap().contains("-b\n+c"), "{}", out["patch"]);
    assert_eq!(out["visual"], json!(2));
    assert_eq!(out["diff"], json!(true));
    assert_eq!(out["skills"], json!(""));
    assert_eq!(out["args"], json!(["x", true, ["hello"]]));
    assert_eq!(out["shell"], json!("string"));
    assert_eq!(
        out["dimension"],
        json!("[Image: original 200x100, displayed at 100x50. Multiply coordinates by 2.00 to map to original image.]")
    );
    assert_eq!(out["png"], json!({ "data": "AAAA", "mimeType": "image/png" }));
    assert_eq!(out["bus"], json!([1]));
    // Built by the Rust theme, which validates token fallbacks like TS.
    assert_eq!(out["theme"], json!("error: Theme token scrollbarTrack has unresolved fallback muted"));
    assert_eq!(
        out["tuiMore"],
        json!({
            "selectUp": "up",
            "km": [true, 1, "k"],
            "latex": "x²",
            "status": "\u{1b}]7501;state=working\u{1b}\\",
            "styled": "\u{1b}[1ma\u{1b}[22m",
            "apple": false,
        })
    );
    assert_eq!(
        out["tuiExtra"],
        json!({ "marked": ["heading", "paragraph"], "suggestions": ["model", "mode"], "sequences": ["a", "\u{1b}[A", "b"] })
    );
    assert_eq!(
        out["tui"],
        json!({
            "hex": "#ff8800",
            "indexed": { "kind": "indexed", "index": 196 },
            "mixKind": "rgb",
            "fg": "\u{1b}[38;2;1;2;3m",
            "png": { "widthPx": 1, "heightPx": 1 },
            "bad": true,
            "link": true,
            "viewport": false,
            "rows": 5,
            "image": [true, true, true],
            "invalid": true,
        })
    );
    assert_eq!(
        out["agent"],
        json!(["system:Be brief.", "user:hi", "assistant:echo", "toolResult:echo ping", "assistant:done"]),
        "{}",
        out["agent"]
    );
    assert_eq!(out["agentEvents"], json!(["agent_start", "agent_end", true]));
    assert_eq!(out["identity"], json!(true));
}
