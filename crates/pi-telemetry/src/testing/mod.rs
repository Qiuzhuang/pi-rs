//! Runner-independent conformance cases for telemetry adapters
//! (`@earendil-works/pi-telemetry/testing`). Cases panic on failure, so they can be registered
//! with any test harness.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use futures::future::BoxFuture;
use parking_lot::Mutex;

use crate::attributes;
use crate::attributes::{AttributeValue, SpanAttributes, TelemetryCarrier};
use crate::context::{
    ContextRef, PropagationRef, SpanError, SpanLink, SpanOptions, SpanRef, SpanStatus,
    TelemetryContext, TelemetryContextExt, same_ref,
};
use crate::memory::{RecordedTelemetryMeasurement, RecordedTelemetrySpan};
use crate::metrics::{InstrumentOptions, MetricKind, MetricsRef};
use crate::noop::noop_telemetry_context;

type SpansReader = Box<dyn Fn() -> BoxFuture<'static, Vec<RecordedTelemetrySpan>> + Send + Sync>;
type MetricsReader =
    Box<dyn Fn() -> BoxFuture<'static, Vec<RecordedTelemetryMeasurement>> + Send + Sync>;

/// A fresh adapter instance and normalized snapshot reader owned by one conformance case.
pub struct TelemetryAdapterFixture {
    pub context: ContextRef,
    pub propagation: Option<PropagationRef>,
    pub metrics: Option<MetricsRef>,
    pub get_metrics: Option<MetricsReader>,
    pub get_spans: SpansReader,
    /// Releases the adapter (`Symbol.asyncDispose`).
    pub dispose: Option<Box<dyn FnOnce() -> BoxFuture<'static, ()> + Send + Sync>>,
}

impl TelemetryAdapterFixture {
    async fn spans(&self) -> Vec<RecordedTelemetrySpan> {
        (self.get_spans)().await
    }

    async fn release(self) {
        if let Some(dispose) = self.dispose {
            dispose().await;
        }
    }
}

/// Creates an isolated adapter fixture for one conformance case.
pub type TelemetryAdapterFixtureFactory =
    Arc<dyn Fn() -> BoxFuture<'static, TelemetryAdapterFixture> + Send + Sync>;

type CaseBody =
    fn(TelemetryAdapterFixtureFactory, TelemetryAdapterFixture) -> BoxFuture<'static, ()>;

/// A runner-independent conformance case that can be registered with any test framework.
#[derive(Clone)]
pub struct TelemetryAdapterConformanceCase {
    pub group: &'static str,
    pub name: &'static str,
    factory: TelemetryAdapterFixtureFactory,
    body: CaseBody,
}

impl TelemetryAdapterConformanceCase {
    /// Run the case on a fresh fixture; panics on failure.
    pub async fn run(&self) {
        let fixture = (self.factory)().await;
        (self.body)(self.factory.clone(), fixture).await;
    }
}

fn find_span<'a>(spans: &'a [RecordedTelemetrySpan], name: &str) -> &'a RecordedTelemetrySpan {
    spans
        .iter()
        .find(|span| span.name == name)
        .unwrap_or_else(|| panic!("Expected recorded span {name}"))
}

fn assert_capabilities(
    context: &dyn TelemetryContext,
    metrics: &Option<MetricsRef>,
    propagation: &Option<PropagationRef>,
) {
    match (context.metrics(), metrics) {
        (Some(actual), Some(expected)) => {
            assert!(same_ref(&actual, expected), "metrics capability changed")
        }
        (None, None) => {}
        _ => panic!("metrics capability changed"),
    }
    match (context.propagation(), propagation) {
        (Some(actual), Some(expected)) => assert!(
            same_ref(&actual, expected),
            "propagation capability changed"
        ),
        (None, None) => {}
        _ => panic!("propagation capability changed"),
    }
}

