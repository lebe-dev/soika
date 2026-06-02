//! Sentry **envelope** protocol parser.
//!
//! An envelope is a newline-delimited stream:
//!
//! ```text
//! {envelope header JSON}\n
//! {item header JSON}\n
//! {item payload}\n
//! {item header JSON}\n
//! {item payload}\n
//! ...
//! ```
//!
//! Each item header carries a `type` and optionally an explicit `length`. When
//! `length` is present the payload is exactly that many bytes (it may itself
//! contain newlines); otherwise the payload runs to the next newline (or EOF).
//! A trailing newline after the last payload is permitted. See
//! <https://develop.sentry.dev/sdk/envelopes/>.

use serde_json::Value;

/// A parsed envelope: its header plus the ordered list of items.
#[derive(Debug, Clone, PartialEq)]
pub struct Envelope {
    /// The envelope header object (may carry a top-level `event_id`, `dsn`, …).
    pub header: Value,
    /// Items in wire order.
    pub items: Vec<EnvelopeItem>,
}

/// A single envelope item: its header and raw payload bytes.
#[derive(Debug, Clone, PartialEq)]
pub struct EnvelopeItem {
    pub header: Value,
    pub payload: Vec<u8>,
}

impl Envelope {
    /// The envelope-level `event_id` (hex32), if the SDK set one in the header.
    pub fn header_event_id(&self) -> Option<String> {
        self.header
            .get("event_id")
            .and_then(Value::as_str)
            .map(str::to_owned)
    }
}

impl EnvelopeItem {
    /// The item `type` field (`event`, `transaction`, `session`, …).
    pub fn item_type(&self) -> Option<&str> {
        self.header.get("type").and_then(Value::as_str)
    }

    /// Parse the payload as JSON (used for `event` items).
    pub fn payload_json(&self) -> Result<Value, EnvelopeError> {
        serde_json::from_slice(&self.payload).map_err(|_| EnvelopeError::ItemPayloadJson)
    }
}

/// Failure modes of envelope parsing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EnvelopeError {
    /// The stream was empty (no header line).
    Empty,
    /// The envelope header line was not valid JSON.
    HeaderJson,
    /// An item header line was not valid JSON.
    ItemHeaderJson,
    /// An item declared a `length` that exceeds the remaining bytes.
    TruncatedItem,
    /// An item payload was expected to be JSON but failed to parse.
    ItemPayloadJson,
}

impl std::fmt::Display for EnvelopeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let msg = match self {
            EnvelopeError::Empty => "empty envelope",
            EnvelopeError::HeaderJson => "invalid envelope header JSON",
            EnvelopeError::ItemHeaderJson => "invalid item header JSON",
            EnvelopeError::TruncatedItem => "item length exceeds payload",
            EnvelopeError::ItemPayloadJson => "invalid item payload JSON",
        };
        f.write_str(msg)
    }
}

impl std::error::Error for EnvelopeError {}

/// Parse a raw (already decompressed) envelope body.
pub fn parse(bytes: &[u8]) -> Result<Envelope, EnvelopeError> {
    let mut cursor = 0usize;

    let header_line = next_line(bytes, &mut cursor).ok_or(EnvelopeError::Empty)?;
    if header_line.is_empty() {
        return Err(EnvelopeError::Empty);
    }
    let header: Value =
        serde_json::from_slice(header_line).map_err(|_| EnvelopeError::HeaderJson)?;

    let mut items = Vec::new();
    while cursor < bytes.len() {
        // A trailing newline after the final payload yields an empty header line.
        let Some(item_header_line) = next_line(bytes, &mut cursor) else {
            break;
        };
        if item_header_line.is_empty() {
            // Blank line between items (or trailing) — skip it.
            continue;
        }

        let item_header: Value =
            serde_json::from_slice(item_header_line).map_err(|_| EnvelopeError::ItemHeaderJson)?;

        let payload = read_payload(bytes, &mut cursor, &item_header)?;
        items.push(EnvelopeItem {
            header: item_header,
            payload,
        });
    }

    Ok(Envelope { header, items })
}

/// Read one item payload, honoring an explicit `length` header when present and
/// falling back to newline-delimited otherwise.
fn read_payload(
    bytes: &[u8],
    cursor: &mut usize,
    item_header: &Value,
) -> Result<Vec<u8>, EnvelopeError> {
    if let Some(len) = declared_length(item_header) {
        let start = *cursor;
        let end = start.checked_add(len).ok_or(EnvelopeError::TruncatedItem)?;
        if end > bytes.len() {
            return Err(EnvelopeError::TruncatedItem);
        }
        let payload = bytes[start..end].to_vec();
        *cursor = end;
        // Consume the single trailing newline that follows a length-prefixed payload.
        if *cursor < bytes.len() && bytes[*cursor] == b'\n' {
            *cursor += 1;
        }
        return Ok(payload);
    }

    // No declared length: payload is the next line (may be the last, no newline).
    let line = next_line(bytes, cursor).unwrap_or(&[]);
    Ok(line.to_vec())
}

