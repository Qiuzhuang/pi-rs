//! Builders for `tools.json` (scripts/golden/gen-tools.mjs).

use pi_coding_agent::core::extensions::types::ToolRenderers;
use pi_coding_agent::core::tools::renderers::with_built_in_renderers;
use pi_coding_agent::modes::interactive::components::diff::{RenderDiffOptions, render_diff};
use pi_coding_agent::modes::interactive::components::tool_execution::{
    ToolExecutionComponent, ToolExecutionComponentToolDefinition, ToolExecutionComponentUpdateResultResult,
    ToolExecutionOptions,
};
use serde_json::Value;

use super::harness::*;

pub fn build(kind: &str, input: &Value) -> Option<Built> {
    Some(match kind {
        "tool_execution" => {
            let tool_name = s(input, "toolName");
            let definition = match s(input, "definition") {
                "builtin" => with_built_in_renderers(tool_name, None),
                "bare" => with_built_in_renderers(tool_name, Some(ToolRenderers::default())),
                _ => None,
            }
            .map(ToolExecutionComponentToolDefinition::ToolRenderers);
            let options: Option<ToolExecutionOptions> = opt_from(input, "options");
            let c = ToolExecutionComponent::new(
                tool_name,
                "call-1",
                input.get("args").cloned().unwrap_or(Value::Null),
                options.as_ref(),
                definition.as_ref(),
                test_tui(),
                CWD,
            );
            for step in input.get("steps").and_then(Value::as_array).into_iter().flatten() {
                match step {
                    Value::String(op) if op == "argsComplete" => c.set_args_complete(),
                    Value::String(op) if op == "expand" => c.set_expanded(true),
                    Value::String(op) if op == "collapse" => c.set_expanded(false),
                    Value::Object(o) if o.contains_key("args") => c.update_args(o["args"].clone()),
                    Value::Object(o) if o.contains_key("result") => {
                        let result: ToolExecutionComponentUpdateResultResult = from(step, "result");
                        c.update_result(&result, Some(b(step, "partial")));
                    }
                    other => panic!("unknown step {other}"),
                }
            }
            component(c)
        }
        "render_diff" => {
            let diff = s(input, "diff").to_string();
            let options = opt_s(input, "filePath").map(|p| RenderDiffOptions { file_path: Some(p.to_string()) });
            Built::Render(Box::new(move |_w| render_diff(&diff, options.as_ref()).split('\n').map(str::to_string).collect()))
        }
        _ => return None,
    })
}
