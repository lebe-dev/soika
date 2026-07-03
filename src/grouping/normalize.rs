//! Normalization helpers for stable grouping.
//!
//! These functions strip volatile detail (line/column numbers, query strings,
//! cache-busting hashes, memory addresses, anonymous-closure suffixes) from
//! frame components so that semantically identical errors collapse into one
//! group while genuinely different errors stay distinct. Normalization affects
//! ONLY the fingerprint — the stored event keeps the raw values.

use crate::domain::stacktrace::Frame;

/// Build a stable grouping signature for a single frame, or `None` when the
/// frame carries no usable identifying information.
///
/// Prefers symbolic identity (`module` + `function`); falls back to a
/// normalized file path when no symbol is available.
pub fn frame_signature(frame: &Frame) -> Option<String> {
    let module = frame
        .module
        .as_deref()
        .map(normalize_module)
        .filter(|s| !s.is_empty());
    let function = frame
        .function
        .as_deref()
        .map(normalize_function)
        .filter(|s| !s.is_empty());

    if let Some(function) = function {
        return Some(match module {
            Some(module) => format!("{module}.{function}"),
            None => function,
        });
    }

    // No usable function symbol — fall back to a normalized path.
    let path = frame
        .filename
        .as_deref()
        .or(frame.abs_path.as_deref())
        .map(normalize_path)
        .filter(|s| !s.is_empty());

    match (module, path) {
        (Some(module), Some(path)) => Some(format!("{module}@{path}")),
        (Some(module), None) => Some(module),
        (None, Some(path)) => Some(path),
        (None, None) => None,
    }
}

/// Normalize an exception type for grouping (trim only; types are stable IDs).
pub fn normalize_type(ty: &str) -> String {
    ty.trim().to_string()
}

/// Normalize a function/method name: trim, strip address suffixes and
/// anonymous-closure noise that vary per build/invocation.
pub fn normalize_function(function: &str) -> String {
    let mut f = function.trim().to_string();

    // Rust monomorphization / address hash suffix: `foo::h1a2b3c4d`.
    f = strip_rust_hash_suffix(&f);

    // Drop trailing memory addresses, e.g. `func+0x2a` or `func (0x55f...)`.
    if let Some(idx) = f.find("+0x") {
        f.truncate(idx);
    }

    f.trim().to_string()
}

/// Normalize a module / package path for grouping.
pub fn normalize_module(module: &str) -> String {
    let m = module.trim();
    // Rust closure/monomorphization hash on a module-ish symbol.
    strip_rust_hash_suffix(m)
}

/// Normalize a source file path: drop query strings, fragments, and
/// cache-busting / content hashes embedded in JS bundle filenames.
pub fn normalize_path(path: &str) -> String {
    let mut p = path.trim();

    // Strip URL query string and fragment.
    if let Some(idx) = p.find('?') {
        p = &p[..idx];
    }
    if let Some(idx) = p.find('#') {
        p = &p[..idx];
    }

    let mut p = p.to_string();

    // Strip a `webpack://` / `app://`-style scheme prefix that some bundlers add.
    for scheme in ["webpack://", "webpack-internal:///", "app://"] {
        if let Some(rest) = p.strip_prefix(scheme) {
            p = rest.to_string();
        }
    }

    strip_bundle_hash(&p)
}

/// Typed placeholders for volatile tokens, mirroring Sentry's message
/// parameterization. Used for BOTH the grouping key and the issue title, so the
/// displayed title matches how events were grouped.
const PLACEHOLDER_INT: &str = "<int>";
const PLACEHOLDER_UUID: &str = "<uuid>";
const PLACEHOLDER_ID: &str = "<id>";

/// Minimum length for a mixed letters+digits token to be treated as a random
/// id/hash (`<id>`) rather than a meaningful word. Keeps `md5`, `log4j`, `v1`
/// readable while `oW2Euf6y8` collapses.
const MIN_RANDOM_TOKEN_LEN: usize = 6;

