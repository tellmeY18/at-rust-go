-- atrg internal: OAuth state storage for PKCE flow
CREATE TABLE IF NOT EXISTS atrg_oauth_states (
    state         TEXT PRIMARY KEY,
    pkce_verifier TEXT NOT NULL,
    nonce         TEXT NOT NULL,
    handle        TEXT NOT NULL,
    created_at    INTEGER NOT NULL DEFAULT (unixepoch()),
    expires_at    INTEGER NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_atrg_oauth_states_expires_at ON atrg_oauth_states(expires_at);
