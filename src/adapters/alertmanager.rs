//! Bounded HTTP transport for the Alertmanager v2 API.
use crate::{Error, Result, domain::notifications::AlertmanagerAlert, ports::AlertmanagerClient};
use async_trait::async_trait;
use std::time::Duration;

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
            .map_err(|e| {
                Error::Internal(format!("Alertmanager delivery failed: {}", e.without_url()))
            })?;
        if !response.status().is_success() {
            return Err(Error::Internal(format!(
                "Alertmanager returned HTTP {}",
                response.status().as_u16()
            )));
        }
        Ok(())
    }
}
