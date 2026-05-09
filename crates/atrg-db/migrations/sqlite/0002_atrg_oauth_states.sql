-- atrg internal: OAuth state storage for PKCE flow
CREATE TABLE IF NOT EXISTS atrg_oauth_states (
    state            TEXT PRIMARY KEY,
    pkce_verifier    TEXT NOT NULL,
    dpop_private_key TEXT NOT NULL,
    token_endpoint   TEXT NOT NULL,
    did              TEXT NOT NULL,
    handle           TEXT NOT NULL,
    nonce            TEXT NOT NULL,
    redirect_after   TEXT NOT NULL DEFAULT '/',
    created_at       INTEGER NOT NULL DEFAULT (unixepoch()),
    expires_at       INTEGER NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_atrg_oauth_states_expires_at ON atrg_oauth_states(expires_at);
