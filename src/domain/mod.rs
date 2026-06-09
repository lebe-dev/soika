//! Domain entities — plain Rust types only.
//!
//! These types carry **no** persistence concerns (no `sqlx`); adapters map
//! to/from them. They are the currency of the port traits in [`crate::ports`].

mod event;
mod invite;
mod issue;
mod membership;
mod project;
mod settings;
pub mod stacktrace;
mod team;
mod user;

pub use event::Event;
pub use invite::Invite;
pub use issue::{Issue, IssueStatus, MuteSpec};
pub use membership::{Membership, Role};
pub use project::Project;
pub use settings::ServiceSettings;
pub use stacktrace::{ExceptionValue, Frame, NormalizedEvent, Stacktrace};
pub use team::{Team, TeamMember};
pub use user::{AuthProvider, Session, User};

/// Common timestamp type used across the domain (UTC).
pub type Timestamp = chrono::DateTime<chrono::Utc>;

/// Common id type used across the domain (UUID v4).
pub type Id = uuid::Uuid;
