//! Block and inline lexer (port of marked's `Lexer` and `Tokenizer`).

use std::collections::HashMap;
use std::sync::LazyLock;

use fancy_regex::Captures;

use super::js_regex::{JsRegex, defined, group, has};
use super::rules::{Other, RULES, Rules};
use super::{Align, MarkedOptions, TableCell, Token, TokenType as T};
use crate::utils::{js_trim, js_trim_end};

/// Link reference definitions: lower-cased label → (href, title).
pub type Links = HashMap<String, (String, Option<String>)>;

#[derive(Debug, Clone, Copy)]
struct State {
    in_link: bool,
    in_raw_block: bool,
    link_emitted: bool,
    top: bool,
}

/// The marked lexer.
pub struct Lexer {
    options: MarkedOptions,
    /// Link reference definitions collected while lexing.
    pub links: Links,
    state: State,
}

fn rules() -> &'static Rules {
    &RULES
}

/// `src.substring(n)` for a byte count that is a prefix length of `src`.
fn advance(src: &str, n: usize) -> &str {
    let mut n = n.min(src.len());
    while !src.is_char_boundary(n) {
        n += 1;
    }
    &src[n..]
}

/// `s.substring(0, s.length - n)` (clamped to a character boundary).
fn cut_end(s: &str, n: usize) -> &str {
    let mut end = s.len().saturating_sub(n);
    while !s.is_char_boundary(end) {
        end -= 1;
    }
    &s[..end]
}

/// `str.slice(n)` with `n` in UTF-16 code units.
fn u16_slice_from(s: &str, units: usize) -> &str {
    let mut count = 0;
    for (i, c) in s.char_indices() {
        if count >= units {
            return &s[i..];
        }
        count += c.len_utf16();
    }
    ""
}

/// marked's `rtrim(str, c)`: remove trailing `c` characters.
fn rtrim(s: &str, c: char) -> &str {
    s.trim_end_matches(c)
}

/// marked's `trimTrailingBlankLines`.
fn trim_trailing_blank_lines(s: &str) -> String {
    let lines: Vec<&str> = s.split('\n').collect();
    let mut end = lines.len() as isize - 1;
    while end >= 0 && rules().other.blank_line.test(lines[end as usize]) {
        end -= 1;
    }
    if lines.len() as isize - end <= 2 {
        return s.to_string();
    }
    lines[..(end + 1) as usize].join("\n")
}

/// marked's `findClosingBracket` (byte index, -1 none, -2 unbalanced).
fn find_closing_bracket(s: &str, open: u8, close: u8) -> isize {
    if !s.as_bytes().contains(&close) {
        return -1;
    }
    let bytes = s.as_bytes();
    let mut level = 0;
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'\\' {
            i += 1;
        } else if bytes[i] == open {
            level += 1;
        } else if bytes[i] == close {
            level -= 1;
            if level < 0 {
                return i as isize;
            }
        }
        i += 1;
    }
    if level > 0 { -2 } else { -1 }
}

/// marked's `expandTabs`.
fn expand_tabs(line: &str, indent: usize) -> String {
    let mut col = indent;
    let mut out = String::with_capacity(line.len());
    for c in line.chars() {
        if c == '\t' {
            let added = 4 - (col % 4);
            out.extend(std::iter::repeat_n(' ', added));
            col += added;
        } else {
            out.push(c);
            col += 1;
        }
    }
    out
}

/// marked's `splitCells`.
fn split_cells(table_row: &str, count: Option<usize>) -> Vec<String> {
    // Ensure every cell-delimiting pipe has a space before it.
    let bytes = table_row.as_bytes();
    let mut row = String::with_capacity(table_row.len() + 8);
    for (i, c) in table_row.char_indices() {
        if c == '|' {
            let mut escaped = false;
            let mut curr = i;
            while curr > 0 && bytes[curr - 1] == b'\\' {
                escaped = !escaped;
                curr -= 1;
            }
            row.push_str(if escaped { "|" } else { " |" });
        } else {
            row.push(c);
        }
    }
    let mut cells: Vec<String> = row.split(" |").map(str::to_string).collect();
    if js_trim(&cells[0]).is_empty() {
        cells.remove(0);
    }
    if !cells.is_empty() && js_trim(cells.last().unwrap()).is_empty() {
        cells.pop();
    }
    if let Some(count) = count {
        if cells.len() > count {
            cells.truncate(count);
        } else {
            while cells.len() < count {
                cells.push(String::new());
            }
        }
    }
    cells.into_iter().map(|c| js_trim(&c).replace("\\|", "|")).collect()
}

