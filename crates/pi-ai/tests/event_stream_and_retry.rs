//! Ports of `event-stream.test.ts` and `provider-retry.test.ts`.

use std::sync::Arc;
use std::time::{Duration, Instant};

use parking_lot::Mutex;
use pi_ai::utils::error_body::ProviderHttpError;
use pi_ai::utils::provider_retry::{AbortError, ProviderRetryOptions, retry_provider_request};
use pi_ai::*;
use reqwest::header::{HeaderMap, HeaderName, HeaderValue};

fn message(timestamp: i64, duration: Option<u64>) -> AssistantMessage {
    AssistantMessage { timestamp, duration_ms: duration, ..Default::default() }
}

#[tokio::test]
async fn drains_in_order_and_ignores_after_completion() {
    let (tx, mut rx) = AssistantMessageEventStream::new();
    let m = message(utils::now_ms(), None);
    tx.push(AssistantMessageEvent::Start { partial: m.clone() });
    tx.push(AssistantMessageEvent::TextStart { content_index: 0, partial: m.clone() });
    tx.push(AssistantMessageEvent::Done { reason: StopReason::Stop, message: m.clone() });
    tx.push(AssistantMessageEvent::TextStart { content_index: 1, partial: m.clone() });
    let mut types = vec![];
    while let Some(e) = rx.next_event().await {
        types.push(e.type_name());
    }
    assert_eq!(types, vec!["start", "text_start", "done"]);
}

#[tokio::test]
async fn end_without_result_wakes_consumers_and_explicit_result_resolves() {
    let (tx, mut rx) = AssistantMessageEventStream::new();
    let waiter = tokio::spawn(async move { rx.next_event().await.is_none() });
    tokio::time::sleep(Duration::from_millis(5)).await;
    tx.end(None);
    assert!(waiter.await.unwrap());

    let (tx, rx) = AssistantMessageEventStream::new();
    let m = message(utils::now_ms(), None);
    tx.push(AssistantMessageEvent::Start { partial: m.clone() });
    tx.end(Some(AssistantMessage { response_id: Some("explicit".into()), ..m }));
    let (events, result) = rx.collect_all().await;
    assert_eq!(events.len(), 1);
    assert_eq!(result.response_id.as_deref(), Some("explicit"));
}

#[tokio::test]
async fn generic_event_stream() {
    let (tx, mut rx) = EventStream::<u32, u32>::new(|e| *e >= 10, |e| *e * 2);
    tx.push(1);
    tx.push(10);
    tx.push(3);
    assert_eq!(rx.next_event().await, Some(1));
    assert_eq!(rx.next_event().await, Some(10));
    assert_eq!(rx.next_event().await, None);
    assert_eq!(rx.try_result().await, Some(20));
}

#[tokio::test]
async fn times_final_messages() {
    let (tx, rx) = AssistantMessageEventStream::new();
    let answer = message(utils::now_ms(), None);
    tokio::time::sleep(Duration::from_millis(20)).await;
    tx.push(AssistantMessageEvent::Done { reason: StopReason::Stop, message: answer });
    let result = rx.result().await;
    assert!(result.duration_ms.unwrap() >= 15);

    let (tx, rx) = AssistantMessageEventStream::new();
    tx.push(AssistantMessageEvent::Done { reason: StopReason::Stop, message: message(utils::now_ms(), Some(1234)) });
    assert_eq!(rx.result().await.duration_ms, Some(1234));

    let (tx, rx) = AssistantMessageEventStream::new();
    tx.push(AssistantMessageEvent::Done { reason: StopReason::Stop, message: message(utils::now_ms() - 60_000, None) });
    assert_eq!(rx.result().await.duration_ms, None);

    let (tx, rx) = AssistantMessageEventStream::new();
    tx.end(Some(message(utils::now_ms(), None)));
    assert!(rx.result().await.duration_ms.is_some());
}

