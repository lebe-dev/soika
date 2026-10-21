//! Short public ids for projects and issues, shared by every database adapter.
//!
//! The generator is pure; each adapter pairs it with its own uniqueness check
//! against the `short_id` column (`unique_short_id`).

/// Alphabet for short public ids — lowercase alphanumerics, URL-friendly and
/// unambiguous in a browser address bar (36^6 ≈ 2.1B combinations at length 6).
const SHORT_ID_ALPHABET: &[u8] = b"abcdefghijklmnopqrstuvwxyz0123456789";

/// Length of a generated short id. Kept small for compact URLs.
const SHORT_ID_LEN: usize = 6;

/// Generate a random short public id (see [`SHORT_ID_ALPHABET`]).
///
/// Callers persist this into a UNIQUE column; the astronomically rare collision
/// is handled by retrying (see each adapter's `unique_short_id`).
pub(crate) fn generate_short_id() -> String {
    (0..SHORT_ID_LEN)
        .map(|_| SHORT_ID_ALPHABET[rand::random_range(0..SHORT_ID_ALPHABET.len())] as char)
        .collect()
}
