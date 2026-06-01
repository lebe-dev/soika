//! Stacktrace domain types and normalization (MVP §6, §7).
//!
//! Stacktraces are stored as received and rendered with frame-level detail:
//! module/function, file, line/column, in-app vs. system frames, and any
//! source-context lines the SDK included. No demangling / no source-map
//! resolution (MVP §1.2 Non-Goals) — frames are kept exactly as reported, and
//! normalization affects only the grouping fingerprint, never the stored event.

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// A single stack frame, mirroring the Sentry frame interface.
///
/// All fields are optional because SDKs populate different subsets; the type
/// preserves whatever the SDK reported so the UI can render it faithfully.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Frame {
    /// Package / module path (e.g. `myapp.tasks`, `github.com/me/app/pkg`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub module: Option<String>,
    /// Function / method name.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub function: Option<String>,
    /// Source file name as reported (relative or bare filename).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub filename: Option<String>,
    /// Absolute source path as reported (`abs_path`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub abs_path: Option<String>,
    /// 1-based line number.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub lineno: Option<i64>,
    /// 1-based column number.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub colno: Option<i64>,
    /// Whether the frame belongs to the application (vs. a library/system frame).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub in_app: Option<bool>,
    /// The source line at `lineno`, if the SDK shipped source context.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub context_line: Option<String>,
    /// Source lines immediately before `context_line`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub pre_context: Vec<String>,
    /// Source lines immediately after `context_line`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub post_context: Vec<String>,
}

impl Frame {
    /// True when the frame is explicitly marked as application code.
    ///
    /// Sentry treats a missing `in_app` as not-in-app for grouping purposes;
    /// we mirror that — only an explicit `true` counts as in-app.
    pub fn is_in_app(&self) -> bool {
        self.in_app == Some(true)
    }

    /// Whether the frame carries any source-context lines.
    pub fn has_source_context(&self) -> bool {
        self.context_line.is_some() || !self.pre_context.is_empty() || !self.post_context.is_empty()
    }

    /// A best-effort location string for display / culprit derivation, e.g.
    /// `module in function` or `filename:lineno`.
    pub fn location(&self) -> Option<String> {
        if let Some(function) = self.function.as_deref().filter(|s| !s.is_empty()) {
            if let Some(module) = self.module.as_deref().filter(|s| !s.is_empty()) {
                return Some(format!("{module} in {function}"));
            }
            if let Some(file) = self.display_file() {
                return Some(format!("{file} in {function}"));
            }
            return Some(function.to_string());
        }
        if let Some(file) = self.display_file() {
            return match self.lineno {
                Some(line) => Some(format!("{file}:{line}")),
                None => Some(file.to_string()),
            };
        }
        self.module
            .as_deref()
            .filter(|s| !s.is_empty())
            .map(str::to_string)
    }

    /// The most specific file label available (`filename`, else `abs_path`).
    fn display_file(&self) -> Option<&str> {
        self.filename
            .as_deref()
            .filter(|s| !s.is_empty())
            .or_else(|| self.abs_path.as_deref().filter(|s| !s.is_empty()))
    }
}

/// A normalized, parsed stacktrace.
///
/// Frames follow the Sentry convention: stored **oldest-first**, so the
/// crashing (most relevant) frame is the **last** element.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Stacktrace {
    pub frames: Vec<Frame>,
}

impl Stacktrace {
    /// Build a stacktrace from a raw Sentry `stacktrace` value (`{ "frames": [..] }`).
    pub fn from_value(value: &Value) -> Self {
        let frames = value
            .get("frames")
            .and_then(Value::as_array)
            .map(|arr| arr.iter().map(parse_frame).collect())
            .unwrap_or_default();
        Stacktrace { frames }
    }

    /// True when there are no frames at all.
    pub fn is_empty(&self) -> bool {
        self.frames.is_empty()
    }

    /// In-app frames only, preserving order (oldest-first).
    pub fn in_app_frames(&self) -> Vec<&Frame> {
        self.frames.iter().filter(|f| f.is_in_app()).collect()
    }

    /// The most relevant frame for display / culprit: the last in-app frame if
    /// any, otherwise the last frame overall.
    pub fn most_relevant_frame(&self) -> Option<&Frame> {
        self.frames
            .iter()
            .rev()
            .find(|f| f.is_in_app())
            .or_else(|| self.frames.last())
    }
}

/// Parse a single raw frame `Value` into a [`Frame`].
fn parse_frame(value: &Value) -> Frame {
    Frame {
        module: string_field(value, "module"),
        function: string_field(value, "function"),
        filename: string_field(value, "filename"),
        abs_path: string_field(value, "abs_path"),
        lineno: int_field(value, "lineno"),
        colno: int_field(value, "colno"),
        in_app: value.get("in_app").and_then(Value::as_bool),
        context_line: string_field(value, "context_line"),
        pre_context: string_array(value, "pre_context"),
        post_context: string_array(value, "post_context"),
    }
}

