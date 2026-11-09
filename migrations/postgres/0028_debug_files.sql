-- Mapping lifetime follows the project; upload chunks expire after 24 hours.
CREATE TABLE proguard_mappings (
    owner_id UUID NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    key TEXT NOT NULL,
    data TEXT NOT NULL,
    expires_at BIGINT NOT NULL,
    PRIMARY KEY (owner_id, key)
);
CREATE TABLE debug_upload_chunks (
    owner_id UUID NOT NULL REFERENCES teams(id) ON DELETE CASCADE,
    key TEXT NOT NULL,
    data TEXT NOT NULL,
    expires_at BIGINT NOT NULL,
    PRIMARY KEY (owner_id, key)
);
CREATE INDEX debug_upload_chunks_expiry ON debug_upload_chunks(expires_at);
