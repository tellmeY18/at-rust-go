# atrg-cli

**CLI for at-rust-go: scaffold, develop, migrate, and build AT Protocol API projects.**

Part of [at-rust-go (atrg)](https://github.com/tellmeY18/at-rust-go) — a batteries-included AT Protocol backend framework for Rust.

## What this crate provides

- **`atrg new <name>`** — Scaffold a new atrg API project with config, routes, and migrations ready to go. Supports `--template` (e.g. `basic`, `social`) and `--force` to overwrite.
- **`atrg dev`** — Start a dev server with automatic file watching via `cargo-watch`.
- **`atrg migrate`** — Run pending SQLite database migrations.
- **`atrg routes`** — Print all registered routes in the project.
- **`atrg build`** — Build the project for release (`cargo build --release`).
- **`atrg generate`** — Generate Rust types and XRPC route stubs from your lexicon JSON files. Accepts `--input` and `--output` directory flags.
- **`atrg version`** — Print the current atrg version.

## Installation

```sh
cargo install atrg-cli
```

## Quick Start

```sh
atrg new my-app
cd my-app
atrg dev
```

That's it — a running AT Protocol API server with OAuth, Jetstream, and XRPC support in under five minutes. Bring your own frontend.

## Scaffold Output

`atrg new my-app` generates:

```text
my-app/
├── Cargo.toml           # workspace pulling in atrg crates
├── rust-toolchain.toml  # pinned to stable
├── atrg.toml            # app config with sensible defaults
├── src/
│   ├── main.rs          # ~5 lines — calls AtrgApp::new().run()
│   └── routes.rs        # example JSON API handlers
└── migrations/
    └── .gitkeep
```

No `templates/` or `static/` directory — atrg projects are pure API backends.

## Lexicon Code Generation

Point `atrg generate` at a directory of `.json` lexicon files to emit typed Rust structs, validators, and Axum route stubs:

```sh
atrg generate --input lexicons/ --output src/generated/
```

The generated code lives in *your* project — atrg never bundles any specific lexicon.

## Commands Reference

| Command | Description |
|---------|-------------|
| `atrg new <name>` | Scaffold a new project (`--template`, `--path`, `--force`) |
| `atrg dev` | Dev server with file watching |
| `atrg migrate` | Run pending DB migrations |
| `atrg routes` | List registered routes |
| `atrg build` | Release build |
| `atrg generate` | Lexicon → Rust codegen (`--input`, `--output`) |
| `atrg version` | Print version |

## Requirements

- Rust stable toolchain
- `cargo-watch` (for `atrg dev` — installed automatically if missing)
- SQLite (bundled via `sqlx`)

## Part of the atrg Workspace

This crate is the developer-facing entry point for the framework. It depends on:

- [`atrg-core`](../atrg-core) — AppState, config, app builder
- [`atrg-db`](../atrg-db) — database migrations
- [`atrg-codegen`](../atrg-codegen) — lexicon-driven code generation

## License

LGPL-3.0-only — see [LICENSE](https://github.com/tellmeY18/at-rust-go/blob/main/LICENSE).