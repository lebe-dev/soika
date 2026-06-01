//! Error grouping / fingerprinting (MVP §7) and the ingest-pipeline tail (§5.4).
//!
//! Default fingerprint derives from exception type + a normalized in-app
//! stacktrace (function/module/path normalized), mirroring Sentry's default
//! grouping. A custom `fingerprint` from the SDK overrides the default.
//!
//! The pipeline tail (upsert issue → insert event → counters → notify check)
//! lives in [`pipeline`] and is written against the repository TRAITS so it can
//! be unit-tested with fakes (hexagonal architecture, MVP §2.2).

pub mod normalize;
pub mod pipeline;

use serde_json::Value;

use crate::domain::stacktrace::{Frame, NormalizedEvent, Stacktrace};

pub use pipeline::{ingest_normalized, IngestInput, IngestOutcome, NotifyKind};

/// The literal Sentry placeholder that expands to the default fingerprint when
/// it appears inside a custom `fingerprint` array.
const DEFAULT_PLACEHOLDER: &str = "{{ default }}";
/// Alternate spelling SDKs sometimes emit (no surrounding spaces).
const DEFAULT_PLACEHOLDER_TIGHT: &str = "{{default}}";

/// Compute the grouping fingerprint for a raw Sentry event payload.
///
/// Honors a custom `fingerprint` field if present, otherwise derives the
/// default from exception type + normalized in-app stacktrace frames (§7).
pub fn fingerprint(event: &Value) -> String {
    let normalized = NormalizedEvent::from_value(event);
    fingerprint_normalized(&normalized)
}

/// [`fingerprint`] for an already-parsed [`NormalizedEvent`] (avoids re-parsing
/// in the pipeline, which has the normalized event in hand).
pub fn fingerprint_normalized(event: &NormalizedEvent) -> String {
    if event.has_custom_fingerprint() {
        return custom_fingerprint(event);
    }
    default_fingerprint(event)
}

/// Resolve a custom SDK fingerprint, expanding any `{{ default }}` placeholder
/// to the default fingerprint MATERIAL (not its hash) and joining stably.
///
/// Expanding to the material — then hashing once — means a fingerprint of
/// exactly `["{{ default }}"]` reproduces the default fingerprint precisely,
/// mirroring Sentry's behaviour.
fn custom_fingerprint(event: &NormalizedEvent) -> String {
    let default_material = default_material(event);

    let parts: Vec<String> = event
        .fingerprint
        .iter()
        .map(|component| {
            let trimmed = component.trim();
            if trimmed == DEFAULT_PLACEHOLDER || trimmed == DEFAULT_PLACEHOLDER_TIGHT {
                return default_material.clone();
            }
            component.clone()
        })
        .collect();

    // A lone `{{ default }}` must equal the default fingerprint exactly.
    if parts.len() == 1 && parts[0] == default_material {
        return hash_hex(&default_material);
    }

    // Join on a control char so distinct component boundaries never collide.
    hash_hex(&parts.join("\u{1f}"))
}

/// Sentry-style default fingerprint: exception type + normalized in-app frames.
fn default_fingerprint(event: &NormalizedEvent) -> String {
    hash_hex(&default_material(event))
}

/// The canonical "material" string the default fingerprint is hashed from.
///
/// Exception type + normalized in-app frame signatures; when no usable
/// stacktrace/exception is available (e.g. a bare message event), falls back to
/// the message text so distinct messages stay distinct while identical messages
/// collapse.
fn default_material(event: &NormalizedEvent) -> String {
    let mut components: Vec<String> = Vec::new();

    if let Some(ty) = event.exception_type.as_deref().filter(|s| !s.is_empty()) {
        components.push(format!("type:{}", normalize::normalize_type(ty)));
    }

    if let Some(st) = event.stacktrace.as_ref() {
        let frame_sigs = grouping_frame_signatures(st);
        if !frame_sigs.is_empty() {
            components.extend(frame_sigs);
        }
    }

    // No exception type and no frames → group by message (message events) or
    // exception value as a last resort.
    if components.is_empty() {
        if let Some(msg) = event.message.as_deref().filter(|s| !s.is_empty()) {
            components.push(format!("message:{}", normalize::normalize_message(msg)));
        } else if let Some(val) = event.exception_value.as_deref().filter(|s| !s.is_empty()) {
            components.push(format!("value:{}", normalize::normalize_message(val)));
        }
    }

    components.join("\n")
}