fn indent_code_compensation(raw: &str, text: &str) -> String {
    let other = &rules().other;
    let Some(m) = other.indent_code_compensation.exec(raw) else {
        return text.to_string();
    };
    let indent_to_code = group(&m, 1).to_string();
    text.split('\n')
        .map(|node| match other.beginning_space.exec(node) {
            None => node.to_string(),
            Some(mi) => {
                let indent_in_node = group(&mi, 0);
                if indent_in_node.len() >= indent_to_code.len() {
                    node[indent_to_code.len()..].to_string()
                } else {
                    node.to_string()
                }
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn js_search(re: &JsRegex, s: &str) -> i64 {
    re.search(s).map_or(-1, |i| crate::utils::utf16_len(&s[..i]) as i64)
}

fn last_char_str(s: &str) -> String {
    match s.chars().next_back() {
        // JavaScript's `slice(-1)` yields a lone surrogate for astral characters; a
        // private-use character behaves the same way in every check that follows.
        Some(c) if c.len_utf16() == 2 => "\u{E000}".to_string(),
        Some(c) => c.to_string(),
        None => String::new(),
    }
}

// --- pi extensions: LaTeX math -----------------------------------------------------------

static PENDING_DOLLAR_MATH: LazyLock<JsRegex> =
    LazyLock::new(|| JsRegex::new(r"\\[A-Za-z]+|[_^=+*/<>()[\]|±≤≥≠≈∈→⇒∞∫∑√-]", ""));
static DOLLAR_SPACE: LazyLock<JsRegex> = LazyLock::new(|| JsRegex::new(r"^\$\s", ""));
static ENDS_WITH_SPACE: LazyLock<JsRegex> = LazyLock::new(|| JsRegex::new(r"\s$", ""));
static STARTS_WITH_DIGIT: LazyLock<JsRegex> = LazyLock::new(|| JsRegex::new(r"^\d", ""));
static CONSTANT_LIKE: LazyLock<JsRegex> =
    LazyLock::new(|| JsRegex::new(r"^[A-Z_][A-Z0-9_]*(?:[^A-Za-z0-9_\s])?$", ""));
static IDENTIFIER_START: LazyLock<JsRegex> = LazyLock::new(|| JsRegex::new(r"^[A-Za-z_][A-Za-z0-9_]*", ""));
static TABLE_DELIMITER_ROW: LazyLock<JsRegex> =
    LazyLock::new(|| JsRegex::new(r"^ {0,3}\|?[ \t]*:?-+:?[ \t]*(?:\|[ \t]*:?-+:?[ \t]*)*\|?[ \t]*$", ""));
static BLOCK_DOLLAR: LazyLock<JsRegex> =
    LazyLock::new(|| JsRegex::new(r"^ {0,3}\$\$[ \t]*(?:\n)?([\s\S]*?)\$\$[ \t]*(?:\n|$)", ""));
static BLOCK_BRACKET: LazyLock<JsRegex> =
    LazyLock::new(|| JsRegex::new(r"^ {0,3}\\\[[ \t]*(?:\n)?([\s\S]*?)\\\][ \t]*(?:\n|$)", ""));
static PENDING_BRACKET: LazyLock<JsRegex> = LazyLock::new(|| JsRegex::new(r"^ {0,3}\\\[[ \t]*(?:\n)?([\s\S]*)$", ""));
static PENDING_DOLLAR: LazyLock<JsRegex> = LazyLock::new(|| JsRegex::new(r"^ {0,3}\$\$[ \t]*(?:\n)?([\s\S]*)$", ""));
static BLOCK_LATEX_START: LazyLock<JsRegex> = LazyLock::new(|| JsRegex::new(r"(?:^|\n) {0,3}(?:\$\$|\\\[)", ""));
static STRICT_STRIKETHROUGH: LazyLock<JsRegex> =
    LazyLock::new(|| JsRegex::new(r"^(~~)(?=[^\s~])((?:\\.|[^\\])*?(?:\\.|[^\s~\\]))\1(?=[^~]|$)", ""));

fn is_escaped(source: &str, index: usize) -> bool {
    source.as_bytes()[..index].iter().rev().take_while(|&&b| b == b'\\').count() % 2 == 1
}

fn find_closing_delimiter(source: &str, closing: &str, start: usize) -> Option<usize> {
    let mut index = source.get(start..)?.find(closing).map(|i| i + start);
    while let Some(i) = index {
        if !is_escaped(source, i) {
            return Some(i);
        }
        let from = i + closing.len();
        index = source.get(from..).and_then(|s| s.find(closing)).map(|j| j + from);
    }
    None
}

fn tokenize_inline_latex(source: &str) -> Option<Token> {
    let (opening, closing) = if source.starts_with("$$") {
        ("$$", "$$")
    } else if source.starts_with("\\(") {
        ("\\(", "\\)")
    } else if source.starts_with("\\[") {
        ("\\[", "\\]")
    } else if source.starts_with('$') && !DOLLAR_SPACE.test(source) {
        ("$", "$")
    } else {
        return None;
    };
    let closing_index = find_closing_delimiter(source, closing, opening.len());
    if let Some(ci) = closing_index
        && opening == "$"
    {
        let inner = &source[opening.len()..ci];
        let after = &source[ci + 1..];
        if ENDS_WITH_SPACE.test(inner)
            || STARTS_WITH_DIGIT.test(after)
            || (CONSTANT_LIKE.test(inner) && IDENTIFIER_START.test(after))
            || inner.contains('`')
        {
            return None;
        }
    }
    let Some(ci) = closing_index else {
        let pending = &source[opening.len()..];
        if opening.starts_with('\\') || PENDING_DOLLAR_MATH.test(pending) {
            let mut t = Token::new(T::Latex, source);
            t.text = pending.to_string();
            t.pending = true;
            return Some(t);
        }
        return None;
    };
    let text = &source[opening.len()..ci];
    if text.is_empty() || text.contains('\n') {
        return None;
    }
    let mut t = Token::new(T::Latex, &source[..ci + closing.len()]);
    t.text = text.to_string();
    Some(t)
}

fn contains_table_start(source: &str) -> bool {
    let lines: Vec<&str> = source.split('\n').collect();
    for index in 0..lines.len().saturating_sub(1) {
        let delimiter_row = lines[index + 1];
        if (delimiter_row.contains('|') || lines[index].contains('|')) && TABLE_DELIMITER_ROW.test(delimiter_row) {
            return true;
        }
    }
    false
}

fn tokenize_block_latex(source: &str) -> Option<Token> {
    let block = |raw: &str, text: String, pending: bool| {
        let mut t = Token::new(T::LatexBlock, raw);
        t.text = text;
        t.pending = pending;
        t
    };
    if let Some(m) = BLOCK_DOLLAR.exec(source)
        && has(&m, 1)
        && !contains_table_start(group(&m, 1))
    {
        return Some(block(group(&m, 0), js_trim(group(&m, 1)).to_string(), false));
    }
    if let Some(m) = BLOCK_BRACKET.exec(source)
        && has(&m, 1)
        && !contains_table_start(group(&m, 1))
    {
        return Some(block(group(&m, 0), js_trim(group(&m, 1)).to_string(), false));
    }
    if let Some(m) = PENDING_BRACKET.exec(source)
        && has(&m, 1)
        && !contains_table_start(group(&m, 1))
    {
        return Some(block(group(&m, 0), group(&m, 1).to_string(), true));
    }
    if let Some(m) = PENDING_DOLLAR.exec(source)
        && has(&m, 1)
        && PENDING_DOLLAR_MATH.test(group(&m, 1))
        && !contains_table_start(group(&m, 1))
    {
        return Some(block(group(&m, 0), group(&m, 1).to_string(), true));
    }
    None
}

fn latex_block_start(source: &str) -> Option<usize> {
    BLOCK_LATEX_START.exec(source).map(|m| {
        let m0 = m.get(0).unwrap();
        m0.start() + usize::from(m0.as_str().starts_with('\n'))
    })
}

fn latex_inline_start(source: &str) -> Option<usize> {
    [source.find('$'), source.find("\\("), source.find("\\[")].into_iter().flatten().min()
}

// --- marked-cjk-friendly ------------------------------------------------------------------

const CJK: &str = r"\u1100-\u11ff\u20a9\u2329-\u232a\u2630-\u2637\u268a-\u268f\u2e80-\u2e99\u2e9b-\u2ef3\u2f00-\u2fd5\u2ff0-\u303e\u3041-\u3096\u3099-\u30ff\u3105-\u312f\u3131-\u318e\u3190-\u31e5\u31ef-\u321e\u3220-\u3247\u3250-\ua48c\ua490-\ua4c6\ua960-\ua97c\uac00-\ud7a3\ud7b0-\ud7c6\ud7cb-\ud7fb\uf900-\ufaff\ufe10-\ufe19\ufe30-\ufe52\ufe54-\ufe66\ufe68-\ufe6b\uff01-\uffbe\uffc2-\uffc7\uffca-\uffcf\uffd2-\uffd7\uffda-\uffdc\uffe0-\uffe6\uffe8-\uffee\u{16fe0}-\u{16fe4}\u{16ff0}-\u{16ff6}\u{17000}-\u{18cda}\u{18cff}-\u{18d20}\u{18d80}-\u{18df2}\u{18e00}-\u{19191}\u{191a0}-\u{191d2}\u{1aff0}-\u{1aff3}\u{1aff5}-\u{1affb}\u{1affd}-\u{1affe}\u{1b000}-\u{1b128}\u{1b132}\u{1b150}-\u{1b152}\u{1b155}\u{1b164}-\u{1b168}\u{1b170}-\u{1b2fb}\u{1d300}-\u{1d356}\u{1d360}-\u{1d376}\u{1f1ae}\u{1f200}\u{1f202}\u{1f210}-\u{1f219}\u{1f21b}-\u{1f22e}\u{1f230}-\u{1f231}\u{1f237}\u{1f23b}\u{1f240}-\u{1f248}\u{1f260}-\u{1f265}\u{1f7da}\u{20000}-\u{3fffd}";

struct Cjk {
    test: JsRegex,
    punctuation: JsRegex,
    r_delim_ast_gfm: JsRegex,
    r_delim_und: JsRegex,
}

fn build_r_delim_ast(punct: &str, punct_space: &str, not_punct_space: &str) -> JsRegex {
    JsRegex::new(
        &format!(
            r"^[^_*]*?__[^_*]*?\*[^_*]*?(?=__)|[^*]+(?=[^*])|(?!\*){punct}(\*+)(?=[\s]|$)|{not_punct_space}(\*+)(?!\*)(?={punct_space}|$)|(?!\*){punct_space}(\*+)(?={not_punct_space})|[\s](\*+)(?!\*)(?={punct})|(?!\*){punct}(\*+)(?!\*)(?={punct})|{not_punct_space}(\*+)(?={not_punct_space})"
        ),
        "gu",
    )
}

fn build_r_delim_und(punct: &str, punct_space: &str, not_punct_space: &str) -> JsRegex {
    JsRegex::new(
        &format!(
            r"^[^_*]*?\*\*[^_*]*?_[^_*]*?(?=\*\*)|[^_]+(?=[^_])|(?!_){punct}(_+)(?=[\s]|$)|{not_punct_space}(_+)(?!_)(?={punct_space}|$)|(?!_){punct_space}(_+)(?={not_punct_space})|[\s](_+)(?!_)(?={punct})|(?!_){punct}(_+)(?!_)(?={punct})"
        ),
        "gu",
    )
}

static CJK_RULES: LazyLock<Cjk> = LazyLock::new(|| {
    let cjk_punct = format!(r"[\p{{P}}\p{{S}}{CJK}]");
    let cjk_punct_space = format!(r"[\s\p{{P}}\p{{S}}{CJK}]");
    let cjk_not_punct_space = format!(r"[^\s\p{{P}}\p{{S}}{CJK}]");
    let cjk_punct_gfm = format!(r"(?!~)[\p{{P}}\p{{S}}{CJK}]");
    let cjk_punct_space_gfm = format!(r"(?!~)[\s\p{{P}}\p{{S}}{CJK}]");
    let cjk_not_punct_space_gfm = format!(r"(?:[^\s\p{{P}}\p{{S}}{CJK}]|~)");
    Cjk {
        test: JsRegex::new(&format!("[{CJK}]"), "u"),
        punctuation: JsRegex::new(&format!(r"^((?![*_])[\s\p{{P}}\p{{S}}{CJK}])"), "u"),
        r_delim_ast_gfm: build_r_delim_ast(&cjk_punct_gfm, &cjk_punct_space_gfm, &cjk_not_punct_space_gfm),
        r_delim_und: build_r_delim_und(&cjk_punct, &cjk_punct_space, &cjk_not_punct_space),
    }
});

fn is_cjk_char(c: char) -> bool {
    CJK_RULES.test.test(c.encode_utf8(&mut [0; 4]))
}

fn is_prev_char_cjk(prev_char: &str, masked_src: &str, src: &str) -> bool {
    let mut prev_is_cjk = CJK_RULES.punctuation.test(prev_char);
    if !prev_is_cjk && !prev_char.is_empty() {
        let prev_idx = masked_src.len().saturating_sub(src.len());
        if prev_idx >= 1 {
            let mut chars = masked_src[..prev_idx].chars().rev();
            let mut c = chars.next();
            if let Some(ch) = c
                && ('\u{FE00}'..='\u{FE0E}').contains(&ch)
            {
                let before = chars.clone().next();
                if before.is_some() {
                    c = chars.next();
                }
            }
            if let Some(ch) = c {
                prev_is_cjk = if ('\u{E0100}'..='\u{E01EF}').contains(&ch) { true } else { is_cjk_char(ch) };
            }
        }
    }
    prev_is_cjk
}

fn is_cjk_adjacent_to_delimiter(m: &Captures<'_>, clipped: &str) -> bool {
    if !(has(m, 1) || has(m, 2) || has(m, 3) || has(m, 4)) {
        return false;
    }
    let m0 = m.get(0).unwrap();
    let before = m0.as_str().chars().next();
    let after = clipped[m0.end()..].chars().next();
    before.is_some_and(is_cjk_char) || after.is_some_and(is_cjk_char)
}

fn first_group<'t>(m: &Captures<'t>) -> Option<&'t str> {
    (1..=6).map(|i| group(m, i)).find(|s| !s.is_empty())
}

/// `maskedSrc.slice(-1 * src.length + lLength)`.
fn clip_masked<'a>(masked: &'a str, src: &str, l_length: usize) -> &'a str {
    let keep = src.len() as isize - l_length as isize;
    if keep <= 0 {
        // slice(-0) / slice(positive) semantics
        let start = (-keep) as usize;
        return masked.get(start..).unwrap_or("");
    }
    let start = masked.len().saturating_sub(keep as usize);
    let mut s = start;
    while !masked.is_char_boundary(s) {
        s += 1;
    }
    &masked[s..]
}

