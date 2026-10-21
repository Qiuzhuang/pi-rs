//! Serializable telemetry schema definitions.
//!
//! The TypeScript package derives exact attribute types from schema literals at compile time. Rust
//! keeps the same serializable data model; [`TypedSpanStarter`] and
//! [`TypedTelemetryMetrics`](crate::TypedTelemetryMetrics) bind contexts to schema vocabularies
//! without runtime validation, as in TypeScript.

use std::future::Future;

use indexmap::IndexMap;
use serde::{Deserialize, Serialize};

use crate::attributes::SpanAttributes;
use crate::context::{ContextRef, SpanFuture, SpanOptions, SpanRef, TelemetryContextExt};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TelemetryAttributeCardinality {
    Low,
    High,
}

/// The value type of an attribute, with optional closed value sets and examples.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum TelemetryAttributeKind {
    #[serde(rename = "string")]
    String {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        values: Option<Vec<String>>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        examples: Option<Vec<String>>,
    },
    #[serde(rename = "number")]
    Number {
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            with = "crate::js::option_numbers"
        )]
        values: Option<Vec<f64>>,
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            with = "crate::js::option_numbers"
        )]
        examples: Option<Vec<f64>>,
    },
    #[serde(rename = "boolean")]
    Boolean {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        values: Option<Vec<bool>>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        examples: Option<Vec<bool>>,
    },
    #[serde(rename = "string[]")]
    StringArray {
        #[serde(
            rename = "elementValues",
            default,
            skip_serializing_if = "Option::is_none"
        )]
        element_values: Option<Vec<String>>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        examples: Option<Vec<Vec<String>>>,
    },
    #[serde(rename = "number[]")]
    NumberArray {
        #[serde(
            rename = "elementValues",
            default,
            skip_serializing_if = "Option::is_none",
            with = "crate::js::option_numbers"
        )]
        element_values: Option<Vec<f64>>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        examples: Option<Vec<Vec<f64>>>,
    },
    #[serde(rename = "boolean[]")]
    BooleanArray {
        #[serde(
            rename = "elementValues",
            default,
            skip_serializing_if = "Option::is_none"
        )]
        element_values: Option<Vec<bool>>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        examples: Option<Vec<Vec<bool>>>,
    },
}

impl TelemetryAttributeKind {
    pub fn string() -> Self {
        TelemetryAttributeKind::String {
            values: None,
            examples: None,
        }
    }

    pub fn string_values<I: IntoIterator<Item = S>, S: Into<String>>(values: I) -> Self {
        TelemetryAttributeKind::String {
            values: Some(values.into_iter().map(Into::into).collect()),
            examples: None,
        }
    }

    pub fn number() -> Self {
        TelemetryAttributeKind::Number {
            values: None,
            examples: None,
        }
    }

    pub fn boolean() -> Self {
        TelemetryAttributeKind::Boolean {
            values: None,
            examples: None,
        }
    }

    pub fn string_array() -> Self {
        TelemetryAttributeKind::StringArray {
            element_values: None,
            examples: None,
        }
    }

    pub fn number_array() -> Self {
        TelemetryAttributeKind::NumberArray {
            element_values: None,
            examples: None,
        }
    }

    pub fn boolean_array() -> Self {
        TelemetryAttributeKind::BooleanArray {
            element_values: None,
            examples: None,
        }
    }

