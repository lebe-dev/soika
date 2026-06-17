//! Mailer port — outbound email behind an abstraction so SMTP can be
//! absent (email degrades to UI-only).

use crate::error::Result;
use async_trait::async_trait;

/// A composed outbound email message.
///
/// `body` is the plain-text part and is always present. When `html_body` is
/// `Some`, delivery sends a `multipart/alternative` message so clients that
/// render HTML get the styled version while text-only clients fall back to
/// `body`.
#[derive(Debug, Clone)]
pub struct OutboundEmail {
    pub to: String,
    pub subject: String,
    /// Plain-text body (the `multipart/alternative` fallback).
    pub body: String,
    /// Optional HTML body. When set, the message is sent as
    /// `multipart/alternative` with `body` as the text fallback.
    pub html_body: Option<String>,
}

/// Sends outbound email. The no-op implementation is used when SMTP is unset.
#[async_trait]
pub trait Mailer: Send + Sync {
    /// Whether email delivery is actually available (SMTP configured).
    fn is_enabled(&self) -> bool;

    /// Send a single message. May be a no-op when disabled.
    async fn send(&self, email: OutboundEmail) -> Result<()>;
}