fn provider_error(status: u16, headers: &[(&str, &str)]) -> anyhow::Error {
    let mut map = HeaderMap::new();
    for (k, v) in headers {
        map.insert(HeaderName::from_bytes(k.as_bytes()).unwrap(), HeaderValue::from_str(v).unwrap());
    }
    let mut error = ProviderHttpError::from_response(status, map, String::new(), pi_ai::utils::error_body::SdkErrorStyle::OpenAI);
    error.message = format!("Provider error: {status}");
    error.into()
}

#[tokio::test]
async fn retries_retryable_provider_errors() {
    let calls = Arc::new(Mutex::new(0));
    let counter = calls.clone();
    let started = Instant::now();
    let result = retry_provider_request(
        move || {
            let counter = counter.clone();
            async move {
                let n = {
                    let mut c = counter.lock();
                    *c += 1;
                    *c
                };
                if n == 1 { Err(provider_error(429, &[("retry-after-ms", "50")])) } else { Ok("ok") }
            }
        },
        &ProviderRetryOptions { max_retries: Some(1), ..Default::default() },
    )
    .await
    .unwrap();
    assert_eq!(result, "ok");
    assert_eq!(*calls.lock(), 2);
    assert!(started.elapsed() >= Duration::from_millis(45));
}

#[tokio::test]
async fn non_retryable_and_capped_delays() {
    let calls = Arc::new(Mutex::new(0));
    let counter = calls.clone();
    let error = retry_provider_request::<(), _, _>(
        move || {
            *counter.lock() += 1;
            async { Err(provider_error(429, &[("x-should-retry", "false")])) }
        },
        &ProviderRetryOptions { max_retries: Some(2), ..Default::default() },
    )
    .await
    .unwrap_err();
    assert_eq!(error.to_string(), "Provider error: 429");
    assert_eq!(*calls.lock(), 1);

    let error = retry_provider_request::<(), _, _>(
        || async { Err(provider_error(504, &[("retry-after-ms", "0")])) },
        &ProviderRetryOptions { max_retries: Some(2), no_retry_statuses: vec![504], ..Default::default() },
    )
    .await
    .unwrap_err();
    assert_eq!(error.to_string(), "Provider error: 504");

    let error = retry_provider_request::<(), _, _>(
        || async { Err(provider_error(429, &[("retry-after", "277403")])) },
        &ProviderRetryOptions { max_retries: Some(1), max_retry_delay_ms: Some(1000), ..Default::default() },
    )
    .await
    .unwrap_err();
    assert!(error.to_string().starts_with("Server requested 277403s retry delay (max: 1s)"), "{error}");
}

#[tokio::test]
async fn aborts_a_provider_requested_delay() {
    let signal = tokio_util::sync::CancellationToken::new();
    let cancel = signal.clone();
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(20)).await;
        cancel.cancel();
    });
    let error = retry_provider_request::<(), _, _>(
        || async { Err(provider_error(429, &[("retry-after", "277403")])) },
        &ProviderRetryOptions { max_retries: Some(2), max_retry_delay_ms: Some(0), signal: Some(signal), ..Default::default() },
    )
    .await
    .unwrap_err();
    assert!(error.downcast_ref::<AbortError>().is_some());
}

#[test]
fn text_helpers() {
    let blocks = vec![
        AssistantContent::Text(TextContent::new("a")),
        AssistantContent::Thinking(ThinkingContent::new("t")),
        AssistantContent::Text(TextContent::new("b")),
    ];
    assert_eq!(assistant_content_text(&blocks, "\n"), "a\nb");
    assert_eq!(assistant_content_text(&blocks, " | "), "a | b");
    assert_eq!(content_text(&UserContent::Text("plain".into()), "\n"), "plain");
    let tool = vec![UserContentBlock::text("x"), UserContentBlock::Image(ImageContent::default()), UserContentBlock::text("y")];
    assert_eq!(blocks_text(&tool, "\n"), "x\ny");
}
