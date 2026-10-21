//! marked's GFM grammar (block and inline rules), built exactly like marked builds it:
//! regex sources are composed by name substitution and then compiled.

use std::sync::LazyLock;

use super::js_regex::JsRegex;

/// Equivalent of marked's `edit()` helper.
struct Edit {
    source: String,
    flags: &'static str,
}

static CARET: LazyLock<regex::Regex> = LazyLock::new(|| regex::Regex::new(r"(^|[^\[])\^").unwrap());

fn edit(source: &str, flags: &'static str) -> Edit {
    Edit { source: source.to_string(), flags }
}

impl Edit {
    /// `.replace('name', val)`: first occurrence of a literal name.
    fn replace(mut self, name: &str, val: &str) -> Self {
        let val = CARET.replace_all(val, "$1").to_string();
        if let Some(pos) = self.source.find(name) {
            self.source.replace_range(pos..pos + name.len(), &val);
        }
        self
    }

    /// `.replace(/name/g, val)`: every occurrence of a literal name.
    fn replace_all(mut self, name: &str, val: &str) -> Self {
        let val = CARET.replace_all(val, "$1").to_string();
        self.source = self.source.replace(name, &val);
        self
    }

    fn source(&self) -> String {
        self.source.clone()
    }

    fn get(self) -> JsRegex {
        JsRegex::new(&self.source, self.flags)
    }
}

/// Miscellaneous helper regexes (`rules.other`).
pub struct Other {
    pub code_remove_indent: JsRegex,
    pub output_link_replace: JsRegex,
    pub indent_code_compensation: JsRegex,
    pub beginning_space: JsRegex,
    pub ending_hash: JsRegex,
    pub ending_space_char: JsRegex,
    pub starting_space_char: JsRegex,
    pub non_space_char: JsRegex,
    pub new_line_char_global: JsRegex,
    pub tab_char_global: JsRegex,
    pub multiple_space_global: JsRegex,
    pub blank_line: JsRegex,
    pub double_blank_line: JsRegex,
    pub blockquote_start: JsRegex,
    pub blockquote_setext_replace: JsRegex,
    pub blockquote_setext_replace2: JsRegex,
    pub list_is_task: JsRegex,
    pub list_replace_task: JsRegex,
    pub list_task_checkbox: JsRegex,
    pub any_line: JsRegex,
    pub href_brackets: JsRegex,
    pub table_delimiter: JsRegex,
    pub table_align_chars: JsRegex,
    pub table_row_blank_line: JsRegex,
    pub table_align_right: JsRegex,
    pub table_align_center: JsRegex,
    pub table_align_left: JsRegex,
    pub start_a_tag: JsRegex,
    pub end_a_tag: JsRegex,
    pub start_pre_script_tag: JsRegex,
    pub end_pre_script_tag: JsRegex,
    pub start_angle_bracket: JsRegex,
    pub end_angle_bracket: JsRegex,
    pub unicode_alpha_numeric: JsRegex,
    pub carriage_return: JsRegex,
    /// Indexed by `min(3, max(0, indent - 1))`.
    pub next_bullet: [JsRegex; 4],
    pub hr: [JsRegex; 4],
    pub fences_begin: [JsRegex; 4],
    pub heading_begin: [JsRegex; 4],
    pub html_begin: [JsRegex; 4],
    pub blockquote_begin: [JsRegex; 4],
}

impl Other {
    pub fn indent_index(indent: usize) -> usize {
        indent.saturating_sub(1).min(3)
    }
}

/// Block-level GFM grammar.
pub struct Block {
    pub newline: JsRegex,
    pub code: JsRegex,
    pub fences: JsRegex,
    pub hr: JsRegex,
    pub heading: JsRegex,
    pub blockquote: JsRegex,
    pub list: JsRegex,
    pub html: JsRegex,
    pub def: JsRegex,
    pub table: JsRegex,
    pub lheading: JsRegex,
    pub paragraph: JsRegex,
    pub text: JsRegex,
}

/// Inline-level GFM grammar.
pub struct Inline {
    pub escape: JsRegex,
    pub tag: JsRegex,
    pub link: JsRegex,
    pub reflink: JsRegex,
    pub nolink: JsRegex,
    pub reflink_search: JsRegex,
    pub block_skip: JsRegex,
    pub any_punctuation: JsRegex,
    pub em_strong_l_delim: JsRegex,
    pub em_strong_r_delim_ast: JsRegex,
    pub em_strong_r_delim_und: JsRegex,
    pub punctuation: JsRegex,
    pub code: JsRegex,
    pub br: JsRegex,
    pub del_l_delim: JsRegex,
    pub del_r_delim: JsRegex,
    pub del: JsRegex,
    pub autolink: JsRegex,
    pub url: JsRegex,
    pub backpedal: JsRegex,
    pub text: JsRegex,
}

