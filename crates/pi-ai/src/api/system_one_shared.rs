//! TypeSafe System One protocol over different transports (port of `api/system-one-shared.ts`).

use indexmap::IndexMap;
use serde_json::{Map, Value, json};

use super::classifier_shared::{normalize_classifier_error, parse_classifier_usage, post_classifier_request, required_number};
use crate::types::{
    ClassifierAnswer, ClassifierContext, ClassifierModel, ClassifierQuestion, ClassifierResult, ClassifierStopReason,
    StreamOptions,
};
use crate::utils::error_body::format_provider_error;

/// TypeSafe System One request body without the transport-specific envelope.
#[derive(Debug, Clone, PartialEq)]
pub struct SystemOneWireRequest {
    pub state: Map<String, Value>,
    pub questions: Map<String, Value>,
}

impl SystemOneWireRequest {
    /// `{ state, questions }` as JSON.
    pub fn to_value(&self) -> Value {
        json!({ "state": self.state, "questions": self.questions })
    }
}

/// Differences between services that serve System One models.
pub struct SystemOneTransport {
    /// Classifier API implemented by this transport.
    pub api: &'static str,
    /// Service name used in error messages.
    pub label: &'static str,
    /// Absolute request URL.
    pub url: fn(&ClassifierModel) -> String,
    /// Wraps the System One request in the service's request envelope.
    pub payload: fn(&ClassifierModel, &SystemOneWireRequest) -> Value,
    /// Extracts the System One output (`{ answers, usage }`) from the response envelope.
    pub output: fn(Value) -> anyhow::Result<Map<String, Value>>,
}

/// `new URL(path, `${baseUrl without trailing slashes}/`)`.
pub fn join_base_url(base_url: &str, path: &str) -> String {
    let base = format!("{}/", base_url.trim_end_matches('/'));
    match url::Url::parse(&base).and_then(|b| b.join(path)) {
        Ok(url) => url.to_string(),
        Err(_) => format!("{base}{path}"),
    }
}

fn probabilities(label: &str, value: Option<&Value>, id: &str) -> anyhow::Result<IndexMap<String, f64>> {
    let Some(map) = value.and_then(Value::as_object) else {
        anyhow::bail!("{label} returned invalid probabilities for {id}");
    };
    map.iter()
        .map(|(key, probability)| Ok((key.clone(), required_number(label, Some(probability), &format!("probability for {id}.{key}"))?)))
        .collect()
}

fn parse_answers(label: &str, value: Option<&Value>, context: &ClassifierContext) -> anyhow::Result<IndexMap<String, ClassifierAnswer>> {
    let Some(value) = value.and_then(Value::as_object) else {
        anyhow::bail!("{label} returned an unexpected response");
    };
    let mut answers = IndexMap::new();
    for (id, question) in &context.questions {
        let Some(answer) = value.get(id).filter(|a| a.is_object()) else {
            anyhow::bail!("{label} did not return an answer for {id}");
        };
        let kind = answer.get("type").and_then(Value::as_str);
        let parsed = match question {
            ClassifierQuestion::Choice { .. } => {
                let (Some("choice"), Some(choice)) = (kind, answer.get("choice").and_then(Value::as_str)) else {
                    anyhow::bail!("{label} did not return a choice answer for {id}");
                };
                ClassifierAnswer::Choice {
                    choice: choice.to_string(),
                    probabilities: probabilities(label, answer.get("probabilities"), id)?,
                    confidence: required_number(label, answer.get("confidence"), &format!("confidence for {id}"))?,
                }
            }
            ClassifierQuestion::Score { .. } => {
                if kind != Some("score") {
                    anyhow::bail!("{label} did not return a score answer for {id}");
                }
                ClassifierAnswer::Score {
                    score: required_number(label, answer.get("score"), &format!("score for {id}"))?,
                    confidence: required_number(label, answer.get("confidence"), &format!("confidence for {id}"))?,
                }
            }
            ClassifierQuestion::Bool { .. } => {
                if kind != Some("noul") {
                    anyhow::bail!("{label} did not return a bool answer for {id}");
                }
                ClassifierAnswer::Bool {
                    probability: required_number(label, answer.get("noul"), &format!("probability for {id}"))?,
                }
            }
        };
        answers.insert(id.clone(), parsed);
    }
    Ok(answers)
}

/// Map public `bool` questions to TypeSafe's wire-level `noul` type.
pub fn wire_request(context: &ClassifierContext) -> SystemOneWireRequest {
    let questions = context
        .questions
        .iter()
        .map(|(id, question)| {
            let mut value = serde_json::to_value(question).unwrap_or(Value::Null);
            if matches!(question, ClassifierQuestion::Bool { .. })
                && let Value::Object(map) = &mut value {
                    map.insert("type".into(), json!("noul"));
                }
            (id.clone(), value)
        })
        .collect();
    SystemOneWireRequest { state: context.state.clone(), questions }
}

/// Run one System One classification over the given transport. Never fails: errors become
/// results.
pub async fn classify_system_one(
    transport: &SystemOneTransport,
    model: &ClassifierModel,
    context: &ClassifierContext,
    options: &StreamOptions,
) -> ClassifierResult {
    let mut output = ClassifierResult {
        api: model.api.clone(),
        provider: model.provider.clone(),
        model: model.id.clone(),
        answers: IndexMap::new(),
        usage: None,
        stop_reason: ClassifierStopReason::Stop,
        error_message: None,
        timestamp: crate::utils::now_ms(),
    };
    let result: anyhow::Result<()> = async {
        if model.api != transport.api {
            anyhow::bail!("Unsupported classifier API: {}", model.api);
        }
        if context.images.as_ref().is_some_and(|i| !i.is_empty()) {
            anyhow::bail!("{} does not support image input", transport.label);
        }
        let body = post_classifier_request(
            transport.label,
            &(transport.url)(model),
            model,
            (transport.payload)(model, &wire_request(context)),
            options,
            &[],
        )
        .await?;
        let result = (transport.output)(body)?;
        // Set before parsing answers: a request with malformed answers was still billed.
        if let Some(usage) = parse_classifier_usage(result.get("usage"), model) {
            output.usage = Some(usage);
        }
        output.answers = parse_answers(transport.label, result.get("answers"), context)?;
        Ok(())
    }
    .await;
    if let Err(error) = result {
        output.stop_reason =
            if options.is_aborted() { ClassifierStopReason::Aborted } else { ClassifierStopReason::Error };
        output.error_message =
            Some(format_provider_error(&normalize_classifier_error(&error), Some(&format!("{} error", transport.label))));
    }
    output
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn joins_base_urls_like_the_url_constructor() {
        assert_eq!(join_base_url("https://api.typesafe.ai/v1/", "systemone"), "https://api.typesafe.ai/v1/systemone");
        assert_eq!(join_base_url("https://openrouter.ai/api/v1", "systemone"), "https://openrouter.ai/api/v1/systemone");
        assert_eq!(join_base_url("https://x.test/a//", "run"), "https://x.test/a/run");
    }
}