/// A single exception entry: type + value + (optional) stacktrace.
///
/// Mirrors `exception.values[]` in the Sentry payload.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExceptionValue {
    /// Exception class/type (e.g. `ValueError`, `TypeError`, `*errors.errorString`).
    #[serde(rename = "type", skip_serializing_if = "Option::is_none")]
    pub ty: Option<String>,
    /// Human-readable message / value.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub value: Option<String>,
    /// Module the exception type is defined in, if reported.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub module: Option<String>,
    /// Associated stacktrace, if present.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stacktrace: Option<Stacktrace>,
}

/// A normalized view of the parts of an event relevant to grouping & display.
///
/// Extracted from the raw event payload; carries the leading exception (the
/// last entry of `exception.values`, which is the one that was thrown) plus a
/// resolved stacktrace and any custom fingerprint sent by the SDK.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct NormalizedEvent {
    /// Exception type of the thrown exception, if this is an exception event.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub exception_type: Option<String>,
    /// Exception value/message, if present.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub exception_value: Option<String>,
    /// Log message (for message events without an exception).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    /// Severity level (`error`, `warning`, ...), if present.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub level: Option<String>,
    /// The resolved stacktrace (from the thrown exception, a thread, or the
    /// top-level `stacktrace`), if any.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stacktrace: Option<Stacktrace>,
    /// Custom fingerprint components sent by the SDK; override default grouping.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub fingerprint: Vec<String>,
}

impl NormalizedEvent {
    /// Parse and normalize a raw Sentry event payload.
    pub fn from_value(event: &Value) -> Self {
        let exceptions = parse_exceptions(event);
        // The thrown exception is the last entry of `exception.values`.
        let thrown = exceptions.last();

        let exception_type = thrown.and_then(|e| e.ty.clone());
        let exception_value = thrown.and_then(|e| e.value.clone());

        let stacktrace = resolve_stacktrace(event, &exceptions);

        NormalizedEvent {
            exception_type,
            exception_value,
            message: parse_message(event),
            level: string_field(event, "level"),
            stacktrace,
            fingerprint: string_array(event, "fingerprint"),
        }
    }

    /// True when the SDK supplied an explicit custom fingerprint.
    pub fn has_custom_fingerprint(&self) -> bool {
        !self.fingerprint.is_empty()
    }
}

/// Parse all `exception.values[]` into [`ExceptionValue`]s (in payload order).
fn parse_exceptions(event: &Value) -> Vec<ExceptionValue> {
    let Some(values) = event
        .get("exception")
        .and_then(|e| e.get("values"))
        .and_then(Value::as_array)
    else {
        return Vec::new();
    };

    values
        .iter()
        .map(|v| ExceptionValue {
            ty: string_field(v, "type"),
            value: string_field(v, "value"),
            module: string_field(v, "module"),
            stacktrace: v
                .get("stacktrace")
                .filter(|s| s.is_object())
                .map(Stacktrace::from_value),
        })
        .collect()
}

/// Resolve the stacktrace to use, preferring (in order): the thrown exception's
/// stacktrace, any exception stacktrace, a thread stacktrace, then the
/// top-level `stacktrace`.
fn resolve_stacktrace(event: &Value, exceptions: &[ExceptionValue]) -> Option<Stacktrace> {
    if let Some(st) = exceptions.last().and_then(|e| e.stacktrace.clone())
        && !st.is_empty()
    {
        return Some(st);
    }
    for exc in exceptions {
        if let Some(st) = exc.stacktrace.clone()
            && !st.is_empty()
        {
            return Some(st);
        }
    }
    if let Some(st) = thread_stacktrace(event)
        && !st.is_empty()
    {
        return Some(st);
    }
    let top = event
        .get("stacktrace")
        .filter(|s| s.is_object())
        .map(Stacktrace::from_value);
    top.filter(|st| !st.is_empty())
}

/// Extract a stacktrace from `threads.values[].stacktrace` (first thread that
/// has one, preferring the crashed thread).
fn thread_stacktrace(event: &Value) -> Option<Stacktrace> {
    let values = event
        .get("threads")
        .and_then(|t| t.get("values"))
        .and_then(Value::as_array)?;

    // Prefer the crashed thread if flagged.
    let crashed = values
        .iter()
        .find(|t| t.get("crashed").and_then(Value::as_bool) == Some(true));

    let candidates = crashed.into_iter().chain(values.iter());
    for thread in candidates {
        if let Some(st) = thread.get("stacktrace").filter(|s| s.is_object()) {
            let st = Stacktrace::from_value(st);
            if !st.is_empty() {
                return Some(st);
            }
        }
    }
    None
}

/// Extract a log message from `message` (string or `{ "formatted"/"message" }`)
/// or fall back to `logentry`.
fn parse_message(event: &Value) -> Option<String> {
    if let Some(msg) = event.get("message") {
        if let Some(s) = msg.as_str() {
            return non_empty(s);
        }
        if let Some(s) = message_object(msg) {
            return Some(s);
        }
    }
    event.get("logentry").and_then(message_object)
}

