//! Same-origin protection for cookie-authenticated and anonymous browser writes.
use crate::AppState;
use axum::{
    extract::{Request, State},
    http::{Method, StatusCode},
    middleware::Next,
    response::{IntoResponse, Response},
};
use openidconnect::url::Url;

pub async fn protect(State(state): State<AppState>, request: Request, next: Next) -> Response {
    if matches!(
        *request.method(),
        Method::GET | Method::HEAD | Method::OPTIONS
    ) {
        return next.run(request).await;
    }
    let headers = request.headers();
    let expected = Url::parse(&state.config.base_url)
        .ok()
        .map(|url| url.origin());
    let supplied_origin = headers.get("origin");
    let source = supplied_origin.or_else(|| headers.get("referer"));
    let valid_source = source
        .and_then(|value| value.to_str().ok())
        .and_then(|raw| Url::parse(raw).ok())
        .is_some_and(|url| {
            url.origin().is_tuple()
                && Some(url.origin()) == expected
                && (supplied_origin.is_none()
                    || url.as_str() == format!("{}/", url.origin().ascii_serialization()))
        });
    let cross_site = headers
        .get("sec-fetch-site")
        .is_some_and(|v| v == "cross-site");
    // A non-browser client (or privacy middleware) may omit Origin/Referer.
    // A custom header is an explicit fallback: HTML forms cannot set it and
    // cross-origin JS needs a CORS preflight that the cookie API never permits.
    let explicit_client =
        source.is_none() && headers.get("x-soika-request").is_some_and(|v| v == "1");
    if cross_site || !(valid_source || explicit_client) {
        return (
            StatusCode::FORBIDDEN,
            axum::Json(serde_json::json!({"error":"same-origin request required"})),
        )
            .into_response();
    }
    next.run(request).await
}
