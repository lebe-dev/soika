-- Short public ids for teams.
--
-- The internal primary key stays UUID (foreign keys, invites, settings);
-- `short_id` is a compact hash-like code (10 lowercase hex chars, 40 random
-- bits) used in web URLs (`/teams/{short_id}`) instead of the verbose UUID.
--
-- The column default assigns it to existing rows (a volatile default is
-- evaluated per row) and to every new insert. The UNIQUE index rejects the
-- astronomically rare clash.

ALTER TABLE teams ADD COLUMN short_id TEXT NOT NULL
    DEFAULT substr(replace(gen_random_uuid()::text, '-', ''), 1, 10);

CREATE UNIQUE INDEX idx_teams_short_id ON teams (short_id);
