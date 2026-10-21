-- Project-level "mute by tags" rules.
--
-- A rule is a set of `key=value` tag pairs (AND): an incoming event matches the
-- rule when ALL of its pairs are present in the event's tags. When any rule
-- matches, the project's notification for that event is suppressed — the event
-- is still ingested, stored, and counted (mirrors issue-level mute). This is a
-- notification filter, NOT an inbound/drop filter.
--
-- The pairs live in a child table so a rule can carry one or more of them with
-- ON DELETE CASCADE cleanup. One value per key within a rule (two values for the
-- same key under AND could never match), enforced by the composite primary key.
CREATE TABLE tag_mute_rules (
    id         TEXT NOT NULL PRIMARY KEY,
    project_id TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    -- Optional human-readable label shown in the settings list.
    name       TEXT,
    -- The user who created the rule (kept for display); NULL if the user is gone.
    created_by TEXT REFERENCES users(id) ON DELETE SET NULL,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);

CREATE TABLE tag_mute_rule_tags (
    rule_id   TEXT NOT NULL REFERENCES tag_mute_rules(id) ON DELETE CASCADE,
    tag_key   TEXT NOT NULL,
    tag_value TEXT NOT NULL,
    PRIMARY KEY (rule_id, tag_key)
);

-- Ingestion lists a project's rules on every event; index the lookup.
CREATE INDEX idx_tag_mute_rules_project ON tag_mute_rules (project_id);
