# Changelog

All notable changes to at-rust-go are documented in this file.

## [0.1.1] — 2026-04-26

### Fixed
- Corrected license to LGPL-3.0-only across all Cargo.toml and documentation (was incorrectly MIT OR Apache-2.0).
- Fixed `unwrap()` in doc example in `atrg-repo`.

### Added
- README.md for all 13 crates (displayed on crates.io).
- `homepage` field in workspace Cargo.toml.
- `readme` field in all crate Cargo.toml files.

## [0.1.0] — 2026-04-26

### Added

#### Core Framework
- `atrg-core`: AppState, Config (atrg.toml), AtrgApp builder, AtrgError, CORS, security headers, request IDs, health/readiness endpoints, pagination helpers.
- `atrg-db`: SQLite connection pool via sqlx, internal + user migration runner.
- `atrg-identity`: DID/handle resolution with moka TTL-backed cache.
- `atrg-cli`: `atrg new`, `atrg dev`, `atrg migrate`, `atrg routes`, `atrg build`, `atrg generate` commands.

#### Authentication
- `atrg-auth`: OAuth login/callback/logout routes, session management, AT Protocol JWT verification, `AuthUser`/`RequireAuth` extractors, transparent token refresh.

#### Real-time Events
- `atrg-stream`: Jetstream WebSocket consumer with bounded backpressure, lag detection, ZSTD dictionary support, automatic reconnection.
- `atrg-firehose`: Full relay firehose (`com.atproto.sync.subscribeRepos`) consumer with CAR v1 decoding, CBOR→JSON conversion, cursor tracking.

#### XRPC & Codegen
- `atrg-xrpc`: XRPC router factory, AT Protocol error envelope, helper constructors.
- `atrg-codegen`: Lexicon JSON → Rust types, validators, and XRPC route stubs.

#### Record Operations
- `atrg-repo`: Typed record CRUD (`get/list/create/put/delete`), blob uploads, AT-URI parsing/validation, TID generation.

#### Feed & Label Frameworks
- `atrg-feed`: Feed generator builder, `describeFeedGenerator` + `getFeedSkeleton` XRPC routes, multi-feed support.
- `atrg-label`: Label service (create/negate/query), SQLite-backed label store, placeholder signing, `queryLabels` XRPC route.

#### Production Hardening
- Graceful shutdown (SIGTERM/SIGINT handling, DB pool drain with timeout).
- Per-IP token-bucket rate limiting middleware.
- Environment variable overrides for all atrg.toml fields (`ATRG_APP__PORT`, etc.).

#### Testing
- `atrg-testing`: `test_state()` with in-memory SQLite, `seed_session()`, `MockAtprotoClient`, `FakeJetstream`.

#### CI/CD
- GitHub Actions: fmt, clippy, test, coverage (per-crate thresholds), E2E scaffold tests, codegen E2E, documentation build.
- GitHub Pages workflow for API docs, llms.txt, llms-full.txt.