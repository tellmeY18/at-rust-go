-- atrg internal: session storage for OAuth authentication
CREATE TABLE IF NOT EXISTS atrg_sessions (
    id            TEXT PRIMARY KEY,
    did           TEXT NOT NULL,
    handle        TEXT NOT NULL,
    access_token  TEXT NOT NULL,
    refresh_token TEXT,
    expires_at    INTEGER NOT NULL,
    created_at    INTEGER NOT NULL DEFAULT (unixepoch()),
    last_used_at  INTEGER NOT NULL DEFAULT (unixepoch())
);

CREATE INDEX IF NOT EXISTS idx_atrg_sessions_did ON atrg_sessions(did);
CREATE INDEX IF NOT EXISTS idx_atrg_sessions_expires_at ON atrg_sessions(expires_at);