impl Lexer {
    pub fn new(options: MarkedOptions) -> Self {
        Self {
            options,
            links: Links::new(),
            state: State { in_link: false, in_raw_block: false, link_emitted: false, top: true },
        }
    }

    /// `lexer.lex(src)`: block tokens with inline tokens resolved.
    pub fn lex(&mut self, src: &str) -> Vec<Token> {
        let src = rules().other.carriage_return.replace_all_str(src, "\n");
        let mut tokens = Vec::new();
        self.block_tokens(&src, &mut tokens, false);
        self.resolve_inline(&mut tokens);
        tokens
    }

    /// `Lexer.lexInline(src)`.
    pub fn lex_inline(&mut self, src: &str) -> Vec<Token> {
        self.inline_tokens(src)
    }

    fn resolve_inline(&mut self, tokens: &mut [Token]) {
        for t in tokens {
            self.resolve_token(t);
        }
    }

    fn resolve_token(&mut self, t: &mut Token) {
        if let Some(src) = t.inline_src.take() {
            let mut inline = self.inline_tokens(&src);
            t.tokens.append(&mut inline);
        } else {
            self.resolve_inline(&mut t.tokens);
        }
        for item in &mut t.items {
            self.resolve_token(item);
        }
        for cell in t.header.iter_mut().chain(t.rows.iter_mut().flatten()) {
            if let Some(src) = cell.inline_src.take() {
                cell.tokens = self.inline_tokens(&src);
            }
        }
    }

    // ---------------------------------------------------------------------------------
    // Block level
    // ---------------------------------------------------------------------------------

    fn block_tokens(&mut self, src: &str, tokens: &mut Vec<Token>, mut last_paragraph_clipped: bool) {
        let mut src = src;
        let mut src_length = usize::MAX;
        while !src.is_empty() {
            if src.len() < src_length {
                src_length = src.len();
            } else {
                break;
            }

            if self.options.latex
                && let Some(tok) = tokenize_block_latex(src)
            {
                src = advance(src, tok.raw.len());
                tokens.push(tok);
                continue;
            }

            if let Some(tok) = self.space(src) {
                src = advance(src, tok.raw.len());
                match tokens.last_mut() {
                    Some(last) if tok.raw.len() == 1 => last.raw.push('\n'),
                    _ => tokens.push(tok),
                }
                continue;
            }

            if let Some(tok) = self.code(src) {
                src = advance(src, tok.raw.len());
                match tokens.last_mut() {
                    Some(last) if matches!(last.kind, T::Paragraph | T::Text) => {
                        if !last.raw.ends_with('\n') {
                            last.raw.push('\n');
                        }
                        last.raw.push_str(&tok.raw);
                        last.text.push('\n');
                        last.text.push_str(&tok.text);
                        last.inline_src = Some(last.text.clone());
                    }
                    _ => tokens.push(tok),
                }
                continue;
            }

            if let Some(tok) = self.fences(src) {
                src = advance(src, tok.raw.len());
                tokens.push(tok);
                continue;
            }
            if let Some(tok) = self.heading(src) {
                src = advance(src, tok.raw.len());
                tokens.push(tok);
                continue;
            }
            if let Some(tok) = self.hr(src) {
                src = advance(src, tok.raw.len());
                tokens.push(tok);
                continue;
            }
            if let Some(tok) = self.blockquote(src) {
                src = advance(src, tok.raw.len());
                tokens.push(tok);
                continue;
            }
            if let Some(tok) = self.list(src) {
                src = advance(src, tok.raw.len());
                tokens.push(tok);
                continue;
            }
            if let Some(tok) = self.html(src) {
                src = advance(src, tok.raw.len());
                tokens.push(tok);
                continue;
            }
            if let Some(tok) = self.def(src) {
                src = advance(src, tok.raw.len());
                match tokens.last_mut() {
                    Some(last) if matches!(last.kind, T::Paragraph | T::Text) => {
                        if !last.raw.ends_with('\n') {
                            last.raw.push('\n');
                        }
                        last.raw.push_str(&tok.raw);
                        last.text.push('\n');
                        last.text.push_str(&tok.raw);
                        last.inline_src = Some(last.text.clone());
                    }
                    _ => {
                        if !self.links.contains_key(&tok.tag) {
                            self.links.insert(tok.tag.clone(), (tok.href.clone(), tok.title.clone()));
                            tokens.push(tok);
                        }
                    }
                }
                continue;
            }
            if let Some(tok) = self.table(src) {
                src = advance(src, tok.raw.len());
                tokens.push(tok);
                continue;
            }
            if let Some(tok) = self.lheading(src) {
                src = advance(src, tok.raw.len());
                tokens.push(tok);
                continue;
            }

            let mut cut_src = src;
            if self.options.latex {
                let first_len = src.chars().next().map_or(0, char::len_utf8);
                if let Some(start) = latex_block_start(&src[first_len..]) {
                    cut_src = &src[..start + first_len];
                }
            }
            if self.state.top
                && let Some(tok) = self.paragraph(cut_src)
            {
                match tokens.last_mut() {
                    Some(last) if last_paragraph_clipped && last.kind == T::Paragraph => {
                        if !last.raw.ends_with('\n') {
                            last.raw.push('\n');
                        }
                        last.raw.push_str(&tok.raw);
                        last.text.push('\n');
                        last.text.push_str(&tok.text);
                        last.inline_src = Some(last.text.clone());
                    }
                    _ => tokens.push(tok.clone()),
                }
                last_paragraph_clipped = cut_src.len() != src.len();
                src = advance(src, tok.raw.len());
                continue;
            }

            if let Some(tok) = self.text(src) {
                src = advance(src, tok.raw.len());
                match tokens.last_mut() {
                    Some(last) if last.kind == T::Text => {
                        if !last.raw.ends_with('\n') {
                            last.raw.push('\n');
                        }
                        last.raw.push_str(&tok.raw);
                        last.text.push('\n');
                        last.text.push_str(&tok.text);
                        last.inline_src = Some(last.text.clone());
                    }
                    _ => tokens.push(tok),
                }
                continue;
            }

            if !src.is_empty() {
                // marked throws "Infinite loop on byte"; stop lexing instead.
                break;
            }
        }
        self.state.top = true;
    }

