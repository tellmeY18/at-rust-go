-- atrg internal: session storage for OAuth authentication (PostgreSQL dialect)
CREATE TABLE IF NOT EXISTS atrg_sessions (
    id            TEXT PRIMARY KEY,
    did           TEXT NOT NULL,
    handle        TEXT NOT NULL,
    access_token  TEXT NOT NULL,
    refresh_token TEXT,
    expires_at    BIGINT NOT NULL,
    created_at    BIGINT NOT NULL DEFAULT EXTRACT(EPOCH FROM NOW())::BIGINT,
    last_used_at  BIGINT NOT NULL DEFAULT EXTRACT(EPOCH FROM NOW())::BIGINT
);

CREATE INDEX IF NOT EXISTS idx_atrg_sessions_did ON atrg_sessions(did);
CREATE INDEX IF NOT EXISTS idx_atrg_sessions_expires_at ON atrg_sessions(expires_at);
