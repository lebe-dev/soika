//! Error grouping / fingerprinting and the ingest-pipeline tail.
//!
//! Default fingerprint derives from exception type + a normalized in-app
//! stacktrace (function/module/path normalized), mirroring Sentry's default
//! grouping. A custom `fingerprint` from the SDK overrides the default.
//!
//! The pipeline tail (upsert issue → insert event → counters → notify check)
//! lives in [`pipeline`] and is written against the repository TRAITS so it can
//! be unit-tested with fakes (hexagonal architecture). The fingerprint/title/
//! culprit derivation lives in [`fingerprint`].

mod fingerprint;
pub mod normalize;
pub mod pipeline;

pub use fingerprint::{
    culprit, culprit_from_normalized, fingerprint, fingerprint_normalized, issue_title,
    title_from_normalized,
};
pub use pipeline::{IngestInput, IngestOutcome, NotifyKind, ingest_normalized};