    fn space(&self, src: &str) -> Option<Token> {
        let cap = rules().block.newline.exec(src)?;
        let raw = group(&cap, 0);
        (!raw.is_empty()).then(|| Token::new(T::Space, raw))
    }

    fn code(&self, src: &str) -> Option<Token> {
        let cap = rules().block.code.exec(src)?;
        let raw = trim_trailing_blank_lines(group(&cap, 0));
        let text = rules().other.code_remove_indent.replace_all_str(&raw, "");
        let mut t = Token::new(T::Code, raw);
        t.indented = true;
        t.text = text;
        Some(t)
    }

    fn fences(&self, src: &str) -> Option<Token> {
        let cap = rules().block.fences.exec(src)?;
        let raw = group(&cap, 0);
        let text = indent_code_compensation(raw, group(&cap, 3));
        let mut t = Token::new(T::Code, raw);
        t.lang = if has(&cap, 2) {
            Some(rules().inline.any_punctuation.replace_all(js_trim(group(&cap, 2)), |c| group(c, 1).to_string()))
        } else if defined(&cap, 2) {
            Some(String::new())
        } else {
            None
        };
        t.text = text;
        Some(t)
    }

    fn heading(&self, src: &str) -> Option<Token> {
        let cap = rules().block.heading.exec(src)?;
        let mut text = js_trim(group(&cap, 2)).to_string();
        if rules().other.ending_hash.test(&text) {
            let trimmed = rtrim(&text, '#');
            if trimmed.is_empty() || rules().other.ending_space_char.test(trimmed) {
                text = js_trim(trimmed).to_string();
            }
        }
        let mut t = Token::new(T::Heading, rtrim(group(&cap, 0), '\n'));
        t.depth = group(&cap, 1).len();
        t.inline_src = Some(text.clone());
        t.text = text;
        Some(t)
    }

    fn hr(&self, src: &str) -> Option<Token> {
        let cap = rules().block.hr.exec(src)?;
        Some(Token::new(T::Hr, rtrim(group(&cap, 0), '\n')))
    }

    fn blockquote(&mut self, src: &str) -> Option<Token> {
        let other = &rules().other;
        let cap = rules().block.blockquote.exec(src)?;
        let cap0 = rtrim(group(&cap, 0), '\n').to_string();
        let mut lines: Vec<String> = cap0.split('\n').map(str::to_string).collect();
        let mut raw = String::new();
        let mut text = String::new();
        let mut tokens: Vec<Token> = Vec::new();

        while !lines.is_empty() {
            let mut in_blockquote = false;
            let mut current_lines: Vec<String> = Vec::new();
            let mut i = 0;
            while i < lines.len() {
                if other.blockquote_start.test(&lines[i]) {
                    current_lines.push(lines[i].clone());
                    in_blockquote = true;
                } else if !in_blockquote {
                    current_lines.push(lines[i].clone());
                } else {
                    break;
                }
                i += 1;
            }
            lines.drain(..i);

            let current_raw = current_lines.join("\n");
            let current_text = other.blockquote_setext_replace.replace_all(&current_raw, |c| format!("\n    {}", group(c, 1)));
            let current_text = other.blockquote_setext_replace2.replace_all_str(&current_text, "");
            raw = if raw.is_empty() { current_raw.clone() } else { format!("{raw}\n{current_raw}") };
            text = if text.is_empty() { current_text.clone() } else { format!("{text}\n{current_text}") };

            let top = self.state.top;
            self.state.top = true;
            self.block_tokens(&current_text, &mut tokens, true);
            self.state.top = top;

            if lines.is_empty() {
                break;
            }

            let last_kind = tokens.last().map(|t| t.kind);
            match last_kind {
                Some(T::Code) => break,
                Some(T::Blockquote) => {
                    let old = tokens.last().unwrap().clone();
                    let continuation = lines.join("\n");
                    let new_text =
                        format!("{}\n{}", old.raw, other.blockquote_setext_replace2.replace_all_str(&continuation, ""));
                    let new_token = self.blockquote(&new_text)?;
                    raw = format!("{raw}\n{continuation}");
                    text = format!("{}{}", cut_end(&text, old.text.len()), new_token.text);
                    *tokens.last_mut().unwrap() = new_token;
                    break;
                }
                Some(T::List) => {
                    let old = tokens.last().unwrap().clone();
                    let new_text = format!("{}\n{}", old.raw, lines.join("\n"));
                    let new_token = self.list(&new_text)?;
                    raw = format!("{}{}", cut_end(&raw, old.raw.len()), new_token.raw);
                    text = format!("{}{}", cut_end(&text, old.raw.len()), new_token.raw);
                    let consumed = new_token.raw.len();
                    *tokens.last_mut().unwrap() = new_token;
                    lines = new_text[consumed.min(new_text.len())..].split('\n').map(str::to_string).collect();
                    continue;
                }
                _ => {}
            }
        }

        let mut t = Token::new(T::Blockquote, raw);
        t.tokens = tokens;
        t.text = text;
        Some(t)
    }