    /// The TypeScript `type` discriminant.
    pub fn type_name(&self) -> &'static str {
        match self {
            TelemetryAttributeKind::String { .. } => "string",
            TelemetryAttributeKind::Number { .. } => "number",
            TelemetryAttributeKind::Boolean { .. } => "boolean",
            TelemetryAttributeKind::StringArray { .. } => "string[]",
            TelemetryAttributeKind::NumberArray { .. } => "number[]",
            TelemetryAttributeKind::BooleanArray { .. } => "boolean[]",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TelemetryAttributeDefinition {
    #[serde(flatten)]
    pub kind: TelemetryAttributeKind,
    pub description: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sensitive: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cardinality: Option<TelemetryAttributeCardinality>,
}

impl TelemetryAttributeDefinition {
    pub fn new(kind: TelemetryAttributeKind, description: impl Into<String>) -> Self {
        Self {
            kind,
            description: description.into(),
            sensitive: None,
            cardinality: None,
        }
    }

    pub fn sensitive(mut self, sensitive: bool) -> Self {
        self.sensitive = Some(sensitive);
        self
    }

    pub fn cardinality(mut self, cardinality: TelemetryAttributeCardinality) -> Self {
        self.cardinality = Some(cardinality);
        self
    }

    pub fn required(self, required: bool) -> TelemetryStartAttributeDefinition {
        TelemetryStartAttributeDefinition {
            attribute: self,
            required,
        }
    }
}

/// An attribute definition with a `required` flag (start, event, and metric attributes).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TelemetryStartAttributeDefinition {
    #[serde(flatten)]
    pub attribute: TelemetryAttributeDefinition,
    pub required: bool,
}

pub type TelemetryEventAttributeDefinition = TelemetryStartAttributeDefinition;
/// Metric attributes should have low cardinality; this is not enforced.
pub type TelemetryMetricAttributeDefinition = TelemetryStartAttributeDefinition;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TelemetryEventDefinition {
    pub description: String,
    pub attributes: IndexMap<String, TelemetryEventAttributeDefinition>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind")]
pub enum TelemetryParentDefinition {
    #[serde(rename = "any")]
    Any,
    #[serde(rename = "root_or_external")]
    RootOrExternal,
    #[serde(rename = "spans")]
    Spans { spans: Vec<String> },
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TelemetrySpanStatusDefinition {
    /// Always `"ok"`.
    pub default: String,
    pub error_when: String,
}

impl TelemetrySpanStatusDefinition {
    pub fn new(error_when: impl Into<String>) -> Self {
        Self {
            default: "ok".to_string(),
            error_when: error_when.into(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TelemetrySpanDefinition {
    pub description: String,
    pub parents: TelemetryParentDefinition,
    pub start_attributes: IndexMap<String, TelemetryStartAttributeDefinition>,
    pub end_attributes: IndexMap<String, TelemetryAttributeDefinition>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub events: Option<IndexMap<String, TelemetryEventDefinition>>,
    pub status: TelemetrySpanStatusDefinition,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TelemetryMetricDefinition {
    pub kind: crate::metrics::MetricKind,
    pub description: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub unit: Option<String>,
    pub attributes: IndexMap<String, TelemetryMetricAttributeDefinition>,
    /// Histograms only.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "crate::js::option_numbers"
    )]
    pub boundaries: Option<Vec<f64>>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TelemetrySchemaDefinition {
    pub version: u32,
    pub spans: IndexMap<String, TelemetrySpanDefinition>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub metrics: Option<IndexMap<String, TelemetryMetricDefinition>>,
}

/// Identity helper for serializable telemetry schema data.
pub fn define_telemetry_schema(schema: TelemetrySchemaDefinition) -> TelemetrySchemaDefinition {
    schema
}

/// A span starter bound to one explicit parent context and the combined span vocabulary of one or
/// more schemas. No runtime schema validation is performed.
#[derive(Clone)]
pub struct TypedSpanStarter {
    context: ContextRef,
}

impl TypedSpanStarter {
    pub fn context(&self) -> &ContextRef {
        &self.context
    }

    /// Start `name` with `attributes`; the callback receives the span and a starter bound to it.
    pub fn start<F, Fut>(
        &self,
        name: &str,
        attributes: SpanAttributes,
        callback: F,
    ) -> SpanFuture<Fut>
    where
        F: FnOnce(SpanRef, TypedSpanStarter) -> Fut,
        Fut: Future,
    {
        self.context
            .start_span(SpanOptions::new(name).attributes(attributes), |span| {
                let child = TypedSpanStarter {
                    context: span.clone(),
                };
                callback(span, child)
            })
    }

    /// Like [`start`](Self::start) for fallible callbacks (see
    /// [`TelemetryContextExt::try_start_span`]).
    pub fn try_start<F, Fut, T, E>(
        &self,
        name: &str,
        attributes: SpanAttributes,
        callback: F,
    ) -> SpanFuture<Fut>
    where
        F: FnOnce(SpanRef, TypedSpanStarter) -> Fut,
        Fut: Future<Output = Result<T, E>>,
        E: std::fmt::Display,
    {
        self.context
            .try_start_span(SpanOptions::new(name).attributes(attributes), |span| {
                let child = TypedSpanStarter {
                    context: span.clone(),
                };
                callback(span, child)
            })
    }
}

/// Bind an explicit parent context to the span vocabulary of one or more schemas. Schema values
/// are only documentation here; no runtime schema validation is performed.
pub fn create_typed_span_starter(
    context: ContextRef,
    _schemas: &[&TelemetrySchemaDefinition],
) -> TypedSpanStarter {
    TypedSpanStarter { context }
}
