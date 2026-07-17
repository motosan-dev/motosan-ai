#![cfg(feature = "openai")]

use motosan_ai::{Client, Message, MotosanError, Provider, RetryCause, RetryPolicy};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio_stream::StreamExt;

#[test]
fn builder_timeout_defaults_are_10s_120s_none() {
    let client = Client::builder()
        .provider(Provider::OpenAI)
        .api_key("k")
        .build()
        .expect("build client");
    assert_eq!(client.connect_timeout(), Duration::from_secs(10));
    assert_eq!(client.read_idle_timeout(), Duration::from_secs(120));
    assert_eq!(client.total_timeout(), None);
}

#[test]
fn builder_timeout_setters_override_defaults() {
    let client = Client::builder()
        .provider(Provider::OpenAI)
        .api_key("k")
        .connect_timeout(Duration::from_secs(3))
        .read_idle_timeout(Duration::from_secs(30))
        .total_timeout(Duration::from_secs(90))
        .build()
        .expect("build client");
    assert_eq!(client.connect_timeout(), Duration::from_secs(3));
    assert_eq!(client.read_idle_timeout(), Duration::from_secs(30));
    assert_eq!(client.total_timeout(), Some(Duration::from_secs(90)));
}

#[test]
fn stream_read_timeout_secs_is_an_alias_for_read_idle() {
    #[allow(deprecated)]
    let client = Client::builder()
        .provider(Provider::OpenAI)
        .api_key("k")
        .stream_read_timeout_secs(30)
        .build()
        .expect("build client");
    assert_eq!(client.read_idle_timeout(), Duration::from_secs(30));
}

#[tokio::test]
async fn hung_stream_yields_stream_read_timeout() {
    let mut server = mockito::Server::new_async().await;
    server
        .mock("POST", "/v1/chat/completions")
        .with_status(200)
        .with_header("content-type", "text/event-stream")
        .with_chunked_body(|w| {
            w.write_all(b"data: {\"choices\":[{\"delta\":{\"content\":\"hello world\"}}]}\n\n")?;
            w.flush()?;
            std::thread::sleep(std::time::Duration::from_millis(1500));
            Ok(())
        })
        .create_async()
        .await;

    let client = Client::builder()
        .provider(Provider::OpenAI)
        .api_key("k")
        .openai_chat_url(format!("{}/v1/chat/completions", server.url()))
        .read_idle_timeout(Duration::from_millis(200))
        .build()
        .expect("build client");

    let mut stream = client
        .stream(vec![Message::user("hi")])
        .await
        .expect("stream opens");
    let mut text = String::new();
    let mut saw_timeout = false;
    let mut saw_done = false;
    while let Some(item) = stream.next().await {
        match item {
            Ok(ev) => {
                saw_done |= ev.done;
                text.push_str(&ev.content);
            }
            Err(MotosanError::StreamReadTimeout(_)) => saw_timeout = true,
            Err(other) => panic!("expected StreamReadTimeout, got {other:?}"),
        }
    }
    assert!(
        saw_timeout,
        "idle stall must yield MotosanError::StreamReadTimeout"
    );
    assert!(
        !saw_done,
        "no fabricated terminal done after an idle timeout"
    );
    assert!(
        text.starts_with("hello"),
        "pre-stall content must be delivered, got {text:?}"
    );
}

#[tokio::test]
async fn total_timeout_does_not_apply_to_streams() {
    let mut server = mockito::Server::new_async().await;
    server
        .mock("POST", "/v1/chat/completions")
        .with_status(200)
        .with_header("content-type", "text/event-stream")
        .with_chunked_body(|w| {
            w.write_all(b"data: {\"choices\":[{\"delta\":{\"content\":\"hello\"}}]}\n\n")?;
            w.flush()?;
            std::thread::sleep(std::time::Duration::from_millis(400));
            w.write_all(b"data: [DONE]\n\n")?;
            w.flush()
        })
        .create_async()
        .await;

    let client = Client::builder()
        .provider(Provider::OpenAI)
        .api_key("k")
        .openai_chat_url(format!("{}/v1/chat/completions", server.url()))
        .total_timeout(Duration::from_millis(100))
        .build()
        .expect("build client");

    let mut stream = client
        .stream(vec![Message::user("hi")])
        .await
        .expect("stream opens");
    let mut events = Vec::new();
    while let Some(item) = stream.next().await {
        events.push(item.expect("stream must outlive total_timeout"));
    }
    assert_eq!(events.first().map(|e| e.content.as_str()), Some("hello"));
    assert!(
        events.last().is_some_and(|e| e.done),
        "terminal done must arrive"
    );
}

#[tokio::test]
async fn chat_total_timeout_maps_to_network_and_is_retried() {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind");
    let addr = listener.local_addr().expect("addr");
    let hits = Arc::new(AtomicUsize::new(0));
    let hits_srv = Arc::clone(&hits);
    std::thread::spawn(move || {
        let mut held = Vec::new();
        for conn in listener.incoming() {
            let Ok(socket) = conn else { break };
            hits_srv.fetch_add(1, Ordering::SeqCst);
            held.push(socket);
        }
    });

    let events: Arc<Mutex<Vec<motosan_ai::RetryEvent>>> = Arc::new(Mutex::new(Vec::new()));
    let sink = Arc::clone(&events);
    let mut policy = RetryPolicy::new()
        .max_retries(1)
        .base_delay_ms(0)
        .max_delay_ms(0)
        .jitter(false);
    policy.on_retry = Some(Arc::new(move |event| sink.lock().unwrap().push(event)));

    let client = Client::builder()
        .provider(Provider::OpenAI)
        .api_key("k")
        .openai_chat_url(format!("http://{addr}/v1/chat/completions"))
        .total_timeout(Duration::from_millis(200))
        .retry_policy(policy)
        .build()
        .expect("build client");

    let result = client.chat(vec![Message::user("hi")]).await;
    assert!(
        matches!(result, Err(MotosanError::Network(_))),
        "total-timeout expiry maps to Network, got {result:?}"
    );
    let recorded = std::mem::take(&mut *events.lock().unwrap());
    assert_eq!(
        recorded.len(),
        1,
        "is_timeout() is retryable -> exactly one retry"
    );
    assert!(matches!(recorded[0].cause, RetryCause::Network(_)));
    assert!(
        hits.load(Ordering::SeqCst) >= 2,
        "each attempt opens a fresh connection"
    );
}