    fn list(&mut self, src: &str) -> Option<Token> {
        let r = rules();
        let other = &r.other;
        let cap = r.block.list.exec(src)?;
        let bull_trimmed = js_trim(group(&cap, 1)).to_string();
        let is_ordered = bull_trimmed.len() > 1;
        let mut list = Token::new(T::List, "");
        list.ordered = is_ordered;
        list.start = if is_ordered { bull_trimmed[..bull_trimmed.len() - 1].parse().ok() } else { None };
        let bull = if is_ordered {
            format!(r"\d{{1,9}}\{}", &bull_trimmed[bull_trimmed.len() - 1..])
        } else {
            format!(r"\{bull_trimmed}")
        };
        let item_regex = JsRegex::new(&format!(r"^( {{0,3}}{bull})((?:[\t ][^\n]*)?(?:\n|$))"), "");
        let mut ends_with_blank_line = false;
        let mut src = src.to_string();

        while !src.is_empty() {
            let mut end_early = false;
            let Some(cap) = item_regex.exec(&src) else { break };
            if r.block.hr.test(&src) {
                break;
            }
            let mut raw = group(&cap, 0).to_string();
            let cap1_len = group(&cap, 1).len();
            let cap2 = group(&cap, 2).to_string();
            drop(cap);
            src = src[raw.len()..].to_string();

            let mut line = expand_tabs(cap2.split('\n').next().unwrap_or(""), cap1_len);
            let mut next_line = src.split('\n').next().unwrap_or("").to_string();
            let mut blank_line = js_trim(&line).is_empty();
            let mut item_contents = String::new();

            let indent: usize;
            if blank_line {
                indent = cap1_len + 1;
            } else {
                let mut ind = js_search(&other.non_space_char, &line);
                if ind > 4 {
                    ind = 1;
                }
                let ind = ind.max(0) as usize;
                item_contents = u16_slice_from(&line, ind).to_string();
                indent = ind + cap1_len;
            }

            if blank_line && other.blank_line.test(&next_line) {
                raw.push_str(&next_line);
                raw.push('\n');
                src = advance(&src, next_line.len() + 1).to_string();
                end_early = true;
            }

            if !end_early {
                let idx = Other::indent_index(indent);
                let next_bullet = &other.next_bullet[idx];
                let hr_regex = &other.hr[idx];
                let fences_begin = &other.fences_begin[idx];
                let heading_begin = &other.heading_begin[idx];
                let html_begin = &other.html_begin[idx];
                let blockquote_begin = &other.blockquote_begin[idx];

                while !src.is_empty() {
                    let raw_line = src.split('\n').next().unwrap_or("").to_string();
                    next_line = raw_line.clone();
                    let next_line_without_tabs = other.tab_char_global.replace_all_str(&next_line, "    ");

                    if fences_begin.test(&next_line)
                        || heading_begin.test(&next_line)
                        || html_begin.test(&next_line)
                        || blockquote_begin.test(&next_line)
                        || next_bullet.test(&next_line)
                        || hr_regex.test(&next_line)
                    {
                        break;
                    }

                    if js_search(&other.non_space_char, &next_line_without_tabs) >= indent as i64
                        || js_trim(&next_line).is_empty()
                    {
                        item_contents.push('\n');
                        item_contents.push_str(u16_slice_from(&next_line_without_tabs, indent));
                    } else {
                        if blank_line {
                            break;
                        }
                        let line_without_tabs = other.tab_char_global.replace_all_str(&line, "    ");
                        if js_search(&other.non_space_char, &line_without_tabs) >= 4 {
                            break;
                        }
                        if fences_begin.test(&line) || heading_begin.test(&line) || hr_regex.test(&line) {
                            break;
                        }
                        item_contents.push('\n');
                        item_contents.push_str(&next_line);
                    }

                    blank_line = js_trim(&next_line).is_empty();
                    raw.push_str(&raw_line);
                    raw.push('\n');
                    src = advance(&src, raw_line.len() + 1).to_string();
                    line = u16_slice_from(&next_line_without_tabs, indent).to_string();
                }
            }

            if !list.loose {
                if ends_with_blank_line {
                    list.loose = true;
                } else if other.double_blank_line.test(&raw) {
                    ends_with_blank_line = true;
                }
            }

            let mut item = Token::new(T::ListItem, raw.clone());
            item.task = other.list_is_task.test(&item_contents);
            item.text = item_contents;
            list.items.push(item);
            list.raw.push_str(&raw);
        }

        let last = list.items.last_mut()?;
        last.raw = js_trim_end(&last.raw).to_string();
        last.text = js_trim_end(&last.text).to_string();
        list.raw = js_trim_end(&list.raw).to_string();

        // First pass: tokenize items and finalize looseness.
        for i in 0..list.items.len() {
            self.state.top = false;
            let text = list.items[i].text.clone();
            let mut item_tokens = Vec::new();
            self.block_tokens(&text, &mut item_tokens, false);
            if !list.loose {
                let spacers: Vec<&Token> = item_tokens.iter().filter(|t| t.kind == T::Space).collect();
                list.loose = !spacers.is_empty() && spacers.iter().any(|t| other.any_line.test(&t.raw));
            }
            list.items[i].tokens = item_tokens;
        }

        // Second pass: task checkboxes.
        let loose = list.loose;
        for item in &mut list.items {
            let first_kind = item.tokens.first().map(|t| t.kind);
            if item.task && matches!(first_kind, Some(T::Text | T::Paragraph)) {
                item.text = other.list_replace_task.replace_first(&item.text, |_| String::new());
                {
                    let first = &mut item.tokens[0];
                    first.raw = other.list_replace_task.replace_first(&first.raw, |_| String::new());
                    first.text = other.list_replace_task.replace_first(&first.text, |_| String::new());
                    if let Some(src) = &first.inline_src
                        && other.list_is_task.test(src)
                    {
                        first.inline_src = Some(other.list_replace_task.replace_first(src, |_| String::new()));
                    }
                }
                if let Some(task_raw) = other.list_task_checkbox.exec(&item.raw) {
                    let m0 = group(&task_raw, 0).to_string();
                    let mut checkbox = Token::new(T::Checkbox, format!("{m0} "));
                    checkbox.checked = Some(m0 != "[ ]");
                    item.checked = checkbox.checked;
                    if loose {
                        let first = &mut item.tokens[0];
                        first.raw = format!("{}{}", checkbox.raw, first.raw);
                        first.text = format!("{}{}", checkbox.raw, first.text);
                        first.tokens.insert(0, checkbox);
                    } else {
                        item.tokens.insert(0, checkbox);
                    }
                }
            } else if item.task {
                item.task = false;
            }
        }

        if list.loose {
            for item in &mut list.items {
                item.loose = true;
                for token in &mut item.tokens {
                    if token.kind == T::Text {
                        token.kind = T::Paragraph;
                    }
                }
            }
        }

        Some(list)
    }

    fn html(&self, src: &str) -> Option<Token> {
        let cap = rules().block.html.exec(src)?;
        let raw = trim_trailing_blank_lines(group(&cap, 0));
        let tag = group(&cap, 1);
        let mut t = Token::new(T::Html, raw.clone());
        t.block = true;
        t.pre = tag == "pre" || tag == "script" || tag == "style";
        t.text = raw;
        Some(t)
    }

    fn def(&self, src: &str) -> Option<Token> {
        let r = rules();
        let cap = r.block.def.exec(src)?;
        let tag = r.other.multiple_space_global.replace_all_str(&group(&cap, 1).to_lowercase(), " ");
        let href = if has(&cap, 2) {
            let h = r.other.href_brackets.replace_first(group(&cap, 2), |c| group(c, 1).to_string());
            r.inline.any_punctuation.replace_all(&h, |c| group(c, 1).to_string())
        } else {
            String::new()
        };
        let title = if has(&cap, 3) {
            let t = group(&cap, 3);
            Some(r.inline.any_punctuation.replace_all(&t[1..t.len() - 1], |c| group(c, 1).to_string()))
        } else if defined(&cap, 3) {
            Some(String::new())
        } else {
            None
        };
        let mut t = Token::new(T::Def, rtrim(group(&cap, 0), '\n'));
        t.tag = tag;
        t.href = href;
        t.title = title;
        Some(t)
    }

