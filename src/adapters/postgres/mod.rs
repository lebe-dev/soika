//! PostgreSQL adapters implementing every repository port.
//!
//! Mirrors [`crate::adapters::sqlite`] port for port; the differences are the
//! dialect (`$n` placeholders, `ON CONFLICT`, `ILIKE`, `= ANY($1)`) and the
//! native column types of `migrations/postgres`. Unlike SQLite, Postgres runs
//! writers concurrently (`READ COMMITTED`), so read-then-write paths take row
//! locks (`SELECT … FOR UPDATE`) or lean on `ON CONFLICT` instead of a
//! database-wide write lock.
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
mod passkey;
mod project;
mod session;
mod settings;
mod team;
mod user;

pub use convert::Db;
pub(crate) use convert::*;

pub use event::PgEventRepository;
pub use favorite::PgFavoriteRepository;
pub use invite::PgInviteRepository;
pub use issue::PgIssueRepository;
pub use mute_rule::PgTagMuteRuleRepository;
pub use passkey::PgPasskeyRepository;
pub use project::PgProjectRepository;
pub use session::PgSessionRepository;
pub use settings::PgSettingsRepository;
pub use team::PgTeamRepository;
pub use user::PgUserRepository;
