//! Project domain type.

use super::{Id, Timestamp};
use serde::{Deserialize, Serialize};

/// Owns a DSN, retention setting, mute flag; container for issues.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Project {
    pub id: Id,
    /// Short, URL-friendly public code (6 chars, `a-z0-9`); used in web URLs and
    /// the SPA-facing API instead of the verbose UUID. The UUID [`id`](Self::id)
    /// remains the internal key (foreign keys, DSN, ingestion).
    pub short_id: String,
    pub team_id: Id,
    pub name: String,
    pub slug: String,
    /// DSN public key used for ingestion auth.
    pub dsn_public_key: String,
    /// Max stored events for this project (overrides `DEFAULT_EVENTS_RETENTION`).
    pub retention_events: i64,
    /// Age-based retention in days; 0 disables age-based pruning.
    /// Overrides `DEFAULT_RETENTION_DAYS` when > 0.
    pub retention_days: i64,
    /// Project-level mute: suppresses notifications; ingestion continues.
    pub muted: bool,
    /// Optional per-project webhook URL for new-issue/regression notifications
    ///. When set, notifications are POSTed as JSON under the same
    /// suppression rules as email.
    pub webhook_url: Option<String>,
    pub created_at: Timestamp,
    pub updated_at: Timestamp,
    pub orphaned_at: Option<Timestamp>,
}

/// Maximum length of a project slug, in characters.
pub const SLUG_MAX_LEN: usize = 48;

/// Why a string is not an acceptable project slug.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SlugError {
    Empty,
    TooLong,
    /// A character outside `a-z`, `0-9` and `-`.
    InvalidChar,
    /// Leading, trailing or repeated `-`.
    MisplacedDash,
}

impl std::fmt::Display for SlugError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let msg = match self {
            SlugError::Empty => "must not be empty".to_string(),
            SlugError::TooLong => format!("must be at most {SLUG_MAX_LEN} characters"),
            SlugError::InvalidChar => {
                "may contain only lowercase latin letters, digits and '-'".to_string()
            }
            SlugError::MisplacedDash => {
                "must not start or end with '-' or contain '--'".to_string()
            }
        };
        f.write_str(&msg)
    }
}

/// Check the strict project slug rule: 1..=[`SLUG_MAX_LEN`] characters from
/// `a-z`, `0-9` and `-`, where `-` only separates non-empty words.
///
/// The slug is the project's public address (`/projects/{slug}`), so it must be
/// stable, URL-safe and unambiguous. [`slugify`] always produces a valid slug.
pub fn validate_slug(slug: &str) -> Result<(), SlugError> {
    if slug.is_empty() {
        return Err(SlugError::Empty);
    }
    if slug.len() > SLUG_MAX_LEN {
        return Err(SlugError::TooLong);
    }
    if !slug
        .bytes()
        .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
    {
        return Err(SlugError::InvalidChar);
    }
    if slug.starts_with('-') || slug.ends_with('-') || slug.contains("--") {
        return Err(SlugError::MisplacedDash);
    }
    Ok(())
}

/// Derive a valid slug (see [`validate_slug`]) from free text.
pub fn slugify(input: &str) -> String {
    let mut slug = String::with_capacity(input.len().min(SLUG_MAX_LEN));
    let mut prev_dash = false;
    for ch in input.chars() {
        if slug.len() >= SLUG_MAX_LEN {
            break;
        }
        if ch.is_ascii_alphanumeric() {
            slug.push(ch.to_ascii_lowercase());
            prev_dash = false;
        } else if !prev_dash && !slug.is_empty() {
            slug.push('-');
            prev_dash = true;
        }
    }
    let trimmed = slug.trim_matches('-').to_string();
    if trimmed.is_empty() {
        // Fall back to a random suffix so slugs are never empty.
        return format!("project-{}", &Id::new_v4().simple().to_string()[..8]);
    }
    trimmed
}

pub fn is_valid_webhook_url(value: &str) -> bool {
    match reqwest::Url::parse(value) {
        Ok(url) => matches!(url.scheme(), "http" | "https"),
        Err(_) => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validate_slug_accepts_kebab_case() {
        for ok in [
            "a",
            "web",
            "checkout-api",
            "v2",
            "2024-site",
            &"a".repeat(SLUG_MAX_LEN),
        ] {
            assert_eq!(validate_slug(ok), Ok(()), "{ok}");
        }
    }

    #[test]
    fn validate_slug_rejects_everything_else() {
        assert_eq!(validate_slug(""), Err(SlugError::Empty));
        assert_eq!(
            validate_slug(&"a".repeat(SLUG_MAX_LEN + 1)),
            Err(SlugError::TooLong)
        );
        for bad in ["Web", "web_api", "web.api", "web api", "проект", "web/api"] {
            assert_eq!(validate_slug(bad), Err(SlugError::InvalidChar), "{bad}");
        }
        for bad in ["-web", "web-", "web--api", "-"] {
            assert_eq!(validate_slug(bad), Err(SlugError::MisplacedDash), "{bad}");
        }
    }

    #[test]
    fn slugify_output_is_always_valid() {
        let long = "Word ".repeat(40);
        for input in [
            "My Cool Project!",
            "  x  ",
            "!!!",
            "Проект",
            long.as_str(),
            "a-b--c",
        ] {
            let slug = slugify(input);
            assert_eq!(validate_slug(&slug), Ok(()), "{input:?} -> {slug:?}");
        }
    }
}
