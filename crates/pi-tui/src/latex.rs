//! Render basic LaTeX math as terminal-friendly Unicode text (port of `latex.ts`).

use std::sync::LazyLock;

use crate::marked::js_regex::{JsRegex, group};
use crate::utils::{js_trim, js_trim_end, js_trim_start, visible_width};

fn lookup(table: &[(&'static str, &'static str)], key: &str) -> Option<&'static str> {
    table.iter().find(|(k, _)| *k == key).map(|(_, v)| *v)
}

fn contains(table: &[&str], key: &str) -> bool {
    table.contains(&key)
}

const NEGATIVE_SPACE: &str = "\u{0}";
const NAMED_OPERATOR_START: &str = "\u{f0004}";
const NAMED_OPERATOR_END: &str = "\u{f0005}";
const LAYOUT_MARKER_START: &str = "\u{f0000}";
const LAYOUT_MARKER_END: &str = "\u{f0001}";
const PROTECTED_SPACE: &str = "\u{f0002}";

static NAMED_OPERATOR_LEFT_SPACING: LazyLock<JsRegex> =
    LazyLock::new(|| JsRegex::new(r"(?<=[\p{L}\p{N})\]}\u{f0001}])\u{f0004}", "gu"));
static NAMED_OPERATOR_RIGHT_SPACING: LazyLock<JsRegex> =
    LazyLock::new(|| JsRegex::new(r"\u{f0005}(?=[\p{L}\p{N}√\u{f0000}])", "gu"));
static LAYOUT_MARKER: LazyLock<JsRegex> = LazyLock::new(|| JsRegex::new(r"\u{f0000}(\d+)\u{f0001}", "gu"));
static TRAILING_LAYOUT_MARKER: LazyLock<JsRegex> = LazyLock::new(|| JsRegex::new(r"\u{f0000}(\d+)\u{f0001}$", "u"));
static SCRIPT_OPERATOR_SPACING: LazyLock<JsRegex> = LazyLock::new(|| JsRegex::new(r"\s*([=+-])\s*", "g"));
static SPACES_TABS: LazyLock<JsRegex> = LazyLock::new(|| JsRegex::new(r"[ \t]+", "g"));
static SIMPLE_NUMERATOR: LazyLock<JsRegex> = LazyLock::new(|| JsRegex::new(r"^[\p{L}\p{N}.]+$", "u"));
static SIMPLE_DENOMINATOR: LazyLock<JsRegex> = LazyLock::new(|| JsRegex::new(r"^[\p{N}.]+$", "u"));
static ASCII_LETTERS: LazyLock<JsRegex> = LazyLock::new(|| JsRegex::new(r"^[A-Za-z]+$", ""));
static UPPER_OR_STAR: LazyLock<JsRegex> = LazyLock::new(|| JsRegex::new(r"[A-Z*∗]", ""));
static LIMITS_MODIFIER: LazyLock<JsRegex> = LazyLock::new(|| JsRegex::new(r"^\\(limits|nolimits)(?![A-Za-z])", ""));
static ENV_ROW_SPLIT: LazyLock<JsRegex> = LazyLock::new(|| JsRegex::new(r"\\\\(?:\[[^\]\n]*\])?", ""));
static LEADING_BRACE_GROUP: LazyLock<JsRegex> = LazyLock::new(|| JsRegex::new(r"^\s*\{[^}]*\}", ""));
static TRAILING_COMMA: LazyLock<JsRegex> = LazyLock::new(|| JsRegex::new(r",\s*$", ""));
static CONDITION_PREFIX: LazyLock<JsRegex> = LazyLock::new(|| JsRegex::new(r"^(?:if|when|for|otherwise)\b", "i"));

/// JavaScript `\s`.
fn is_js_space(c: char) -> bool {
    matches!(
        c,
        '\t' | '\n'
            | '\u{0B}'
            | '\u{0C}'
            | '\r'
            | ' '
            | '\u{A0}'
            | '\u{1680}'
            | '\u{2000}'..='\u{200A}'
            | '\u{2028}'
            | '\u{2029}'
            | '\u{202F}'
            | '\u{205F}'
            | '\u{3000}'
            | '\u{FEFF}'
    )
}

fn replace_characters(value: &str, replacements: &[(&'static str, &'static str)]) -> Option<String> {
    let mut result = String::new();
    for c in value.chars() {
        result.push_str(lookup(replacements, c.encode_utf8(&mut [0; 4]))?);
    }
    Some(result)
}

