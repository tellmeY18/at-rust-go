# Changelog

All notable changes to at-rust-go (atrg) will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.1.0] — Unreleased

### Added

#### Core (`atrg-core`)
- `AtrgApp` builder with `new()`, `mount()`, `with_auth_routes()`, `with_cleanup_task()`, `on_event()`, and `run()`
- `Config` loader from `atrg.toml` with validation and sane defaults
- `AppState` with config, database pool, HTTP client, and identity resolver
- `AtrgError` enum with JSON `IntoResponse` (400/401/404/500)
- CORS layer builder from config
- Cursor-based pagination helpers (`encode_cursor`, `decode_cursor`, `PaginationParams`)
- Security headers middleware (production mode)
- Request ID middleware (`X-Request-Id`)
- Built-in `/healthz` and `/readyz` endpoints

#### Auth (`atrg-auth`)
- OAuth login/callback/logout routes
- `AuthUser` (optional) and `RequireAuth` (strict) Axum extractors
- Session management with SQLite storage
- AT Protocol JWT claim parsing and verification
- Cookie + Bearer token dual authentication path
- Periodic session cleanup task

#### Streaming (`atrg-stream`)
- Jetstream WebSocket consumer with automatic reconnection
- Bounded backpressure via configurable `mpsc` channel
- Lag detection and event dropping at threshold
- Exponential backoff (1s → 60s cap)
- ZSTD dictionary auto-fetch with disk caching
- Atomic metrics counters

#### XRPC (`atrg-xrpc`)
- `xrpc_router()` factory with 501 fallback for unmatched methods
- `XrpcError` type implementing the AT Protocol error envelope
- All 7 error variants: InvalidRequest, AuthRequired, Forbidden, NotFound, RateLimitExceeded, InternalServerError, MethodNotImplemented
- `From<AtrgError>` conversion for seamless error propagation

#### Database (`atrg-db`)
- SQLite connection pool with WAL mode and foreign keys
- Internal migration runner (sessions, OAuth states)
- User migration runner from project `migrations/` directory

#### Identity (`atrg-identity`)
- `IdentityResolver` with `moka` TTL-backed cache
- DID resolution (did:plc via PLC directory, did:web)
- Handle resolution
- Cache metrics (hits, misses, entry count)

#### Code Generation (`atrg-codegen`)
- Lexicon JSON parser with validation
- Rust struct generation from record/object/query/procedure definitions
- XRPC route stub generation
- `prettyplease`-formatted output

#### Testing (`atrg-testing`)
- `test_state()` and `test_app()` builders with in-memory SQLite
- `seed_session()` for authenticated handler tests
- `MockAtprotoClient` with call recording and response scripting
- `FakeJetstream` with synthetic event emission

#### CLI (`atrg-cli`)
- `atrg new <name>` — project scaffolding
- `atrg dev` — development server with cargo-watch
- `atrg migrate` — database migration runner
- `atrg routes` — route listing
- `atrg build` — release build wrapper