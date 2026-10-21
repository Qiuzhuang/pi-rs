use std::sync::Arc;

use once_cell::sync::Lazy;

use crate::attributes::{SpanAttributes, TelemetryCarrier};
use crate::context::{
    ContextRef, PropagationRef, SpanLink, SpanOptions, SpanOutcome, SpanRef, SpanStatus,
    TelemetryContext, TelemetryPropagation, TelemetrySpan,
};
use crate::metrics::{
    CounterRef, HistogramRef, InstrumentOptions, MetricsRef, TelemetryCounter, TelemetryHistogram,
    TelemetryMetrics, TelemetryUpDownCounter, UpDownCounterRef,
};

struct NoopInstrument;

impl TelemetryCounter for NoopInstrument {
    fn add(&self, _value: f64, _attributes: SpanAttributes) {}
}

impl TelemetryUpDownCounter for NoopInstrument {
    fn add(&self, _value: f64, _attributes: SpanAttributes) {}
}

impl TelemetryHistogram for NoopInstrument {
    fn record(
        &self,
        _value: f64,
        _attributes: SpanAttributes,
        _context: Option<&dyn TelemetryContext>,
    ) {
    }
}

static NOOP_COUNTER: Lazy<CounterRef> = Lazy::new(|| Arc::new(NoopInstrument));
static NOOP_UP_DOWN_COUNTER: Lazy<UpDownCounterRef> = Lazy::new(|| Arc::new(NoopInstrument));
static NOOP_HISTOGRAM: Lazy<HistogramRef> = Lazy::new(|| Arc::new(NoopInstrument));

struct NoopMetrics;

impl TelemetryMetrics for NoopMetrics {
    fn counter(&self, _name: &str, _options: InstrumentOptions) -> CounterRef {
        NOOP_COUNTER.clone()
    }

    fn up_down_counter(&self, _name: &str, _options: InstrumentOptions) -> UpDownCounterRef {
        NOOP_UP_DOWN_COUNTER.clone()
    }

    fn histogram(&self, _name: &str, _options: InstrumentOptions) -> HistogramRef {
        NOOP_HISTOGRAM.clone()
    }
}

struct NoopPropagation;

impl TelemetryPropagation for NoopPropagation {
    fn inject(&self, _context: &dyn TelemetryContext) -> Option<TelemetryCarrier> {
        None
    }

    fn extract(&self, _carrier: Option<&TelemetryCarrier>) -> ContextRef {
        NOOP_SPAN.clone()
    }
}

struct NoopSpan;

impl TelemetryContext for NoopSpan {
    fn metrics(&self) -> Option<MetricsRef> {
        Some(NOOP_METRICS.clone())
    }

    fn propagation(&self) -> Option<PropagationRef> {
        Some(NOOP_PROPAGATION.clone())
    }

    fn begin_span(&self, _options: SpanOptions) -> SpanRef {
        NOOP_SPAN.clone()
    }
}

impl TelemetrySpan for NoopSpan {
    fn add_link(&self, _link: SpanLink) {}
    fn add_event(&self, _name: &str, _attributes: SpanAttributes) {}
    fn set_attributes(&self, _attributes: SpanAttributes) {}
    fn set_status(&self, _status: SpanStatus) {}
    fn end(&self, _outcome: SpanOutcome) {}
}

static NOOP_METRICS: Lazy<MetricsRef> = Lazy::new(|| Arc::new(NoopMetrics));
static NOOP_PROPAGATION: Lazy<PropagationRef> = Lazy::new(|| Arc::new(NoopPropagation));
static NOOP_SPAN: Lazy<SpanRef> = Lazy::new(|| Arc::new(NoopSpan));

/// Shared telemetry context used when an application does not provide one. Every child span is
/// this same inert span.
pub fn noop_telemetry_context() -> SpanRef {
    NOOP_SPAN.clone()
}

/// Shared instruments that never inspect or retain metadata, measurements, or contexts.
pub fn noop_telemetry_metrics() -> MetricsRef {
    NOOP_METRICS.clone()
}

/// Shared propagation capability that neither inspects nor retains its inputs.
pub fn noop_telemetry_propagation() -> PropagationRef {
    NOOP_PROPAGATION.clone()
}