fn normalize_script_value(value: &str) -> String {
    SCRIPT_OPERATOR_SPACING.replace_all(js_trim(value), |c| group(c, 1).to_string())
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum ScriptKind {
    Sub,
    Sup,
}

fn format_unicode_script(value: &str, kind: ScriptKind) -> Option<String> {
    replace_characters(&normalize_script_value(value), if kind == ScriptKind::Sub { SUBSCRIPTS } else { SUPERSCRIPTS })
}

fn format_script(value: &str, kind: ScriptKind) -> String {
    let value = normalize_script_value(value);
    if let Some(unicode) = format_unicode_script(&value, kind) {
        return unicode;
    }
    let prefix = if kind == ScriptKind::Sub { "_" } else { "^" };
    if value.chars().count() == 1 || (kind == ScriptKind::Sub && ASCII_LETTERS.test(&value)) {
        return format!("{prefix}{value}");
    }
    format!("{prefix}({value})")
}

fn format_fraction(numerator: &str, denominator: &str) -> String {
    let numerator = js_trim(numerator);
    let denominator = js_trim(denominator);
    let simple_numerator = SIMPLE_NUMERATOR.test(numerator);
    let simple_denominator = SIMPLE_DENOMINATOR.test(denominator) || denominator.chars().count() == 1;
    let n = if simple_numerator { numerator.to_string() } else { format!("({numerator})") };
    let d = if simple_denominator { denominator.to_string() } else { format!("({denominator})") };
    format!("{n}/{d}")
}

fn format_root(value: &str, symbol: &str) -> String {
    let value = js_trim(value);
    if SIMPLE_NUMERATOR.test(value) { format!("{symbol}{value}") } else { format!("{symbol}({value})") }
}

fn normalize_output(value: &str) -> String {
    let v = NAMED_OPERATOR_LEFT_SPACING.replace_all_str(value, " ");
    let v = v.replace(NAMED_OPERATOR_START, "");
    let v = NAMED_OPERATOR_RIGHT_SPACING.replace_all_str(&v, " ");
    let v = v.replace(NAMED_OPERATOR_END, "");
    let lines: Vec<String> = v.split('\n').map(|line| js_trim(&SPACES_TABS.replace_all_str(line, " ")).to_string()).collect();
    let n = lines.len();
    let kept: Vec<&str> = lines
        .iter()
        .enumerate()
        .filter(|(i, l)| !l.is_empty() || (*i > 0 && *i < n - 1))
        .map(|(_, l)| l.as_str())
        .collect();
    js_trim(&kept.join("\n")).to_string()
}

#[derive(Debug, Clone)]
enum LayoutNode {
    Fraction { numerator: String, denominator: String },
    Operator { operator: String, lower: Option<String>, upper: Option<String> },
    Script { lower: Option<String>, upper: Option<String> },
    Matrix { lines: Vec<String>, baseline: usize },
}

struct Layout {
    lines: Vec<String>,
    width: usize,
    baseline: usize,
}

fn pad_layout_line(line: &str, width: usize, centered: bool) -> String {
    let padding = width.saturating_sub(visible_width(line));
    let left = if centered { padding / 2 } else { 0 };
    format!("{}{line}{}", " ".repeat(left), " ".repeat(padding - left))
}

fn join_layouts(layouts: &[Layout]) -> Layout {
    if layouts.is_empty() {
        return Layout { lines: vec![String::new()], width: 0, baseline: 0 };
    }
    let baseline = layouts.iter().map(|l| l.baseline).max().unwrap_or(0) as isize;
    let below = layouts.iter().map(|l| l.lines.len() as isize - l.baseline as isize - 1).max().unwrap_or(0);
    let mut lines = Vec::new();
    let mut row = 0isize;
    while row <= baseline + below {
        let mut line = String::new();
        for layout in layouts {
            let source_row = row - baseline + layout.baseline as isize;
            if source_row >= 0 && (source_row as usize) < layout.lines.len() {
                line.push_str(&pad_layout_line(&layout.lines[source_row as usize], layout.width, false));
            } else {
                line.push_str(&" ".repeat(layout.width));
            }
        }
        lines.push(js_trim_end(&line).to_string());
        row += 1;
    }
    Layout { lines, width: layouts.iter().map(|l| l.width).sum(), baseline: baseline as usize }
}

fn text_layout(text: String) -> Layout {
    let width = visible_width(&text);
    Layout { lines: vec![text], width, baseline: 0 }
}

fn render_layout(source: &str, nodes: &[LayoutNode]) -> Layout {
    let mut rendered_lines: Vec<String> = Vec::new();
    let mut first_baseline = 0;
    for source_line in source.split('\n') {
        let mut layouts: Vec<Layout> = Vec::new();
        let mut position = 0;
        let mut previous_node: Option<&LayoutNode> = None;
        for m in LAYOUT_MARKER.match_all(source_line) {
            let whole = m.get(0).unwrap();
            let index = whole.start();
            let Some(node) = group(&m, 1).parse::<usize>().ok().and_then(|i| nodes.get(i)) else { continue };
            if index > position {
                let sliced = &source_line[position..index];
                let trimmed = js_trim_end(if previous_node.is_some() { js_trim_start(sliced) } else { sliced });
                let preserve_leading = matches!(previous_node, Some(LayoutNode::Matrix { .. }))
                    && sliced.chars().next().is_some_and(is_js_space);
                let preserve_trailing =
                    matches!(node, LayoutNode::Matrix { .. }) && sliced.chars().next_back().is_some_and(is_js_space);
                let text = if !trimmed.is_empty() {
                    format!(
                        "{}{trimmed}{}",
                        if preserve_leading { " " } else { "" },
                        if preserve_trailing { " " } else { "" }
                    )
                } else if preserve_leading || preserve_trailing {
                    " ".to_string()
                } else {
                    String::new()
                };
                layouts.push(text_layout(text));
            }
            match node {
                LayoutNode::Fraction { numerator, denominator } => {
                    let numerator = render_layout(numerator, nodes);
                    let denominator = render_layout(denominator, nodes);
                    let content_width = numerator.width.max(denominator.width).max(1);
                    let width = content_width + 2;
                    let mut lines: Vec<String> = numerator.lines.iter().map(|l| pad_layout_line(l, width, true)).collect();
                    lines.push(format!(" {} ", "─".repeat(content_width)));
                    lines.extend(denominator.lines.iter().map(|l| pad_layout_line(l, width, true)));
                    layouts.push(Layout { lines, width, baseline: numerator.lines.len() });
                }
                LayoutNode::Operator { operator, lower, upper } => {
                    let content_width = visible_width(operator)
                        .max(lower.as_deref().map_or(0, visible_width))
                        .max(upper.as_deref().map_or(0, visible_width));
                    let mut lines = Vec::new();
                    if let Some(u) = upper {
                        lines.push(format!("{} ", pad_layout_line(u, content_width, true)));
                    }
                    lines.push(format!("{} ", pad_layout_line(operator, content_width, true)));
                    if let Some(l) = lower {
                        lines.push(format!("{} ", pad_layout_line(l, content_width, true)));
                    }
                    layouts.push(Layout { lines, width: content_width + 1, baseline: usize::from(upper.is_some()) });
                }
                LayoutNode::Script { lower, upper } => {
                    let upper = upper.as_deref().map(|u| render_layout(u, nodes));
                    let lower = lower.as_deref().map(|l| render_layout(l, nodes));
                    let width = upper.as_ref().map_or(0, |u| u.width).max(lower.as_ref().map_or(0, |l| l.width));
                    let mut lines: Vec<String> = upper
                        .as_ref()
                        .map(|u| u.lines.iter().map(|l| pad_layout_line(l, width, false)).collect())
                        .unwrap_or_default();
                    lines.push(" ".repeat(width));
                    if let Some(lower) = &lower {
                        lines.extend(lower.lines.iter().map(|l| pad_layout_line(l, width, false)));
                    }
                    layouts.push(Layout { lines, width, baseline: upper.as_ref().map_or(0, |u| u.lines.len()) });
                }
                LayoutNode::Matrix { lines, baseline } => {
                    let width = lines.iter().map(|l| visible_width(l)).max().unwrap_or(0);
                    layouts.push(Layout {
                        lines: lines.iter().map(|l| pad_layout_line(l, width, false)).collect(),
                        width,
                        baseline: *baseline,
                    });
                }
            }
            position = whole.end();
            previous_node = Some(node);
        }
        if position < source_line.len() {
            let sliced = &source_line[position..];
            let trimmed = if previous_node.is_some() { js_trim_start(sliced) } else { sliced };
            let text = if matches!(previous_node, Some(LayoutNode::Matrix { .. }))
                && sliced.chars().next().is_some_and(is_js_space)
            {
                format!(" {trimmed}")
            } else {
                trimmed.to_string()
            };
            layouts.push(text_layout(text));
        }
        let line_layout = join_layouts(&layouts);
        if rendered_lines.is_empty() {
            first_baseline = line_layout.baseline;
        }
        rendered_lines.extend(line_layout.lines);
    }
    let width = rendered_lines.iter().map(|l| visible_width(l)).max().unwrap_or(0);
    Layout { lines: rendered_lines, width, baseline: first_baseline }
}

struct LatexParser<'a> {
    source: Vec<char>,
    layout_nodes: &'a mut Vec<LayoutNode>,
    display: bool,
    position: usize,
    supported: bool,
    stack_fractions: bool,
    script_depth: usize,
}