fn capabilities(
    _factory: TelemetryAdapterFixtureFactory,
    fixture: TelemetryAdapterFixture,
) -> BoxFuture<'static, ()> {
    Box::pin(async move {
        let metrics = fixture
            .metrics
            .clone()
            .or_else(|| fixture.context.metrics());
        let propagation = fixture
            .propagation
            .clone()
            .or_else(|| fixture.context.propagation());
        assert_capabilities(fixture.context.as_ref(), &metrics, &propagation);
        let settled: Arc<Mutex<Option<SpanRef>>> = Arc::new(Mutex::new(None));
        {
            let (metrics, propagation, settled) =
                (metrics.clone(), propagation.clone(), settled.clone());
            fixture
                .context
                .start_span("capabilities", |span| async move {
                    *settled.lock() = Some(span.clone());
                    assert_capabilities(span.as_ref(), &metrics, &propagation);
                    {
                        let (metrics, propagation) = (metrics.clone(), propagation.clone());
                        span.start_span("capabilities.child", |child| async move {
                            assert_capabilities(child.as_ref(), &metrics, &propagation);
                        })
                        .await;
                    }
                    if let Some(propagation_ref) = &propagation {
                        let parent =
                            propagation_ref.extract(propagation_ref.inject(span.as_ref()).as_ref());
                        assert_capabilities(parent.as_ref(), &metrics, &propagation);
                        parent
                            .start_span("capabilities.extracted", |child| async move {
                                assert_capabilities(child.as_ref(), &metrics, &propagation);
                            })
                            .await;
                    }
                })
                .await;
        }
        let settled = settled.lock().clone().expect("callback ran");
        settled
            .start_span("capabilities.inert", |child| async move {
                assert_capabilities(child.as_ref(), &metrics, &propagation);
            })
            .await;
        fixture.release().await;
    })
}

fn ordered_measurements(
    _factory: TelemetryAdapterFixtureFactory,
    fixture: TelemetryAdapterFixture,
) -> BoxFuture<'static, ()> {
    Box::pin(async move {
        let Some(metrics) = fixture.metrics.clone() else {
            return fixture.release().await;
        };
        let get_metrics = fixture
            .get_metrics
            .as_ref()
            .expect("Metrics fixtures require a normalized measurement reader");
        metrics
            .counter(
                "bytes",
                InstrumentOptions::new()
                    .description("Encoded bytes")
                    .unit("By"),
            )
            .add(12.0, attributes! { "direction" => "out" });
        let active = metrics.up_down_counter("active", InstrumentOptions::new());
        active.add(1.0, SpanAttributes::new());
        active.add(-1.0, SpanAttributes::new());
        let histogram = metrics.histogram(
            "duration",
            InstrumentOptions::new()
                .unit("ms")
                .boundaries(vec![1.0, 10.0]),
        );
        fixture
            .context
            .start_span("measured", |span| async move {
                histogram.record(5.0, attributes! { "method" => "read" }, Some(span.as_ref()));
            })
            .await;
        let measurements = get_metrics().await;
        let summary: Vec<(MetricKind, String, f64)> = measurements
            .iter()
            .map(|m| (m.kind, m.name.clone(), m.value))
            .collect();
        assert_eq!(
            summary,
            vec![
                (MetricKind::Counter, "bytes".to_string(), 12.0),
                (MetricKind::UpDownCounter, "active".to_string(), 1.0),
                (MetricKind::UpDownCounter, "active".to_string(), -1.0),
                (MetricKind::Histogram, "duration".to_string(), 5.0),
            ]
        );
        assert_eq!(
            measurements[0].attributes,
            attributes! { "direction" => "out" }
        );
        assert_eq!(
            measurements[0].description.as_deref(),
            Some("Encoded bytes")
        );
        assert_eq!(measurements[0].unit.as_deref(), Some("By"));
        assert_eq!(measurements[3].boundaries, Some(vec![1.0, 10.0]));
        let spans = fixture.spans().await;
        assert_eq!(
            measurements[3].context.as_ref(),
            Some(&find_span(&spans, "measured").context)
        );
        fixture.release().await;
    })
}

