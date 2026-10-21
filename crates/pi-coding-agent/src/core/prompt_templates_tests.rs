//! Port of `test/prompt-templates.test.ts`.

use super::*;
use crate::core::source_info::SourceOrigin;

fn sub(template: &str, args: &[&str]) -> String {
    let args: Vec<String> = args.iter().map(|s| s.to_string()).collect();
    substitute_args(template, &args)
}

fn parse(input: &str) -> Vec<String> {
    parse_command_args(input)
}

#[test]
fn substitute_args_basics() {
    let cases: &[(&str, &[&str], &str)] = &[
        ("Test: $ARGUMENTS", &["a", "b", "c"], "Test: a b c"),
        ("Test: $@", &["a", "b", "c"], "Test: a b c"),
        ("$ARGUMENTS", &["$1", "$ARGUMENTS"], "$1 $ARGUMENTS"),
        ("$@", &["$100", "$1"], "$100 $1"),
        ("$ARGUMENTS", &["$100", "$1"], "$100 $1"),
        ("$1: $ARGUMENTS", &["prefix", "a", "b"], "prefix: prefix a b"),
        ("$1: $@", &["prefix", "a", "b"], "prefix: prefix a b"),
        ("Test: $ARGUMENTS", &[], "Test: "),
        ("Test: $@", &[], "Test: "),
        ("Test: $1", &[], "Test: "),
        ("$ARGUMENTS and $ARGUMENTS", &["a", "b"], "a b and a b"),
        ("$@ and $@", &["a", "b"], "a b and a b"),
        ("$@ and $ARGUMENTS", &["a", "b"], "a b and a b"),
        ("$1 $2: $ARGUMENTS", &["arg100", "@user"], "arg100 @user: arg100 @user"),
        ("$1 $2 $3 $4 $5", &["a", "b"], "a b   "),
        ("$ARGUMENTS", &["日本語", "🎉", "café"], "日本語 🎉 café"),
        ("$1 $2", &["line1\nline2", "tab\tthere"], "line1\nline2 tab\tthere"),
        ("$1$2", &["a", "b"], "ab"),
        ("$ARGUMENTS", &["first arg", "second arg"], "first arg second arg"),
        ("Test: $ARGUMENTS", &["only"], "Test: only"),
        ("Test: $@", &["only"], "Test: only"),
        ("$0", &["a", "b"], ""),
        ("$1.5", &["a"], "a.5"),
        ("pre$ARGUMENTS", &["a", "b"], "prea b"),
        ("pre$@", &["a", "b"], "prea b"),
        ("$ARGUMENTS", &["a", "", "c"], "a  c"),
        ("$ARGUMENTS", &["  leading  ", "trailing  "], "  leading   trailing  "),
        ("Prefix $ARGUMENTS suffix", &["ARGUMENTS"], "Prefix ARGUMENTS suffix"),
        ("$A $$ $ $ARGS", &["a"], "$A $$ $ $ARGS"),
        ("$arguments $Arguments $ARGUMENTS", &["a", "b"], "$arguments $Arguments a b"),
        ("$@ and $ARGUMENTS", &["x", "y", "z"], "x y z and x y z"),
        ("$ARGUMENTS and $@", &["x", "y", "z"], "x y z and x y z"),
        ("$1 $2 $3", &["a", "b", "c"], "a b c"),
        ("Price: \\$100", &[], "Price: \\"),
        (
            "$1: $@ ($ARGUMENTS)",
            &["first", "second", "third"],
            "first: first second third (first second third)",
        ),
        ("Just plain text", &["a", "b"], "Just plain text"),
        ("$1 $2 $@", &["a", "b", "c"], "a b a b c"),
    ];
    for (template, args, expected) in cases {
        assert_eq!(sub(template, args), *expected, "template {template:?}");
    }

    let args: Vec<String> = (0..100).map(|i| format!("arg{i}")).collect();
    assert_eq!(substitute_args("$ARGUMENTS", &args), args.join(" "));
    let args: Vec<String> = (0..15).map(|i| format!("val{i}")).collect();
    assert_eq!(substitute_args("$10 $12 $15", &args), "val9 val11 val14");
}

