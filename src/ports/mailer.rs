//! Mailer port (MVP §12) — outbound email behind an abstraction so SMTP can be
//! absent (email degrades to UI-only).

use crate::error::Result;
use async_trait::async_trait;

/// A composed outbound email message.
#[derive(Debug, Clone)]
pub struct OutboundEmail {
    pub to: String,
    pub subject: String,
    /// Plain-text body.
    pub body: String,
}

/// Sends outbound email. The no-op implementation is used when SMTP is unset.
#[async_trait]
pub trait Mailer: Send + Sync {
    /// Whether email delivery is actually available (SMTP configured).
    fn is_enabled(&self) -> bool;

    /// Send a single message. May be a no-op when disabled.
    async fn send(&self, email: OutboundEmail) -> Result<()>;
}
