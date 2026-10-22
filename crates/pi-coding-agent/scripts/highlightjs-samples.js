const hljs = require("./package/lib/index.js");
const samples = [
  ["typescript", "import { x } from './y';\ninterface A<T> { a: T; b?: string }\nexport async function f(a: number): Promise<void> {\n  const re = /foo+/gi; // comment\n  return `t ${a + 1}`;\n}\nconst el = <div className=\"x\">{a}</div>;\n"],
  ["javascript", "class A extends B {\n  constructor() { super(); this.x = 0x1F; }\n  get y() { return [1, 2.5e3, 'str', \"d\"]; }\n}\n/** @param {string} a */\nfunction g(a) { if (a) { throw new Error('x'); } }\nlet h = (x) => x * 2;\nconst jsx = <Foo bar={1}><span>hi</span></Foo>;\n"],
  ["python", "@decorator\ndef f(a: int, *args, **kw) -> str:\n    \"\"\"Docstring\n\n    more\n    \"\"\"\n    x = f'val {a!r}' + b'bytes' + r'\\d'\n    return None if a else True\nclass C(Base):\n    pass\n# comment\nprint(0o17, 1_000, 3.14j)\n"],
  ["rust", "#[derive(Debug)]\npub struct S<'a> { x: &'a str }\nimpl<'a> S<'a> {\n    fn new(x: &'a str) -> Self { Self { x } }\n}\nfn main() {\n    let v: Vec<u8> = vec![1, 2];\n    println!(\"{}\", r#\"raw\"#);\n    match v.len() { 0 => (), _ => unreachable!() }\n}\n// c\n/* block */\n"],
  ["go", "package main\n\nimport \"fmt\"\n\nfunc main() {\n\tx := []int{1, 2}\n\tfor i, v := range x {\n\t\tfmt.Println(i, v, `raw`)\n\t}\n\tvar c chan<- string\n\t_ = c\n}\n"],
  ["java", "package a.b;\nimport java.util.*;\n@Override\npublic class Foo<T> implements Bar {\n  private static final int X = 0b101;\n  public void run(String[] args) throws Exception {\n    System.out.println(\"hi\" + 'c' + 1.5f);\n  }\n}\n"],
  ["c", "#include <stdio.h>\n#define MAX(a,b) ((a)>(b)?(a):(b))\nint main(int argc, char **argv) {\n  unsigned long x = 0xFFul;\n  printf(\"%d\\n\", argc); /* c */\n  return 0;\n}\n"],
  ["cpp", "#include <vector>\ntemplate <typename T>\nclass V : public std::vector<T> {\npublic:\n  auto size() const noexcept -> std::size_t override { return 0; }\n};\nint main() { std::string s = R\"(raw)\"; nullptr; }\n"],
  ["bash", "#!/bin/bash\nset -euo pipefail\nfor f in *.txt; do\n  echo \"file: $f ${HOME}\" | grep -v x > /dev/null 2>&1\ndone\nif [[ -z \"$1\" ]]; then exit 1; fi\nfoo() { local a=$(( 1 + 2 )); }\n# comment\n"],
  ["json", "{\n  \"a\": 1,\n  \"b\": [true, false, null],\n  \"c\": {\"d\": \"e\"}\n}\n"],
  ["ruby", "class Foo < Bar\n  def initialize(a, b = 2)\n    @a = a\n    puts \"#{a} and #{b}\" if a\n  end\nend\n:sym\n%w[a b]\n# c\n=begin\nblock\n=end\n"],
  ["php", "<?php\nnamespace App;\nclass A {\n  public function f($x): ?int { return $x ?? null; }\n}\necho \"Hello $name\";\n?>\n<div><?= $y ?></div>\n"],
  ["csharp", "using System;\nnamespace N {\n  public class C<T> where T : class {\n    public async Task<int> M(string s) { var x = $\"{s}\"; return await Task.FromResult(1); }\n  }\n}\n"],
  ["kotlin", "data class P(val x: Int)\nfun main() {\n  val s = \"x=${'$'}{p.x}\"\n  when (x) { is Int -> println(1) else -> {} }\n}\n"],
  ["swift", "import UIKit\nstruct S: Codable { var x: Int? }\nfunc f(_ a: Int) -> String { return \"\\(a)\" }\nlet c = { (x: Int) in x * 2 }\n"],
  ["lua", "local function f(a, ...)\n  for i = 1, #a do print(a[i]) end\n  return {x = 1, [\"y\"] = 2}\nend\n-- comment\n--[[ block ]]\n"],
  ["xml", "<?xml version=\"1.0\"?>\n<!DOCTYPE html>\n<root attr=\"v\">\n  <child/>\n  <!-- comment -->\n  <![CDATA[ x ]]>\n</root>\n"],
  ["html", "<html><head><style>body { color: red; }</style><script>let x = 1;</script></head><body class=\"a\">&amp;</body></html>\n"],
  ["css", "@media (max-width: 600px) {\n  .a > #b:hover::before { color: #fff; margin: 0 1px !important; }\n}\n"],
  ["yaml", "key: value\nlist:\n  - 1\n  - \"str\"\nanchor: &a\n  x: true\n# comment\n"],
  ["diff", "--- a/file\n+++ b/file\n@@ -1,2 +1,2 @@\n-old\n+new\n context\n"],
  ["markdown", "# Title\n\nSome *em* and **strong** and `code`.\n\n```js\nlet x = 1;\n```\n\n- item\n[link](http://x)\n"],
  ["sql", "SELECT a, COUNT(*) FROM t WHERE b = 'x' AND c > 10 GROUP BY a; -- c\n"],
  ["scala", "object Main extends App {\n  case class P(x: Int)\n  val s = s\"x=${1}\"\n  def f[A](a: A): A = a\n}\n"],
  ["dart", "void main() async {\n  final x = <int>[1, 2];\n  print('v: ${x.length}');\n}\n"],
  ["perl", "use strict;\nmy $x = qw(a b);\nsub f { my ($a) = @_; return $a =~ s/x/y/gr; }\nprint \"$x\\n\";\n"],
  ["nix", "{ pkgs ? import <nixpkgs> {} }:\nlet x = \"a${b}\"; in pkgs.mkShell { buildInputs = [ pkgs.git ]; }\n"],
  ["groovy", "def f(String s) { println \"${s}\" }\nclass A { int x = 1 }\n"],
  ["r", "f <- function(x, ...) {\n  if (is.null(x)) return(NA)\n  x %>% filter(y > 1)\n}\n# c\n"],
  ["dockerfile", "FROM node:18\nRUN npm ci && npm run build\nCMD [\"node\", \"x.js\"]\n"],
  ["shell", "$ ls -la\ntotal 0\n$ echo hi\nhi\n"],
  ["makefile", "all: build\n\nbuild:\n\t$(CC) -o $@ $<\n"],
  ["mathematica", "f[x_] := Sin[x]^2 + Plot[foo, {x, 0, 1}]\n"],
];
const out = samples.map(([lang, code]) => ({ lang, code, value: hljs.highlight(code, { language: lang, ignoreIllegals: true }).value }));
const autos = ["fn main() { let x = 1; }", "def f():\n    return 1", "SELECT * FROM t;", "<div>hi</div>", "#include <stdio.h>\nint main() {}"].map((code) => {
  const r = hljs.highlightAuto(code);
  return { code, language: r.language, relevance: r.relevance, value: r.value };
});
const autoSubset = { code: "let x = 1;", subset: ["javascript", "rust", "python"], ...(({ language, value, relevance }) => ({ language, value, relevance }))(hljs.highlightAuto("let x = 1;", ["javascript", "rust", "python"])) };
console.log(JSON.stringify({ highlight: out, auto: autos, autoSubset }, null, 1));
