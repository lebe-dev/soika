//! Mailer adapters implementing the [`Mailer`] port (MVP §12).

use crate::config::SmtpConfig;
use crate::error::Result;
use crate::ports::{Mailer, OutboundEmail};
use async_trait::async_trait;

/// SMTP-backed mailer via `lettre`. Constructed only when SMTP is configured.
pub struct SmtpMailer {
    config: SmtpConfig,
}

impl SmtpMailer {
    pub fn new(config: SmtpConfig) -> Self {
        SmtpMailer { config }
    }
}

#[async_trait]
impl Mailer for SmtpMailer {
    fn is_enabled(&self) -> bool {
        true
    }

    async fn send(&self, email: OutboundEmail) -> Result<()> {
        crate::mail::send_via_smtp(&self.config, email).await
    }
}

/// No-op mailer used when SMTP is unset; email features degrade to UI-only.
#[derive(Debug, Clone, Default)]
pub struct NoopMailer;

#[async_trait]
impl Mailer for NoopMailer {
    fn is_enabled(&self) -> bool {
        false
    }

    async fn send(&self, _email: OutboundEmail) -> Result<()> {
        // Intentionally a no-op: instances without SMTP still function (§3, §9).
        Ok(())
    }
}
