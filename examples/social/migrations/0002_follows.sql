CREATE TABLE IF NOT EXISTS follows (
    subject_did TEXT NOT NULL,
    target_did  TEXT NOT NULL,
    rkey        TEXT NOT NULL DEFAULT '',
    created_at  TEXT NOT NULL DEFAULT '',
    indexed_at  INTEGER NOT NULL DEFAULT (unixepoch()),
    PRIMARY KEY (subject_did, target_did)
);
CREATE INDEX IF NOT EXISTS idx_follows_target ON follows(target_did);