#[test]
fn substitute_args_positional_defaults() {
    assert_eq!(sub("List exactly ${1:-7} next steps", &[]), "List exactly 7 next steps");
    let template = "${@:-default}\n${ARGUMENTS:-default}";
    assert_eq!(sub(template, &[]), "default\ndefault");
    assert_eq!(
        sub(template, &["This", "would", "be", "the", "arguments"]),
        "This would be the arguments\nThis would be the arguments"
    );
    assert_eq!(sub("List exactly ${1:-7} next steps", &["3"]), "List exactly 3 next steps");
    assert_eq!(sub("Mode: ${1:-brief}", &[""]), "Mode: brief");
    assert_eq!(sub("${1:-7} ${2:-brief}", &[]), "7 brief");
    assert_eq!(sub("${1:-7} ${2:-brief}", &["3"]), "3 brief");
    assert_eq!(sub("${1:-7} ${2:-brief}", &["3", "verbose"]), "3 verbose");
    assert_eq!(sub("${1:-7}", &["$ARGUMENTS"]), "$ARGUMENTS");
    assert_eq!(sub("${1:-7}", &["$1"]), "$1");
    assert_eq!(sub("${1:-$ARGUMENTS}", &["a", "b"]), "a");
    assert_eq!(sub("${3:-$ARGUMENTS}", &["a", "b"]), "$ARGUMENTS");
    assert_eq!(sub("${1:-seven steps}", &[]), "seven steps");
    assert_eq!(sub("${3:-fallback}", &["a", "b"]), "fallback");
    assert_eq!(sub("$1 ${2:-x} $ARGUMENTS", &["a"]), "a x a");
}

#[test]
fn substitute_args_array_slicing() {
    let cases: &[(&str, &[&str], &str)] = &[
        ("${@:2}", &["a", "b", "c", "d"], "b c d"),
        ("${@:1}", &["a", "b", "c"], "a b c"),
        ("${@:3}", &["a", "b", "c", "d"], "c d"),
        ("${@:2:2}", &["a", "b", "c", "d"], "b c"),
        ("${@:1:1}", &["a", "b", "c"], "a"),
        ("${@:3:1}", &["a", "b", "c", "d"], "c"),
        ("${@:2:3}", &["a", "b", "c", "d", "e"], "b c d"),
        ("${@:99}", &["a", "b"], ""),
        ("${@:5}", &["a", "b"], ""),
        ("${@:10:5}", &["a", "b"], ""),
        ("${@:2:0}", &["a", "b", "c"], ""),
        ("${@:1:0}", &["a", "b"], ""),
        ("${@:2:99}", &["a", "b", "c"], "b c"),
        ("${@:1:10}", &["a", "b"], "a b"),
        ("${@:2} vs $@", &["a", "b", "c"], "b c vs a b c"),
        ("First: ${@:1:1}, All: $@", &["x", "y", "z"], "First: x, All: x y z"),
        ("${@:1}", &["${@:2}", "test"], "${@:2} test"),
        ("${@:2}", &["a", "${@:3}", "c"], "${@:3} c"),
        ("$1: ${@:2}", &["cmd", "arg1", "arg2"], "cmd: arg1 arg2"),
        ("$1 $2 ${@:3}", &["a", "b", "c", "d"], "a b c d"),
        ("${@:0}", &["a", "b", "c"], "a b c"),
        ("${@:2}", &[], ""),
        ("${@:1}", &[], ""),
        ("${@:1}", &["only"], "only"),
        ("${@:2}", &["only"], ""),
        ("Process ${@:2} with $1", &["tool", "file1", "file2"], "Process file1 file2 with tool"),
        ("${@:1:1} and ${@:2}", &["a", "b", "c"], "a and b c"),
        ("${@:1:2} vs ${@:3:2}", &["a", "b", "c", "d", "e"], "a b vs c d"),
        ("${@:2}", &["cmd", "first arg", "second arg"], "first arg second arg"),
        ("${@:2}", &["cmd", "$100", "@user", "#tag"], "$100 @user #tag"),
        ("${@:1}", &["日本語", "🎉", "café"], "日本語 🎉 café"),
        (
            "Run $1 on ${@:2:2}, then process $@",
            &["eslint", "file1.ts", "file2.ts", "file3.ts"],
            "Run eslint on file1.ts file2.ts, then process eslint file1.ts file2.ts file3.ts",
        ),
        ("prefix${@:2}suffix", &["a", "b", "c"], "prefixb csuffix"),
    ];
    for (template, args, expected) in cases {
        assert_eq!(sub(template, args), *expected, "template {template:?}");
    }
    let args: Vec<String> = (1..=10).map(|i| format!("arg{i}")).collect();
    assert_eq!(
        substitute_args("${@:5:100}", &args),
        "arg5 arg6 arg7 arg8 arg9 arg10"
    );
}

