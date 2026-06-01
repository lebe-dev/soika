//! System clock adapter implementing the [`Clock`] port.

use crate::domain::Timestamp;
use crate::ports::Clock;

/// Real wall-clock time source (UTC).
#[derive(Debug, Clone, Default)]
pub struct SystemClock;

impl Clock for SystemClock {
    fn now(&self) -> Timestamp {
        chrono::Utc::now()
    }
}