pub struct Rules {
    pub other: Other,
    pub block: Block,
    pub inline: Inline,
}

pub static RULES: LazyLock<Rules> = LazyLock::new(build);

fn indexed(f: impl Fn(usize) -> JsRegex) -> [JsRegex; 4] {
    [f(0), f(1), f(2), f(3)]
}

fn build() -> Rules {
    let other = Other {
        code_remove_indent: JsRegex::new(r"^(?: {1,4}| {0,3}\t)", "gm"),
        output_link_replace: JsRegex::new(r"\\([\[\]])", "g"),
        indent_code_compensation: JsRegex::new(r"^(\s+)(?:```)", ""),
        beginning_space: JsRegex::new(r"^\s+", ""),
        ending_hash: JsRegex::new(r"#$", ""),
        starting_space_char: JsRegex::new(r"^ ", ""),
        ending_space_char: JsRegex::new(r" $", ""),
        non_space_char: JsRegex::new(r"[^ ]", ""),
        new_line_char_global: JsRegex::new(r"\n", "g"),
        tab_char_global: JsRegex::new(r"\t", "g"),
        multiple_space_global: JsRegex::new(r"\s+", "g"),
        blank_line: JsRegex::new(r"^[ \t]*$", ""),
        double_blank_line: JsRegex::new(r"\n[ \t]*\n[ \t]*$", ""),
        blockquote_start: JsRegex::new(r"^ {0,3}>", ""),
        blockquote_setext_replace: JsRegex::new(r"\n {0,3}((?:=+|-+) *)(?=\n|$)", "g"),
        blockquote_setext_replace2: JsRegex::new(r"^ {0,3}>[ \t]?", "gm"),
        list_is_task: JsRegex::new(r"^\[[ xX]\] +\S", ""),
        list_replace_task: JsRegex::new(r"^\[[ xX]\] +", ""),
        list_task_checkbox: JsRegex::new(r"\[[ xX]\]", ""),
        any_line: JsRegex::new(r"\n.*\n", ""),
        href_brackets: JsRegex::new(r"^<(.*)>$", ""),
        table_delimiter: JsRegex::new(r"[:|]", ""),
        table_align_chars: JsRegex::new(r"^\||\| *$", "g"),
        table_row_blank_line: JsRegex::new(r"\n[ \t]*$", ""),
        table_align_right: JsRegex::new(r"^ *-+: *$", ""),
        table_align_center: JsRegex::new(r"^ *:-+: *$", ""),
        table_align_left: JsRegex::new(r"^ *:-+ *$", ""),
        start_a_tag: JsRegex::new(r"^<a ", "i"),
        end_a_tag: JsRegex::new(r"^<\/a>", "i"),
        start_pre_script_tag: JsRegex::new(r"^<(pre|code|kbd|script)(\s|>)", "i"),
        end_pre_script_tag: JsRegex::new(r"^<\/(pre|code|kbd|script)(\s|>)", "i"),
        start_angle_bracket: JsRegex::new(r"^<", ""),
        end_angle_bracket: JsRegex::new(r">$", ""),
        unicode_alpha_numeric: JsRegex::new(r"[\p{L}\p{N}]", "u"),
        carriage_return: JsRegex::new(r"\r\n|\r", "g"),
        next_bullet: indexed(|i| JsRegex::new(&format!(r"^ {{0,{i}}}(?:[*+-]|\d{{1,9}}[.)])((?:[ \t][^\n]*)?(?:\n|$))"), "")),
        hr: indexed(|i| JsRegex::new(&format!(r"^ {{0,{i}}}((?:- *){{3,}}|(?:_ *){{3,}}|(?:\* *){{3,}})(?:\n+|$)"), "")),
        fences_begin: indexed(|i| JsRegex::new(&format!(r"^ {{0,{i}}}(?:```|~~~)"), "")),
        heading_begin: indexed(|i| JsRegex::new(&format!(r"^ {{0,{i}}}#"), "")),
        html_begin: indexed(|i| JsRegex::new(&format!(r"^ {{0,{i}}}<(?:[a-z].*>|!--)"), "i")),
        blockquote_begin: indexed(|i| JsRegex::new(&format!(r"^ {{0,{i}}}>"), "")),
    };

    // Block grammar ---------------------------------------------------------------
    let newline = r"^(?:[ \t]*(?:\n|$))+";
    let block_code = r"^((?: {4}| {0,3}\t)[^\n]+(?:\n(?:[ \t]*(?:\n|$))*)?)+";
    let fences = r"^ {0,3}(`{3,}(?=[^`\n]*(?:\n|$))|~{3,})([^\n]*)(?:\n|$)(?:|([\s\S]*?)(?:\n|$))(?: {0,3}\1[~`]* *(?=\n|$)|$)";
    let hr = r"^ {0,3}((?:-[\t ]*){3,}|(?:_[ \t]*){3,}|(?:\*[ \t]*){3,})(?:\n+|$)";
    let heading = r"^ {0,3}(#{1,6})(?=\s|$)(.*)(?:\n+|$)";
    let bullet = r" {0,3}(?:[*+-]|\d{1,9}[.)])";
    let lheading_core = r"^(?!bull |blockCode|fences|blockquote|heading|html|table)((?:.|\n(?!\s*?\n|bull |blockCode|fences|blockquote|heading|html|table))+?)\n {0,3}(=+|-+) *(?:\n+|$)";
    let lheading_gfm = edit(lheading_core, "")
        .replace_all("bull", bullet)
        .replace_all("blockCode", r"(?: {4}| {0,3}\t)")
        .replace_all("fences", r" {0,3}(?:`{3,}|~{3,})")
        .replace_all("blockquote", r" {0,3}>")
        .replace_all("heading", r" {0,3}#{1,6}(?:\s|$)")
        .replace_all("html", r" {0,3}<[^\n>]+>\n")
        .replace_all("table", r" {0,3}\|?(?:[:\- ]*\|)+[\:\- ]*\n")
        .get();
    let paragraph_src = r"^([^\n]+(?:\n(?!hr|heading|lheading|blockquote|fences|list|html|table|[ \t]+\n)[^\n]+)*)";
    let block_text = r"^[^\n]+";
    let block_label = r"(?!\s*\])(?:\\[\s\S]|[^\[\]\\])+";
    let def = edit(
        r#"^ {0,3}\[(label)\]: *(?:\n[ \t]*)?([^<\s][^\s]*|<.*?>)(?:(?: +(?:\n[ \t]*)?| *\n[ \t]*)(title))? *(?:\n+|$)"#,
        "",
    )
    .replace("label", block_label)
    .replace("title", r#"(?:"(?:\\"?|[^"\\])*"|'[^'\n]*(?:\n[^'\n]+)*\n?'|\([^()]*\))"#)
    .get();
    let list = edit(r"^(bull)([ \t][^\n]*?)?(?:\n|$)", "").replace_all("bull", bullet).get();
    let tag_names = "address|article|aside|base|basefont|blockquote|body|caption|center|col|colgroup|dd|details|dialog|dir|div|dl|dt|fieldset|figcaption|figure|footer|form|frame|frameset|h[1-6]|head|header|hr|html|iframe|legend|li|link|main|menu|menuitem|meta|nav|noframes|ol|optgroup|option|p|param|search|section|summary|table|tbody|td|tfoot|th|thead|title|tr|track|ul";
    let comment = r"<!--(?:-?>|[\s\S]*?(?:-->|$))";
    let html = edit(
        concat!(
            r"^ {0,3}(?:",
            r"<(script|pre|style|textarea)[\s>][\s\S]*?(?:</\1>[^\n]*\n*|$)",
            r"|comment[^\n]*(\n+|$)",
            r"|<\?[\s\S]*?(?:\?>[^\n]*\n*|$)",
            r"|<![A-Z][\s\S]*?(?:>[^\n]*\n*|$)",
            r"|<!\[CDATA\[[\s\S]*?(?:\]\]>[^\n]*\n*|$)",
            "|</?(tag)(?: +|\\n|/?>)[\\s\\S]*?(?:(?:\\n[ \t]*)+\\n|$)",
            "|<(?!script|pre|style|textarea)([a-z][\\w-]*)(?:attribute)*? */?>(?=[ \\t]*(?:\\n|$))[\\s\\S]*?(?:(?:\\n[ \t]*)+\\n|$)",
            "|</(?!script|pre|style|textarea)[a-z][\\w-]*\\s*>(?=[ \\t]*(?:\\n|$))[\\s\\S]*?(?:(?:\\n[ \t]*)+\\n|$)",
            ")"
        ),
        "i",
    )
    .replace("comment", comment)
    .replace("tag", tag_names)
    .replace("attribute", r#" +[a-zA-Z:_][\w.:-]*(?: *= *"[^"\n]*"| *= *'[^'\n]*'| *= *[^\s"'=<>`]+)?"#)
    .get();

    let blockquote_paragraph = edit(paragraph_src, "")
        .replace("hr", hr)
        .replace("heading", r" {0,3}#{1,6}(?:\s|$)")
        .replace("|lheading", "")
        .replace("|table", "")
        .replace("blockquote", r" {0,3}>")
        .replace("fences", r" {0,3}(?:`{3,}(?=[^`\n]*(?:\n|$))|~~~)[^\n]*(?:\n|$)")
        .replace("list", r" {0,3}(?:[*+-]|\d{1,9}[.)])(?:[ \t]|\n|$)")
        .replace("html", r"</?(?:tag)(?: +|\n|/?>)|<(?:script|pre|style|textarea|!--)")
        .replace("tag", tag_names)
        .source();
    let blockquote = edit(r"^( {0,3}> ?(paragraph|[^\n]*)(?:\n|$))+", "").replace("paragraph", &blockquote_paragraph).get();

    let gfm_table = edit(
        concat!(
            r"^ *([^\n ].*)\n",
            r" {0,3}((?:\| *)?:?-+:? *(?:\| *:?-+:? *)*(?:\| *)?)",
            r"(?:\n((?:(?! *\n|hr|heading|blockquote|code|fences|list|html).*(?:\n|$))*)\n*|$)"
        ),
        "",
    )
    .replace("hr", hr)
    .replace("heading", r" {0,3}#{1,6}(?:\s|$)")
    .replace("blockquote", r" {0,3}>")
    .replace("code", "(?: {4}| {0,3}\t)[^\\n]")
    .replace("fences", r" {0,3}(?:`{3,}(?=[^`\n]*(?:\n|$))|~~~)[^\n]*(?:\n|$)")
    .replace("list", r" {0,3}(?:[*+-]|1[.)])[ \t]")
    .replace("html", r"</?(?:tag)(?: +|\n|/?>)|<(?:script|pre|style|textarea|!--)")
    .replace("tag", tag_names)
    .source();
    let gfm_paragraph = edit(paragraph_src, "")
        .replace("hr", hr)
        .replace("heading", r" {0,3}#{1,6}(?:\s|$)")
        .replace("|lheading", "")
        .replace("table", &gfm_table)
        .replace("blockquote", r" {0,3}>")
        .replace("fences", r" {0,3}(?:`{3,}(?=[^`\n]*(?:\n|$))|~~~)[^\n]*(?:\n|$)")
        .replace("list", r" {0,3}(?:[*+-]|1[.)])[ \t]+[^ \t\n]")
        .replace("html", r"</?(?:tag)(?: +|\n|/?>)|<(?:script|pre|style|textarea|!--)")
        .replace("tag", tag_names)
        .get();

    let block = Block {
        newline: JsRegex::new(newline, ""),
        code: JsRegex::new(block_code, ""),
        fences: JsRegex::new(fences, ""),
        hr: JsRegex::new(hr, ""),
        heading: JsRegex::new(heading, ""),
        blockquote,
        list,
        html,
        def,
        table: JsRegex::new(&gfm_table, ""),
        lheading: lheading_gfm,
        paragraph: gfm_paragraph,
        text: JsRegex::new(block_text, ""),
    };

    // Inline grammar --------------------------------------------------------------
    let punctuation_class = r"[\p{P}\p{S}]";
    let punctuation_or_space = r"[\s\p{P}\p{S}]";
    let not_punctuation_or_space = r"[^\s\p{P}\p{S}]";
    let punctuation = edit(r"^((?![*_])punctSpace)", "u").replace_all("punctSpace", punctuation_or_space).get();
    let punct_gfm = r"(?!~)[\p{P}\p{S}]";
    let punct_space_gfm = r"(?!~)[\s\p{P}\p{S}]";
    let not_punct_space_gfm = r"(?:[^\s\p{P}\p{S}]|~)";

    let block_skip = edit("link|precode-code|html", "g")
        .replace("link", r"\[(?:[^\[\]`]|(?<a>`+)[^`]+\k<a>(?!`))*?\]\((?:\\[\s\S]|[^\\\(\)]|\((?:\\[\s\S]|[^\\\(\)])*\))*\)")
        .replace("precode-", "(?<!`)()")
        .replace("code", r"(?<b>`+)[^`]+\k<b>(?!`)")
        .replace("html", r"<(?! )[^<>]*?>")
        .get();

    let em_strong_l_delim_core = r"^(?:\*+(?:((?!\*)punct)|([^\s*]))?)|^_+(?:((?!_)punct)|([^\s_]))?";
    let em_strong_l_delim_gfm = edit(em_strong_l_delim_core, "u").replace_all("punct", punct_gfm).get();

    let em_strong_r_delim_ast_core = concat!(
        r"^[^_*]*?__[^_*]*?\*[^_*]*?(?=__)",
        r"|[^*]+(?=[^*])",
        r"|(?!\*)punct(\*+)(?=[\s]|$)",
        r"|notPunctSpace(\*+)(?!\*)(?=punctSpace|$)",
        r"|(?!\*)punctSpace(\*+)(?=notPunctSpace)",
        r"|[\s](\*+)(?!\*)(?=punct)",
        r"|(?!\*)punct(\*+)(?!\*)(?=punct)",
        r"|notPunctSpace(\*+)(?=notPunctSpace)"
    );
    let em_strong_r_delim_ast_gfm = edit(em_strong_r_delim_ast_core, "gu")
        .replace_all("notPunctSpace", not_punct_space_gfm)
        .replace_all("punctSpace", punct_space_gfm)
        .replace_all("punct", punct_gfm)
        .get();
    let em_strong_r_delim_und = edit(
        concat!(
            r"^[^_*]*?\*\*[^_*]*?_[^_*]*?(?=\*\*)",
            r"|[^_]+(?=[^_])",
            r"|(?!_)punct(_+)(?=[\s]|$)",
            r"|notPunctSpace(_+)(?!_)(?=punctSpace|$)",
            r"|(?!_)punctSpace(_+)(?=notPunctSpace)",
            r"|[\s](_+)(?!_)(?=punct)",
            r"|(?!_)punct(_+)(?!_)(?=punct)"
        ),
        "gu",
    )
    .replace_all("notPunctSpace", not_punctuation_or_space)
    .replace_all("punctSpace", punctuation_or_space)
    .replace_all("punct", punctuation_class)
    .get();

    let del_l_delim = edit(r"^~~?(?:((?!~)punct)|[^\s~])", "u").replace_all("punct", punctuation_class).get();
    let del_r_delim = edit(
        concat!(
            r"^[^~]+(?=[^~])",
            r"|(?!~)punct(~~?)(?=[\s]|$)",
            r"|notPunctSpace(~~?)(?!~)(?=punctSpace|$)",
            r"|(?!~)punctSpace(~~?)(?=notPunctSpace)",
            r"|[\s](~~?)(?!~)(?=punct)",
            r"|(?!~)punct(~~?)(?!~)(?=punct)",
            r"|notPunctSpace(~~?)(?=notPunctSpace)"
        ),
        "gu",
    )
    .replace_all("notPunctSpace", not_punctuation_or_space)
    .replace_all("punctSpace", punctuation_or_space)
    .replace_all("punct", punctuation_class)
    .get();

    let any_punctuation = edit(r"\\(punct)", "gu").replace_all("punct", punctuation_class).get();
    let autolink = edit(r"^<(scheme:[^\s\x00-\x1f<>]*|email)>", "")
        .replace("scheme", r"[a-zA-Z][a-zA-Z0-9+.-]{1,31}")
        .replace(
            "email",
            r"[a-zA-Z0-9.!#$%&'*+/=?^_`{|}~-]+(@)[a-zA-Z0-9](?:[a-zA-Z0-9-]{0,61}[a-zA-Z0-9])?(?:\.[a-zA-Z0-9](?:[a-zA-Z0-9-]{0,61}[a-zA-Z0-9])?)+(?![-_])",
        )
        .get();
    let inline_comment = edit(comment, "").replace("(?:-->|$)", "-->").source();
    let tag = edit(
        concat!(
            "^comment",
            r"|^</[a-zA-Z][\w:-]*\s*>",
            r"|^<[a-zA-Z][\w-]*(?:attribute)*?\s*/?>",
            r"|^<\?[\s\S]*?\?>",
            r"|^<![a-zA-Z]+\s[\s\S]*?>",
            r"|^<!\[CDATA\[[\s\S]*?\]\]>"
        ),
        "",
    )
    .replace("comment", &inline_comment)
    .replace("attribute", r#"\s+[a-zA-Z:_][\w.:-]*(?:\s*=\s*"[^"]*"|\s*=\s*'[^']*'|\s*=\s*[^\s"'=<>`]+)?"#)
    .get();
    let inline_label = r"(?:\[(?:\\[\s\S]|[^\[\]\\])*\]|\\[\s\S]|`+(?!`)[^`]*?`+(?!`)|``+(?=\])|[^\[\]\\`])*?";
    let link = edit(r"^!?\[(label)\]\(\s*(href)(?:(?:[ \t]+(?:\n[ \t]*)?|\n[ \t]*)(title))?\s*\)", "")
        .replace("label", inline_label)
        .replace("href", r"<(?:\\.|[^\n<>\\])+>|[^ \t\n\x00-\x1f]+|(?=\))")
        .replace("title", r#""(?:\\"?|[^"\\])*"|'(?:\\'?|[^'\\])*'|\((?:\\\)?|[^)\\])*\)"#)
        .get();
    let reflink_edit = edit(r"^!?\[(label)\]\[(ref)\]", "").replace("label", inline_label).replace("ref", block_label);
    let reflink_src = reflink_edit.source();
    let nolink_edit = edit(r"^!?\[(ref)\](?:\[\])?", "").replace("ref", block_label);
    let nolink_src = nolink_edit.source();
    let reflink_search = edit(r"reflink|nolink(?!\()", "g").replace("reflink", &reflink_src).replace("nolink", &nolink_src).get();
    let case_insensitive_protocol = r"[hH][tT][tT][pP][sS]?|[fF][tT][pP]";
    let url = edit(r"^((?:protocol):\/\/|www\.)(?:[a-zA-Z0-9\-]+\.?)+[^\s<]*|^email", "")
        .replace("protocol", case_insensitive_protocol)
        .replace("email", r"[A-Za-z0-9._+-]+(@)[a-zA-Z0-9-_]+(?:\.[a-zA-Z0-9-_]*[a-zA-Z0-9])+(?![-_])")
        .get();
    let text = edit(
        r"^(`+|~+|[^`~])(?:(?=[`~])|(?= {2,}\n)|(?=[a-zA-Z0-9.!#$%&'*+\/=?_`{\|}~-]+@)|[\s\S]*?(?:(?=[\\<!\[`*~_]|\b_|protocol:\/\/|www\.|$)|[^ ](?= {2,}\n)|[^a-zA-Z0-9.!#$%&'*+\/=?_`{\|}~-](?=[a-zA-Z0-9.!#$%&'*+\/=?_`{\|}~-]+@)))",
        "",
    )
    .replace("protocol", case_insensitive_protocol)
    .get();

    let inline = Inline {
        escape: JsRegex::new(r##"^\\([!"#$%&'()*+,\-./:;<=>?@\[\]\\^_`{|}~])"##, ""),
        tag,
        link,
        reflink: reflink_edit.get(),
        nolink: nolink_edit.get(),
        reflink_search,
        block_skip,
        any_punctuation,
        em_strong_l_delim: em_strong_l_delim_gfm,
        em_strong_r_delim_ast: em_strong_r_delim_ast_gfm,
        em_strong_r_delim_und,
        punctuation,
        code: JsRegex::new(r"^(`+)([^`]|[^`][\s\S]*?[^`])\1(?!`)", ""),
        br: JsRegex::new(r"^( {2,}|\\)\n(?!\s*$)", ""),
        del_l_delim,
        del_r_delim,
        del: JsRegex::new(r"^(~~?)(?=[^\s~])((?:\\[\s\S]|[^\\])*?(?:\\[\s\S]|[^\s~\\]))\1(?=[^~]|$)", ""),
        autolink,
        url,
        backpedal: JsRegex::new(r#"(?:[^?!.,:;*_'"~()&]+|\([^)]*\)|&(?![a-zA-Z0-9]+;$)|[?!.,:;*_'"~)]+(?!$))+"#, ""),
        text,
    };

    Rules { other, block, inline }
}

#[cfg(test)]
mod tests {
    #[test]
    fn grammar_compiles() {
        let _ = &*super::RULES;
    }
}
