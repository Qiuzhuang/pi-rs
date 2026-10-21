//! Port of `test/tool-search.test.ts` (tokenize and ranker; the codemode catalog tests live in
//! `extensions::codemode::tests`).

use super::tool::*;
use crate::core::extensions::types::ToolNamespace;
use serde_json::{Value, json};

fn doc(name: &str, description: &str, properties: Value) -> ToolSearchDocument {
    create_tool_search_document(
        &CreateToolSearchDocumentTool {
            name: name.into(),
            description: description.into(),
            parameters: json!({ "type": "object", "properties": properties }),
        },
        None,
    )
}

#[test]
fn tokenize_splits_camel_and_snake_case_drops_stop_words_and_folds_plurals() {
    assert_eq!(tokenize("listIssues for the GitHub_repo"), vec!["list", "issue", "git", "hub", "repo"]);
    assert_eq!(tokenize("searches queries HTTPServer"), vec!["search", "query", "http", "server"]);
}

fn documents() -> Vec<ToolSearchDocument> {
    vec![
        doc(
            "mcp__github__list_issues",
            "List issues in a repository.",
            json!({ "state": { "type": "string", "description": "open or closed" } }),
        ),
        doc("mcp__github__create_pull_request", "Open a pull request.", json!({})),
        doc("mcp__linear__search_issues", "Search Linear issues by text.", json!({})),
        doc("mcp__docs__search", "Search the documentation.", json!({})),
    ]
}

fn names(matches: Vec<ToolSearchMatch>) -> Vec<String> {
    matches.into_iter().map(|m| m.name).collect()
}

#[test]
fn bm25_ranks_by_term_relevance_and_respects_the_limit() {
    let documents = documents();
    let ranker = Bm25Ranker::new(None);
    assert_eq!(
        names(ranker.rank("issue", &documents, 8)),
        vec!["mcp__linear__search_issues", "mcp__github__list_issues"]
    );
    assert_eq!(ranker.rank("pull requests", &documents, 8)[0].name, "mcp__github__create_pull_request");
    assert_eq!(ranker.rank("search", &documents, 1).len(), 1);
    // Property names and descriptions are searchable.
    assert_eq!(names(ranker.rank("closed", &documents, 8)), vec!["mcp__github__list_issues"]);
}

#[test]
fn bm25_returns_nothing_for_unknown_or_empty_queries() {
    let documents = documents();
    let ranker = Bm25Ranker::new(None);
    assert!(ranker.rank("kubernetes", &documents, 8).is_empty());
    assert!(ranker.rank("the", &documents, 8).is_empty());
    // Known v1 limit: no synonyms, so "tickets" does not find "issues".
    assert!(ranker.rank("tickets", &documents, 8).is_empty());
}

#[test]
fn bm25_includes_the_namespace_in_the_search_text() {
    let document = create_tool_search_document(
        &CreateToolSearchDocumentTool {
            name: "mcp__x__run".into(),
            description: "Run it.".into(),
            parameters: json!({ "type": "object", "properties": {} }),
        },
        Some(&ToolNamespace {
            name: "mcp__x".into(),
            description: Some("Kubernetes cluster tools".into()),
            ..Default::default()
        }),
    );
    let matches = Bm25Ranker::new(None).rank("kubernetes", &[document], 8);
    assert_eq!(matches.len(), 1);
    assert_eq!(matches[0].name, "mcp__x__run");
    assert!(matches[0].score > 0.0);
}

#[tokio::test]
async fn tool_search_loads_matches_and_reports_them() {
    use crate::core::extensions::types::{ToolExposure, ToolInfo};
    use crate::core::source_info::SourceInfo;
    use parking_lot::Mutex;
    use std::sync::Arc;

    let info = |name: &str, description: &str, exposure: ToolExposure| ToolInfo {
        name: name.into(),
        description: description.into(),
        parameters: json!({ "type": "object", "properties": {} }),
        prompt_guidelines: None,
        reaches_tools: None,
        exposure,
        namespace: None,
        annotations: None,
        source_info: SourceInfo::default(),
    };
    let all = vec![
        info("read", "Read files.", ToolExposure::Direct),
        info("mcp__github__list_issues", "List issues.\nMore text.", ToolExposure::Deferred),
        info("mcp__linear__search_issues", "Search issues.", ToolExposure::Codemode),
        info("secret_issues", "Hidden issues.", ToolExposure::Hidden),
    ];
    let active = Arc::new(Mutex::new(vec!["read".to_string()]));
    let set_active = active.clone();
    let get_active = active.clone();
    let definition = create_tool_search_tool_definition(Some(&ToolSearchToolOptions {
        tools: Some(ToolSearchToolOptionsTools {
            get_all_tools: Arc::new(move || all.clone()),
            get_active_tools: Arc::new(move || get_active.lock().clone()),
            set_active_tools: Arc::new(move |names| *set_active.lock() = names),
        }),
    }))
    .unwrap();
    assert_eq!(definition.exposure, Some(ToolExposure::ModelOnly));

    let result = (definition.execute)("call".into(), json!({ "query": "issues" }), None, None, None).await.unwrap();
    let text = result.content[0].as_text().unwrap().text.clone();
    assert!(text.starts_with("Loaded 2 tools. They are available from your next call:\n"), "{text}");
    assert!(text.contains("- mcp__github__list_issues: List issues."));
    assert!(!text.contains("More text"));
    assert_eq!(active.lock().len(), 3);
    assert_eq!(active.lock()[0], "read");

    let again = (definition.execute)("call".into(), json!({ "query": "issues" }), None, None, None).await.unwrap();
    assert_eq!(again.content[0].as_text().unwrap().text, "No matching tools found.");
    assert_eq!(again.details, Some(json!({ "loaded": [] })));

    let empty = (definition.execute)("call".into(), json!({ "query": "  " }), None, None, None).await;
    assert_eq!(empty.unwrap_err().to_string(), "query must not be empty");
    let bad_limit = (definition.execute)("call".into(), json!({ "query": "x", "limit": 1.5 }), None, None, None).await;
    assert_eq!(bad_limit.unwrap_err().to_string(), "limit must be a positive integer");
}