fn copied_metadata(
    _factory: TelemetryAdapterFixtureFactory,
    fixture: TelemetryAdapterFixture,
) -> BoxFuture<'static, ()> {
    Box::pin(async move {
        let Some(metrics) = fixture.metrics.clone() else {
            return fixture.release().await;
        };
        let get_metrics = fixture
            .get_metrics
            .as_ref()
            .expect("Metrics fixtures require a normalized measurement reader");
        let mut options = InstrumentOptions::new()
            .description("Original")
            .boundaries(vec![1.0, 10.0]);
        let histogram = metrics.histogram("snapshot", options.clone());
        options.boundaries.as_mut().unwrap().push(100.0);
        options.description = Some("Changed".into());
        let mut tags = vec!["original".to_string()];
        histogram.record(-1.0, attributes! { "tags" => tags.clone() }, None);
        tags.push("changed".into());
        let measurements = get_metrics().await;
        assert_eq!(measurements.len(), 1);
        assert_eq!(
            measurements[0].attributes,
            attributes! { "tags" => vec!["original"] }
        );
        assert_eq!(measurements[0].boundaries, Some(vec![1.0, 10.0]));
        assert_eq!(measurements[0].description.as_deref(), Some("Original"));
        assert_eq!(measurements[0].context, None);
        fixture.release().await;
    })
}

fn initial_and_late_links(
    _factory: TelemetryAdapterFixtureFactory,
    fixture: TelemetryAdapterFixture,
) -> BoxFuture<'static, ()> {
    Box::pin(async move {
        let target = fixture
            .context
            .start_span("target", |span| async move { span })
            .await;
        let target_context: ContextRef = target.clone();
        let mut reasons = vec!["initial".to_string()];
        let options = SpanOptions::new("linked").link(SpanLink::with_attributes(
            target_context.clone(),
            attributes! { "reason" => reasons.clone() },
        ));
        fixture
            .context
            .start_span(options, |span| async move {
                reasons.push("mutated".into());
                span.add_link(SpanLink::with_attributes(
                    target_context,
                    attributes! { "reason" => "late" },
                ));
                span.add_link(SpanLink::new(noop_telemetry_context()));
            })
            .await;
        let spans = fixture.spans().await;
        let linked = find_span(&spans, "linked");
        assert_eq!(linked.parent_id, None);
        assert_eq!(linked.parent_context, None);
        let target_carrier = find_span(&spans, "target").context.clone();
        assert_eq!(linked.links.len(), 2);
        assert_eq!(linked.links[0].context, target_carrier);
        assert_eq!(
            linked.links[0].attributes,
            attributes! { "reason" => vec!["initial"] }
        );
        assert_eq!(linked.links[1].context, target_carrier);
        assert_eq!(
            linked.links[1].attributes,
            attributes! { "reason" => "late" }
        );
        fixture.release().await;
    })
}

fn links_after_settlement(
    _factory: TelemetryAdapterFixtureFactory,
    fixture: TelemetryAdapterFixture,
) -> BoxFuture<'static, ()> {
    Box::pin(async move {
        let source = fixture
            .context
            .start_span("link-source", |span| async move { span })
            .await;
        source.add_link(SpanLink::new(source.clone()));
        assert!(
            find_span(&fixture.spans().await, "link-source")
                .links
                .is_empty()
        );
        fixture.release().await;
    })
}

fn json_roundtrip(carrier: &TelemetryCarrier) -> TelemetryCarrier {
    serde_json::from_str(&serde_json::to_string(carrier).unwrap()).unwrap()
}

fn roundtrips_parent_identity(
    _factory: TelemetryAdapterFixtureFactory,
    fixture: TelemetryAdapterFixture,
) -> BoxFuture<'static, ()> {
    Box::pin(async move {
        let Some(propagation) = fixture.propagation.clone() else {
            return fixture.release().await;
        };
        fixture
            .context
            .start_span("sender", |span| async move {
                let carrier = propagation.inject(span.as_ref()).expect("carrier");
                let mut wire = json_roundtrip(&carrier);
                let parent = propagation.extract(Some(&wire));
                for value in wire.values_mut() {
                    *value = "mutated".into();
                }
                assert_eq!(parent.start_span("receiver", |_| async { 42 }).await, 42);
                assert_eq!(propagation.inject(span.as_ref()), Some(carrier));
            })
            .await;
        let spans = fixture.spans().await;
        assert_eq!(
            find_span(&spans, "receiver").parent_context.as_ref(),
            Some(&find_span(&spans, "sender").context)
        );
        fixture.release().await;
    })
}

