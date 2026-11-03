//! Static asset serving + SPA fallback, and the shared 501 stub helper.
//!
//! Frontend assets are embedded into the binary via `rust-embed`,
//! with a SPA fallback to `index.html` for client-side routes.

use axum::body::Body;
use axum::http::{StatusCode, Uri, header};
use axum::response::{IntoResponse, Response};
use rust_embed::RustEmbed;

/// Embedded frontend build output. The directory is created at build time; an
/// empty placeholder keeps the crate compiling before the frontend exists.
#[derive(RustEmbed)]
#[folder = "web/dist"]
struct Assets;

/// A uniform `501 Not Implemented` response used by every stub handler so the
/// router type-checks and runs before feature agents fill in bodies.
pub fn not_implemented(what: &str) -> Response {
    (
        StatusCode::NOT_IMPLEMENTED,
        format!("not implemented: {what}"),
    )
        .into_response()
}

/// SPA fallback handler: serves the embedded asset for `uri`, falling back to
/// `index.html` for unknown (client-routed) paths.
pub async fn spa_fallback(uri: Uri) -> Response {
    let path = uri.path().trim_start_matches('/');
    let path = if path.is_empty() { "index.html" } else { path };

    if let Some(content) = Assets::get(path) {
        let mime = mime_guess::from_path(path).first_or_octet_stream();
        return (
            [
                (header::CONTENT_TYPE, mime.as_ref()),
                (header::CACHE_CONTROL, cache_control(path)),
            ],
            content.data.into_owned(),
        )
            .into_response();
    }

    // Fallback to index.html for client-side routing.
    match Assets::get("index.html") {
        Some(content) => (
            [
                (header::CONTENT_TYPE, "text/html"),
                (header::CACHE_CONTROL, cache_control("index.html")),
            ],
            content.data.into_owned(),
        )
            .into_response(),
        None => (
            StatusCode::NOT_FOUND,
            Body::from("frontend assets not embedded"),
        )
            .into_response(),
    }
}

/// `Cache-Control` for an embedded asset. SvelteKit fingerprints everything
/// under `_app/immutable/`, so those files never change at a given URL and can
/// be cached forever; everything else (the `index.html` shell, `version.json`,
/// favicon) must be revalidated so a new release is picked up.
fn cache_control(path: &str) -> &'static str {
    if path.starts_with("_app/immutable/") {
        return "public, max-age=31536000, immutable";
    }
    "no-cache"
}

#[cfg(test)]
mod tests {
    use super::cache_control;

    #[test]
    fn fingerprinted_assets_are_cached_forever() {
        assert_eq!(
            cache_control("_app/immutable/chunks/abc123.js"),
            "public, max-age=31536000, immutable"
        );
    }

    #[test]
    fn shell_and_unversioned_files_are_revalidated() {
        for path in ["index.html", "_app/version.json", "favicon.svg"] {
            assert_eq!(cache_control(path), "no-cache", "{path}");
        }
    }
}
