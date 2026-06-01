//! Normalization helpers for stable grouping (MVP §7).
//!
//! These functions strip volatile detail (line/column numbers, query strings,
//! cache-busting hashes, memory addresses, anonymous-closure suffixes) from
//! frame components so that semantically identical errors collapse into one
//! group while genuinely different errors stay distinct. Normalization affects
//! ONLY the fingerprint — the stored event keeps the raw values (§6).

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

/// Normalize a free-form message into a stable grouping key.
///
/// Collapses runs of digits to `0` and whitespace to single spaces so that
/// numerically varying messages (ids, counts, timestamps) group together.
pub fn normalize_message(message: &str) -> String {
    let mut out = String::with_capacity(message.len());
    let mut prev_digit = false;
    let mut prev_space = false;

    for ch in message.trim().chars() {
        if ch.is_ascii_digit() {
            if !prev_digit {
                out.push('0');
            }
            prev_digit = true;
            prev_space = false;
            continue;
        }
        prev_digit = false;

        if ch.is_whitespace() {
            if !prev_space {
                out.push(' ');
            }
            prev_space = true;
            continue;
        }
        prev_space = false;
        out.push(ch);
    }

    out.trim().to_string()
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
        assert_eq!(normalize_message("user 1 not found"), "user 0 not found");
    }

    #[test]
    fn message_whitespace_collapses() {
        assert_eq!(normalize_message("a   b\t c"), "a b c");
    }
}
