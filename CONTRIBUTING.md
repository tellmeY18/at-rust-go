# Contributing to at-rust-go

Thank you for considering contributing to atrg! This document covers the basics.

## Development Setup

```bash
git clone https://github.com/tellmeY18/at-rust-go.git
cd at-rust-go
cargo build --workspace
cargo test --workspace
```

## Before Submitting a PR

1. **Run the full check suite:**
   ```bash
   cargo fmt --all -- --check
   cargo clippy --workspace --all-targets -- -D warnings
   cargo test --workspace
   ```

2. **Follow the coding standards:**
   - No `unwrap()` in library code — use `?` and `AtrgError`
   - `unwrap()` in tests is fine
   - All public types get doc comments
   - Keep files under ~300 lines
   - Prefer `Arc<T>` over `Mutex<T>` for shared read-only state

3. **Update the roadmap:** Check off completed items in `ROADMAP.md`.

4. **No frontend code.** atrg is API-only. No HTML, no templates, no static assets.

5. **No bundled lexicons.** atrg is a transport-layer framework. Lexicons belong to the developer's project.

## Non-Trivial Changes

Features that touch the public API, add a crate, or change auth/XRPC semantics require a lightweight RFC:
- Create `docs/rfcs/NNNN-title.md` describing the proposal
- PRs without an RFC for these categories will be asked to file one

## License

Contributions are accepted under the LGPL-3.0-only license.