fn push_node(nodes: &mut Vec<LayoutNode>, node: LayoutNode) -> String {
    nodes.push(node);
    format!("{LAYOUT_MARKER_START}{}{LAYOUT_MARKER_END}", nodes.len() - 1)
}

impl<'a> LatexParser<'a> {
    fn new(source: &str, layout_nodes: &'a mut Vec<LayoutNode>, display: bool) -> Self {
        Self {
            source: source.chars().collect(),
            layout_nodes,
            display,
            position: 0,
            supported: true,
            stack_fractions: true,
            script_depth: 0,
        }
    }

    fn at(&self, i: usize) -> Option<char> {
        self.source.get(i).copied()
    }

    fn slice(&self, from: usize, to: usize) -> String {
        self.source[from.min(self.source.len())..to.min(self.source.len())].iter().collect()
    }

    fn index_of(&self, needle: &str, from: usize) -> Option<usize> {
        let needle: Vec<char> = needle.chars().collect();
        if needle.is_empty() {
            return Some(from);
        }
        (from..self.source.len()).find(|&i| self.source[i..].starts_with(&needle))
    }

    fn render(mut self) -> Option<String> {
        let rendered = self.parse_sequence(None);
        if !self.supported || self.position != self.source.len() {
            return None;
        }
        Some(normalize_output(&rendered))
    }

    fn parse_sequence(&mut self, end_character: Option<char>) -> String {
        let mut result = String::new();
        while self.position < self.source.len() {
            let character = self.source[self.position];
            if end_character == Some(character) {
                self.position += 1;
                return result;
            }
            if character == '}' {
                self.supported = false;
                return result;
            }
            if character == '{' {
                self.position += 1;
                result.push_str(&self.parse_sequence(Some('}')));
                continue;
            }
            if character == '\\' {
                let command = self.parse_command();
                if command == NEGATIVE_SPACE {
                    result = js_trim_end(&result).to_string();
                    if let Some(stripped) = result.strip_suffix(NAMED_OPERATOR_END) {
                        result = stripped.to_string();
                    }
                } else {
                    result.push_str(&command);
                }
                continue;
            }
            if character == '^' || character == '_' {
                self.position += 1;
                result = js_trim_end(&result).to_string();
                let script = self.parse_scripts(character);
                if let Some(stripped) = result.strip_suffix(NAMED_OPERATOR_END) {
                    result = format!("{stripped}{script}{NAMED_OPERATOR_END}");
                } else {
                    result.push_str(&script);
                }
                continue;
            }
            if is_js_space(character) {
                result.push_str(&self.parse_whitespace());
                continue;
            }
            if character == '=' || character == '<' || character == '>' {
                result = format!("{} {character} ", js_trim_end(&result));
                self.position += 1;
                continue;
            }
            if character == '&' {
                self.position += 1;
                continue;
            }
            if character == '~' {
                self.position += 1;
                result.push(' ');
                continue;
            }
            if character == '.'
                && let Some(marker) = TRAILING_LAYOUT_MARKER.exec(&result)
                && let Ok(index) = group(&marker, 1).parse::<usize>()
                && let Some(LayoutNode::Matrix { lines, .. }) = self.layout_nodes.get_mut(index)
            {
                if let Some(last) = lines.last_mut() {
                    last.push(character);
                }
                self.position += 1;
                continue;
            }
            result.push(character);
            self.position += 1;
        }
        if end_character.is_some() {
            self.supported = false;
        }
        result
    }

    fn parse_scripts(&mut self, initial_marker: char) -> String {
        let mut sub: Option<String> = None;
        let mut sup: Option<String> = None;
        let mut order: Vec<ScriptKind> = Vec::new();
        let mut parse = |this: &mut Self, marker: char| {
            let kind = if marker == '_' { ScriptKind::Sub } else { ScriptKind::Sup };
            this.script_depth += 1;
            let value = this.parse_required_argument(false);
            this.script_depth -= 1;
            if kind == ScriptKind::Sub {
                sub = Some(value);
            } else {
                sup = Some(value);
            }
            order.push(kind);
        };
        parse(self, initial_marker);
        let mut next_position = self.position;
        while next_position < self.source.len() && is_js_space(self.source[next_position]) {
            next_position += 1;
        }
        let next_marker = self.at(next_position);
        if let Some(nm) = next_marker
            && (nm == '^' || nm == '_')
            && nm != initial_marker
        {
            self.position = next_position + 1;
            parse(self, nm);
        }

        let sub_unicode = sub.as_deref().and_then(|s| format_unicode_script(s, ScriptKind::Sub));
        let sup_unicode = sup.as_deref().and_then(|s| format_unicode_script(s, ScriptKind::Sup));
        let can_use_layout = ![&sub, &sup].iter().any(|value| {
            value.as_deref().is_some_and(|value| {
                value.contains('/')
                    || (!value.contains(LAYOUT_MARKER_START) && value.chars().count() > 1 && !UPPER_OR_STAR.test(value))
            })
        });
        let needs_layout = self.display
            && can_use_layout
            && (self.script_depth > 0
                || (sub.is_some() && sub_unicode.is_none())
                || (sup.is_some() && sup_unicode.is_none()));
        if !needs_layout {
            return order
                .iter()
                .map(|kind| match kind {
                    ScriptKind::Sub => {
                        sub_unicode.clone().unwrap_or_else(|| format_script(sub.as_deref().unwrap_or(""), *kind))
                    }
                    ScriptKind::Sup => {
                        sup_unicode.clone().unwrap_or_else(|| format_script(sup.as_deref().unwrap_or(""), *kind))
                    }
                })
                .collect();
        }
        push_node(
            self.layout_nodes,
            LayoutNode::Script {
                lower: sub.as_deref().map(normalize_output),
                upper: sup.as_deref().map(normalize_output),
            },
        )
    }

    fn parse_whitespace(&mut self) -> String {
        while self.position < self.source.len() && is_js_space(self.source[self.position]) {
            self.position += 1;
        }
        " ".to_string()
    }

