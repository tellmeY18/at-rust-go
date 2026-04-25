CREATE TABLE IF NOT EXISTS likes (
    did         TEXT NOT NULL,
    rkey        TEXT NOT NULL,
    subject_uri TEXT NOT NULL DEFAULT '',
    created_at  TEXT NOT NULL DEFAULT '',
    indexed_at  INTEGER NOT NULL DEFAULT (unixepoch()),
    PRIMARY KEY (did, rkey)
);
CREATE INDEX IF NOT EXISTS idx_likes_subject ON likes(subject_uri);