#[test]
fn parse_command_args_cases() {
    let cases: &[(&str, &[&str])] = &[
        ("a b c", &["a", "b", "c"]),
        ("\"first arg\" second", &["first arg", "second"]),
        ("'first arg' second", &["first arg", "second"]),
        ("\"double\" 'single' \"double again\"", &["double", "single", "double again"]),
        ("", &[]),
        ("a  b   c", &["a", "b", "c"]),
        ("a\tb\tc", &["a", "b", "c"]),
        ("\"\" \" \"", &["", " "]),
        ("a '' c", &["a", "", "c"]),
        ("a \"\"", &["a", ""]),
        ("$100 @user #tag", &["$100", "@user", "#tag"]),
        ("日本語 🎉 café", &["日本語", "🎉", "café"]),
        ("\"line1\nline2\" second", &["line1\nline2", "second"]),
        (
            "label-2\n\nHere is some description #2.",
            &["label-2", "Here", "is", "some", "description", "#2."],
        ),
        ("a\n\n\tb  c", &["a", "b", "c"]),
        ("\"quoted \\\"text\\\"\"", &["quoted \\text\\"]),
        ("a b c   ", &["a", "b", "c"]),
        ("   a b c", &["a", "b", "c"]),
    ];
    for (input, expected) in cases {
        assert_eq!(parse(input), *expected, "input {input:?}");
    }
}

fn template(name: &str, content: &str) -> PromptTemplate {
    PromptTemplate {
        name: name.to_string(),
        description: "test".to_string(),
        argument_hint: None,
        content: content.to_string(),
        source_info: SourceInfo {
            path: format!("/tmp/{name}.md"),
            source: "local".to_string(),
            scope: SourceScope::Temporary,
            origin: SourceOrigin::TopLevel,
            base_dir: None,
        },
        file_path: format!("/tmp/{name}.md"),
    }
}

#[test]
fn expand_prompt_template_cases() {
    let review = template("review", "Review $2. Focus: ${1:-correctness}.");
    assert_eq!(
        expand_prompt_template("/review \"\" src/index.ts", std::slice::from_ref(&review)),
        "Review src/index.ts. Focus: correctness."
    );
    assert_eq!(
        expand_prompt_template("/review '' src/index.ts", std::slice::from_ref(&review)),
        "Review src/index.ts. Focus: correctness."
    );

    let arg_test = template("arg-test", "- arg1: $1\n- rest: ${@:2}");
    assert_eq!(
        expand_prompt_template(
            "/arg-test label-2\n\nHere is some description #2.",
            std::slice::from_ref(&arg_test)
        ),
        "- arg1: label-2\n- rest: Here is some description #2."
    );

    let arg_test = template("arg-test", "arg1: $1");
    assert_eq!(
        expand_prompt_template("/arg-test\nlabel-2", std::slice::from_ref(&arg_test)),
        "arg1: label-2"
    );
    assert_eq!(
        expand_prompt_template("/unknown x", std::slice::from_ref(&arg_test)),
        "/unknown x"
    );
    assert_eq!(expand_prompt_template("plain", &[arg_test]), "plain");
}

#[test]
fn parse_and_substitute_integration() {
    let args = parse("Button \"onClick handler\" \"disabled support\"");
    assert_eq!(
        substitute_args("Create component $1 with features: $ARGUMENTS", &args),
        "Create component Button with features: Button onClick handler disabled support"
    );
    let args = parse("feature1 feature2 feature3");
    assert_eq!(
        substitute_args("Implement: $@", &args),
        substitute_args("Implement: $ARGUMENTS", &args)
    );
}

fn load_from(dir: &std::path::Path) -> LoadPromptTemplatesResult {
    load_prompt_templates(&LoadPromptTemplatesOptions {
        cwd: crate::utils::paths::current_dir_string(),
        agent_dir: crate::config::get_agent_dir(),
        prompt_paths: vec![dir.to_string_lossy().into_owned()],
        include_defaults: false,
    })
}

