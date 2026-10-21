-- Issue-level timed mute: extend the existing `muted` status with an optional
-- expiry / auto-resurface condition.
--
-- Two mutually-exclusive mute shapes (a plain "mute forever" leaves all NULL):
--   * time-based:  `muted_until` set → the scheduler auto-unmutes once it passes.
--   * event-rate:  `mute_threshold` + `mute_window_seconds` set → ingestion
--                  auto-unmutes once `threshold` events arrive within the rolling
--                  `window_seconds` after the mute began.
--
-- `muted_at` records when the mute was applied; it is the baseline for the
-- event-rate window so events from before the mute never trip it.
ALTER TABLE issues ADD COLUMN muted_at TEXT;
ALTER TABLE issues ADD COLUMN muted_until TEXT;
ALTER TABLE issues ADD COLUMN mute_threshold INTEGER;
ALTER TABLE issues ADD COLUMN mute_window_seconds INTEGER;

-- The scheduler sweep finds expired time-based mutes by (status, muted_until).
CREATE INDEX idx_issues_muted_until ON issues (status, muted_until);