/// Normalized signatures for the frames that participate in grouping.
///
/// Mirrors Sentry: prefer in-app frames; if none are flagged in-app, fall back
/// to all frames (so libraries-only traces still group). Frames with no usable
/// signature (no function/module/path) are skipped.
fn grouping_frame_signatures(stacktrace: &Stacktrace) -> Vec<String> {
    let in_app = stacktrace.in_app_frames();
    let frames: Vec<&Frame> = if in_app.is_empty() {
        stacktrace.frames.iter().collect()
    } else {
        in_app
    };

    frames
        .iter()
        .filter_map(|frame| normalize::frame_signature(frame))
        .map(|sig| format!("frame:{sig}"))
        .collect()
}

/// Derive a human-readable issue title from the event payload (§8 issues list).
///
/// Exception events → `Type: value`; message events → the message; otherwise a
/// generic placeholder.
pub fn issue_title(event: &Value) -> String {
    let normalized = NormalizedEvent::from_value(event);
    title_from_normalized(&normalized)
}

/// [`issue_title`] for an already-parsed [`NormalizedEvent`].
pub fn title_from_normalized(event: &NormalizedEvent) -> String {
    if let Some(ty) = event.exception_type.as_deref().filter(|s| !s.is_empty()) {
        return match event.exception_value.as_deref().filter(|s| !s.is_empty()) {
            Some(value) => format!("{ty}: {}", truncate_title(value)),
            None => ty.to_string(),
        };
    }
    if let Some(msg) = event.message.as_deref().filter(|s| !s.is_empty()) {
        return truncate_title(msg);
    }
    if let Some(val) = event.exception_value.as_deref().filter(|s| !s.is_empty()) {
        return truncate_title(val);
    }
    "Unknown error".to_string()
}

/// Derive the culprit (the most relevant in-app frame location) (§6, §8).
pub fn culprit(event: &Value) -> Option<String> {
    let normalized = NormalizedEvent::from_value(event);
    culprit_from_normalized(&normalized)
}

/// [`culprit`] for an already-parsed [`NormalizedEvent`].
pub fn culprit_from_normalized(event: &NormalizedEvent) -> Option<String> {
    event
        .stacktrace
        .as_ref()
        .and_then(|st| st.most_relevant_frame())
        .and_then(Frame::location)
}

/// Truncate long titles to keep them list-friendly (on a char boundary).
fn truncate_title(s: &str) -> String {
    const MAX: usize = 200;
    let s = s.trim();
    if s.chars().count() <= MAX {
        return s.to_string();
    }
    let truncated: String = s.chars().take(MAX).collect();
    format!("{truncated}…")
}

