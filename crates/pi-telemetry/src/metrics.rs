use std::any::Any;
use std::collections::HashMap;
use std::sync::Arc;

use serde::{Deserialize, Serialize};

use crate::attributes::SpanAttributes;
use crate::context::TelemetryContext;
use crate::schema::{TelemetryMetricDefinition, TelemetrySchemaDefinition};

pub type MetricsRef = Arc<dyn TelemetryMetrics>;
pub type CounterRef = Arc<dyn TelemetryCounter>;
pub type UpDownCounterRef = Arc<dyn TelemetryUpDownCounter>;
pub type HistogramRef = Arc<dyn TelemetryHistogram>;

/// Instrument metadata. `boundaries` are bucket hints for histograms only; callers supply finite,
/// strictly increasing values.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct InstrumentOptions {
    pub description: Option<String>,
    pub unit: Option<String>,
    pub boundaries: Option<Vec<f64>>,
}

impl InstrumentOptions {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn description(mut self, description: impl Into<String>) -> Self {
        self.description = Some(description.into());
        self
    }

    pub fn unit(mut self, unit: impl Into<String>) -> Self {
        self.unit = Some(unit.into());
        self
    }

    pub fn boundaries(mut self, boundaries: impl Into<Vec<f64>>) -> Self {
        self.boundaries = Some(boundaries.into());
        self
    }
}

impl From<&TelemetryMetricDefinition> for InstrumentOptions {
    fn from(definition: &TelemetryMetricDefinition) -> Self {
        Self {
            description: Some(definition.description.clone()),
            unit: definition.unit.clone(),
            boundaries: definition.boundaries.clone(),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum MetricKind {
    #[serde(rename = "counter")]
    Counter,
    #[serde(rename = "upDownCounter")]
    UpDownCounter,
    #[serde(rename = "histogram")]
    Histogram,
}

impl MetricKind {
    pub fn as_str(self) -> &'static str {
        match self {
            MetricKind::Counter => "counter",
            MetricKind::UpDownCounter => "upDownCounter",
            MetricKind::Histogram => "histogram",
        }
    }
}

pub trait TelemetryCounter: Send + Sync {
    fn add(&self, value: f64, attributes: SpanAttributes);
}

pub trait TelemetryUpDownCounter: Send + Sync {
    fn add(&self, value: f64, attributes: SpanAttributes);
}

pub trait TelemetryHistogram: Send + Sync {
    /// `context` is an optional exemplar span.
    fn record(
        &self,
        value: f64,
        attributes: SpanAttributes,
        context: Option<&dyn TelemetryContext>,
    );
}

/// Optional adapter capability with synchronous factories and recording methods.
/// Typed inputs are trusted; callers supply finite measurements and nonnegative counter increments.
/// Malformed input behavior is unspecified.
pub trait TelemetryMetrics: Any + Send + Sync {
    fn counter(&self, name: &str, options: InstrumentOptions) -> CounterRef;
    fn up_down_counter(&self, name: &str, options: InstrumentOptions) -> UpDownCounterRef;
    fn histogram(&self, name: &str, options: InstrumentOptions) -> HistogramRef;
}

/// Counter bound to a schema metric (also used for up-down counters).
#[derive(Clone)]
pub enum SchemaTelemetryCounter {
    Counter(CounterRef),
    UpDownCounter(UpDownCounterRef),
}

impl SchemaTelemetryCounter {
    pub fn add(&self, value: f64, attributes: SpanAttributes) {
        match self {
            SchemaTelemetryCounter::Counter(counter) => counter.add(value, attributes),
            SchemaTelemetryCounter::UpDownCounter(counter) => counter.add(value, attributes),
        }
    }
}

/// Histogram bound to a schema metric.
#[derive(Clone)]
pub struct SchemaTelemetryHistogram(pub HistogramRef);

impl SchemaTelemetryHistogram {
    pub fn record(
        &self,
        value: f64,
        attributes: SpanAttributes,
        context: Option<&dyn TelemetryContext>,
    ) {
        self.0.record(value, attributes, context)
    }
}

/// Instrument factories bound to the metric definitions of one or more schemas. Names, kinds and
/// attributes are checked at compile time in TypeScript; here the definitions supply the metadata
/// and no runtime validation is performed.
#[derive(Clone)]
pub struct TypedTelemetryMetrics {
    metrics: MetricsRef,
    definitions: Arc<HashMap<String, TelemetryMetricDefinition>>,
}

impl TypedTelemetryMetrics {
    fn options(&self, name: &str) -> InstrumentOptions {
        self.definitions
            .get(name)
            .map(InstrumentOptions::from)
            .unwrap_or_default()
    }

    pub fn counter(&self, name: &str) -> SchemaTelemetryCounter {
        SchemaTelemetryCounter::Counter(self.metrics.counter(name, self.options(name)))
    }

    pub fn up_down_counter(&self, name: &str) -> SchemaTelemetryCounter {
        SchemaTelemetryCounter::UpDownCounter(
            self.metrics.up_down_counter(name, self.options(name)),
        )
    }

    pub fn histogram(&self, name: &str) -> SchemaTelemetryHistogram {
        SchemaTelemetryHistogram(self.metrics.histogram(name, self.options(name)))
    }

    pub fn metrics(&self) -> &MetricsRef {
        &self.metrics
    }
}

/// Bind instrument names, kinds, and metadata to one or more schemas. Later schemas win for
/// duplicate metric names (TypeScript rejects duplicates at compile time).
pub fn create_typed_metrics(
    metrics: MetricsRef,
    schemas: &[&TelemetrySchemaDefinition],
) -> TypedTelemetryMetrics {
    let mut definitions = HashMap::new();
    for schema in schemas {
        if let Some(metrics) = &schema.metrics {
            for (name, definition) in metrics {
                definitions.insert(name.clone(), definition.clone());
            }
        }
    }
    TypedTelemetryMetrics {
        metrics,
        definitions: Arc::new(definitions),
    }
}
