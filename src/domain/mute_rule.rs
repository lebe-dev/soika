//! Project-level "mute by tags" rule.
//!
//! A [`TagMuteRule`] is a set of `key=value` tag pairs combined with AND: an
//! event matches the rule when every pair is present in the event's tags. When
//! any rule on a project matches, the notification for that event is suppressed
//! (the event is still ingested and counted — same semantics as issue-level
//! mute, [`IssueStatus::Muted`](super::IssueStatus)).

use super::{Id, Timestamp};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// A single `key=value` tag condition within a rule.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TagMatch {
    pub key: String,
    pub value: String,
}

/// A project-level rule that mutes notifications for events whose tags match.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TagMuteRule {
    pub id: Id,
    pub project_id: Id,
    /// Optional human-readable label shown in the settings list.
    pub name: Option<String>,
    /// The tag pairs that must ALL match (AND). Never empty for a persisted rule.
    pub tags: Vec<TagMatch>,
    /// The user who created the rule, if still present.
    pub created_by: Option<Id>,
    pub created_at: Timestamp,
    pub updated_at: Timestamp,
}

impl TagMuteRule {
    /// True when every tag pair in this rule is present in `event_tags` with the
    /// exact same value (AND semantics). An empty rule never matches — it would
    /// otherwise mute everything, which is never the intent.
    pub fn matches(&self, event_tags: &BTreeMap<String, String>) -> bool {
        if self.tags.is_empty() {
            return false;
        }
        self.tags
            .iter()
            .all(|t| event_tags.get(&t.key).is_some_and(|v| v == &t.value))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;

    fn rule(tags: &[(&str, &str)]) -> TagMuteRule {
        TagMuteRule {
            id: Id::new_v4(),
            project_id: Id::new_v4(),
            name: None,
            tags: tags
                .iter()
                .map(|(k, v)| TagMatch {
                    key: (*k).to_string(),
                    value: (*v).to_string(),
                })
                .collect(),
            created_by: None,
            created_at: Utc::now(),
            updated_at: Utc::now(),
        }
    }

    fn tags(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
        pairs
            .iter()
            .map(|(k, v)| ((*k).to_string(), (*v).to_string()))
            .collect()
    }

    #[test]
    fn single_pair_matches_when_present() {
        let r = rule(&[("environment", "staging")]);
        assert!(r.matches(&tags(&[("environment", "staging"), ("server", "ci")])));
    }

    #[test]
    fn single_pair_does_not_match_on_different_value() {
        let r = rule(&[("environment", "staging")]);
        assert!(!r.matches(&tags(&[("environment", "production")])));
    }

    #[test]
    fn all_pairs_required_and_semantics() {
        let r = rule(&[("environment", "staging"), ("server", "ci")]);
        assert!(r.matches(&tags(&[("environment", "staging"), ("server", "ci")])));
        // Missing the second pair → no match.
        assert!(!r.matches(&tags(&[("environment", "staging")])));
    }

    #[test]
    fn empty_rule_never_matches() {
        let r = rule(&[]);
        assert!(!r.matches(&tags(&[("environment", "staging")])));
        assert!(!r.matches(&tags(&[])));
    }
}
