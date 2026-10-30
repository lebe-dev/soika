//! Bounded HTTP transport for the Alertmanager v2 API.
use crate::{Error, Result, domain::notifications::AlertmanagerAlert, ports::AlertmanagerClient};
use async_trait::async_trait;
use std::time::Duration;

fn cause_detail(error: &(dyn std::error::Error + 'static)) -> Option<&'static str> {
    let mut cause = Some(error);
    while let Some(err) = cause {
        if let Some(io) = err.downcast_ref::<std::io::Error>() {
            match io.kind() {
                std::io::ErrorKind::ConnectionRefused => return Some("connection refused"),
                std::io::ErrorKind::ConnectionReset => return Some("connection reset"),
                std::io::ErrorKind::TimedOut => return Some("connection timed out"),
                std::io::ErrorKind::NetworkUnreachable => return Some("network unreachable"),
                std::io::ErrorKind::HostUnreachable => return Some("host unreachable"),
                _ => {}
            }
        }
        let message = err.to_string().to_ascii_lowercase();
        for (needle, detail) in [
            ("unknownissuer", "TLS certificate has an unknown issuer"),
            ("unknown issuer", "TLS certificate has an unknown issuer"),
            (
                "notvalidforname",
                "TLS certificate does not match the hostname",
            ),
            (
                "not valid for name",
                "TLS certificate does not match the hostname",
            ),
            ("expired", "TLS certificate expired"),
            ("notvalidyet", "TLS certificate is not valid yet"),
            (
                "invalid peer certificate",
                "TLS certificate validation failed",
            ),
            ("dns error", "DNS lookup failed"),
            ("failed to lookup address", "DNS lookup failed"),
            ("name or service not known", "DNS lookup failed"),
            ("nodename nor servname", "DNS lookup failed"),
        ] {
            if message.contains(needle) {
                return Some(detail);
            }
        }
        cause = err.source();
    }
    None
}

fn transport_error(error: reqwest::Error) -> Error {
    let detail = cause_detail(&error).unwrap_or_else(|| {
        if error.is_timeout() {
            "request timed out"
        } else if error.is_connect() {
            "connection failed"
        } else if error.is_body() {
            "request or response body failed"
        } else if error.is_decode() {
            "response decoding failed"
        } else {
            "HTTP request failed"
        }
    });
    Error::Alertmanager(format!("Alertmanager delivery failed: {detail}"))
}

pub struct HttpAlertmanager {
    client: reqwest::Client,
}
impl HttpAlertmanager {
    pub fn new() -> Result<Self> {
        Ok(Self {
            client: reqwest::Client::builder()
                .timeout(Duration::from_secs(5))
                .redirect(reqwest::redirect::Policy::none())
                .build()
                .map_err(|_| Error::Internal("building Alertmanager client failed".into()))?,
        })
    }
}
#[async_trait]
impl AlertmanagerClient for HttpAlertmanager {
    async fn send(&self, endpoint: &str, alert: &AlertmanagerAlert) -> Result<()> {
        let response = self
            .client
            .post(endpoint)
            .json(&[alert])
            .send()
            .await
            .map_err(transport_error)?;
        if !response.status().is_success() {
            return Err(Error::Alertmanager(format!(
                "Alertmanager returned HTTP {}",
                response.status().as_u16()
            )));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn diagnoses_safe_details_without_exposing_error_text() {
        let refused = std::io::Error::from(std::io::ErrorKind::ConnectionRefused);
        assert_eq!(cause_detail(&refused), Some("connection refused"));
        for (message, expected) in [
            (
                "invalid peer certificate: UnknownIssuer; secret-token",
                "TLS certificate has an unknown issuer",
            ),
            ("dns error: secret-token", "DNS lookup failed"),
            (
                "invalid peer certificate: secret-token",
                "TLS certificate validation failed",
            ),
        ] {
            let error = std::io::Error::other(message);
            assert_eq!(cause_detail(&error), Some(expected));
        }
    }

    #[tokio::test]
    async fn connection_failure_reports_reason_without_url() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        drop(listener);
        let alert = crate::alertmanager::render_alert(
            &crate::domain::notifications::AlertmanagerConfig::default(),
            &crate::domain::notifications::sample_variables(),
            chrono::Utc::now(),
        )
        .unwrap();
        let error = HttpAlertmanager::new()
            .unwrap()
            .send(&format!("http://{address}/secret-token"), &alert)
            .await
            .unwrap_err()
            .to_string();
        assert!(error.contains("connection refused"), "{error}");
        assert!(!error.contains("secret-token"), "{error}");
    }
}