    fn parse_command(&mut self) -> String {
        self.position += 1;
        if self.position >= self.source.len() {
            self.supported = false;
            return String::new();
        }
        let first = self.source[self.position];
        if first == '\n' || first == '\r' {
            self.position += 1;
            if first == '\r' && self.at(self.position) == Some('\n') {
                self.position += 1;
            }
            return " ".to_string();
        }
        let command: String = if first.is_ascii_alphabetic() {
            let start = self.position;
            while self.position < self.source.len() && self.source[self.position].is_ascii_alphabetic() {
                self.position += 1;
            }
            self.slice(start, self.position)
        } else {
            self.position += 1;
            first.to_string()
        };
        let command = command.as_str();

        if command == "\\" {
            return "\n".to_string();
        }
        if contains(SPACING_COMMANDS, command) {
            return " ".to_string();
        }
        if contains(NEGATIVE_SPACING_COMMANDS, command) {
            return NEGATIVE_SPACE.to_string();
        }
        if contains(FONT_SWITCH_COMMANDS, command) {
            while self.position < self.source.len() && is_js_space(self.source[self.position]) {
                self.position += 1;
            }
            return String::new();
        }
        if contains(IGNORED_COMMANDS, command) {
            return String::new();
        }
        if matches!(command, "{" | "}" | "$" | "%" | "#" | "_" | "&") {
            return command.to_string();
        }
        if command == "|" {
            return "‖".to_string();
        }
        if command == "not" {
            let value = js_trim(&self.parse_required_argument(false)).to_string();
            if let Some(negated) = lookup(NEGATED_SYMBOLS, &value) {
                return format!(" {negated} ");
            }
            let mut chars = value.chars();
            let Some(first) = chars.next() else {
                self.supported = false;
                return String::new();
            };
            return format!(" {first}\u{0338}{} ", chars.as_str());
        }
        if contains(LIMIT_OPERATORS, command) {
            return self.parse_operator(command, true, true, true);
        }
        if let Some(symbol) = lookup(SYMBOLS, command) {
            if contains(DISPLAY_LIMIT_SYMBOLS, command) {
                return self.parse_operator(symbol, false, true, false);
            }
            return if command == "cdot" || command == "times" || contains(RELATION_COMMANDS, command) {
                format!(" {symbol} ")
            } else {
                symbol.to_string()
            };
        }
        if contains(NAMED_OPERATORS, command) {
            return format!("{NAMED_OPERATOR_START}{command}{NAMED_OPERATOR_END}");
        }
        if contains(SIZE_COMMANDS, command) {
            return String::new();
        }
        if matches!(command, "left" | "middle" | "right") {
            if self.at(self.position) == Some('.') {
                self.position += 1;
            }
            return String::new();
        }
        if matches!(command, "frac" | "dfrac" | "tfrac") {
            let should_stack = self.display && self.stack_fractions && command != "tfrac";
            let numerator = self.parse_required_argument(!should_stack);
            let denominator = self.parse_required_argument(!should_stack);
            if should_stack {
                return push_node(
                    self.layout_nodes,
                    LayoutNode::Fraction {
                        numerator: normalize_output(&numerator),
                        denominator: normalize_output(&denominator),
                    },
                );
            }
            return format_fraction(&numerator, &denominator);
        }
        if command == "sqrt" {
            let degree = self.parse_optional_argument().map(|d| js_trim(&d).to_string());
            let value = self.parse_required_argument(true);
            return match degree.as_deref() {
                None | Some("2") => format_root(&value, "√"),
                Some("3") => format_root(&value, "∛"),
                Some("4") => format_root(&value, "∜"),
                Some(d) => format!("{}{}", format_script(d, ScriptKind::Sup), format_root(&value, "√")),
            };
        }
        if command == "boxed" || command == "fbox" {
            return format!("[{}]", js_trim(&self.parse_required_argument(true)));
        }
        if matches!(command, "binom" | "dbinom" | "tbinom") {
            let a = self.parse_required_argument(true);
            let b = self.parse_required_argument(true);
            return format!("({a} choose {b})");
        }
        if let Some(accent) = lookup(ACCENTS, command) {
            let value = self.parse_required_argument(true);
            return if value.chars().count() == 1 { format!("{value}{accent}") } else { format!("{command}({value})") };
        }
        if command == "mathbb" {
            let value = self.parse_required_argument(true);
            return value
                .chars()
                .map(|c| lookup(BLACKBOARD, c.encode_utf8(&mut [0; 4])).map_or_else(|| c.to_string(), str::to_string))
                .collect();
        }
        if command == "operatorname" {
            let starred = self.at(self.position) == Some('*');
            if starred {
                self.position += 1;
            }
            let operator = js_trim(&normalize_output(&self.parse_required_argument(true))).to_string();
            return self.parse_operator(&operator, true, starred, true);
        }
        if command == "mod" || command == "bmod" {
            return " mod ".to_string();
        }
        if command == "pmod" || command == "pod" {
            let value = js_trim(&self.parse_required_argument(true)).to_string();
            return if command == "pmod" { format!(" (mod {value})") } else { format!(" ({value})") };
        }
        if command == "overset" || command == "stackrel" {
            let upper = self.parse_required_argument(true);
            let value = js_trim(&self.parse_required_argument(true)).to_string();
            return format!("{value}{}", format_script(&upper, ScriptKind::Sup));
        }
        if command == "underset" {
            let lower = self.parse_required_argument(true);
            let value = js_trim(&self.parse_required_argument(true)).to_string();
            return format!("{value}{}", format_script(&lower, ScriptKind::Sub));
        }
        if contains(PLAIN_WRAPPERS, command) {
            let value = self.parse_required_argument(true);
            return if command.starts_with("text") || command == "mbox" { value } else { js_trim(&value).to_string() };
        }
        if command == "begin" {
            return self.parse_environment();
        }
        if command == "end" {
            self.supported = false;
            return String::new();
        }
        self.supported = false;
        format!("\\{command}")
    }

