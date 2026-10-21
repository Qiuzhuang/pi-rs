//! Header helpers (port of `utils/headers.ts`).

use indexmap::IndexMap;
use reqwest::header::HeaderMap;

use crate::types::ProviderHeaders;

/// Convert response headers to a plain map (lower-case names, as `Headers` iterates).
pub fn headers_to_record(headers: &HeaderMap) -> IndexMap<String, String> {
    let mut result = IndexMap::new();
    for (name, value) in headers {
        let value = String::from_utf8_lossy(value.as_bytes()).to_string();
        result
            .entry(name.as_str().to_string())
            .and_modify(|existing: &mut String| {
                existing.push_str(", ");
                existing.push_str(&value);
            })
            .or_insert(value);
    }
    result
}

/// Merge header sources case-insensitively; later sources win and `None` removes a header.
pub fn provider_headers_to_record(sources: &[Option<&ProviderHeaders>]) -> Option<IndexMap<String, String>> {
    let mut merged: IndexMap<String, (String, String)> = IndexMap::new();
    for source in sources.iter().flatten() {
        for (name, value) in source.iter() {
            let normalized = name.to_lowercase();
            merged.shift_remove(&normalized);
            if let Some(value) = value {
                merged.insert(normalized, (name.clone(), value.clone()));
            }
        }
    }
    if merged.is_empty() { None } else { Some(merged.into_values().collect()) }
}

/// Convert plain string headers into provider headers.
pub fn to_provider_headers(headers: &IndexMap<String, String>) -> ProviderHeaders {
    headers.iter().map(|(k, v)| (k.clone(), Some(v.clone()))).collect()
}
