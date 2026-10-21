use std::sync::Arc;

use parking_lot::Mutex;
use pi_telemetry::{
    AttributeValue, ContextRef, InMemoryTelemetryContext, RecordedTelemetrySpan, SpanLink,
    SpanOptions, SpanRef, TelemetryCarrier, TelemetryContextExt, attributes,
    noop_telemetry_context, noop_telemetry_propagation, same_ref,
};

fn carrier(pairs: &[(&str, &str)]) -> TelemetryCarrier {
    pairs
        .iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect()
}

#[tokio::test]
async fn continues_a_parent_in_another_recorder_without_shared_state() {
    // The TypeScript test runs the receiver in another Node process; a fresh recorder that only
    // sees the JSON carrier is equivalent.
    let telemetry = InMemoryTelemetryContext::new();
    let inner = telemetry.clone();
    let sent = telemetry
        .start_span("caller", |span| async move { inner.inject(span.as_ref()) })
        .await
        .unwrap();
    let wire = serde_json::to_string(&sent).unwrap();
    let output = {
        let worker = InMemoryTelemetryContext::new();
        let carrier: TelemetryCarrier = serde_json::from_str(&wire).unwrap();
        worker
            .extract(Some(&carrier))
            .start_span("remote", |_| async {})
            .await;
        serde_json::to_string(&worker.get_spans()).unwrap()
    };
    let recorded: Vec<RecordedTelemetrySpan> = serde_json::from_str(&output).unwrap();
    assert_eq!(recorded.len(), 1);
    assert_eq!(recorded[0].parent_context.as_ref(), Some(&sent));
    assert_eq!(recorded[0].parent_id, None);
    assert_ne!(recorded[0].context, sent);
    let json: serde_json::Value = serde_json::from_str(&output).unwrap();
    assert!(json[0]["parentId"].is_null());
}

#[tokio::test]
async fn isolates_crossed_concurrent_remote_parents_and_links_shared_roots() {
    type SharedUnit = futures::future::Shared<futures::future::BoxFuture<'static, ()>>;
    let caller = InMemoryTelemetryContext::new();
    let provider = InMemoryTelemetryContext::new();
    let activation: Arc<Mutex<Option<SpanRef>>> = Arc::default();
    let shared: Arc<Mutex<Option<SharedUnit>>> = Arc::default();
    let (finish, gate) = tokio::sync::oneshot::channel::<()>();
    let gate: SharedUnit = futures::FutureExt::shared(Box::pin(async move {
        let _ = gate.await;
    })
        as futures::future::BoxFuture<'static, ()>);
    let finish = Arc::new(Mutex::new(Some(finish)));
    let run = |name: &'static str| {
        let (caller_inner, provider, activation, shared, gate, finish) = (
            caller.clone(),
            provider.clone(),
            activation.clone(),
            shared.clone(),
            gate.clone(),
            finish.clone(),
        );
        caller.start_span(name, move |trigger| async move {
            let parent = provider.extract(caller_inner.inject(trigger.as_ref()).as_ref());
            parent
                .start_span(format!("{name}.method"), |getter| async move {
                    let current = {
                        let mut shared = shared.lock();
                        if shared.is_none() {
                            let activation = activation.clone();
                            let getter_context: ContextRef = getter.clone();
                            let future = provider.start_span(
                                SpanOptions::new("activation").link(SpanLink::new(getter_context)),
                                move |span| {
                                    *activation.lock() = Some(span);
                                    gate
                                },
                            );
                            let boxed: futures::future::BoxFuture<'static, ()> = Box::pin(future);
                            *shared = Some(futures::FutureExt::shared(boxed));
                        }
                        shared.clone().unwrap()
                    };
                    let current_activation = activation.lock().clone();
                    if let Some(activation) = current_activation {
                        getter.add_link(SpanLink::new(activation));
                    }
                    if name == "later"
                        && let Some(finish) = finish.lock().take()
                    {
                        let _ = finish.send(());
                    }
                    current.await;
                })
                .await;
        })
    };
    let (first, later) = (run("first"), run("later"));
    futures::join!(first, later);
    let spans = provider.get_spans();
    let find = |name: &str| spans.iter().find(|span| span.name == name).unwrap().clone();
    let root = find("activation");
    assert_eq!(root.parent_context, None);
    let first = find("first.method");
    let later = find("later.method");
    assert_eq!(
        first.parent_context.as_ref(),
        Some(&caller.get_spans()[0].context)
    );
    assert_eq!(
        later.parent_context.as_ref(),
        Some(&caller.get_spans()[1].context)
    );
    assert_eq!(root.links[0].context, first.context);
    assert_eq!(later.links[0].context, root.context);
}