/// Pull `formatted` (preferred) or `message` from a message-object value.
fn message_object(value: &Value) -> Option<String> {
    value
        .get("formatted")
        .and_then(Value::as_str)
        .and_then(non_empty)
        .or_else(|| {
            value
                .get("message")
                .and_then(Value::as_str)
                .and_then(non_empty)
        })
}

/// Read a non-empty trimmed string field.
fn string_field(value: &Value, key: &str) -> Option<String> {
    value.get(key).and_then(Value::as_str).and_then(non_empty)
}

/// Read an integer field, tolerating numbers expressed as JSON strings.
fn int_field(value: &Value, key: &str) -> Option<i64> {
    let v = value.get(key)?;
    if let Some(i) = v.as_i64() {
        return Some(i);
    }
    if let Some(f) = v.as_f64() {
        return Some(f as i64);
    }
    v.as_str().and_then(|s| s.trim().parse::<i64>().ok())
}

/// Read an array of strings, skipping non-string / null entries.
fn string_array(value: &Value, key: &str) -> Vec<String> {
    value
        .get(key)
        .and_then(Value::as_array)
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default()
}

/// Trim a string and return `None` when it is empty.
fn non_empty(s: &str) -> Option<String> {
    let trimmed = s.trim();
    if trimmed.is_empty() {
        return None;
    }
    Some(trimmed.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn parses_exception_with_stacktrace() {
        let event = json!({
            "level": "error",
            "exception": {
                "values": [{
                    "type": "ValueError",
                    "value": "bad input",
                    "stacktrace": {
                        "frames": [
                            { "function": "main", "filename": "app.py", "lineno": 10, "in_app": true },
                            { "function": "parse", "module": "app.parser", "lineno": 42, "in_app": true }
                        ]
                    }
                }]
            }
        });

        let norm = NormalizedEvent::from_value(&event);
        assert_eq!(norm.exception_type.as_deref(), Some("ValueError"));
        assert_eq!(norm.exception_value.as_deref(), Some("bad input"));
        assert_eq!(norm.level.as_deref(), Some("error"));

        let st = norm.stacktrace.expect("stacktrace present");
        assert_eq!(st.frames.len(), 2);
        // most relevant = last in-app frame
        let top = st.most_relevant_frame().unwrap();
        assert_eq!(top.function.as_deref(), Some("parse"));
    }

    #[test]
    fn missing_in_app_is_not_in_app() {
        let frame = Frame {
            function: Some("f".into()),
            ..Frame::default()
        };
        assert!(!frame.is_in_app());
    }

    #[test]
    fn uses_thrown_exception_when_chained() {
        // exception.values is ordered oldest→newest; the thrown one is last.
        let event = json!({
            "exception": { "values": [
                { "type": "IOError", "value": "disk" },
                { "type": "RuntimeError", "value": "wrapped" }
            ]}
        });
        let norm = NormalizedEvent::from_value(&event);
        assert_eq!(norm.exception_type.as_deref(), Some("RuntimeError"));
    }

    #[test]
    fn falls_back_to_thread_stacktrace() {
        let event = json!({
            "threads": { "values": [
                { "crashed": true, "stacktrace": { "frames": [
                    { "function": "run", "in_app": true }
                ]}}
            ]}
        });
        let norm = NormalizedEvent::from_value(&event);
        let st = norm.stacktrace.expect("thread stacktrace");
        assert_eq!(st.frames.len(), 1);
    }

    #[test]
    fn parses_message_event() {
        let event = json!({ "message": "something happened", "level": "warning" });
        let norm = NormalizedEvent::from_value(&event);
        assert_eq!(norm.message.as_deref(), Some("something happened"));
        assert!(norm.exception_type.is_none());
        assert!(norm.stacktrace.is_none());
    }

    #[test]
    fn parses_structured_logentry() {
        let event = json!({ "logentry": { "formatted": "user 5 failed login" } });
        let norm = NormalizedEvent::from_value(&event);
        assert_eq!(norm.message.as_deref(), Some("user 5 failed login"));
    }

    #[test]
    fn custom_fingerprint_is_captured() {
        let event = json!({ "fingerprint": ["my-group", "{{ default }}"] });
        let norm = NormalizedEvent::from_value(&event);
        assert!(norm.has_custom_fingerprint());
        assert_eq!(norm.fingerprint, vec!["my-group", "{{ default }}"]);
    }

    #[test]
    fn lineno_tolerates_string_numbers() {
        let frame = parse_frame(&json!({ "function": "f", "lineno": "33" }));
        assert_eq!(frame.lineno, Some(33));
    }

    #[test]
    fn source_context_round_trips() {
        let frame = parse_frame(&json!({
            "function": "f",
            "pre_context": ["a", "b"],
            "context_line": "c",
            "post_context": ["d"]
        }));
        assert!(frame.has_source_context());
        assert_eq!(frame.pre_context, vec!["a", "b"]);
        assert_eq!(frame.context_line.as_deref(), Some("c"));
        assert_eq!(frame.post_context, vec!["d"]);
    }
}
