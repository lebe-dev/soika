//! Brute-force protection for password logins.
//!
//! A per-(client IP, email) counter guards every credential check. Failures
//! accumulate over a rolling window; once they cross the configured threshold
//! the key is locked for an exponentially growing duration (so a sustained
//! attack backs off ever harder), capped at a maximum. A successful login
//! clears the key entirely.
//!
//! Like [`crate::ingest::RateLimiter`], the guard is in-memory and lives on
//! [`crate::AppState`] (built in `build_state`) so it is per-instance and
//! injectable in tests. State is keyed off a monotonic [`Instant`] so wall-clock
//! changes can't shorten a lockout. The guard is best-effort and single-process:
//! it is not shared across replicas and is reset on restart.

use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use axum::http::HeaderMap;

use crate::config::LockoutConfig;

/// Above this many tracked keys, opportunistically drop idle entries on write to
/// keep the map bounded under a wide spray of distinct (IP, email) pairs.
const PRUNE_THRESHOLD: usize = 10_000;

/// Outcome of a pre-login lockout check.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LockoutDecision {
    /// The credential check may proceed.
    Allowed,
    /// The key is locked; reject and suggest retrying after this many seconds.
    Locked { retry_after_secs: u64 },
}

impl LockoutDecision {
    /// True when the request should be rejected with `429`.
    pub fn is_locked(self) -> bool {
        matches!(self, LockoutDecision::Locked { .. })
    }
}

/// Per-key tracking state.
#[derive(Debug, Clone, Copy)]
struct Entry {
    /// Failures accrued since `window_start` that have not yet tripped a lockout.
    failures: u32,
    /// Start of the current rolling failure-counting window.
    window_start: Instant,
    /// When the key unlocks, if currently locked.
    locked_until: Option<Instant>,
    /// Number of lockouts triggered for this key; drives exponential backoff.
    /// Reset only on a successful login.
    level: u32,
}

impl Entry {
    fn new(now: Instant) -> Self {
        Entry {
            failures: 0,
            window_start: now,
            locked_until: None,
            level: 0,
        }
    }

    /// True when the entry carries no useful state and can be dropped: not
    /// locked (or the lock has elapsed) and its failure window has expired.
    fn is_idle(&self, now: Instant, window: Duration) -> bool {
        let unlocked = self.locked_until.is_none_or(|until| now >= until);
        unlocked && now.duration_since(self.window_start) >= window
    }
}

/// In-memory brute-force guard shared across login attempts.
#[derive(Debug)]
pub struct LoginGuard {
    config: LockoutConfig,
    entries: Mutex<HashMap<String, Entry>>,
}

impl LoginGuard {
    /// Construct a guard from the resolved configuration.
    pub fn new(config: LockoutConfig) -> Self {
        Self {
            config,
            entries: Mutex::new(HashMap::new()),
        }
    }

    /// Check whether `key` is currently locked, using the real monotonic clock.
    pub fn check(&self, key: &str) -> LockoutDecision {
        self.check_at(key, Instant::now())
    }

    /// Record a failed credential check for `key`, using the real clock.
    pub fn record_failure(&self, key: &str) {
        self.record_failure_at(key, Instant::now());
    }

    /// Clear all state for `key` after a successful login.
    pub fn record_success(&self, key: &str) {
        if !self.config.enabled {
            return;
        }
        self.entries
            .lock()
            .expect("login guard mutex poisoned")
            .remove(key);
    }

    /// Check `key` against an explicit instant (testable form of [`check`]).
    pub fn check_at(&self, key: &str, now: Instant) -> LockoutDecision {
        if !self.config.enabled {
            return LockoutDecision::Allowed;
        }
        let entries = self.entries.lock().expect("login guard mutex poisoned");
        match entries.get(key).and_then(|e| e.locked_until) {
            Some(until) if now < until => LockoutDecision::Locked {
                retry_after_secs: secs_until(now, until),
            },
            _ => LockoutDecision::Allowed,
        }
    }

