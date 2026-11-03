-- Short public ids for teams.
--
-- The internal primary key stays UUID (foreign keys, invites, settings);
-- `short_id` is a compact hash-like code (10 lowercase hex chars, 40 random
-- bits) used in web URLs (`/teams/{short_id}`) instead of the verbose UUID.
--
-- The database assigns it: existing rows here, new rows through the trigger
-- below, so every insert path (API, configuration sync) gets one without
-- generating it itself. The UNIQUE index rejects the astronomically rare clash.

ALTER TABLE teams ADD COLUMN short_id TEXT;

UPDATE teams SET short_id = lower(hex(randomblob(5)));

CREATE UNIQUE INDEX idx_teams_short_id ON teams (short_id);

CREATE TRIGGER teams_assign_short_id AFTER INSERT ON teams
WHEN NEW.short_id IS NULL
BEGIN
    UPDATE teams SET short_id = lower(hex(randomblob(5))) WHERE id = NEW.id;
END;
