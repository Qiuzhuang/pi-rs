use pi_mcp::{CallToolResult, ContentBlock, LlmContent, to_llm_content};
use serde_json::json;

fn text(text: &str) -> LlmContent {
    LlmContent::Text { text: text.into() }
}

#[test]
fn passes_text_and_images_through_and_replaces_other_blocks_with_text() {
    let blocks: Vec<ContentBlock> = serde_json::from_value(json!([
        { "type": "text", "text": "hello", "annotations": { "priority": 1 } },
        { "type": "image", "data": "aW1n", "mimeType": "image/png", "_meta": { "x": 1 } },
        { "type": "audio", "data": "YXVk", "mimeType": "audio/wav" },
        { "type": "resource_link", "uri": "file:///a.txt", "name": "a.txt" },
        { "type": "resource", "resource": { "uri": "file:///b.txt", "text": "inline" } },
        { "type": "resource", "resource": { "uri": "file:///c.png", "mimeType": "image/png", "blob": "Yw==" } },
        { "type": "resource", "resource": { "uri": "file:///d.bin", "blob": "ZA==" } },
        { "type": "hologram" },
    ]))
    .unwrap();
    assert_eq!(
        to_llm_content(&blocks, None),
        vec![
            text("hello"),
            LlmContent::Image { data: "aW1n".into(), mime_type: "image/png".into() },
            text("[audio audio/wav omitted]"),
            text("a.txt: file:///a.txt"),
            text("inline"),
            LlmContent::Image { data: "Yw==".into(), mime_type: "image/png".into() },
            text("[binary resource file:///d.bin (unknown type) omitted]"),
            text("[unsupported MCP content hologram]"),
        ]
    );
    // Blocks serialize back to their wire form.
    assert_eq!(
        serde_json::to_value(&blocks[0]).unwrap(),
        json!({ "type": "text", "text": "hello", "annotations": { "priority": 1 } })
    );
    assert_eq!(serde_json::to_value(&blocks[7]).unwrap(), json!({ "type": "hologram" }));
    assert_eq!(
        serde_json::to_value(LlmContent::Image { data: "x".into(), mime_type: "image/png".into() }).unwrap(),
        json!({ "type": "image", "data": "x", "mimeType": "image/png" })
    );
}

#[test]
fn falls_back_to_the_structured_content_as_json_when_there_are_no_blocks() {
    // #10416: structured content may be any JSON value.
    assert_eq!(to_llm_content(&[], Some(&json!([1, 2]))), vec![text("[\n  1,\n  2\n]")]);
    assert_eq!(to_llm_content(&[], Some(&json!(null))), vec![text("null")]);
    assert_eq!(to_llm_content(&[], Some(&json!({ "n": 1 }))), vec![text("{\n  \"n\": 1\n}")]);
    assert_eq!(to_llm_content(&[ContentBlock::text("n=1")], Some(&json!({ "n": 1 }))), vec![text("n=1")]);
    let result: CallToolResult = serde_json::from_value(json!({ "content": [], "structuredContent": null })).unwrap();
    assert_eq!(result.structured_content, Some(json!(null)));
    assert_eq!(result.to_llm_content(), vec![text("null")]);
    assert_eq!(serde_json::to_value(&result).unwrap(), json!({ "content": [], "structuredContent": null }));
}