fn unknown_carriers(
    _factory: TelemetryAdapterFixtureFactory,
    fixture: TelemetryAdapterFixture,
) -> BoxFuture<'static, ()> {
    Box::pin(async move {
        let Some(propagation) = fixture.propagation.clone() else {
            return fixture.release().await;
        };
        assert_eq!(propagation.inject(noop_telemetry_context().as_ref()), None);
        let mut unknown = TelemetryCarrier::new();
        unknown.insert("unknown".into(), "format".into());
        let carriers = [None, Some(TelemetryCarrier::new()), Some(unknown)];
        for (index, carrier) in carriers.iter().enumerate() {
            let context = propagation.extract(carrier.as_ref());
            assert_eq!(
                context
                    .start_span(format!("fallback-{index}"), |_| async { 42 })
                    .await,
                42
            );
        }
        for span in fixture.spans().await {
            assert_eq!(span.parent_context, None);
        }
        fixture.release().await;
    })
}

fn multiple_hops(
    factory: TelemetryAdapterFixtureFactory,
    fixture: TelemetryAdapterFixture,
) -> BoxFuture<'static, ()> {
    Box::pin(async move {
        let Some(propagation) = fixture.propagation.clone() else {
            return fixture.release().await;
        };
        let receiver = factory().await;
        let worker = factory().await;
        let (Some(receiver_propagation), Some(worker_propagation)) =
            (receiver.propagation.clone(), worker.propagation.clone())
        else {
            panic!("Expected propagation on each fixture");
        };
        let sender_carrier = fixture
            .context
            .start_span(
                "sender",
                |span| async move { propagation.inject(span.as_ref()) },
            )
            .await
            .expect("sender carrier");
        let receiver_carrier = receiver_propagation
            .extract(Some(&json_roundtrip(&sender_carrier)))
            .start_span("receiver", |span| async move {
                receiver_propagation.inject(span.as_ref())
            })
            .await
            .expect("receiver carrier");
        worker_propagation
            .extract(Some(&json_roundtrip(&receiver_carrier)))
            .start_span("worker", |_| async {})
            .await;
        assert_eq!(
            find_span(&receiver.spans().await, "receiver")
                .parent_context
                .as_ref(),
            Some(&sender_carrier)
        );
        assert_eq!(
            find_span(&worker.spans().await, "worker")
                .parent_context
                .as_ref(),
            Some(&receiver_carrier)
        );
        worker.release().await;
        receiver.release().await;
        fixture.release().await;
    })
}

fn admits_synchronously(
    _factory: TelemetryAdapterFixtureFactory,
    fixture: TelemetryAdapterFixture,
) -> BoxFuture<'static, ()> {
    Box::pin(async move {
        let admitted = Arc::new(AtomicBool::new(false));
        let calls = Arc::new(AtomicUsize::new(0));
        let result = {
            let (admitted, calls) = (admitted.clone(), calls.clone());
            fixture.context.start_span("success", move |_| {
                admitted.store(true, Ordering::SeqCst);
                calls.fetch_add(1, Ordering::SeqCst);
                async { vec![42] }
            })
        };
        assert!(admitted.load(Ordering::SeqCst));
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        assert_eq!(result.await, vec![42]);
        let spans = fixture.spans().await;
        assert_eq!(find_span(&spans, "success").status, SpanStatus::Ok);
        assert!(find_span(&spans, "success").settled);
        fixture.release().await;
    })
}

#[derive(Debug, PartialEq)]
struct Rejection(&'static str);

impl std::fmt::Display for Rejection {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.0)
    }
}

fn rejection_values(
    _factory: TelemetryAdapterFixtureFactory,
    fixture: TelemetryAdapterFixture,
) -> BoxFuture<'static, ()> {
    Box::pin(async move {
        let result: Result<(), Rejection> = fixture
            .context
            .try_start_span("sync-error", |_| std::future::ready(Err(Rejection("sync"))))
            .await;
        assert_eq!(result, Err(Rejection("sync")));
        let result: Result<(), Rejection> = fixture
            .context
            .try_start_span("async-error", |_| async { Err(Rejection("async")) })
            .await;
        assert_eq!(result, Err(Rejection("async")));
        let result: Result<(), Rejection> = fixture
            .context
            .try_start_span("undefined-error", |_| async { Err(Rejection("")) })
            .await;
        assert_eq!(result, Err(Rejection("")));
        let spans = fixture.spans().await;
        for name in ["sync-error", "async-error", "undefined-error"] {
            assert!(find_span(&spans, name).status.is_error(), "{name}");
        }
        fixture.release().await;
    })
}