/// Stable 64-bit FNV-1a hash, rendered as zero-padded hex.
///
/// A hand-rolled FNV (rather than `DefaultHasher`) keeps fingerprints
/// deterministic and portable across builds/platforms — important because they
/// are persisted and compared over time.
fn hash_hex(input: &str) -> String {
    const FNV_OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
    const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;

    let mut hash = FNV_OFFSET;
    for byte in input.as_bytes() {
        hash ^= *byte as u64;
        hash = hash.wrapping_mul(FNV_PRIME);
    }
    format!("{hash:016x}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn exception_event(ty: &str, frames: serde_json::Value) -> serde_json::Value {
        json!({
            "exception": { "values": [{
                "type": ty,
                "value": "boom",
                "stacktrace": { "frames": frames }
            }]}
        })
    }

    #[test]
    fn fingerprint_is_stable_across_calls() {
        let frames = json!([
            { "function": "handle", "module": "app.web", "lineno": 12, "in_app": true },
            { "function": "query", "module": "app.db", "lineno": 88, "in_app": true }
        ]);
        let event = exception_event("DatabaseError", frames);
        assert_eq!(fingerprint(&event), fingerprint(&event));
    }

    #[test]
    fn fingerprint_ignores_line_numbers_and_columns() {
        let a = exception_event(
            "ValueError",
            json!([{ "function": "f", "module": "m", "lineno": 10, "colno": 4, "in_app": true }]),
        );
        let b = exception_event(
            "ValueError",
            json!([{ "function": "f", "module": "m", "lineno": 999, "colno": 1, "in_app": true }]),
        );
        assert_eq!(
            fingerprint(&a),
            fingerprint(&b),
            "line/column drift must not change grouping"
        );
    }

    #[test]
    fn fingerprint_differs_by_exception_type() {
        let frames = json!([{ "function": "f", "module": "m", "lineno": 1, "in_app": true }]);
        let a = exception_event("ValueError", frames.clone());
        let b = exception_event("KeyError", frames);
        assert_ne!(fingerprint(&a), fingerprint(&b));
    }

    #[test]
    fn fingerprint_differs_by_frame_function() {
        let a = exception_event(
            "E",
            json!([{ "function": "alpha", "module": "m", "in_app": true }]),
        );
        let b = exception_event(
            "E",
            json!([{ "function": "beta", "module": "m", "in_app": true }]),
        );
        assert_ne!(fingerprint(&a), fingerprint(&b));
    }

    #[test]
    fn fingerprint_ignores_system_frames() {
        // Two events whose only difference is a non-in-app (system) frame must
        // group together, because system frames are excluded from grouping.
        let a = exception_event(
            "E",
            json!([
                { "function": "app_fn", "module": "app", "in_app": true },
                { "function": "lib_a", "module": "std", "in_app": false }
            ]),
        );
        let b = exception_event(
            "E",
            json!([
                { "function": "app_fn", "module": "app", "in_app": true },
                { "function": "lib_b", "module": "std", "in_app": false }
            ]),
        );
        assert_eq!(fingerprint(&a), fingerprint(&b));
    }

    #[test]
    fn custom_fingerprint_overrides_default() {
        let mut event = exception_event(
            "ValueError",
            json!([{ "function": "f", "module": "m", "in_app": true }]),
        );
        let default = fingerprint(&event);

        event["fingerprint"] = json!(["custom-group-key"]);
        let custom = fingerprint(&event);
        assert_ne!(default, custom);

        // Same custom key on a totally different stacktrace ⇒ same group.
        let other = json!({
            "fingerprint": ["custom-group-key"],
            "exception": { "values": [{ "type": "Other" }] }
        });
        assert_eq!(custom, fingerprint(&other));
    }

    #[test]
    fn custom_fingerprint_default_placeholder_expands() {
        let event = json!({
            "fingerprint": ["{{ default }}"],
            "exception": { "values": [{
                "type": "ValueError",
                "stacktrace": { "frames": [{ "function": "f", "in_app": true }] }
            }]}
        });
        let bare = json!({
            "exception": { "values": [{
                "type": "ValueError",
                "stacktrace": { "frames": [{ "function": "f", "in_app": true }] }
            }]}
        });
        // `["{{ default }}"]` alone should reproduce the default fingerprint.
        assert_eq!(fingerprint(&event), fingerprint(&bare));
    }

    #[test]
    fn message_events_group_by_message() {
        let a = json!({ "message": "disk full" });
        let b = json!({ "message": "disk full" });
        let c = json!({ "message": "out of memory" });
        assert_eq!(fingerprint(&a), fingerprint(&b));
        assert_ne!(fingerprint(&a), fingerprint(&c));
    }

    #[test]
    fn title_for_exception() {
        let event = json!({
            "exception": { "values": [{ "type": "ValueError", "value": "bad input" }]}
        });
        assert_eq!(issue_title(&event), "ValueError: bad input");
    }

    #[test]
    fn title_for_message() {
        let event = json!({ "message": "queue backed up" });
        assert_eq!(issue_title(&event), "queue backed up");
    }

    #[test]
    fn culprit_is_most_relevant_in_app_frame() {
        let event = exception_event(
            "E",
            json!([
                { "function": "outer", "module": "app", "in_app": true },
                { "function": "inner", "module": "app.detail", "in_app": true },
                { "function": "panic", "module": "runtime", "in_app": false }
            ]),
        );
        assert_eq!(culprit(&event).as_deref(), Some("app.detail in inner"));
    }

    #[test]
    fn hash_hex_is_deterministic_and_fixed_width() {
        assert_eq!(hash_hex("abc"), hash_hex("abc"));
        assert_eq!(hash_hex("abc").len(), 16);
        assert_ne!(hash_hex("abc"), hash_hex("abd"));
    }
}