    /// Record a failure against an explicit instant (testable form of
    /// [`record_failure`]). Trips a lockout once failures reach the configured
    /// threshold.
    pub fn record_failure_at(&self, key: &str, now: Instant) {
        if !self.config.enabled {
            return;
        }
        let mut entries = self.entries.lock().expect("login guard mutex poisoned");

        if entries.len() >= PRUNE_THRESHOLD {
            let window = self.config.window;
            entries.retain(|_, e| !e.is_idle(now, window));
        }

        let entry = entries
            .entry(key.to_owned())
            .or_insert_with(|| Entry::new(now));

        // Roll the failure-counting window forward when it has elapsed (and we
        // are not mid-lockout), so sporadic typos don't accumulate forever.
        let lock_active = entry.locked_until.is_some_and(|until| now < until);
        if !lock_active && now.duration_since(entry.window_start) >= self.config.window {
            entry.failures = 0;
            entry.window_start = now;
        }

        entry.failures += 1;
        if entry.failures < self.config.max_attempts {
            return;
        }

        // Threshold reached: escalate and (re)arm the lockout. `level` only ever
        // grows here and is reset by `record_success`, so repeated attack series
        // back off exponentially up to the cap.
        entry.level += 1;
        entry.locked_until = Some(now + self.lockout_duration(entry.level));
        entry.failures = 0;
        entry.window_start = now;
    }

    /// Exponential lockout duration for the given level: `base * 2^(level-1)`,
    /// saturating at `max_lockout`.
    fn lockout_duration(&self, level: u32) -> Duration {
        let base = self.config.base_lockout;
        // 2^(level-1), saturating so a high level can't overflow the shift.
        let shift = level.saturating_sub(1);
        let factor = if shift >= 31 { u32::MAX } else { 1u32 << shift };
        // checked_mul saturates the duration to the cap on overflow.
        base.checked_mul(factor)
            .unwrap_or(self.config.max_lockout)
            .min(self.config.max_lockout)
    }
}

/// Round the gap between `now` and `until` up to whole seconds, never below 1 so
/// clients honor a non-zero `Retry-After`.
fn secs_until(now: Instant, until: Instant) -> u64 {
    let remaining = until.saturating_duration_since(now);
    let ceil = if remaining.subsec_nanos() > 0 {
        remaining.as_secs() + 1
    } else {
        remaining.as_secs()
    };
    ceil.max(1)
}

/// Build the lockout map key from a client IP and a normalized email.
pub fn lockout_key(ip: &str, email: &str) -> String {
    format!("{ip}|{email}")
}

