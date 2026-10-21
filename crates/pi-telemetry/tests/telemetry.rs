use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use pi_telemetry::{
    InMemoryTelemetryContext, SpanError, SpanLink, SpanStatus, TelemetryContextExt,
    TelemetrySchemaDefinition, attributes, create_typed_span_starter, define_telemetry_schema,
    noop_telemetry_context, noop_telemetry_metrics, noop_telemetry_propagation, same_ref,
};

fn operation_schema() -> TelemetrySchemaDefinition {
    serde_json::from_value(serde_json::json!({
        "version": 1,
        "spans": {
            "operation": {
                "description": "Operation",
                "parents": { "kind": "root_or_external" },
                "startAttributes": {
                    "kind": { "type": "string", "required": true, "values": ["read", "write"], "description": "Kind" }
                },
                "endAttributes": {},
                "status": { "default": "ok", "errorWhen": "The operation fails" }
            }
        }
    }))
    .unwrap()
}

fn request_schema() -> TelemetrySchemaDefinition {
    serde_json::from_value(serde_json::json!({
        "version": 3,
        "spans": {
            "request": {
                "description": "Request",
                "parents": { "kind": "spans", "spans": ["operation"] },
                "startAttributes": { "provider": { "type": "string", "required": true, "description": "Provider" } },
                "endAttributes": { "response": { "type": "string", "description": "Response kind" } },
                "status": { "default": "ok", "errorWhen": "The request fails" }
            }
        }
    }))
    .unwrap()
}

#[test]
fn preserves_serializable_definitions() {
    let definition = serde_json::json!({
        "version": 1,
        "spans": {
            "operation": {
                "description": "Test operation",
                "parents": { "kind": "any" },
                "startAttributes": {
                    "kind": { "type": "string", "values": ["read", "write"], "description": "Kind", "required": true }
                },
                "endAttributes": {},
                "events": {
                    "result": {
                        "description": "Result",
                        "attributes": {
                            "outcome": { "type": "string", "values": ["ok", "error"], "description": "Outcome", "required": true }
                        }
                    }
                },
                "status": { "default": "ok", "errorWhen": "The operation fails" }
            }
        }
    });
    let schema = define_telemetry_schema(serde_json::from_value(definition.clone()).unwrap());
    assert_eq!(serde_json::to_value(&schema).unwrap(), definition);
}

#[tokio::test]
async fn combines_schema_vocabularies_and_binds_child_starters_to_their_parent_spans() {
    let (operation, request) = (operation_schema(), request_schema());
    let telemetry = InMemoryTelemetryContext::new();
    let start = create_typed_span_starter(telemetry.clone(), &[&operation, &request]);
    let result = start
        .start(
            "operation",
            attributes! { "kind" => "read" },
            |_operation, child| async move {
                child
                    .start(
                        "request",
                        attributes! { "provider" => "example" },
                        |request, _| async move {
                            request.set_attributes(attributes! { "response" => "cached" });
                            42
                        },
                    )
                    .await
            },
        )
        .await;
    assert_eq!(result, 42);
    let spans = telemetry.get_spans();
    let operation_span = spans.iter().find(|span| span.name == "operation").unwrap();
    let request_span = spans.iter().find(|span| span.name == "request").unwrap();
    assert_eq!(operation_span.parent_id, None);
    assert_eq!(request_span.parent_id, Some(operation_span.id));
    assert_eq!(
        request_span.attributes,
        attributes! { "provider" => "example", "response" => "cached" }
    );

    let sync: Result<(), &str> = start
        .try_start("operation", attributes! { "kind" => "write" }, |_, _| {
            std::future::ready(Err("sync"))
        })
        .await;
    assert_eq!(sync, Err("sync"));
    let failed: Result<(), &str> = start
        .try_start(
            "request",
            attributes! { "provider" => "example" },
            |_, _| async { Err("async") },
        )
        .await;
    assert_eq!(failed, Err("async"));
    let spans = telemetry.get_spans();
    assert_eq!(
        spans[2].status,
        SpanStatus::error(SpanError::new("Error", "sync"))
    );
    assert_eq!(
        spans[3].status,
        SpanStatus::error(SpanError::new("Error", "async"))
    );
}

#[tokio::test]
async fn noop_admits_callbacks_synchronously_and_reuses_one_inert_span() {
    let admitted = Arc::new(AtomicBool::new(false));
    let noop = noop_telemetry_context();
    let flag = admitted.clone();
    let result = noop.start_span("first", move |span| {
        flag.store(true, Ordering::SeqCst);
        async move {
            let child = span.start_span("child", |child| async move { child }).await;
            assert!(same_ref(&child, &span));
            span
        }
    });
    assert!(admitted.load(Ordering::SeqCst));
    let first = result.await;
    assert!(same_ref(&first, &noop));
    assert!(same_ref(
        &noop.metrics().unwrap(),
        &noop_telemetry_metrics()
    ));
    assert!(same_ref(
        &noop.propagation().unwrap(),
        &noop_telemetry_propagation()
    ));
    assert!(same_ref(
        &first.metrics().unwrap(),
        &noop_telemetry_metrics()
    ));
}

#[tokio::test]
async fn noop_preserves_rejection_values() {
    let noop = noop_telemetry_context();
    let sync: Result<(), &str> = noop
        .try_start_span("sync", |_| std::future::ready(Err("sync")))
        .await;
    assert_eq!(sync, Err("sync"));
    let failed: Result<(), &str> = noop
        .try_start_span("async", |_| async { Err("async") })
        .await;
    assert_eq!(failed, Err("async"));
}

#[tokio::test]
async fn noop_ignores_typed_recording_payloads() {
    let result = noop_telemetry_context()
        .start_span(
            pi_telemetry::SpanOptions::new("operation")
                .attributes(attributes! { "kind" => "test" }),
            |span| async move {
                span.add_event("event", attributes! { "count" => 1 });
                span.set_attributes(attributes! { "complete" => true });
                span.set_status(SpanStatus::error(SpanError::new("Expected", "test")));
                span.add_link(SpanLink::new(span.clone()));
                42
            },
        )
        .await;
    assert_eq!(result, 42);
}

#[tokio::test]
async fn dropped_and_panicking_callbacks_settle_their_spans() {
    let telemetry = InMemoryTelemetryContext::new();
    let pending = telemetry.start_span("dropped", |_| std::future::pending::<()>());
    drop(pending);
    let spans = telemetry.get_spans();
    assert!(spans[0].settled);
    assert_eq!(
        spans[0].status,
        SpanStatus::error(SpanError::new(
            "AbortError",
            "The span callback was cancelled"
        ))
    );
}
