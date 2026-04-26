# OAuth Authentication

atrg handles the AT Protocol OAuth DPOP flow automatically.

## How It Works

1. Client calls `GET /auth/login?handle=alice.example.com`
2. atrg resolves the handle to a PDS, initiates PKCE OAuth
3. User authenticates on their PDS
4. PDS redirects to `GET /auth/callback`
5. atrg exchanges the code for tokens, creates a session, sets a cookie
6. Client uses the session cookie or bearer token for subsequent requests

## Two Token Types

atrg transparently handles two kinds of credentials:

| Token Type | Source | Use Case |
|------------|--------|----------|
| **atrg session token** | Issued by atrg after OAuth login | Browser cookies, SPA bearer tokens |
| **AT Protocol JWT** | Issued by the user's PDS | Service-to-service calls, PDS-originated requests |

Both produce the same `AtrgSession` struct in your handlers.

## Extractors

```rust
use atrg_core::{AuthUser, RequireAuth};

// Optional — returns None if not authenticated
async fn maybe_auth(AuthUser(user): AuthUser) -> impl IntoResponse { ... }

// Required — returns 401 if not authenticated
async fn must_auth(RequireAuth(user): RequireAuth) -> impl IntoResponse { ... }
```

## Endpoints

| Endpoint | Method | Description |
|----------|--------|-------------|
| `/auth/login?handle=...` | GET | Initiate OAuth |
| `/auth/callback` | GET | OAuth callback |
| `/auth/logout` | POST | Clear session (204) |
| `/auth/session` | GET | Current session or 401 |
| `/client-metadata.json` | GET | OAuth client metadata |

## Production Setup

For production, you need a publicly accessible `client_id` URL. Use a tunneling service like ngrok for development:

```bash
ngrok http 3000
```

Then update `atrg.toml`:

```toml
[auth]
client_id = "https://your-domain.ngrok.io/client-metadata.json"
redirect_uri = "https://your-domain.ngrok.io/auth/callback"
```