    /// `inline_bracket`: inline lower limits render as `op[lower]` (otherwise as a subscript).
    fn parse_operator(&mut self, operator: &str, inline_bracket: bool, display_limits: bool, spaced: bool) -> String {
        let mut use_display_limits = display_limits;
        let mut modifier_position = self.position;
        while modifier_position < self.source.len() && matches!(self.source[modifier_position], ' ' | '\t') {
            modifier_position += 1;
        }
        let rest = self.slice(modifier_position, self.source.len());
        if let Some(m) = LIMITS_MODIFIER.exec(&rest) {
            use_display_limits = group(&m, 1) == "limits";
            self.position = modifier_position + group(&m, 0).chars().count();
        }

        let mut lower: Option<String> = None;
        let mut upper: Option<String> = None;
        loop {
            let mut script_position = self.position;
            while script_position < self.source.len() && matches!(self.source[script_position], ' ' | '\t') {
                script_position += 1;
            }
            let kind = self.at(script_position);
            if kind != Some('_') && kind != Some('^') {
                break;
            }
            self.position = script_position + 1;
            let value = normalize_output(&self.parse_required_argument(false)).replace(' ', "");
            if kind == Some('_') {
                if lower.is_some() {
                    self.supported = false;
                }
                lower = Some(value);
            } else {
                if upper.is_some() {
                    self.supported = false;
                }
                upper = Some(value);
            }
        }

        if self.display && use_display_limits && (lower.is_some() || upper.is_some()) {
            return push_node(self.layout_nodes, LayoutNode::Operator { operator: operator.to_string(), lower, upper });
        }
        let mut rendered = operator.to_string();
        if let Some(lower) = &lower {
            if inline_bracket {
                rendered.push_str(&format!("[{lower}]"));
            } else {
                rendered.push_str(&format_script(lower, ScriptKind::Sub));
            }
        }
        if let Some(upper) = &upper {
            rendered.push_str(&format_script(upper, ScriptKind::Sup));
        }
        if spaced { format!(" {rendered} ") } else { rendered }
    }

    fn parse_required_argument(&mut self, stack_fractions: bool) -> String {
        let previous = self.stack_fractions;
        self.stack_fractions = previous && stack_fractions;
        let value = self.parse_required_argument_value();
        self.stack_fractions = previous;
        value
    }

    fn parse_required_argument_value(&mut self) -> String {
        while self.position < self.source.len() && is_js_space(self.source[self.position]) {
            self.position += 1;
        }
        if self.position >= self.source.len() {
            self.supported = false;
            return String::new();
        }
        if self.source[self.position] == '{' {
            self.position += 1;
            return self.parse_sequence(Some('}'));
        }
        if self.source[self.position] == '\\' {
            return self.parse_command();
        }
        let value = self.source[self.position];
        self.position += 1;
        value.to_string()
    }

    fn parse_optional_argument(&mut self) -> Option<String> {
        while self.position < self.source.len() && matches!(self.source[self.position], ' ' | '\t') {
            self.position += 1;
        }
        if self.at(self.position) != Some('[') {
            return None;
        }
        let Some(end) = self.index_of("]", self.position + 1) else {
            self.supported = false;
            return None;
        };
        let value = self.slice(self.position + 1, end);
        self.position = end + 1;
        Some(self.render_nested(&value, true))
    }

    fn read_raw_group(&mut self) -> Option<String> {
        while self.position < self.source.len() && matches!(self.source[self.position], ' ' | '\t') {
            self.position += 1;
        }
        if self.at(self.position) != Some('{') {
            self.supported = false;
            return None;
        }
        self.position += 1;
        let start = self.position;
        let mut depth = 1;
        while self.position < self.source.len() {
            let c = self.source[self.position];
            if c == '\\' {
                self.position += 2;
                continue;
            }
            if c == '{' {
                depth += 1;
            }
            if c == '}' {
                depth -= 1;
            }
            if depth == 0 {
                let value = self.slice(start, self.position);
                self.position += 1;
                return Some(value);
            }
            self.position += 1;
        }
        self.supported = false;
        None
    }

    fn split_environment_rows(body: &str) -> Vec<String> {
        ENV_ROW_SPLIT.split(body)
    }

    fn parse_environment(&mut self) -> String {
        let Some(environment) = self.read_raw_group().filter(|e| !e.is_empty()) else {
            return String::new();
        };
        let end_marker = format!("\\end{{{environment}}}");
        let Some(end) = self.index_of(&end_marker, self.position) else {
            self.supported = false;
            return String::new();
        };
        let body = self.slice(self.position, end);
        self.position = end + end_marker.chars().count();
        let env = environment.as_str();

        if matches!(env, "equation" | "equation*" | "displaymath") {
            return js_trim(&self.render_nested(&body, true)).to_string();
        }
        if matches!(
            env,
            "aligned"
                | "align"
                | "align*"
                | "alignedat"
                | "alignat"
                | "alignat*"
                | "gather"
                | "gathered"
                | "multline"
                | "multline*"
                | "split"
        ) {
            let aligned_at = matches!(env, "alignedat" | "alignat" | "alignat*");
            let aligned_body =
                if aligned_at { LEADING_BRACE_GROUP.replace_first(&body, |_| String::new()) } else { body.clone() };
            let rows: Vec<String> = Self::split_environment_rows(&aligned_body)
                .iter()
                .map(|row| {
                    let cells: Vec<&str> = row.split('&').collect();
                    let source = if aligned_at {
                        cells.chunks(2).map(|c| c.join("")).collect::<Vec<_>>().join(" ")
                    } else {
                        cells.join("")
                    };
                    js_trim(&self.render_nested(&source, true)).to_string()
                })
                .filter(|r| !r.is_empty())
                .collect();
            return rows.join("\n");
        }
        if env == "cases" || env == "cases*" {
            return self.render_cases(&body);
        }
        if matches!(env, "array" | "matrix" | "smallmatrix" | "pmatrix" | "bmatrix" | "Bmatrix" | "vmatrix" | "Vmatrix") {
            let matrix_body =
                if env == "array" { LEADING_BRACE_GROUP.replace_first(&body, |_| String::new()) } else { body.clone() };
            return self.render_matrix(env, &matrix_body);
        }
        self.supported = false;
        body
    }

    fn render_cells(&mut self, body: &str) -> Vec<Vec<String>> {
        Self::split_environment_rows(body)
            .iter()
            .map(|row| row.split('&').map(|cell| js_trim(&self.render_nested(cell, false)).to_string()).collect::<Vec<_>>())
            .filter(|row: &Vec<String>| row.iter().any(|c| !c.is_empty()))
            .collect()
    }

    fn render_cases(&mut self, body: &str) -> String {
        let rows = self.render_cells(body);
        let strip_comma = |s: &str| TRAILING_COMMA.replace_first(s, |_| String::new());
        let value_width = rows.iter().map(|r| visible_width(&strip_comma(r.first().map_or("", |s| s)))).max().unwrap_or(0);
        let contents: Vec<String> = rows
            .iter()
            .map(|row| {
                let value = strip_comma(row.first().map_or("", |s| s));
                let condition = row.get(1).map_or("", |s| s.as_str());
                if condition.is_empty() {
                    return value;
                }
                let prefix = if CONDITION_PREFIX.test(condition) { " " } else { " if " };
                format!("{value}{}{prefix}{condition}", PROTECTED_SPACE.repeat(value_width - visible_width(&value)))
            })
            .collect();
        if contents.len() <= 1 {
            return contents.first().map_or(String::new(), |c| format!("⎧ {c}"));
        }
        let middle = contents.len() / 2;
        let mut visual_rows: Vec<Option<String>> = contents.into_iter().map(Some).collect();
        if visual_rows.len().is_multiple_of(2) {
            visual_rows.insert(middle, None);
        }
        let n = visual_rows.len();
        let lines: Vec<String> = visual_rows
            .iter()
            .enumerate()
            .map(|(index, content)| {
                let delimiter = if index == 0 {
                    "⎧"
                } else if index == n - 1 {
                    "⎩"
                } else {
                    "⎨"
                };
                match content {
                    None => delimiter.to_string(),
                    Some(c) => format!("{delimiter} {c}"),
                }
            })
            .collect();
        push_node(self.layout_nodes, LayoutNode::Matrix { lines, baseline: middle })
    }

