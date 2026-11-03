//! Verify delivery errors at the Sentry transport boundary in an isolated process.
use soika::{adapters::alertmanager::HttpAlertmanager, ports::AlertmanagerClient};

#[derive(Default)]
struct Capture(std::sync::Mutex<Vec<sentry::Envelope>>);

impl sentry::Transport for Capture {
    fn send_envelope(&self, envelope: sentry::Envelope) {
        self.0.lock().unwrap().push(envelope);
    }
}

#[tokio::test]
async fn delivery_failures_reach_sentry_without_secrets() {
    use sentry::SentryFutureExt;
    use tracing::instrument::WithSubscriber;
    use tracing_subscriber::prelude::*;

    let capture = std::sync::Arc::new(Capture::default());
    let client = sentry::Client::from_config(sentry::ClientOptions {
        dsn: Some("https://key@sentry.invalid/1".parse().unwrap()),
        transport: Some(std::sync::Arc::new(capture.clone())),
        ..Default::default()
    });
    let hub = std::sync::Arc::new(sentry::Hub::new(
        Some(std::sync::Arc::new(client)),
        std::sync::Arc::new(sentry::Scope::default()),
    ));
    let subscriber = tracing_subscriber::registry().with(sentry::integrations::tracing::layer());
    async {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        drop(listener);
        let alert = soika::alertmanager::render_alert(
            &soika::domain::notifications::AlertmanagerConfig::default(),
            &soika::domain::notifications::sample_variables(),
            chrono::Utc::now(),
        ).unwrap();
        HttpAlertmanager::new(false).unwrap()
            .send(&format!("http://{address}/secret-token?key=secret-query"), &alert)
            .await.unwrap_err();

        // Exercise a real non-success HTTP response, including a sensitive body.
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            use tokio::io::{AsyncReadExt, AsyncWriteExt};
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut buffer = [0; 1];
            socket.read_exact(&mut buffer).await.unwrap();
            socket.write_all(b"HTTP/1.1 503 Service Unavailable\r\nContent-Length: 11\r\nConnection: close\r\n\r\nsecret-body").await.unwrap();
        });
        HttpAlertmanager::new(false).unwrap()
            .send(&format!("http://{address}/secret-token"), &alert)
            .await.unwrap_err();
        server.await.unwrap();
    }
    .with_subscriber(subscriber)
    .bind_hub(hub)
    .await;

    let envelopes = capture.0.lock().unwrap();
    assert_eq!(envelopes.len(), 2);
    let mut events = String::new();
    for envelope in envelopes.iter() {
        let mut bytes = Vec::new();
        envelope.to_writer(&mut bytes).unwrap();
        events.push_str(std::str::from_utf8(&bytes).unwrap());
    }
    assert!(events.contains("connection refused"), "{events}");
    assert!(events.contains("HTTP 503"), "{events}");
    assert!(!events.contains("secret-"), "{events}");
    assert!(!events.contains("127.0.0.1"), "{events}");
}