fn explicit_status(
    _factory: TelemetryAdapterFixtureFactory,
    fixture: TelemetryAdapterFixture,
) -> BoxFuture<'static, ()> {
    Box::pin(async move {
        let expected = |message: &str| SpanStatus::error(SpanError::new("Expected", message));
        fixture
            .context
            .start_span("last-status", move |span| async move {
                span.set_status(SpanStatus::error(SpanError::new("Expected", "first")));
                span.set_status(SpanStatus::Ok);
            })
            .await;
        let result: Result<(), Rejection> = fixture
            .context
            .try_start_span("explicit-before-throw", |span| {
                span.set_status(SpanStatus::Ok);
                std::future::ready(Err(Rejection("after explicit status")))
            })
            .await;
        assert_eq!(result, Err(Rejection("after explicit status")));
        let result: Result<(), Rejection> = fixture
            .context
            .try_start_span("explicit-before-rejection", |span| async move {
                span.set_status(SpanStatus::error(SpanError::new(
                    "Expected",
                    "async failure",
                )));
                Err(Rejection("after async explicit status"))
            })
            .await;
        assert_eq!(result, Err(Rejection("after async explicit status")));
        let returned = fixture
            .context
            .start_span("expected-failure", |span| async move {
                span.set_status(SpanStatus::error(SpanError::new(
                    "Expected",
                    "returned failure",
                )));
                false
            })
            .await;
        assert!(!returned);
        let spans = fixture.spans().await;
        assert_eq!(find_span(&spans, "last-status").status, SpanStatus::Ok);
        assert_eq!(
            find_span(&spans, "explicit-before-throw").status,
            SpanStatus::Ok
        );
        assert_eq!(
            find_span(&spans, "explicit-before-rejection").status,
            expected("async failure")
        );
        assert_eq!(
            find_span(&spans, "expected-failure").status,
            expected("returned failure")
        );
        fixture.release().await;
    })
}

fn merges_attributes(
    _factory: TelemetryAdapterFixtureFactory,
    fixture: TelemetryAdapterFixture,
) -> BoxFuture<'static, ()> {
    Box::pin(async move {
        let options = SpanOptions::new("recording").attributes(attributes! {
            "start" => "value",
            "overwrite" => "start",
            "ignored" => None::<&str>,
        });
        fixture
            .context
            .start_span(options, |span| async move {
                span.set_attributes(attributes! { "count" => 1, "overwrite" => "middle" });
                span.set_attributes(attributes! { "count" => None::<i64>, "overwrite" => "end" });
                span.add_event(
                    "first",
                    attributes! { "index" => 1, "ignored" => None::<&str> },
                );
                span.add_event("second", attributes! { "index" => 2 });
            })
            .await;
        let spans = fixture.spans().await;
        let span = find_span(&spans, "recording");
        let mut expected = SpanAttributes::new();
        expected.insert("start".into(), AttributeValue::from("value"));
        expected.insert("overwrite".into(), AttributeValue::from("end"));
        expected.insert("count".into(), AttributeValue::from(1));
        assert_eq!(span.attributes, expected);
        assert_eq!(span.events.len(), 2);
        assert_eq!(span.events[0].name, "first");
        assert_eq!(span.events[0].attributes, attributes! { "index" => 1 });
        assert_eq!(span.events[1].name, "second");
        assert_eq!(span.events[1].attributes, attributes! { "index" => 2 });
        fixture.release().await;
    })
}