    fn table(&self, src: &str) -> Option<Token> {
        let other = &rules().other;
        let cap = rules().block.table.exec(src)?;
        if !other.table_delimiter.test(group(&cap, 2)) {
            return None;
        }
        let headers = split_cells(group(&cap, 1), None);
        let aligns: Vec<String> =
            other.table_align_chars.replace_all_str(group(&cap, 2), "").split('|').map(str::to_string).collect();
        let rows: Vec<String> = if defined(&cap, 3) && !js_trim(group(&cap, 3)).is_empty() {
            other.table_row_blank_line.replace_first(group(&cap, 3), |_| String::new()).split('\n').map(str::to_string).collect()
        } else {
            Vec::new()
        };
        if headers.len() != aligns.len() {
            return None;
        }
        let mut item = Token::new(T::Table, rtrim(group(&cap, 0), '\n'));
        for align in &aligns {
            item.align.push(if other.table_align_right.test(align) {
                Some(Align::Right)
            } else if other.table_align_center.test(align) {
                Some(Align::Center)
            } else if other.table_align_left.test(align) {
                Some(Align::Left)
            } else {
                None
            });
        }
        for (i, h) in headers.iter().enumerate() {
            item.header.push(TableCell {
                text: h.clone(),
                tokens: Vec::new(),
                header: true,
                align: item.align[i],
                inline_src: Some(h.clone()),
            });
        }
        for row in &rows {
            let cells = split_cells(row, Some(item.header.len()));
            item.rows.push(
                cells
                    .into_iter()
                    .enumerate()
                    .map(|(i, cell)| TableCell {
                        text: cell.clone(),
                        tokens: Vec::new(),
                        header: false,
                        align: item.align.get(i).copied().flatten(),
                        inline_src: Some(cell),
                    })
                    .collect(),
            );
        }
        Some(item)
    }

    fn lheading(&self, src: &str) -> Option<Token> {
        let cap = rules().block.lheading.exec(src)?;
        let text = js_trim(group(&cap, 1)).to_string();
        let mut t = Token::new(T::Heading, rtrim(group(&cap, 0), '\n'));
        t.depth = if group(&cap, 2).starts_with('=') { 1 } else { 2 };
        t.inline_src = Some(text.clone());
        t.text = text;
        Some(t)
    }

    fn paragraph(&self, src: &str) -> Option<Token> {
        let cap = rules().block.paragraph.exec(src)?;
        let c1 = group(&cap, 1);
        let text = c1.strip_suffix('\n').unwrap_or(c1).to_string();
        let mut t = Token::new(T::Paragraph, group(&cap, 0));
        t.inline_src = Some(text.clone());
        t.text = text;
        Some(t)
    }

    fn text(&self, src: &str) -> Option<Token> {
        let cap = rules().block.text.exec(src)?;
        let raw = group(&cap, 0);
        let mut t = Token::new(T::Text, raw);
        t.text = raw.to_string();
        t.inline_src = Some(raw.to_string());
        Some(t)
    }

    // ---------------------------------------------------------------------------------
    // Inline level
    // ---------------------------------------------------------------------------------

    /// Does this link text hold a link already? (Images do not count.)
    fn link_in_text(&self, text: &str) -> bool {
        if !text.contains('[') {
            return false;
        }
        let inline = &rules().inline;
        for m in inline.block_skip.match_all(text) {
            let m0 = m.get(0).unwrap();
            let before_is_bang = text[..m0.start()].ends_with('!');
            if inline.link.test(m0.as_str()) && !before_is_bang {
                return true;
            }
        }
        for m in inline.reflink_search.match_all(text) {
            let match0 = group(&m, 0);
            let ref_start = match0.rfind('[').unwrap_or(0);
            let key = &match0[(ref_start + 1).min(match0.len())..match0.len().saturating_sub(1).max(ref_start + 1)];
            if match0.starts_with('!') || !self.links.contains_key(key) {
                continue;
            }
            if ref_start > 1 && self.link_in_text(&match0[1..ref_start - 1]) {
                continue;
            }
            return true;
        }
        false
    }

    fn mask_reflink(&self, match0: &str) -> String {
        let ref_start = match0.rfind('[').unwrap_or(0);
        let key_end = match0.len().saturating_sub(1).max(ref_start + 1);
        let key = &match0[(ref_start + 1).min(match0.len())..key_end.min(match0.len())];
        if !self.links.contains_key(key) {
            return match0.to_string();
        }
        if ref_start > 1 && !match0.starts_with('!') {
            let text = &match0[1..ref_start - 1];
            if self.link_in_text(text) {
                let inner = rules().inline.reflink_search.replace_all(text, |c| self.mask_reflink(group(c, 0)));
                return format!("[{inner}][{}]", "a".repeat(match0.len() - ref_start - 2));
            }
        }
        format!("[{}]", "a".repeat(match0.len().saturating_sub(2)))
    }

    pub(crate) fn inline_tokens(&mut self, src: &str) -> Vec<Token> {
        let inline = &rules().inline;
        let mut tokens: Vec<Token> = Vec::new();

        // Mask reflinks, escaped punctuation and code/link/html spans (length-preserving).
        let mut masked = src.to_string();
        if src.contains('[') {
            masked = inline.reflink_search.replace_all(&masked, |c| self.mask_reflink(group(c, 0)));
        }
        masked = inline.any_punctuation.replace_all(&masked, |c| "+".repeat(group(c, 0).len()));
        masked = inline.block_skip.replace_all(&masked, |c| {
            let m0 = group(c, 0);
            format!("[{}]", "a".repeat(m0.len().saturating_sub(2)))
        });

        let mut src: &str = src;
        let mut keep_prev_char = false;
        let mut prev_char = String::new();
        let mut src_length = usize::MAX;
        while !src.is_empty() {
            if src.len() < src_length {
                src_length = src.len();
            } else {
                break;
            }
            if !keep_prev_char {
                prev_char.clear();
            }
            keep_prev_char = false;

            if self.options.latex
                && let Some(tok) = tokenize_inline_latex(src)
            {
                src = advance(src, tok.raw.len());
                tokens.push(tok);
                continue;
            }
            if let Some(tok) = self.escape(src) {
                src = advance(src, tok.raw.len());
                tokens.push(tok);
                continue;
            }
            if let Some(tok) = self.tag(src) {
                src = advance(src, tok.raw.len());
                tokens.push(tok);
                continue;
            }
            if let Some(tok) = self.link(src) {
                src = advance(src, tok.raw.len());
                tokens.push(tok);
                continue;
            }
            if let Some(tok) = self.reflink(src) {
                src = advance(src, tok.raw.len());
                match tokens.last_mut() {
                    Some(last) if tok.kind == T::Text && last.kind == T::Text => {
                        last.raw.push_str(&tok.raw);
                        last.text.push_str(&tok.text);
                    }
                    _ => tokens.push(tok),
                }
                continue;
            }
            let em = if self.options.cjk_friendly {
                self.em_strong_cjk(src, &masked, &prev_char)
            } else {
                self.em_strong(src, &masked, &prev_char)
            };
            if let Some(tok) = em {
                src = advance(src, tok.raw.len());
                tokens.push(tok);
                continue;
            }
            if let Some(tok) = self.codespan(src) {
                src = advance(src, tok.raw.len());
                tokens.push(tok);
                continue;
            }
            if let Some(tok) = self.br(src) {
                src = advance(src, tok.raw.len());
                tokens.push(tok);
                continue;
            }
            let del = if self.options.strict_strikethrough {
                self.del_strict(src)
            } else {
                self.del(src, &masked, &prev_char)
            };
            if let Some(tok) = del {
                src = advance(src, tok.raw.len());
                tokens.push(tok);
                continue;
            }
            if let Some(tok) = self.autolink(src) {
                src = advance(src, tok.raw.len());
                tokens.push(tok);
                continue;
            }
            if !self.state.in_link
                && let Some(tok) = self.url(src)
            {
                src = advance(src, tok.raw.len());
                tokens.push(tok);
                continue;
            }

            let mut cut_src = src;
            if self.options.latex {
                let first_len = src.chars().next().map_or(0, char::len_utf8);
                if let Some(start) = latex_inline_start(&src[first_len..]) {
                    cut_src = &src[..start + first_len];
                }
            }
            if let Some(tok) = self.inline_text(cut_src) {
                src = advance(src, tok.raw.len());
                if !tok.raw.ends_with('_') {
                    prev_char = last_char_str(&tok.raw);
                }
                keep_prev_char = true;
                match tokens.last_mut() {
                    Some(last) if last.kind == T::Text => {
                        last.raw.push_str(&tok.raw);
                        last.text.push_str(&tok.text);
                    }
                    _ => tokens.push(tok),
                }
                continue;
            }

            if !src.is_empty() {
                break;
            }
        }
        tokens
    }