/// Normalize a free-form message into a stable grouping key / display form.
///
/// Collapses whitespace to single spaces and replaces volatile tokens with
/// typed placeholders (Sentry-style): integers → `<int>`, UUIDs → `<uuid>`, and
/// random ids/hashes embedded in paths (e.g. `/download/oW2Euf6y8`) → `<id>`.
/// Messages differing only in such tokens group together while genuinely
/// different messages stay distinct.
pub fn normalize_message(message: &str) -> String {
    let trimmed = message.trim();
    let mut out = String::with_capacity(trimmed.len());
    let mut prev_space = false;
    let mut i = 0;

    while i < trimmed.len() {
        let ch = trimmed[i..].chars().next().unwrap();

        if ch.is_whitespace() {
            if !prev_space {
                out.push(' ');
            }
            prev_space = true;
            i += ch.len_utf8();
            continue;
        }
        prev_space = false;

        // A standalone UUID collapses whole, even though its `-` separators
        // would otherwise split it into short hex tokens that survive masking.
        if trimmed.get(i..i + 36).is_some_and(is_uuid) {
            out.push_str(PLACEHOLDER_UUID);
            i += 36;
            continue;
        }

        if ch.is_ascii_alphanumeric() {
            let start = i;
            while i < trimmed.len()
                && trimmed[i..]
                    .chars()
                    .next()
                    .is_some_and(|c| c.is_ascii_alphanumeric())
            {
                i += 1; // ASCII alphanumerics are single-byte.
            }
            out.push_str(&normalize_token(&trimmed[start..i]));
            continue;
        }

        out.push(ch);
        i += ch.len_utf8();
    }

    out.trim().to_string()
}

/// Normalize one alphanumeric token for message grouping/display.
///
/// Pure-digit tokens → `<int>`; long mixed letters+digits tokens (random
/// ids/hashes) → `<id>`; short/plain tokens keep their letters but still
/// collapse any internal digit runs to `<int>`.
fn normalize_token(token: &str) -> String {
    let has_digit = token.bytes().any(|b| b.is_ascii_digit());
    let has_alpha = token.bytes().any(|b| b.is_ascii_alphabetic());

    if has_digit && !has_alpha {
        return PLACEHOLDER_INT.to_string();
    }
    if has_digit && has_alpha && token.len() >= MIN_RANDOM_TOKEN_LEN {
        return PLACEHOLDER_ID.to_string();
    }
    collapse_digit_runs(token)
}

/// Replace each maximal run of digits in `token` with `<int>`, leaving letters
/// untouched (`v1` → `v<int>`, `x86` → `x<int>`).
fn collapse_digit_runs(token: &str) -> String {
    let mut out = String::with_capacity(token.len());
    let mut prev_digit = false;
    for ch in token.chars() {
        if ch.is_ascii_digit() {
            if !prev_digit {
                out.push_str(PLACEHOLDER_INT);
            }
            prev_digit = true;
            continue;
        }
        prev_digit = false;
        out.push(ch);
    }
    out
}

/// Whether `s` is exactly a canonical `8-4-4-4-12` hex UUID.
fn is_uuid(s: &str) -> bool {
    let bytes = s.as_bytes();
    if bytes.len() != 36 {
        return false;
    }
    bytes.iter().enumerate().all(|(idx, &b)| match idx {
        8 | 13 | 18 | 23 => b == b'-',
        _ => b.is_ascii_hexdigit(),
    })
}

/// Strip a Rust symbol hash suffix like `::h1a2b3c4d5e6f7a8b` (16 hex chars).
fn strip_rust_hash_suffix(symbol: &str) -> String {
    let Some(idx) = symbol.rfind("::h") else {
        return symbol.to_string();
    };
    let suffix = &symbol[idx + 3..];
    if suffix.len() >= 12 && suffix.chars().all(|c| c.is_ascii_hexdigit()) {
        return symbol[..idx].to_string();
    }
    symbol.to_string()
}

/// Strip a cache-busting / content hash from a bundle filename, e.g.
/// `main.4f3a9b2c.js` → `main.js`, `chunk-8Kd2.mjs` → `chunk.mjs`.
fn strip_bundle_hash(path: &str) -> String {
    let (dir, file) = match path.rfind('/') {
        Some(idx) => (&path[..=idx], &path[idx + 1..]),
        None => ("", path),
    };

    // Split off the final extension so the hash segment (if any) sits at the end.
    let (stem, ext) = match file.rfind('.') {
        Some(idx) => (&file[..idx], &file[idx..]),
        None => return path.to_string(),
    };

    // A hash segment is the last `.`-delimited part of the stem made only of
    // hex/base62 chars and of "hash-like" length.
    if let Some(dot) = stem.rfind('.') {
        let candidate = &stem[dot + 1..];
        if is_hash_like(candidate) {
            return format!("{dir}{}{ext}", &stem[..dot]);
        }
    }

    // Vite/webpack also use `name-<hash>.ext`. Strip a trailing `-<hash>`.
    if let Some(dash) = stem.rfind('-') {
        let candidate = &stem[dash + 1..];
        if is_hash_like(candidate) {
            return format!("{dir}{}{ext}", &stem[..dash]);
        }
    }

    path.to_string()
}

