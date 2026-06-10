//! Extract an event's tags into a flat `key -> value` map.
//!
//! Sentry payloads carry `tags` in two shapes the SDKs use interchangeably:
//!   * an object map: `{"environment": "staging", "server": "ci"}`
//!   * an array of pairs: `[["environment", "staging"], {"key": ..., "value": ...}]`
//!
//! Mirrors the frontend's `extractTags` (`frontend/src/lib/event-context.ts`).
//! Empty keys/values are dropped — they cannot meaningfully match a rule.

use serde_json::Value;
use std::collections::BTreeMap;

/// Parse `payload.tags` into a `key -> value` map (both shapes; empties dropped).
pub fn event_tags(payload: &Value) -> BTreeMap<String, String> {
    let mut out = BTreeMap::new();
    let Some(tags) = payload.get("tags") else {
        return out;
    };

    if let Some(map) = tags.as_object() {
        for (key, value) in map {
            insert_pair(&mut out, key.clone(), scalar_str(value));
        }
        return out;
    }

    if let Some(arr) = tags.as_array() {
        for entry in arr {
            // `["key", "value"]` pair form.
            if let Some(pair) = entry.as_array()
                && pair.len() == 2
            {
                insert_pair(&mut out, scalar_str(&pair[0]), scalar_str(&pair[1]));
                continue;
            }
            // `{"key": ..., "value": ...}` object form.
            if let Some(obj) = entry.as_object() {
                let key = obj.get("key").map(scalar_str).unwrap_or_default();
                let value = obj.get("value").map(scalar_str).unwrap_or_default();
                insert_pair(&mut out, key, value);
            }
        }
    }

    out
}

/// Insert a pair only when both sides are non-empty.
fn insert_pair(out: &mut BTreeMap<String, String>, key: String, value: String) {
    if !key.is_empty() && !value.is_empty() {
        out.insert(key, value);
    }
}

/// Render a scalar tag value as a string; non-scalars become empty.
fn scalar_str(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        Value::Number(n) => n.to_string(),
        Value::Bool(b) => b.to_string(),
        _ => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn object_shaped_tags() {
        let tags = event_tags(&json!({
            "tags": { "environment": "staging", "shard": 3, "empty": "" }
        }));
        assert_eq!(tags.get("environment").map(String::as_str), Some("staging"));
        assert_eq!(tags.get("shard").map(String::as_str), Some("3"));
        assert!(!tags.contains_key("empty"), "empty value dropped");
    }

    #[test]
    fn array_shaped_tags() {
        let tags = event_tags(&json!({
            "tags": [["runtime", "node"], { "key": "shard", "value": "3" }]
        }));
        assert_eq!(tags.get("runtime").map(String::as_str), Some("node"));
        assert_eq!(tags.get("shard").map(String::as_str), Some("3"));
    }

    #[test]
    fn missing_or_non_tag_payload_is_empty() {
        assert!(event_tags(&json!({})).is_empty());
        assert!(event_tags(&json!({ "tags": 42 })).is_empty());
        assert!(event_tags(&json!("nope")).is_empty());
    }
}
