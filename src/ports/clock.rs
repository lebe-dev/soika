//! Clock port — injectable time source for testability.

use crate::domain::Timestamp;

/// Provides the current time. Adapters supply a real (UTC) clock; tests can
/// supply a fixed one.
pub trait Clock: Send + Sync {
    /// Current instant in UTC.
    fn now(&self) -> Timestamp;
}
