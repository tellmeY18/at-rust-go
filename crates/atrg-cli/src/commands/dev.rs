//! `atrg dev` — start the development server with optional file watching.
//!
//! Tries `cargo-watch` first for automatic reloads on file changes.
//! Falls back to a plain `cargo run` if `cargo-watch` is not installed.

use std::process::Command;

/// Run the dev server.
///
/// Sets `ATRG_ENV=development` and a sensible default `RUST_LOG` filter
/// before launching the underlying cargo command.
pub fn run() -> anyhow::Result<()> {
    // Safety: these env vars are set before spawning any threads.
    // The child process inherits them explicitly via `.env()` anyway,
    // so the ambient set_var is only for diagnostics printed here.
    #[allow(deprecated)]
    {
        std::env::set_var("ATRG_ENV", "development");
    }
    if std::env::var("RUST_LOG").is_err() {
        #[allow(deprecated)]
        {
            std::env::set_var("RUST_LOG", "info,atrg=debug,tower_http=debug");
        }
    }

    let has_watch = Command::new("cargo")
        .args(["watch", "--version"])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false);

    let status = if has_watch {
        println!("Starting dev server with cargo-watch...");
        Command::new("cargo")
            .args(["watch", "-x", "run"])
            .env("ATRG_ENV", "development")
            .status()?
    } else {
        println!("cargo-watch not found. Install with: cargo install cargo-watch");
        println!("Starting dev server with cargo run...");
        Command::new("cargo")
            .arg("run")
            .env("ATRG_ENV", "development")
            .status()?
    };

    if !status.success() {
        anyhow::bail!("Dev server exited with status: {}", status);
    }
    Ok(())
}
