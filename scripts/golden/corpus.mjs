// Shared inputs for the golden generators.

export const MARKDOWN = {
	plain: "Hello! I can help with that. Let me take a look at the repository first.",
	paragraphs:
		"First paragraph with **bold**, *italic*, `inline code` and ~~strike~~ text.\n\nSecond paragraph that is long enough to wrap at narrow widths because it keeps going and going with more words.",
	headings: "# Heading 1\n\n## Heading 2\n\n### Heading 3\n\n#### Heading 4\n\nBody text under the headings.",
	lists:
		"Steps:\n\n1. Read the file\n2. Edit the function\n   - keep the signature\n   - update callers\n3. Run the tests\n\n- bullet one\n- bullet two with `code`\n  - nested bullet\n\n- [ ] todo item\n- [x] done item",
	code: "Here is the fix:\n\n```ts\nexport function add(a: number, b: number): number {\n\t// sum two numbers\n\treturn a + b;\n}\n```\n\nAnd a shell command:\n\n```bash\nnpm run build && npm test -- --watch=false\n```",
	codeNoLang: "```\nplain fenced block\n  indented line\n```",
	rust: "```rust\nfn main() {\n    let v: Vec<u32> = (0..10).map(|x| x * 2).collect();\n    println!(\"{:?}\", v);\n}\n```",
	python: "```python\ndef greet(name: str) -> str:\n    \"\"\"Return a greeting.\"\"\"\n    return f\"Hello, {name}!\"\n\nprint(greet('pi'))\n```",
	json: '```json\n{\n  "name": "pi",\n  "version": "1.0.0",\n  "private": true,\n  "count": 42\n}\n```',
	table:
		"| File | Lines | Status |\n|------|------:|:------:|\n| src/main.rs | 120 | ok |\n| src/lib.rs | 2048 | changed |\n| README.md | 12 | new |",
	wideTable:
		"| Column A | Column B with a much longer header | C |\n|---|---|---|\n| short | a cell with quite a lot of text in it that will need wrapping at small widths | x |\n| 2 | `code cell` | **bold** |",
	quote: "> Note: this is a blockquote\n> spanning two lines.\n\n> Nested\n>> deeper quote",
	links: "See [the docs](https://example.com/docs) and <https://example.com/auto>. Plain URL https://example.com/plain.",
	hr: "Above\n\n---\n\nBelow",
	unicode: "Unicode: 你好世界, emoji 🎉🚀, accents café naïve, arrows → ←.",
	longWord: "A long token: aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa end.",
	mixed:
		"## Summary\n\nI updated **3 files**:\n\n| File | Change |\n|---|---|\n| `a.ts` | added `foo()` |\n| `b.ts` | removed dead code |\n\n```diff\n- old line\n+ new line\n```\n\n> All tests pass.\n\n1. Done\n2. Also done",
	html: "Some <b>inline html</b> and a <br> tag.\n\n<div>block html</div>",
	escapes: "Escaped \\*not italic\\* and 1\\. not a list, plus a backslash \\\\ here.",
	math: "Inline math $x^2 + y^2 = z^2$ and display:\n\n$$\\sum_{i=1}^{n} i = \\frac{n(n+1)}{2}$$",
};

export const THINKING = {
	short: "The user wants a summary. I should read the README first.",
	long: "Let me think about this carefully.\n\nFirst, the function `parse` is called with untrusted input, so it must validate the length. Second, the error path returns early without closing the file handle, which leaks a descriptor.\n\n- check bounds\n- close handle",
};

/** An assistant message with defaults filled in. */
export function assistant(content, extra = {}) {
	return {
		role: "assistant",
		content,
		api: "faux",
		provider: "faux",
		model: "faux-model",
		usage: {
			input: 0,
			output: 0,
			cacheRead: 0,
			cacheWrite: 0,
			totalTokens: 0,
			cost: { input: 0, output: 0, cacheRead: 0, cacheWrite: 0, total: 0 },
		},
		stopReason: "stop",
		timestamp: 0,
		...extra,
	};
}

export const TS_FILE = `import { readFile } from "node:fs/promises";

export interface Config {
\tname: string;
\tretries?: number;
}

/** Load the config from disk. */
export async function loadConfig(path: string): Promise<Config> {
\tconst raw = await readFile(path, "utf8");
\tconst parsed = JSON.parse(raw) as Config;
\treturn { retries: 3, ...parsed };
}
`;

export const PY_FILE = `import sys


def main(argv: list[str]) -> int:
    for arg in argv[1:]:
        print(f"arg: {arg}")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
`;

export const LONG_OUTPUT = Array.from({ length: 30 }, (_, i) => `line ${i + 1}: ${"x".repeat((i * 7) % 50)}`).join("\n");