fn inert_after_settlement(
    _factory: TelemetryAdapterFixtureFactory,
    fixture: TelemetryAdapterFixture,
) -> BoxFuture<'static, ()> {
    Box::pin(async move {
        let span = fixture
            .context
            .start_span(
                SpanOptions::new("settled").attributes(attributes! { "value" => "initial" }),
                |span| async move { span },
            )
            .await;
        span.set_attributes(attributes! { "value" => "late" });
        span.add_event("late", attributes! { "value" => true });
        span.set_status(SpanStatus::Error(None));
        let admitted = Arc::new(AtomicBool::new(false));
        let child = {
            let admitted = admitted.clone();
            span.start_span("late-child", move |_| {
                admitted.store(true, Ordering::SeqCst);
                async { 7 }
            })
        };
        assert!(admitted.load(Ordering::SeqCst));
        assert_eq!(child.await, 7);
        let spans = fixture.spans().await;
        assert_eq!(spans.len(), 1);
        assert_eq!(spans[0].attributes, attributes! { "value" => "initial" });
        assert!(spans[0].events.is_empty());
        assert_eq!(spans[0].status, SpanStatus::Ok);
        fixture.release().await;
    })
}

fn nested_and_concurrent(
    _factory: TelemetryAdapterFixtureFactory,
    fixture: TelemetryAdapterFixture,
) -> BoxFuture<'static, ()> {
    Box::pin(async move {
        let (release, gate) = futures::channel::oneshot::channel::<()>();
        fixture
            .context
            .start_span("parent", |parent| async move {
                let first = parent.start_span("first-child", |_| async move {
                    let _ = gate.await;
                });
                let second = parent.start_span("second-child", |_| async { "done" });
                let finish = async move {
                    let result = second.await;
                    let _ = release.send(());
                    result
                };
                let ((), result) = futures::join!(first, finish);
                assert_eq!(result, "done");
            })
            .await;
        let spans = fixture.spans().await;
        let parent = find_span(&spans, "parent");
        let first = find_span(&spans, "first-child");
        let second = find_span(&spans, "second-child");
        assert_eq!(parent.parent_id, None);
        assert_eq!(first.parent_id, Some(parent.id));
        assert_eq!(second.parent_id, Some(parent.id));
        let (Some(first_end), Some(second_end), Some(parent_end)) =
            (first.end_sequence, second.end_sequence, parent.end_sequence)
        else {
            panic!("expected end sequences");
        };
        assert!(second_end < first_end);
        assert!(first_end < parent_end);
        fixture.release().await;
    })
}

/// Creates runner-independent cases for the callback telemetry adapter contract.
pub fn create_telemetry_adapter_conformance(
    factory: TelemetryAdapterFixtureFactory,
) -> Vec<TelemetryAdapterConformanceCase> {
    let cases: [(&'static str, &'static str, CaseBody); 14] = [
        (
            "capabilities",
            "carries adapter capabilities through contexts and spans",
            capabilities,
        ),
        (
            "metrics",
            "records ordered measurements and optional histogram exemplars",
            ordered_measurements,
        ),
        (
            "metrics",
            "copies instrument metadata and measurement attributes",
            copied_metadata,
        ),
        (
            "links",
            "records initial and late links without changing parents",
            initial_and_late_links,
        ),
        (
            "links",
            "ignores links added after source settlement",
            links_after_settlement,
        ),
        (
            "propagation",
            "roundtrips parent identity through JSON without retaining the carrier",
            roundtrips_parent_identity,
        ),
        (
            "propagation",
            "absent and unknown carriers produce recorded roots",
            unknown_carriers,
        ),
        (
            "propagation",
            "propagates across fresh adapters and multiple hops",
            multiple_hops,
        ),
        (
            "callback lifecycle",
            "admits once synchronously and preserves the result",
            admits_synchronously,
        ),
        (
            "callback lifecycle",
            "preserves synchronous and asynchronous rejection values",
            rejection_values,
        ),
        (
            "status",
            "uses last explicit status without automatic overwrite",
            explicit_status,
        ),
        (
            "recording",
            "merges attributes and records ordered events",
            merges_attributes,
        ),
        (
            "recording",
            "makes calls after settlement inert",
            inert_after_settlement,
        ),
        (
            "parentage",
            "records nested and concurrent child relationships",
            nested_and_concurrent,
        ),
    ];
    cases
        .into_iter()
        .map(|(group, name, body)| TelemetryAdapterConformanceCase {
            group,
            name,
            factory: factory.clone(),
            body,
        })
        .collect()
}