    fn render_matrix(&mut self, environment: &str, body: &str) -> String {
        let matrix = self.render_cells(body);
        let column_count = matrix.iter().map(Vec::len).max().unwrap_or(0);
        let column_widths: Vec<usize> = (0..column_count)
            .map(|c| matrix.iter().map(|r| visible_width(r.get(c).map_or("", |s| s))).max().unwrap_or(0))
            .collect();
        let rows: Vec<String> = matrix
            .iter()
            .map(|row| {
                (0..column_count)
                    .map(|c| {
                        let cell = row.get(c).map_or("", |s| s.as_str());
                        format!("{cell}{}", PROTECTED_SPACE.repeat(column_widths[c].saturating_sub(visible_width(cell))))
                    })
                    .collect::<Vec<_>>()
                    .join(" │ ")
            })
            .collect();
        let lines: Vec<String> = if matches!(environment, "array" | "matrix" | "smallmatrix") {
            rows
        } else {
            let delimiter: [&str; 6] = match environment {
                "pmatrix" => ["⎛", "⎞", "⎜", "⎟", "⎝", "⎠"],
                "bmatrix" => ["⎡", "⎤", "⎢", "⎥", "⎣", "⎦"],
                "Bmatrix" => ["⎧", "⎫", "⎨", "⎬", "⎩", "⎭"],
                "vmatrix" => ["│", "│", "│", "│", "│", "│"],
                "Vmatrix" => ["║", "║", "║", "║", "║", "║"],
                _ => {
                    self.supported = false;
                    return rows.join("\n");
                }
            };
            let n = rows.len();
            rows.iter()
                .enumerate()
                .map(|(index, row)| {
                    let (left, right) = if index == 0 {
                        (delimiter[0], delimiter[1])
                    } else if index == n - 1 {
                        (delimiter[4], delimiter[5])
                    } else {
                        (delimiter[2], delimiter[3])
                    };
                    format!("{left} {row} {right}")
                })
                .collect()
        };
        if lines.len() <= 1 {
            return lines.into_iter().next().unwrap_or_default();
        }
        push_node(self.layout_nodes, LayoutNode::Matrix { lines, baseline: 0 })
    }

    fn render_nested(&mut self, source: &str, stack_fractions: bool) -> String {
        let display = self.display && stack_fractions;
        let rendered = LatexParser::new(source, self.layout_nodes, display).render();
        match rendered {
            Some(r) => r,
            None => {
                self.supported = false;
                source.to_string()
            }
        }
    }
}

/// Options for [`render_latex`].
#[derive(Debug, Clone, Copy, Default)]
pub struct RenderLatexOptions {
    /// Stack fractions and operator limits vertically (display math).
    pub display: bool,
}

/// Render a basic LaTeX math expression as terminal-friendly Unicode text.
/// Returns `None` when the expression contains unsupported or malformed syntax.
pub fn render_latex(source: &str, options: RenderLatexOptions) -> Option<String> {
    let mut layout_nodes: Vec<LayoutNode> = Vec::new();
    let rendered = LatexParser::new(source, &mut layout_nodes, options.display).render()?;
    if layout_nodes.is_empty() {
        return Some(rendered.replace(PROTECTED_SPACE, " "));
    }
    let lines = render_layout(&rendered, &layout_nodes).lines;
    let indentation = lines
        .iter()
        .filter(|l| !js_trim(l).is_empty())
        .map(|l| l.chars().count() - js_trim_start(l).chars().count())
        .min()
        .unwrap_or(0);
    let joined = lines
        .iter()
        .map(|l| js_trim_end(&l.chars().skip(indentation).collect::<String>()).to_string())
        .collect::<Vec<_>>()
        .join("\n");
    Some(js_trim_end(&joined).replace(PROTECTED_SPACE, " "))
}

