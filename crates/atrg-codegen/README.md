# atrg-codegen

**Lexicon-driven Rust code generation for AT Protocol applications.**

Part of [at-rust-go (atrg)](https://github.com/tellmeY18/at-rust-go) — a batteries-included AT Protocol backend framework for Rust.

## What this crate provides

This crate takes AT Protocol lexicon JSON files and generates typed Rust code. **It ships zero lexicon files** — only the generator tooling.

- **`generate(input_dir, output_dir, opts)`** — walks a directory of `.json` lexicon files and emits Rust source to the output directory
- **`GenOptions`** — controls what gets generated (handler stubs, route wiring)
- **`GenReport`** — summary of a generation run (files processed, types/stubs generated, output paths)

### Generated output includes

- Strongly-typed `serde`-derived structs for records, objects, params, and outputs
- Axum handler stubs with correct input/output types returning `Result<Json<Output>, XrpcError>`
- A `xrpc_routes()` function wiring all generated handlers into an Axum router
- AT-URI helper functions for each record type

## Usage

```toml
[dependencies]
atrg-codegen = "0.1"
```

```rust
use std::path::Path;
use atrg_codegen::{generate, GenOptions};

fn main() -> anyhow::Result<()> {
    let report = generate(
        Path::new("lexicons/"),
        Path::new("src/generated/"),
        GenOptions::default(),
    )?;

    println!(
        "Processed {} files, generated {} types and {} stubs",
        report.files_processed,
        report.types_generated,
        report.stubs_generated,
    );

    for path in &report.output_files {
        println!("  wrote {}", path);
    }

    Ok(())
}
```

### Skipping handler stubs

If you only want type definitions without Axum route stubs:

```rust
let opts = GenOptions {
    generate_stubs: false,
    generate_routes: false,
    ..Default::default()
};

let report = generate(
    Path::new("lexicons/"),
    Path::new("src/generated/"),
    opts,
)?;
```

### CLI usage

The `atrg` CLI wraps this crate:

```bash
atrg generate lexicons/
```

This writes generated code to `src/generated/` by default.

## How it works

1. Walks `input_dir` for `*.json` files
2. Parses each file as an AT Protocol lexicon via `atproto-lexicon`
3. Generates Rust types for every `record`, `object`, `query`, and `procedure` definition
4. Optionally generates Axum handler stubs and a route-wiring function
5. Writes formatted Rust source files to `output_dir`

Generated code lives in **your** project — atrg's published crates never embed any specific lexicon's output.

## License

LGPL-3.0-only — see [LICENSE](https://github.com/tellmeY18/at-rust-go/blob/main/LICENSE).