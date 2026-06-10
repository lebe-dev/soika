//! Internal JSON API consumed by the SvelteKit frontend.
//!
//! Session-cookie auth. Each submodule owns a slice of the API; handlers return
//! a `501` stub until feature agents fill in the bodies.

pub mod client_config;
pub mod events;
pub mod invites;
pub mod issues;
pub mod members;
pub mod mute_rules;
pub mod profile;
pub mod projects;
pub mod settings;
pub mod teams;