static SYMBOLS: &[(&str, &str)] = &[
    ("alpha", "α"),
    ("beta", "β"),
    ("gamma", "γ"),
    ("delta", "δ"),
    ("epsilon", "ϵ"),
    ("varepsilon", "ε"),
    ("zeta", "ζ"),
    ("eta", "η"),
    ("theta", "θ"),
    ("vartheta", "ϑ"),
    ("iota", "ι"),
    ("kappa", "κ"),
    ("varkappa", "ϰ"),
    ("lambda", "λ"),
    ("mu", "μ"),
    ("nu", "ν"),
    ("xi", "ξ"),
    ("pi", "π"),
    ("varpi", "ϖ"),
    ("rho", "ρ"),
    ("varrho", "ϱ"),
    ("sigma", "σ"),
    ("varsigma", "ς"),
    ("tau", "τ"),
    ("upsilon", "υ"),
    ("phi", "ϕ"),
    ("varphi", "φ"),
    ("chi", "χ"),
    ("psi", "ψ"),
    ("omega", "ω"),
    ("Gamma", "Γ"),
    ("Delta", "Δ"),
    ("Theta", "Θ"),
    ("Lambda", "Λ"),
    ("Xi", "Ξ"),
    ("Pi", "Π"),
    ("Sigma", "Σ"),
    ("Upsilon", "Υ"),
    ("Phi", "Φ"),
    ("Psi", "Ψ"),
    ("Omega", "Ω"),
    ("pm", "±"),
    ("mp", "∓"),
    ("times", "×"),
    ("div", "÷"),
    ("cdot", "·"),
    ("ast", "∗"),
    ("star", "⋆"),
    ("circ", "∘"),
    ("bullet", "•"),
    ("oplus", "⊕"),
    ("ominus", "⊖"),
    ("otimes", "⊗"),
    ("oslash", "⊘"),
    ("odot", "⊙"),
    ("bigcirc", "○"),
    ("dagger", "†"),
    ("ddagger", "‡"),
    ("amalg", "⨿"),
    ("uplus", "⊎"),
    ("sqcap", "⊓"),
    ("sqcup", "⊔"),
    ("bowtie", "⋈"),
    ("Join", "⋈"),
    ("ltimes", "⋉"),
    ("rtimes", "⋊"),
    ("leftouterjoin", "⟕"),
    ("rightouterjoin", "⟖"),
    ("fullouterjoin", "⟗"),
    ("triangleleft", "◁"),
    ("triangleright", "▷"),
    ("wr", "≀"),
    ("cap", "∩"),
    ("cup", "∪"),
    ("bigcap", "⋂"),
    ("bigcup", "⋃"),
    ("bigwedge", "⋀"),
    ("bigvee", "⋁"),
    ("bigsqcup", "⨆"),
    ("biguplus", "⨄"),
    ("bigoplus", "⨁"),
    ("bigotimes", "⨂"),
    ("bigodot", "⨀"),
    ("setminus", "∖"),
    ("in", "∈"),
    ("notin", "∉"),
    ("ni", "∋"),
    ("subset", "⊂"),
    ("supset", "⊃"),
    ("subseteq", "⊆"),
    ("supseteq", "⊇"),
    ("sqsubset", "⊏"),
    ("sqsupset", "⊐"),
    ("sqsubseteq", "⊑"),
    ("sqsupseteq", "⊒"),
    ("prec", "≺"),
    ("preceq", "≼"),
    ("succ", "≻"),
    ("succeq", "≽"),
    ("ll", "≪"),
    ("gg", "≫"),
    ("le", "≤"),
    ("leq", "≤"),
    ("leqslant", "≤"),
    ("ge", "≥"),
    ("geq", "≥"),
    ("geqslant", "≥"),
    ("ne", "≠"),
    ("neq", "≠"),
    ("equiv", "≡"),
    ("approx", "≈"),
    ("sim", "∼"),
    ("simeq", "≃"),
    ("cong", "≅"),
    ("asymp", "≍"),
    ("doteq", "≐"),
    ("propto", "∝"),
    ("parallel", "∥"),
    ("perp", "⊥"),
    ("mid", "∣"),
    ("vdash", "⊢"),
    ("dashv", "⊣"),
    ("models", "⊨"),
    ("Vdash", "⊩"),
    ("Vvdash", "⊪"),
    ("nvdash", "⊬"),
    ("nvDash", "⊭"),
    ("forall", "∀"),
    ("exists", "∃"),
    ("nexists", "∄"),
    ("neg", "¬"),
    ("land", "∧"),
    ("wedge", "∧"),
    ("lor", "∨"),
    ("vee", "∨"),
    ("to", "→"),
    ("rightarrow", "→"),
    ("longrightarrow", "→"),
    ("leftarrow", "←"),
    ("longleftarrow", "←"),
    ("gets", "←"),
    ("leftrightarrow", "↔"),
    ("longleftrightarrow", "↔"),
    ("hookleftarrow", "↩"),
    ("hookrightarrow", "↪"),
    ("twoheadleftarrow", "↞"),
    ("twoheadrightarrow", "↠"),
    ("leftharpoonup", "↼"),
    ("leftharpoondown", "↽"),
    ("rightharpoonup", "⇀"),
    ("rightharpoondown", "⇁"),
    ("rightleftharpoons", "⇌"),
    ("leftrightharpoons", "⇋"),
    ("nearrow", "↗"),
    ("searrow", "↘"),
    ("swarrow", "↙"),
    ("nwarrow", "↖"),
    ("rightsquigarrow", "⇝"),
    ("leadsto", "⇝"),
    ("Rightarrow", "⇒"),
    ("Longrightarrow", "⇒"),
    ("Leftarrow", "⇐"),
    ("Longleftarrow", "⇐"),
    ("Leftrightarrow", "⇔"),
    ("Longleftrightarrow", "⇔"),
    ("implies", "⇒"),
    ("iff", "⇔"),
    ("mapsto", "↦"),
    ("longmapsto", "↦"),
    ("uparrow", "↑"),
    ("downarrow", "↓"),
    ("partial", "∂"),
    ("nabla", "∇"),
    ("int", "∫"),
    ("iint", "∬"),
    ("iiint", "∭"),
    ("oint", "∮"),
    ("sum", "∑"),
    ("prod", "∏"),
    ("coprod", "∐"),
    ("infty", "∞"),
    ("emptyset", "∅"),
    ("varnothing", "∅"),
    ("angle", "∠"),
    ("therefore", "∴"),
    ("because", "∵"),
    ("aleph", "ℵ"),
    ("beth", "ℶ"),
    ("gimel", "ℷ"),
    ("daleth", "ℸ"),
    ("top", "⊤"),
    ("bot", "⊥"),
    ("triangle", "△"),
    ("square", "□"),
    ("lozenge", "◊"),
    ("checkmark", "✓"),
    ("complement", "∁"),
    ("wp", "℘"),
    ("prime", "′"),
    ("ldots", "…"),
    ("dots", "…"),
    ("cdots", "⋯"),
    ("vdots", "⋮"),
    ("ddots", "⋱"),
    ("ell", "ℓ"),
    ("hbar", "ℏ"),
    ("Im", "ℑ"),
    ("Re", "ℜ"),
    ("langle", "⟨"),
    ("rangle", "⟩"),
    ("vert", "|"),
    ("lvert", "|"),
    ("rvert", "|"),
    ("Vert", "‖"),
    ("lVert", "‖"),
    ("rVert", "‖"),
    ("lbrace", "{"),
    ("rbrace", "}"),
    ("backslash", "\\"),
    ("lfloor", "⌊"),
    ("rfloor", "⌋"),
    ("lceil", "⌈"),
    ("rceil", "⌉"),
    ("colon", ":"),
];

static NEGATED_SYMBOLS: &[(&str, &str)] = &[
    ("<", "≮"),
    (">", "≯"),
    ("=", "≠"),
    ("∈", "∉"),
    ("∋", "∌"),
    ("∣", "∤"),
    ("∥", "∦"),
    ("∼", "≁"),
    ("≃", "≄"),
    ("≅", "≇"),
    ("≈", "≉"),
    ("≡", "≢"),
    ("≤", "≰"),
    ("≥", "≱"),
    ("≺", "⊀"),
    ("≻", "⊁"),
    ("⊂", "⊄"),
    ("⊃", "⊅"),
    ("⊆", "⊈"),
    ("⊇", "⊉"),
    ("⊢", "⊬"),
    ("⊨", "⊭"),
    ("↔", "↮"),
    ("←", "↚"),
    ("→", "↛"),
    ("⇒", "⇏"),
    ("⇐", "⇍"),
    ("⇔", "⇎"),
    ("≼", "⋠"),
    ("≽", "⋡"),
];

static BLACKBOARD: &[(&str, &str)] = &[
    ("C", "ℂ"),
    ("H", "ℍ"),
    ("N", "ℕ"),
    ("P", "ℙ"),
    ("Q", "ℚ"),
    ("R", "ℝ"),
    ("Z", "ℤ"),
];

