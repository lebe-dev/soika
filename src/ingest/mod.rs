//! Sentry-compatible event ingestion.
//!
//! Endpoint: `POST /api/{project_id}/envelope/`. Auth via DSN public key, body
//! may be gzip/zlib-compressed. Pipeline: auth → decode → parse → normalize →
//! fingerprint → upsert issue → insert event → counters → notify. The HTTP
//! handler bodies live in [`handlers`].

mod enrich;
mod envelope;
mod handlers;
mod ratelimit;
mod tags;

pub use enrich::{RequestMeta, enrich};
pub use envelope::{Envelope, EnvelopeError, EnvelopeItem};
pub use handlers::{envelope, store};
pub use ratelimit::{DEFAULT_LIMIT, DEFAULT_WINDOW, Decision, RateLimiter};
