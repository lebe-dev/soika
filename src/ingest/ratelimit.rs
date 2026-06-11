//! Soft per-project in-memory rate limiting.
//!
//! A simple fixed-window counter keyed by project id. When a project exceeds
//! its per-window budget the ingestion handler responds `429` with a
//! `Retry-After` hint so SDKs back off correctly. This is intentionally
//! minimal — full Sentry rate-limit rules are a roadmap item.
//!
//! The limiter lives on [`crate::AppState`] (built in `build_state`) so it is
//! per-instance rather than process-global — that keeps it configurable in
//! tests and ready to be driven from config later.

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

/// Default number of accepted events per project, per window.
pub const DEFAULT_LIMIT: u32 = 300;

/// Default window length.
pub const DEFAULT_WINDOW: Duration = Duration::from_secs(60);

/// Outcome of a rate-limit check.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Decision {
    /// The request is within budget.
    Allow,
    /// The request is over budget; suggest retrying after this many seconds.
    Limited { retry_after_secs: u64 },
}

impl Decision {
    /// True when the request should be rejected with `429`.
    pub fn is_limited(self) -> bool {
        matches!(self, Decision::Limited { .. })
    }
}

/// A fixed-window counter for a single project.
#[derive(Debug, Clone, Copy)]
struct Window {
    window_start: Instant,
    count: u32,
}

/// Fixed-window rate limiter shared across projects.
#[derive(Debug)]
pub struct RateLimiter {
    limit: u32,
    window: Duration,
    state: Mutex<HashMap<String, Window>>,
}

impl RateLimiter {
    /// Construct a limiter with an explicit budget and window.
    pub fn new(limit: u32, window: Duration) -> Self {
        Self {
            limit,
            window,
            state: Mutex::new(HashMap::new()),
        }
    }

    /// Check-and-count a request for `project_key`, using a monotonic `now`.
    ///
    /// Each call that is allowed consumes one unit of the project's budget.
    pub fn check_at(&self, project_key: &str, now: Instant) -> Decision {
        // Recover from a poisoned lock rather than panicking: under panic=abort
        // a panic on the hot path would take the process down. The critical
        // section is panic-free arithmetic, so poisoning just degrades to the
        // existing in-memory state.
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        let entry = state.entry(project_key.to_owned()).or_insert(Window {
            window_start: now,
            count: 0,
        });

        // Roll the window forward if it has elapsed.
        if now.duration_since(entry.window_start) >= self.window {
            entry.window_start = now;
            entry.count = 0;
        }

        if entry.count >= self.limit {
            let elapsed = now.duration_since(entry.window_start);
            let remaining = self.window.saturating_sub(elapsed);
            // At least 1s so SDKs honor a non-zero Retry-After.
            let retry_after_secs = remaining.as_secs().max(1);
            return Decision::Limited { retry_after_secs };
        }

        entry.count += 1;
        Decision::Allow
    }

    /// Convenience wrapper using the real monotonic clock.
    pub fn check(&self, project_key: &str) -> Decision {
        self.check_at(project_key, Instant::now())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn allows_up_to_limit_then_limits() {
        let rl = RateLimiter::new(3, Duration::from_secs(60));
        let t0 = Instant::now();
        assert_eq!(rl.check_at("p1", t0), Decision::Allow);
        assert_eq!(rl.check_at("p1", t0), Decision::Allow);
        assert_eq!(rl.check_at("p1", t0), Decision::Allow);
        let d = rl.check_at("p1", t0);
        assert!(d.is_limited());
    }

    #[test]
    fn projects_have_independent_budgets() {
        let rl = RateLimiter::new(1, Duration::from_secs(60));
        let t0 = Instant::now();
        assert_eq!(rl.check_at("a", t0), Decision::Allow);
        assert!(rl.check_at("a", t0).is_limited());
        // Different project still has its full budget.
        assert_eq!(rl.check_at("b", t0), Decision::Allow);
    }

    #[test]
    fn window_resets_after_elapsing() {
        let window = Duration::from_secs(60);
        let rl = RateLimiter::new(1, window);
        let t0 = Instant::now();
        assert_eq!(rl.check_at("p", t0), Decision::Allow);
        assert!(rl.check_at("p", t0).is_limited());
        // Advance past the window.
        let t1 = t0 + window + Duration::from_secs(1);
        assert_eq!(rl.check_at("p", t1), Decision::Allow);
    }

    #[test]
    fn retry_after_is_non_zero() {
        let rl = RateLimiter::new(1, Duration::from_secs(60));
        let t0 = Instant::now();
        let _ = rl.check_at("p", t0);
        match rl.check_at("p", t0) {
            Decision::Limited { retry_after_secs } => assert!(retry_after_secs >= 1),
            Decision::Allow => panic!("expected limited"),
        }
    }
}