static SUPERSCRIPTS: &[(&str, &str)] = &[
    ("0", "⁰"),
    ("1", "¹"),
    ("2", "²"),
    ("3", "³"),
    ("4", "⁴"),
    ("5", "⁵"),
    ("6", "⁶"),
    ("7", "⁷"),
    ("8", "⁸"),
    ("9", "⁹"),
    ("+", "⁺"),
    ("-", "⁻"),
    ("=", "⁼"),
    ("(", "⁽"),
    (")", "⁾"),
    ("a", "ᵃ"),
    ("b", "ᵇ"),
    ("c", "ᶜ"),
    ("d", "ᵈ"),
    ("e", "ᵉ"),
    ("f", "ᶠ"),
    ("g", "ᵍ"),
    ("h", "ʰ"),
    ("i", "ⁱ"),
    ("j", "ʲ"),
    ("k", "ᵏ"),
    ("l", "ˡ"),
    ("m", "ᵐ"),
    ("n", "ⁿ"),
    ("o", "ᵒ"),
    ("p", "ᵖ"),
    ("r", "ʳ"),
    ("s", "ˢ"),
    ("t", "ᵗ"),
    ("u", "ᵘ"),
    ("v", "ᵛ"),
    ("w", "ʷ"),
    ("x", "ˣ"),
    ("y", "ʸ"),
    ("z", "ᶻ"),
];

static SUBSCRIPTS: &[(&str, &str)] = &[
    ("0", "₀"),
    ("1", "₁"),
    ("2", "₂"),
    ("3", "₃"),
    ("4", "₄"),
    ("5", "₅"),
    ("6", "₆"),
    ("7", "₇"),
    ("8", "₈"),
    ("9", "₉"),
    ("+", "₊"),
    ("-", "₋"),
    ("=", "₌"),
    ("(", "₍"),
    (")", "₎"),
    ("a", "ₐ"),
    ("e", "ₑ"),
    ("h", "ₕ"),
    ("i", "ᵢ"),
    ("j", "ⱼ"),
    ("k", "ₖ"),
    ("l", "ₗ"),
    ("m", "ₘ"),
    ("n", "ₙ"),
    ("o", "ₒ"),
    ("p", "ₚ"),
    ("r", "ᵣ"),
    ("s", "ₛ"),
    ("t", "ₜ"),
    ("u", "ᵤ"),
    ("v", "ᵥ"),
    ("x", "ₓ"),
];

static ACCENTS: &[(&str, &str)] = &[
    ("acute", "́"),
    ("bar", "̅"),
    ("breve", "̆"),
    ("check", "̌"),
    ("ddot", "̈"),
    ("dot", "̇"),
    ("grave", "̀"),
    ("hat", "̂"),
    ("mathring", "̊"),
    ("overleftarrow", "⃖"),
    ("overleftrightarrow", "⃡"),
    ("overline", "̅"),
    ("overrightarrow", "⃗"),
    ("tilde", "̃"),
    ("underline", "̲"),
    ("vec", "⃗"),
    ("widehat", "̂"),
    ("widetilde", "̃"),
];

static NAMED_OPERATORS: &[&str] = &["arccos", "arcsin", "arctan", "arg", "cos", "cosh", "cot", "coth", "csc", "deg", "det", "dim", "exp", "gcd", "hom", "inf", "ker", "lg", "lim", "liminf", "limsup", "ln", "log", "max", "min", "Pr", "sec", "sin", "sinh", "sup", "tan", "tanh"];

static LIMIT_OPERATORS: &[&str] = &["argmax", "argmin", "inf", "injlim", "lim", "liminf", "limsup", "max", "min", "projlim", "sup"];

static DISPLAY_LIMIT_SYMBOLS: &[&str] = &["bigcap", "bigcup", "bigodot", "bigoplus", "bigotimes", "bigsqcup", "biguplus", "bigvee", "bigwedge", "coprod", "int", "iint", "iiint", "oint", "prod", "sum"];

static RELATION_COMMANDS: &[&str] = &["Leftarrow", "Leftrightarrow", "Longleftarrow", "Longleftrightarrow", "Longrightarrow", "Rightarrow", "Join", "Vdash", "Vvdash", "approx", "asymp", "bowtie", "cong", "dashv", "fullouterjoin", "doteq", "downarrow", "equiv", "ge", "geq", "geqslant", "gets", "gg", "hookleftarrow", "hookrightarrow", "iff", "implies", "in", "leadsto", "le", "leftarrow", "leftharpoondown", "leftharpoonup", "leftrightarrow", "leftrightharpoons", "leftouterjoin", "leq", "leqslant", "ll", "longleftarrow", "longleftrightarrow", "longmapsto", "longrightarrow", "ltimes", "mapsto", "mid", "models", "ne", "nearrow", "neq", "ni", "notin", "nvdash", "nvDash", "nwarrow", "parallel", "perp", "prec", "preceq", "propto", "rightharpoondown", "rightharpoonup", "rightleftharpoons", "rightouterjoin", "rightarrow", "rightsquigarrow", "rtimes", "searrow", "sim", "simeq", "sqsubset", "sqsubseteq", "sqsupset", "sqsupseteq", "subset", "subseteq", "succ", "succeq", "supset", "supseteq", "swarrow", "to", "triangleleft", "triangleright", "twoheadleftarrow", "twoheadrightarrow", "uparrow", "vdash"];

static SPACING_COMMANDS: &[&str] = &[",", ":", ";", " ", ">", "enspace", "enskip", "medspace", "quad", "qquad", "thickspace", "thinspace"];

static NEGATIVE_SPACING_COMMANDS: &[&str] = &["!", "negmedspace", "negthickspace", "negthinspace"];

static FONT_SWITCH_COMMANDS: &[&str] = &["bf", "cal", "it", "rm", "sf", "sl", "tt"];

static IGNORED_COMMANDS: &[&str] = &["displaystyle", "limits", "nolimits", "scriptstyle", "scriptscriptstyle", "textstyle"];

static SIZE_COMMANDS: &[&str] = &["big", "Big", "bigg", "Bigg", "bigl", "Bigl", "biggl", "Biggl", "bigr", "Bigr", "biggr", "Biggr"];

static PLAIN_WRAPPERS: &[&str] = &["emph", "mathcal", "mathbf", "mathfrak", "mathit", "mathrm", "mathnormal", "mathscr", "mathsf", "mathtt", "mathup", "mbox", "overbrace", "pmb", "smash", "substack", "text", "textbf", "textit", "textmd", "textnormal", "textrm", "textsc", "textsf", "textsl", "texttt", "textup", "underbrace", "bm", "boldsymbol"];