    fn escape(&self, src: &str) -> Option<Token> {
        let cap = rules().inline.escape.exec(src)?;
        let mut t = Token::new(T::Escape, group(&cap, 0));
        t.text = group(&cap, 1).to_string();
        Some(t)
    }

    fn tag(&mut self, src: &str) -> Option<Token> {
        let other = &rules().other;
        let cap = rules().inline.tag.exec(src)?;
        let raw = group(&cap, 0);
        if !self.state.in_link && other.start_a_tag.test(raw) {
            self.state.in_link = true;
        } else if self.state.in_link && other.end_a_tag.test(raw) {
            self.state.in_link = false;
        }
        if !self.state.in_raw_block && other.start_pre_script_tag.test(raw) {
            self.state.in_raw_block = true;
        } else if self.state.in_raw_block && other.end_pre_script_tag.test(raw) {
            self.state.in_raw_block = false;
        }
        let mut t = Token::new(T::Html, raw);
        t.in_link = self.state.in_link;
        t.in_raw_block = self.state.in_raw_block;
        t.block = false;
        t.text = raw.to_string();
        Some(t)
    }

    fn output_link(&mut self, label: &str, href: String, title: Option<String>, raw: String, is_image: bool) -> Option<Token> {
        let text = rules().other.output_link_replace.replace_all(label, |c| group(c, 1).to_string());
        self.state.in_link = true;
        let outer_link_emitted = self.state.link_emitted;
        let outer_in_raw_block = self.state.in_raw_block;
        self.state.link_emitted = false;
        let tokens = self.inline_tokens(&text);
        let text_has_link = self.state.link_emitted;
        self.state.link_emitted = outer_link_emitted;
        self.state.in_link = false;
        if !is_image {
            if text_has_link {
                self.state.in_raw_block = outer_in_raw_block;
                return None;
            }
            self.state.link_emitted = true;
        }
        let mut t = Token::new(if is_image { T::Image } else { T::Link }, raw);
        t.href = href;
        t.title = title.filter(|t| !t.is_empty());
        t.text = text;
        t.tokens = tokens;
        Some(t)
    }

    fn link(&mut self, src: &str) -> Option<Token> {
        let r = rules();
        let cap = r.inline.link.exec(src)?;
        let mut cap0 = group(&cap, 0).to_string();
        let cap1 = group(&cap, 1).to_string();
        let mut cap2 = group(&cap, 2).to_string();
        let mut cap3 = group(&cap, 3).to_string();
        drop(cap);
        let trimmed_url = js_trim(&cap2).to_string();
        if r.other.start_angle_bracket.test(&trimmed_url) {
            if !r.other.end_angle_bracket.test(&trimmed_url) {
                return None;
            }
            let rtrim_slash = rtrim(&trimmed_url[..trimmed_url.len() - 1], '\\');
            if (trimmed_url.len() - rtrim_slash.len()).is_multiple_of(2) {
                return None;
            }
        } else {
            let last_paren_index = find_closing_bracket(&cap2, b'(', b')');
            if last_paren_index == -2 {
                return None;
            }
            if last_paren_index > -1 {
                let start = if cap0.starts_with('!') { 5 } else { 4 };
                let link_len = start + cap1.len() + last_paren_index as usize;
                cap2 = cap2[..last_paren_index as usize].to_string();
                cap0 = js_trim(&cap0[..link_len.min(cap0.len())]).to_string();
                cap3 = String::new();
            }
        }
        let mut href = js_trim(&cap2).to_string();
        let title = if cap3.is_empty() { String::new() } else { cap3[1..cap3.len() - 1].to_string() };
        if r.other.start_angle_bracket.test(&href) {
            href = href[1..href.len().saturating_sub(1).max(1)].to_string();
        }
        let unescape = |s: &str| r.inline.any_punctuation.replace_all(s, |c| group(c, 1).to_string());
        let href = if href.is_empty() { href } else { unescape(&href) };
        let title = if title.is_empty() { title } else { unescape(&title) };
        let is_image = cap0.starts_with('!');
        self.output_link(&cap1, href, Some(title), cap0, is_image)
    }

    fn reflink(&mut self, src: &str) -> Option<Token> {
        let r = rules();
        let cap = r.inline.reflink.exec(src).or_else(|| r.inline.nolink.exec(src))?;
        let cap0 = group(&cap, 0).to_string();
        let cap1 = group(&cap, 1).to_string();
        let link_string = if has(&cap, 2) { group(&cap, 2) } else { group(&cap, 1) };
        let link_string = r.other.multiple_space_global.replace_all_str(link_string, " ");
        drop(cap);
        let Some((href, title)) = self.links.get(&link_string.to_lowercase()).cloned() else {
            let text: String = cap0.chars().next().map(String::from).unwrap_or_default();
            let mut t = Token::new(T::Text, text.clone());
            t.text = text;
            return Some(t);
        };
        let is_image = cap0.starts_with('!');
        self.output_link(&cap1, href, title, cap0, is_image)
    }

    fn em_strong(&mut self, src: &str, masked_src: &str, prev_char: &str) -> Option<Token> {
        let r = rules();
        let m = r.inline.em_strong_l_delim.exec(src)?;
        if !(has(&m, 1) || has(&m, 2) || has(&m, 3) || has(&m, 4)) {
            return None;
        }
        if has(&m, 4) && r.other.unicode_alpha_numeric.test(prev_char) {
            return None;
        }
        let next_char = if has(&m, 1) { group(&m, 1) } else { group(&m, 3) };
        if !(next_char.is_empty() || prev_char.is_empty() || r.inline.punctuation.test(prev_char)) {
            return None;
        }
        let m0 = group(&m, 0);
        let l_length = m0.chars().count() - 1;
        let delim_char = m0.chars().next().unwrap();
        let mid_run = prev_char.starts_with(delim_char) && prev_char.chars().count() == 1;
        let end_reg = if delim_char == '*' { &r.inline.em_strong_r_delim_ast } else { &r.inline.em_strong_r_delim_und };
        let clipped = clip_masked(masked_src, src, l_length);
        let mut delim_total = l_length as isize;
        let mut mid_delim_total = 0isize;
        let mut pos = 0;
        while let Some(rm) = end_reg.exec_at(clipped, pos) {
            let whole = rm.get(0).unwrap();
            pos = if whole.end() > whole.start() { whole.end() } else { whole.end() + 1 };
            let Some(r_delim) = first_group(&rm) else { continue };
            let mut r_length = r_delim.chars().count() as isize;
            if has(&rm, 3) || has(&rm, 4) {
                delim_total += r_length;
                continue;
            } else if has(&rm, 5) || has(&rm, 6) {
                if !l_length.is_multiple_of(3) && (l_length as isize + r_length) % 3 == 0 {
                    mid_delim_total += r_length;
                    continue;
                }
                if mid_run {
                    break;
                }
            }
            delim_total -= r_length;
            if delim_total > 0 {
                continue;
            }
            r_length = r_length.min(r_length + delim_total + mid_delim_total);
            let last_char_length = whole.as_str().chars().next().map_or(1, char::len_utf8);
            let end = l_length + whole.start() + last_char_length + r_length as usize;
            return Some(self.em_or_strong(src, end, l_length, r_length as usize));
        }
        None
    }