/// Best-effort client IP for rate-keying.
///
/// Honors the first hop in `X-Forwarded-For`, then `X-Real-IP`, falling back to
/// the TCP peer address. These headers are trusted only when the service runs
/// behind a reverse proxy that sets them; direct deployments fall through to the
/// socket peer. Returns `"unknown"` when no source is available so keying still
/// works (all such attempts then share one bucket).
pub fn client_ip(headers: &HeaderMap, peer: Option<SocketAddr>) -> String {
    if let Some(ip) = headers
        .get("x-forwarded-for")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.split(',').next())
        .map(str::trim)
        .filter(|v| !v.is_empty())
    {
        return ip.to_string();
    }

    if let Some(ip) = headers
        .get("x-real-ip")
        .and_then(|v| v.to_str().ok())
        .map(str::trim)
        .filter(|v| !v.is_empty())
    {
        return ip.to_string();
    }

    peer.map(|p| p.ip().to_string())
        .unwrap_or_else(|| "unknown".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::HeaderValue;

    fn cfg(max_attempts: u32) -> LockoutConfig {
        LockoutConfig {
            enabled: true,
            max_attempts,
            window: Duration::from_secs(900),
            base_lockout: Duration::from_secs(60),
            max_lockout: Duration::from_secs(3600),
        }
    }

    #[test]
    fn allows_until_threshold_then_locks() {
        let guard = LoginGuard::new(cfg(3));
        let t0 = Instant::now();

        assert_eq!(guard.check_at("k", t0), LockoutDecision::Allowed);
        guard.record_failure_at("k", t0);
        guard.record_failure_at("k", t0);
        // Still under the limit (2 < 3).
        assert_eq!(guard.check_at("k", t0), LockoutDecision::Allowed);

        // Third failure trips the lockout.
        guard.record_failure_at("k", t0);
        assert!(guard.check_at("k", t0).is_locked());
    }

    #[test]
    fn lock_expires_after_base_duration() {
        let guard = LoginGuard::new(cfg(1));
        let t0 = Instant::now();

        guard.record_failure_at("k", t0);
        assert!(guard.check_at("k", t0).is_locked());

        // Just before expiry: still locked.
        let almost = t0 + Duration::from_secs(59);
        assert!(guard.check_at("k", almost).is_locked());

        // Past the base lockout (60s): unlocked.
        let after = t0 + Duration::from_secs(61);
        assert_eq!(guard.check_at("k", after), LockoutDecision::Allowed);
    }

    #[test]
    fn repeated_lockouts_grow_exponentially() {
        let guard = LoginGuard::new(cfg(1));
        let mut now = Instant::now();

        // First lockout: base (60s).
        guard.record_failure_at("k", now);
        match guard.check_at("k", now) {
            LockoutDecision::Locked { retry_after_secs } => assert_eq!(retry_after_secs, 60),
            other => panic!("expected lock, got {other:?}"),
        }

        // After it expires, the next failure locks for base * 2 (120s).
        now += Duration::from_secs(61);
        guard.record_failure_at("k", now);
        match guard.check_at("k", now) {
            LockoutDecision::Locked { retry_after_secs } => assert_eq!(retry_after_secs, 120),
            other => panic!("expected lock, got {other:?}"),
        }

        // And again: base * 4 (240s).
        now += Duration::from_secs(121);
        guard.record_failure_at("k", now);
        match guard.check_at("k", now) {
            LockoutDecision::Locked { retry_after_secs } => assert_eq!(retry_after_secs, 240),
            other => panic!("expected lock, got {other:?}"),
        }
    }

    #[test]
    fn lockout_duration_caps_at_max() {
        let guard = LoginGuard::new(cfg(1));
        // A very high level would overflow base * 2^(level-1); it must saturate
        // at max_lockout (3600s) rather than panic or wrap.
        assert_eq!(guard.lockout_duration(50), Duration::from_secs(3600));
    }

    #[test]
    fn success_resets_escalation() {
        let guard = LoginGuard::new(cfg(1));
        let t0 = Instant::now();

        guard.record_failure_at("k", t0);
        assert!(guard.check_at("k", t0).is_locked());

        guard.record_success("k");
        assert_eq!(guard.check_at("k", t0), LockoutDecision::Allowed);

        // Next failure starts back at base, proving level was reset.
        guard.record_failure_at("k", t0);
        match guard.check_at("k", t0) {
            LockoutDecision::Locked { retry_after_secs } => assert_eq!(retry_after_secs, 60),
            other => panic!("expected lock, got {other:?}"),
        }
    }

    #[test]
    fn failures_outside_window_do_not_accumulate() {
        let guard = LoginGuard::new(cfg(3));
        let t0 = Instant::now();

        guard.record_failure_at("k", t0);
        guard.record_failure_at("k", t0);
        // Third failure lands after the window — counter resets, no lockout.
        let later = t0 + Duration::from_secs(901);
        guard.record_failure_at("k", later);
        assert_eq!(guard.check_at("k", later), LockoutDecision::Allowed);
    }

    #[test]
    fn keys_are_independent() {
        let guard = LoginGuard::new(cfg(1));
        let t0 = Instant::now();

        guard.record_failure_at("a", t0);
        assert!(guard.check_at("a", t0).is_locked());
        // A different key keeps its full budget.
        assert_eq!(guard.check_at("b", t0), LockoutDecision::Allowed);
    }

    #[test]
    fn disabled_guard_never_locks() {
        let mut config = cfg(1);
        config.enabled = false;
        let guard = LoginGuard::new(config);
        let t0 = Instant::now();

        guard.record_failure_at("k", t0);
        guard.record_failure_at("k", t0);
        assert_eq!(guard.check_at("k", t0), LockoutDecision::Allowed);
    }

    #[test]
    fn client_ip_prefers_forwarded_for_first_hop() {
        let mut headers = HeaderMap::new();
        headers.insert(
            "x-forwarded-for",
            HeaderValue::from_static("203.0.113.7, 10.0.0.1"),
        );
        let peer = Some("198.51.100.2:443".parse().unwrap());
        assert_eq!(client_ip(&headers, peer), "203.0.113.7");
    }

    #[test]
    fn client_ip_falls_back_to_real_ip_then_peer() {
        let mut headers = HeaderMap::new();
        headers.insert("x-real-ip", HeaderValue::from_static("203.0.113.9"));
        assert_eq!(client_ip(&headers, None), "203.0.113.9");

        let peer = Some("198.51.100.2:443".parse().unwrap());
        assert_eq!(client_ip(&HeaderMap::new(), peer), "198.51.100.2");
    }

    #[test]
    fn client_ip_unknown_when_no_source() {
        assert_eq!(client_ip(&HeaderMap::new(), None), "unknown");
    }
}