#[tokio::test]
async fn detaches_carrier_and_link_snapshots_and_retains_settled_target_identities() {
    let telemetry = InMemoryTelemetryContext::new();
    let target = telemetry
        .start_span("target", |span| async move { span })
        .await;
    let original = telemetry.inject(target.as_ref());
    let mut injected = telemetry.inject(target.as_ref()).expect("carrier");
    injected.insert("pi-memory-span".into(), "999".into());
    assert_eq!(telemetry.inject(target.as_ref()), original);
    telemetry
        .start_span(
            SpanOptions::new("linked").link(SpanLink::with_attributes(
                target.clone(),
                attributes! { "tags" => vec!["original"] },
            )),
            |_| async {},
        )
        .await;
    let mut spans = telemetry.get_spans();
    spans[0]
        .context
        .insert("pi-memory-span".into(), "123".into());
    spans[1].links[0]
        .context
        .insert("pi-memory-span".into(), "456".into());
    spans[1].links[0]
        .attributes
        .insert("tags".into(), AttributeValue::from(vec!["mutated"]));
    let fresh = telemetry.get_spans();
    assert_eq!(fresh[1].links.len(), 1);
    assert_eq!(Some(&fresh[1].links[0].context), original.as_ref());
    assert_eq!(
        fresh[1].links[0].attributes,
        attributes! { "tags" => vec!["original"] }
    );
}

#[tokio::test]
async fn links_a_new_root_to_an_extracted_remote_target() {
    let sender = InMemoryTelemetryContext::new();
    let receiver = InMemoryTelemetryContext::new();
    let inner = sender.clone();
    let sent = sender
        .start_span("trigger", |span| async move { inner.inject(span.as_ref()) })
        .await
        .unwrap();
    let wire: TelemetryCarrier =
        serde_json::from_str(&serde_json::to_string(&sent).unwrap()).unwrap();
    let target = receiver.extract(Some(&wire));
    let late = target.clone();
    receiver
        .start_span(
            SpanOptions::new("background").link(SpanLink::with_attributes(
                target,
                attributes! { "reason" => "restart" },
            )),
            |span| async move { span.add_link(SpanLink::new(late)) },
        )
        .await;
    let recorded = &receiver.get_spans()[0];
    assert_eq!(recorded.parent_context, None);
    assert_eq!(recorded.links.len(), 2);
    assert_eq!(recorded.links[0].context, sent);
    assert_eq!(
        recorded.links[0].attributes,
        attributes! { "reason" => "restart" }
    );
    assert_eq!(recorded.links[1].context, sent);
    assert_eq!(recorded.links[1].attributes, attributes! {});
}

#[tokio::test]
async fn uses_the_root_for_absent_and_unknown_carriers() {
    let telemetry = InMemoryTelemetryContext::new();
    let unknown = carrier(&[("unknown", "format")]);
    for carrier in [None, Some(TelemetryCarrier::new()), Some(unknown.clone())] {
        assert!(same_ref(&telemetry.extract(carrier.as_ref()), &telemetry));
    }
    let noop = noop_telemetry_context();
    assert_eq!(noop_telemetry_propagation().inject(noop.as_ref()), None);
    assert!(same_ref(
        &noop_telemetry_propagation().extract(Some(&unknown)),
        &noop
    ));
    noop.start_span("noop", |span| async move {
        span.add_link(SpanLink::new(span.clone()))
    })
    .await;
}

#[tokio::test]
async fn treats_recognized_carrier_identifiers_as_opaque_strings() {
    let telemetry = InMemoryTelemetryContext::new();
    let external = carrier(&[
        ("pi-memory-scope", "external-recorder"),
        ("pi-memory-span", "external-span"),
        ("pi-memory-trace", "external-trace"),
    ]);
    telemetry
        .extract(Some(&external))
        .start_span("receiver", |_| async {})
        .await;
    assert_eq!(
        telemetry.get_spans()[0].parent_context.as_ref(),
        Some(&external)
    );
}
