//! Ports — async repository & service traits (hexagonal architecture).
//!
//! Domain/service logic depends on these TRAITS; infra (`crate::adapters`)
//! implements them. All methods return domain types and `crate::error::Result`.
//!
//! These traits are THE contract between foundation and feature agents.

mod clock;
mod event;
mod favorite;
mod invite;
mod issue;
mod mailer;
mod mute_rule;
mod passkey;
mod project;
mod session;
mod settings;
mod team;
mod user;

pub use clock::Clock;
pub use event::{EventRepository, NewEvent};
pub use favorite::FavoriteRepository;
pub use invite::{InviteRepository, NewInvite};
pub use issue::{IssueFilter, IssueRepository, IssueSort, IssueUpsert, UpsertOutcome};
pub use mailer::{Mailer, OutboundEmail};
pub use mute_rule::{NewTagMuteRule, TagMuteRuleRepository};
pub use passkey::{NewPasskey, PasskeyRepository};
pub use project::{NewProject, ProjectRepository, ProjectUpdate};
pub use session::SessionRepository;
pub use settings::SettingsRepository;
pub use team::TeamRepository;
pub use user::{NewUser, UserRepository, UserUpdate};
