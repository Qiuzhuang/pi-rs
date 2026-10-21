use pi_telemetry::{
    AttributeValue, InMemoryTelemetryContext, InstrumentOptions, MetricKind,
    TelemetryAttributeCardinality, TelemetryAttributeDefinition, TelemetryAttributeKind,
    TelemetryContextExt, TelemetryMetricDefinition, TelemetrySchemaDefinition, attributes,
    create_typed_metrics, define_telemetry_schema, noop_telemetry_context, noop_telemetry_metrics,
    same_ref,
};

fn traffic() -> TelemetrySchemaDefinition {
    serde_json::from_value(serde_json::json!({
        "version": 1,
        "spans": {},
        "metrics": {
            "network.bytes": {
                "kind": "counter",
                "description": "Encoded bytes",
                "unit": "By",
                "attributes": {
                    "direction": {
                        "type": "string",
                        "values": ["in", "out"],
                        "required": true,
                        "description": "Direction",
                        "cardinality": "low"
                    }
                }
            },
            "network.active": { "kind": "upDownCounter", "description": "Open links", "attributes": {} }
        }
    }))
    .unwrap()
}

fn latency() -> TelemetrySchemaDefinition {
    let mut metrics = indexmap::IndexMap::new();
    metrics.insert(
        "call.duration".to_string(),
        TelemetryMetricDefinition {
            kind: MetricKind::Histogram,
            description: "Call duration".into(),
            unit: Some("ms".into()),
            attributes: [(
                "method".to_string(),
                TelemetryAttributeDefinition::new(TelemetryAttributeKind::string(), "Method")
                    .required(false),
            )]
            .into_iter()
            .collect(),
            boundaries: Some(vec![1.0, 10.0, 100.0]),
        },
    );
    define_telemetry_schema(TelemetrySchemaDefinition {
        version: 1,
        spans: Default::default(),
        metrics: Some(metrics),
    })
}

#[tokio::test]
async fn binds_schema_names_kinds_metadata_and_measurement_attributes() {
    let adapter = InMemoryTelemetryContext::new();
    let (traffic, latency) = (traffic(), latency());
    let metrics = create_typed_metrics(adapter.clone(), &[&traffic, &latency]);
    metrics
        .counter("network.bytes")
        .add(128.0, attributes! { "direction" => "out" });
    metrics
        .up_down_counter("network.active")
        .add(1.0, attributes! {});
    metrics
        .up_down_counter("network.active")
        .add(-1.0, attributes! {});
    let typed = metrics.clone();
    adapter
        .start_span("request", |span| async move {
            typed.histogram("call.duration").record(
                12.0,
                attributes! { "method" => "read" },
                Some(span.as_ref()),
            );
            typed
                .histogram("call.duration")
                .record(13.0, attributes! {}, None);
            typed
                .histogram("call.duration")
                .record(14.0, attributes! {}, Some(span.as_ref()));
        })
        .await;
    let measurements = adapter.get_metrics();
    assert_eq!(measurements[0].kind, MetricKind::Counter);
    assert_eq!(measurements[0].name, "network.bytes");
    assert_eq!(
        measurements[0].description.as_deref(),
        Some("Encoded bytes")
    );
    assert_eq!(measurements[0].unit.as_deref(), Some("By"));
    assert_eq!(measurements[0].value, 128.0);
    assert_eq!(
        measurements[0].attributes,
        attributes! { "direction" => "out" }
    );
    assert_eq!(measurements[3].kind, MetricKind::Histogram);
    assert_eq!(measurements[3].boundaries, Some(vec![1.0, 10.0, 100.0]));
    let span_context = adapter.get_spans()[0].context.clone();
    assert_eq!(measurements[3].context.as_ref(), Some(&span_context));
    assert_eq!(measurements[4].context, None);
    assert_eq!(measurements[5].context.as_ref(), Some(&span_context));
    // Schemas stay serializable.
    let json = serde_json::to_value(&traffic).unwrap();
    assert_eq!(
        json["metrics"]["network.bytes"]["attributes"]["direction"]["cardinality"],
        "low"
    );
    assert_eq!(
        traffic.metrics.as_ref().unwrap()["network.bytes"].attributes["direction"]
            .attribute
            .cardinality,
        Some(TelemetryAttributeCardinality::Low)
    );
    let roundtrip: TelemetrySchemaDefinition = serde_json::from_value(json).unwrap();
    assert_eq!(roundtrip, traffic);
}

#[test]
fn requires_histogram_attributes_when_the_schema_declares_them() {
    let schema: TelemetrySchemaDefinition = serde_json::from_value(serde_json::json!({
        "version": 1,
        "spans": {},
        "metrics": {
            "required": {
                "kind": "histogram",
                "description": "Required tag",
                "attributes": { "cache": { "type": "boolean", "required": true, "description": "Cache hit" } }
            }
        }
    }))
    .unwrap();
    let adapter = InMemoryTelemetryContext::new();
    let metrics = create_typed_metrics(adapter.clone(), &[&schema]);
    metrics
        .histogram("required")
        .record(1.0, attributes! { "cache" => true }, None);
    assert_eq!(
        adapter.get_metrics()[0].attributes,
        attributes! { "cache" => true }
    );
    assert_eq!(
        adapter.get_metrics()[0].description.as_deref(),
        Some("Required tag")
    );
}

#[tokio::test]
async fn returns_detached_measurement_snapshots_including_exemplars_and_boundaries() {
    let telemetry = InMemoryTelemetryContext::new();
    let histogram = telemetry.histogram(
        "duration",
        InstrumentOptions::new().boundaries(vec![1.0, 2.0]),
    );
    telemetry
        .start_span("request", |span| async move {
            histogram.record(
                1.0,
                attributes! { "tags" => vec!["original"] },
                Some(span.as_ref()),
            );
        })
        .await;
    let mut measurement = telemetry.get_metrics()[0].clone();
    let context = measurement.context.as_mut().expect("measurement context");
    context.insert("pi-memory-span".into(), "999".into());
    measurement.boundaries.as_mut().unwrap().push(3.0);
    measurement
        .attributes
        .insert("tags".into(), AttributeValue::from(vec!["mutated"]));
    let fresh = &telemetry.get_metrics()[0];
    assert_eq!(fresh.boundaries, Some(vec![1.0, 2.0]));
    assert_eq!(fresh.attributes, attributes! { "tags" => vec!["original"] });
    assert_eq!(
        fresh.context.as_ref(),
        Some(&telemetry.get_spans()[0].context)
    );
}

#[test]
fn shares_no_op_instruments() {
    let metrics = noop_telemetry_metrics();
    let options = InstrumentOptions::new().unit("By");
    let counter = metrics.counter("bytes", options.clone());
    let active = metrics.up_down_counter("active", options.clone());
    let histogram = metrics.histogram("duration", options);
    assert!(same_ref(
        &counter,
        &metrics.counter("other", InstrumentOptions::new())
    ));
    counter.add(1.0, attributes! { "direction" => "out" });
    active.add(-1.0, attributes! { "direction" => "out" });
    let noop = noop_telemetry_context();
    histogram.record(
        1.0,
        attributes! { "direction" => "out" },
        Some(noop.as_ref()),
    );
}