/// Heuristic: a token that looks like a generated content hash.
fn is_hash_like(token: &str) -> bool {
    let len = token.len();
    if !(6..=24).contains(&len) {
        return false;
    }
    let has_digit = token.bytes().any(|b| b.is_ascii_digit());
    let all_alnum = token.bytes().all(|b| b.is_ascii_alphanumeric());
    // Require at least one digit so plain words (`utils`, `helpers`) survive.
    all_alnum && has_digit
}

#[cfg(test)]
mod tests {
    use super::*;

    fn frame(module: Option<&str>, function: Option<&str>, file: Option<&str>) -> Frame {
        Frame {
            module: module.map(str::to_string),
            function: function.map(str::to_string),
            filename: file.map(str::to_string),
            ..Frame::default()
        }
    }

    #[test]
    fn signature_prefers_module_and_function() {
        let f = frame(Some("app.web"), Some("handle"), Some("web.py"));
        assert_eq!(frame_signature(&f).as_deref(), Some("app.web.handle"));
    }

    #[test]
    fn signature_falls_back_to_path() {
        let f = frame(None, None, Some("src/index.js"));
        assert_eq!(frame_signature(&f).as_deref(), Some("src/index.js"));
    }

    #[test]
    fn signature_none_when_empty() {
        assert!(frame_signature(&Frame::default()).is_none());
    }

    #[test]
    fn rust_hash_suffix_stripped() {
        assert_eq!(
            normalize_function("core::ptr::drop_in_place::h1a2b3c4d5e6f7a8b"),
            "core::ptr::drop_in_place"
        );
        // Not a hash → preserved.
        assert_eq!(normalize_function("do::hthing"), "do::hthing");
    }

    #[test]
    fn address_suffix_stripped() {
        assert_eq!(normalize_function("runtime.main+0x2a"), "runtime.main");
    }

    #[test]
    fn path_query_and_fragment_dropped() {
        assert_eq!(normalize_path("app.js?v=123#frag"), "app.js");
    }

    #[test]
    fn bundle_content_hash_stripped() {
        assert_eq!(normalize_path("assets/main.4f3a9b2c.js"), "assets/main.js");
        assert_eq!(normalize_path("chunk-8Kd2x9.mjs"), "chunk.mjs");
    }

    #[test]
    fn plain_filename_preserved() {
        // No hash-like segment, keep as-is.
        assert_eq!(
            normalize_path("src/utils/helpers.js"),
            "src/utils/helpers.js"
        );
    }

    #[test]
    fn message_digits_collapse() {
        assert_eq!(
            normalize_message("user 12345 not found"),
            normalize_message("user 6789 not found")
        );
        assert_eq!(
            normalize_message("user 1 not found"),
            "user <int> not found"
        );
    }

    #[test]
    fn message_whitespace_collapses() {
        assert_eq!(normalize_message("a   b\t c"), "a b c");
    }

    #[test]
    fn message_random_path_token_masked() {
        // The screenshot case: same error, different random download token.
        assert_eq!(
            normalize_message("Not found: /download/oW2Euf6y8"),
            normalize_message("Not found: /download/GaycV4viT")
        );
        assert_eq!(
            normalize_message("Not found: /download/oW2Euf6y8"),
            "Not found: /download/<id>"
        );
    }

    #[test]
    fn message_uuid_masked() {
        assert_eq!(
            normalize_message("session 550e8400-e29b-41d4-a716-446655440000 expired"),
            normalize_message("session 067e6162-3b6f-4ae2-a171-2470b63dff00 expired")
        );
        assert_eq!(
            normalize_message("session 550e8400-e29b-41d4-a716-446655440000 expired"),
            "session <uuid> expired"
        );
    }

    #[test]
    fn message_plain_words_and_short_tokens_survive() {
        // Real words stay untouched; short mixed tokens keep their letters and
        // only their digit runs collapse, so distinct errors stay distinct.
        assert_eq!(
            normalize_message("connection refused"),
            "connection refused"
        );
        assert_eq!(normalize_message("md5 mismatch"), "md<int> mismatch");
        assert_eq!(
            normalize_message("http error v1.2"),
            "http error v<int>.<int>"
        );
    }

    #[test]
    fn message_distinct_text_stays_distinct() {
        assert_ne!(
            normalize_message("Not found: /download/oW2Euf6y8"),
            normalize_message("Not found: /upload/oW2Euf6y8")
        );
    }

    #[test]
    fn is_uuid_matches_canonical_form_only() {
        assert!(is_uuid("550e8400-e29b-41d4-a716-446655440000"));
        assert!(!is_uuid("550e8400e29b41d4a716446655440000"));
        assert!(!is_uuid("550e8400-e29b-41d4-a716-44665544000g"));
        assert!(!is_uuid("short"));
    }
}
