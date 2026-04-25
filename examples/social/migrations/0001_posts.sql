CREATE TABLE IF NOT EXISTS posts (
    did        TEXT NOT NULL,
    rkey       TEXT NOT NULL,
    text       TEXT NOT NULL DEFAULT '',
    created_at TEXT NOT NULL DEFAULT '',
    indexed_at INTEGER NOT NULL DEFAULT (unixepoch()),
    PRIMARY KEY (did, rkey)
);
CREATE INDEX IF NOT EXISTS idx_posts_indexed_at ON posts(indexed_at DESC);
