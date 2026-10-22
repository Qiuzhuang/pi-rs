//! Port of `packages/coding-agent/src/cli/list-models.ts`.
//!
//! List available models with optional fuzzy search.

use crate::compat::AuthOperationOptions;
use crate::core::auth_guidance::format_no_models_available_message;
use crate::core::model_runtime::ModelRuntime;
use crate::utils::chalk;
use pi_ai::types::Model;
use std::sync::Arc;
use tokio_util::sync::CancellationToken;

/// JS `Number.prototype.toFixed(1)` for the positive values used here.
fn to_fixed_1(value: f64) -> String {
    format!("{:.1}", (value * 10.0).round() / 10.0)
}

/// Format a number as human-readable (e.g., 200000 -> "200K", 1000000 -> "1M")
pub fn format_token_count(count: f64) -> String {
    if count >= 1_000_000.0 {
        let millions = count / 1_000_000.0;
        return if millions % 1.0 == 0.0 { format!("{millions}M") } else { format!("{}M", to_fixed_1(millions)) };
    }
    if count >= 1_000.0 {
        let thousands = count / 1_000.0;
        return if thousands % 1.0 == 0.0 { format!("{thousands}K") } else { format!("{}K", to_fixed_1(thousands)) };
    }
    format!("{count}")
}

/// JS `localeCompare` approximation (case-insensitive first, then by code point).
fn locale_compare(a: &str, b: &str) -> std::cmp::Ordering {
    a.to_lowercase().cmp(&b.to_lowercase()).then_with(|| b.cmp(a))
}

fn pad_end(value: &str, width: usize) -> String {
    let len = value.chars().count();
    if len >= width { value.to_string() } else { format!("{value}{}", " ".repeat(width - len)) }
}

/// The table printed by [`list_models`] (one string per line).
pub fn format_model_table(models: &[Model]) -> Vec<String> {
    let mut sorted: Vec<&Model> = models.iter().collect();
    // Sort by provider, then by model id
    sorted.sort_by(|a, b| locale_compare(&a.provider, &b.provider).then_with(|| locale_compare(&a.id, &b.id)));

    let headers = ["provider", "model", "context", "max-out", "thinking", "images"];
    let rows: Vec<[String; 6]> = sorted
        .iter()
        .map(|m| {
            [
                m.provider.clone(),
                m.id.clone(),
                format_token_count(m.context_window as f64),
                format_token_count(m.max_tokens as f64),
                if m.reasoning { "yes" } else { "no" }.to_string(),
                if m.input.contains(&pi_ai::ModelInputModality::Image) { "yes" } else { "no" }.to_string(),
            ]
        })
        .collect();
    let mut widths = headers.map(|h| h.chars().count());
    for row in &rows {
        for (index, cell) in row.iter().enumerate() {
            widths[index] = widths[index].max(cell.chars().count());
        }
    }
    let format_line = |cells: [&str; 6]| -> String {
        cells.iter().enumerate().map(|(index, cell)| pad_end(cell, widths[index])).collect::<Vec<_>>().join("  ")
    };
    let mut lines = vec![format_line(headers)];
    for row in &rows {
        lines.push(format_line([&row[0], &row[1], &row[2], &row[3], &row[4], &row[5]]));
    }
    lines
}

/// List available models, optionally filtered by search pattern
pub async fn list_models(
    model_runtime: Arc<ModelRuntime>,
    search_pattern: Option<&str>,
    signal: Option<CancellationToken>,
) -> anyhow::Result<()> {
    if let Some(load_error) = model_runtime.get_error() {
        eprintln!("{}", chalk::yellow(&format!("Warning: errors loading models.json:\n{load_error}")));
    }

    let models = model_runtime.get_available(None, Some(&AuthOperationOptions { signal })).await?;

    if models.is_empty() {
        println!("{}", format_no_models_available_message());
        return Ok(());
    }

    // Apply fuzzy filter if search pattern provided
    let filtered_models = match search_pattern.filter(|p| !p.is_empty()) {
        Some(pattern) => pi_tui::fuzzy::fuzzy_filter(&models, pattern, |m| format!("{} {}", m.provider, m.id)),
        None => models,
    };

    if filtered_models.is_empty() {
        println!("No models matching \"{}\"", search_pattern.unwrap_or_default());
        return Ok(());
    }

    for line in format_model_table(&filtered_models) {
        println!("{line}");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_token_counts() {
        assert_eq!(format_token_count(200_000.0), "200K");
        assert_eq!(format_token_count(1_000_000.0), "1M");
        assert_eq!(format_token_count(1_048_576.0), "1.0M");
        assert_eq!(format_token_count(128_500.0), "128.5K");
        assert_eq!(format_token_count(512.0), "512");
        assert_eq!(format_token_count(2_500_000.0), "2.5M");
    }
}
