//! SQLite adapters implementing every repository port.
//!
//! SQL is kept ANSI-friendly so a PostgreSQL backend can be added later. The
//! SQLite-specific representation helpers (id/timestamp/bool conversion, short
//! ids, conflict classification) live in [`convert`].
//!
//! Queries use runtime-checked `sqlx::query*` with explicit `FromRow` row
//! structs (rather than the compile-time `query!` macros) so the crate builds
//! without a live database or an offline metadata cache.

mod convert;
mod event;
mod favorite;
mod invite;
mod issue;
mod mute_rule;
mod project;
mod session;
mod settings;
mod team;
mod user;

#[cfg(test)]
pub(crate) mod tests;

pub(crate) use convert::*;
pub use convert::{Db, backfill_short_ids};

pub use event::SqliteEventRepository;
pub use favorite::SqliteFavoriteRepository;
pub use invite::SqliteInviteRepository;
pub use issue::SqliteIssueRepository;
pub use mute_rule::SqliteTagMuteRuleRepository;
pub use project::SqliteProjectRepository;
pub use session::SqliteSessionRepository;
pub use settings::SqliteSettingsRepository;
pub use team::SqliteTeamRepository;
pub use user::SqliteUserRepository;