    fn em_strong_cjk(&mut self, src: &str, masked_src: &str, prev_char: &str) -> Option<Token> {
        let r = rules();
        let m = r.inline.em_strong_l_delim.exec(src)?;
        if !(has(&m, 1) || has(&m, 2) || has(&m, 3) || has(&m, 4)) {
            return None;
        }
        if has(&m, 4) && r.other.unicode_alpha_numeric.test(prev_char) {
            // returns `false`; marked's own tokenizer then also declines
            return None;
        }
        let next_char = if has(&m, 1) { group(&m, 1) } else { group(&m, 3) };
        let prev_is_cjk = is_prev_char_cjk(prev_char, masked_src, src);
        if !(next_char.is_empty()
            || prev_char.is_empty()
            || r.inline.punctuation.test(prev_char)
            || prev_is_cjk
            || CJK_RULES.test.test(next_char))
        {
            return None;
        }
        let m0 = group(&m, 0);
        let l_length = m0.chars().count() - 1;
        let delim_char = m0.chars().next().unwrap();
        let prev_char_is_same_delimiter = prev_char.starts_with(delim_char) && prev_char.chars().count() == 1;
        let end_reg = if delim_char == '*' { &CJK_RULES.r_delim_ast_gfm } else { &CJK_RULES.r_delim_und };
        let clipped = clip_masked(masked_src, src, l_length);
        let mut delim_total = l_length as isize;
        let mut mid_delim_total = 0isize;
        let mut pos = 0;
        while let Some(rm) = end_reg.exec_at(clipped, pos) {
            let whole = rm.get(0).unwrap();
            pos = if whole.end() > whole.start() { whole.end() } else { whole.end() + 1 };
            let Some(r_delim) = first_group(&rm) else { continue };
            let mut r_length = r_delim.chars().count() as isize;
            let is_cjk_adjacent = is_cjk_adjacent_to_delimiter(&rm, clipped);
            let is_left_only = has(&rm, 3) || has(&rm, 4);
            let is_both = has(&rm, 5) || has(&rm, 6);
            if is_left_only && !is_cjk_adjacent {
                delim_total += r_length;
                continue;
            }
            if is_both || is_cjk_adjacent {
                if !l_length.is_multiple_of(3) && (l_length as isize + r_length) % 3 == 0 {
                    mid_delim_total += r_length;
                    continue;
                }
                if prev_char_is_same_delimiter && is_both {
                    break;
                }
            }
            delim_total -= r_length;
            if delim_total > 0 {
                continue;
            }
            r_length = r_length.min(r_length + delim_total + mid_delim_total);
            let last_char_length = whole.as_str().chars().next().map_or(1, char::len_utf8);
            let end = l_length + whole.start() + last_char_length + r_length as usize;
            return Some(self.em_or_strong(src, end, l_length, r_length as usize));
        }
        None
    }

    fn em_or_strong(&mut self, src: &str, end: usize, l_length: usize, r_length: usize) -> Token {
        let mut end = end.min(src.len());
        while !src.is_char_boundary(end) {
            end -= 1;
        }
        let raw = &src[..end];
        if l_length.min(r_length) % 2 == 1 {
            let text = raw.get(1..raw.len().saturating_sub(1)).unwrap_or("").to_string();
            let mut t = Token::new(T::Em, raw);
            t.tokens = self.inline_tokens(&text);
            t.text = text;
            return t;
        }
        let text = raw.get(2..raw.len().saturating_sub(2)).unwrap_or("").to_string();
        let mut t = Token::new(T::Strong, raw);
        t.tokens = self.inline_tokens(&text);
        t.text = text;
        t
    }

    fn codespan(&self, src: &str) -> Option<Token> {
        let other = &rules().other;
        let cap = rules().inline.code.exec(src)?;
        let mut text = group(&cap, 2).replace('\n', " ");
        let has_non_space = other.non_space_char.test(&text);
        let both_ends = text.starts_with(' ') && text.ends_with(' ');
        if has_non_space && both_ends {
            text = text[1..text.len() - 1].to_string();
        }
        let mut t = Token::new(T::Codespan, group(&cap, 0));
        t.text = text;
        Some(t)
    }

    fn br(&self, src: &str) -> Option<Token> {
        let cap = rules().inline.br.exec(src)?;
        Some(Token::new(T::Br, group(&cap, 0)))
    }

    fn del_strict(&mut self, src: &str) -> Option<Token> {
        let m = STRICT_STRIKETHROUGH.exec(src)?;
        let raw = group(&m, 0).to_string();
        let text = group(&m, 2).to_string();
        drop(m);
        let mut t = Token::new(T::Del, raw);
        t.tokens = self.inline_tokens(&text);
        t.text = text;
        Some(t)
    }

    fn del(&mut self, src: &str, masked_src: &str, prev_char: &str) -> Option<Token> {
        let r = rules();
        let m = r.inline.del_l_delim.exec(src)?;
        let next_char = group(&m, 1);
        if !(next_char.is_empty() || prev_char.is_empty() || r.inline.punctuation.test(prev_char)) {
            return None;
        }
        let l_length = group(&m, 0).chars().count() - 1;
        let clipped = clip_masked(masked_src, src, l_length);
        let mut delim_total = l_length as isize;
        let mut pos = 0;
        while let Some(rm) = r.inline.del_r_delim.exec_at(clipped, pos) {
            let whole = rm.get(0).unwrap();
            pos = if whole.end() > whole.start() { whole.end() } else { whole.end() + 1 };
            let Some(r_delim) = first_group(&rm) else { continue };
            let mut r_length = r_delim.chars().count() as isize;
            if r_length != l_length as isize {
                continue;
            }
            if has(&rm, 3) || has(&rm, 4) {
                delim_total += r_length;
                continue;
            }
            delim_total -= r_length;
            if delim_total > 0 {
                continue;
            }
            r_length = r_length.min(r_length + delim_total);
            let last_char_length = whole.as_str().chars().next().map_or(1, char::len_utf8);
            let end = (l_length + whole.start() + last_char_length + r_length as usize).min(src.len());
            let raw = src[..end].to_string();
            let text = raw.get(l_length..raw.len().saturating_sub(l_length)).unwrap_or("").to_string();
            let mut t = Token::new(T::Del, raw);
            t.tokens = self.inline_tokens(&text);
            t.text = text;
            return Some(t);
        }
        None
    }

    fn autolink(&self, src: &str) -> Option<Token> {
        let cap = rules().inline.autolink.exec(src)?;
        let text = group(&cap, 1).to_string();
        let href = if group(&cap, 2) == "@" { format!("mailto:{text}") } else { text.clone() };
        let mut t = Token::new(T::Link, group(&cap, 0));
        t.href = href;
        let mut inner = Token::new(T::Text, text.clone());
        inner.text = text.clone();
        t.tokens = vec![inner];
        t.text = text;
        Some(t)
    }

    fn url(&self, src: &str) -> Option<Token> {
        let inline = &rules().inline;
        let cap = inline.url.exec(src)?;
        let (text, href, raw);
        if group(&cap, 2) == "@" {
            text = group(&cap, 0).to_string();
            href = format!("mailto:{text}");
            raw = text.clone();
        } else {
            let mut cap0 = group(&cap, 0).to_string();
            loop {
                let prev = cap0.clone();
                cap0 = inline.backpedal.exec(&cap0).map(|m| group(&m, 0).to_string()).unwrap_or_default();
                if prev == cap0 {
                    break;
                }
            }
            text = cap0.clone();
            href = if group(&cap, 1) == "www." { format!("http://{cap0}") } else { cap0.clone() };
            raw = cap0;
        }
        if raw.is_empty() {
            return None;
        }
        let mut t = Token::new(T::Link, raw);
        t.href = href;
        let mut inner = Token::new(T::Text, text.clone());
        inner.text = text.clone();
        t.tokens = vec![inner];
        t.text = text;
        Some(t)
    }

    fn inline_text(&self, src: &str) -> Option<Token> {
        let cap = rules().inline.text.exec(src)?;
        let raw = group(&cap, 0);
        if raw.is_empty() {
            return None;
        }
        let mut t = Token::new(T::Text, raw);
        t.text = raw.to_string();
        t.escaped = self.state.in_raw_block;
        Some(t)
    }
}