/// Extract a non-negative `length` from an item header (number form).
fn declared_length(item_header: &Value) -> Option<usize> {
    let len = item_header.get("length")?.as_u64()?;
    usize::try_from(len).ok()
}

/// Return the bytes up to (but excluding) the next `\n`, advancing the cursor
/// past the newline. Returns `None` only when already at end of input.
fn next_line<'a>(bytes: &'a [u8], cursor: &mut usize) -> Option<&'a [u8]> {
    if *cursor >= bytes.len() {
        return None;
    }
    let start = *cursor;
    match bytes[start..].iter().position(|&b| b == b'\n') {
        Some(rel) => {
            let end = start + rel;
            *cursor = end + 1; // skip the newline
            Some(&bytes[start..end])
        }
        None => {
            // Last line with no trailing newline.
            *cursor = bytes.len();
            Some(&bytes[start..])
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_length_delimited_event() {
        // Payload declares an explicit length; it contains no inner newline here.
        let payload = "{\"message\":\"hello world\"}";
        let body = format!(
            "{{\"event_id\":\"9ec79c33ec9942ab8353589fcb2e04c0\",\"dsn\":\"https://e@x/1\"}}\n{{\"type\":\"event\",\"length\":{}}}\n{}\n",
            payload.len(),
            payload,
        );
        let env = parse(body.as_bytes()).expect("parse");
        assert_eq!(env.items.len(), 1);
        assert_eq!(env.items[0].item_type(), Some("event"));
        let json = env.items[0].payload_json().expect("json");
        assert_eq!(
            json.get("message").and_then(|v| v.as_str()),
            Some("hello world")
        );
    }

    #[test]
    fn parses_newline_delimited_items() {
        let body = concat!(
            "{\"event_id\":\"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa\"}\n",
            "{\"type\":\"session\"}\n",
            "{\"sid\":\"x\"}\n",
            "{\"type\":\"event\"}\n",
            "{\"message\":\"boom\"}\n",
        );
        let env = parse(body.as_bytes()).expect("parse");
        assert_eq!(env.items.len(), 2);
        assert_eq!(env.items[0].item_type(), Some("session"));
        assert_eq!(env.items[1].item_type(), Some("event"));
        assert_eq!(
            env.header_event_id().as_deref(),
            Some("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa")
        );
    }

    #[test]
    fn last_item_without_trailing_newline() {
        let body = concat!(
            "{}\n",
            "{\"type\":\"event\"}\n",
            "{\"message\":\"no newline\"}",
        );
        let env = parse(body.as_bytes()).expect("parse");
        assert_eq!(env.items.len(), 1);
        let json = env.items[0].payload_json().expect("json");
        assert_eq!(
            json.get("message").and_then(|v| v.as_str()),
            Some("no newline")
        );
    }

    #[test]
    fn length_prefixed_payload_with_inner_newline() {
        let payload = "{\"a\":1,\n\"b\":2}";
        let body = format!(
            "{{}}\n{{\"type\":\"event\",\"length\":{}}}\n{}\n",
            payload.len(),
            payload
        );
        let env = parse(body.as_bytes()).expect("parse");
        assert_eq!(env.items.len(), 1);
        let json = env.items[0].payload_json().expect("json");
        assert_eq!(json.get("a").and_then(|v| v.as_i64()), Some(1));
        assert_eq!(json.get("b").and_then(|v| v.as_i64()), Some(2));
    }

    #[test]
    fn empty_body_is_error() {
        assert_eq!(parse(b""), Err(EnvelopeError::Empty));
    }

    #[test]
    fn bad_header_json_is_error() {
        assert_eq!(parse(b"not json\n"), Err(EnvelopeError::HeaderJson));
    }

    #[test]
    fn truncated_length_is_error() {
        let body = "{}\n{\"type\":\"event\",\"length\":9999}\nshort\n";
        assert_eq!(parse(body.as_bytes()), Err(EnvelopeError::TruncatedItem));
    }

    #[test]
    fn header_only_envelope_has_no_items() {
        let env = parse(b"{\"event_id\":\"x\"}\n").expect("parse");
        assert!(env.items.is_empty());
    }
}
