use std::sync::Arc;

use pi_telemetry::testing::{
    TelemetryAdapterFixture, TelemetryAdapterFixtureFactory, create_telemetry_adapter_conformance,
};
use pi_telemetry::{
    AttributeValue, InMemoryTelemetryContext, SpanOptions, TelemetryContextExt, attributes,
};

fn factory() -> TelemetryAdapterFixtureFactory {
    Arc::new(|| {
        Box::pin(async {
            let context = InMemoryTelemetryContext::new();
            let (spans, metrics) = (context.clone(), context.clone());
            TelemetryAdapterFixture {
                context: context.clone(),
                propagation: Some(context.clone()),
                metrics: Some(context.clone()),
                get_metrics: Some(Box::new(move || {
                    let metrics = metrics.clone();
                    Box::pin(async move { metrics.get_metrics() })
                })),
                get_spans: Box::new(move || {
                    let spans = spans.clone();
                    Box::pin(async move { spans.get_spans() })
                }),
                dispose: None,
            }
        })
    })
}

#[tokio::test]
async fn in_memory_telemetry_context_conformance() {
    let cases = create_telemetry_adapter_conformance(factory());
    assert_eq!(cases.len(), 14);
    for case in cases {
        eprintln!("{} > {}", case.group, case.name);
        case.run().await;
    }
}

#[tokio::test]
async fn returns_detached_snapshots_without_exposing_mutable_recording_state() {
    let context = InMemoryTelemetryContext::new();
    let inner = context.clone();
    let (open_settled, open_end_sequence) = context
        .start_span(
            SpanOptions::new("snapshot").attributes(attributes! { "tags" => vec!["initial"] }),
            |span| async move {
                span.add_event("event", attributes! { "value" => 1 });
                let open = inner.get_spans()[0].clone();
                (open.settled, open.end_sequence)
            },
        )
        .await;
    assert!(!open_settled);
    assert_eq!(open_end_sequence, None);
    let mut first = context.get_spans()[0].clone();
    assert!(first.settled);
    assert_eq!(first.end_sequence, Some(1));
    first
        .attributes
        .insert("tags".into(), AttributeValue::from(vec!["mutated"]));
    first.events[0]
        .attributes
        .insert("value".into(), AttributeValue::from(2));
    let second = context.get_spans()[0].clone();
    assert_eq!(second.attributes, attributes! { "tags" => vec!["initial"] });
    assert_eq!(second.events[0].attributes, attributes! { "value" => 1 });
}