#[test]
fn load_prompt_templates_argument_hints() {
    let temp = tempfile::tempdir().unwrap();
    let write = |name: &str, content: &str| {
        std::fs::write(temp.path().join(format!("{name}.md")), content).unwrap();
    };
    write(
        "pr",
        "---\ndescription: Review PRs from URLs with structured issue and code analysis\nargument-hint: \"<PR-URL>\"\n---\nYou are given one or more GitHub PR URLs: $@",
    );
    write(
        "wr",
        "---\ndescription: Finish the current task end-to-end with changelog, commit, and push\nargument-hint: \"[instructions]\"\n---\nWrap it. Additional instructions: $ARGUMENTS",
    );
    write(
        "cl",
        "---\ndescription: Audit changelog entries before release\n---\nAudit changelog entries for all commits since the last release.",
    );
    write(
        "empty-hint",
        "---\ndescription: A command with empty hint\nargument-hint: \"\"\n---\nDo something",
    );
    write(
        "is",
        "---\ndescription: Analyze GitHub issues (bugs or feature requests)\nargument-hint: \"<issue>\"\n---\nAnalyze GitHub issue(s): $ARGUMENTS",
    );

    let result = load_from(temp.path());
    let find = |name: &str| result.templates.iter().find(|t| t.name == name).unwrap();
    assert_eq!(find("pr").argument_hint.as_deref(), Some("<PR-URL>"));
    assert_eq!(
        find("pr").description,
        "Review PRs from URLs with structured issue and code analysis"
    );
    assert_eq!(find("wr").argument_hint.as_deref(), Some("[instructions]"));
    assert_eq!(find("cl").argument_hint, None);
    assert_eq!(find("empty-hint").argument_hint, None);
    assert_eq!(find("is").argument_hint.as_deref(), Some("<issue>"));
    assert_eq!(
        find("is").source_info.base_dir.as_deref(),
        Some(temp.path().to_str().unwrap())
    );
}

#[test]
fn load_prompt_templates_reports_invalid_yaml_and_keeps_siblings() {
    let temp = tempfile::tempdir().unwrap();
    let invalid_path = temp.path().join("invalid.md");
    std::fs::write(
        &invalid_path,
        "---\ndescription: Broken: unquoted colon\n---\nDo something.\n",
    )
    .unwrap();
    std::fs::write(temp.path().join("valid.md"), "Valid prompt content.").unwrap();

    let result = load_from(temp.path());
    let names: Vec<&str> = result.templates.iter().map(|t| t.name.as_str()).collect();
    assert_eq!(names, vec!["valid"]);
    assert_eq!(result.diagnostics.len(), 1);
    assert_eq!(result.diagnostics[0].r#type, ResourceDiagnosticType::Warning);
    assert_eq!(
        result.diagnostics[0].path.as_deref(),
        Some(invalid_path.to_str().unwrap())
    );
    assert!(result.diagnostics[0].message.contains("line 1"));
    assert_eq!(result.templates[0].description, "Valid prompt content.");
}

#[test]
fn description_falls_back_to_truncated_first_line() {
    let temp = tempfile::tempdir().unwrap();
    let long_line = "x".repeat(70);
    std::fs::write(temp.path().join("long.md"), format!("\n\n{long_line}\nmore")).unwrap();
    let result = load_from(temp.path());
    assert_eq!(result.templates[0].description, format!("{}...", "x".repeat(60)));
}

#[test]
fn default_directories_set_user_and_project_scope() {
    let temp = tempfile::tempdir().unwrap();
    let agent_dir = temp.path().join("agent");
    let cwd = temp.path().join("project");
    std::fs::create_dir_all(agent_dir.join("prompts")).unwrap();
    std::fs::create_dir_all(cwd.join(".pi").join("prompts")).unwrap();
    std::fs::write(agent_dir.join("prompts").join("global.md"), "Global").unwrap();
    std::fs::write(cwd.join(".pi").join("prompts").join("local.md"), "Local").unwrap();
    let result = load_prompt_templates(&LoadPromptTemplatesOptions {
        cwd: cwd.to_string_lossy().into_owned(),
        agent_dir: agent_dir.to_string_lossy().into_owned(),
        prompt_paths: vec![],
        include_defaults: true,
    });
    assert_eq!(result.templates.len(), 2);
    assert_eq!(result.templates[0].name, "global");
    assert_eq!(result.templates[0].source_info.scope, SourceScope::User);
    assert_eq!(result.templates[1].name, "local");
    assert_eq!(result.templates[1].source_info.scope, SourceScope::Project);
}
