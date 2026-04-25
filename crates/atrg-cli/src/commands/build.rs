//! `atrg build` — release build wrapper with timing.

use std::process::Command;
use std::time::Instant;

/// Run `cargo build --release` and report the result with timing.
pub fn run() -> anyhow::Result<()> {
    println!("Building for release...");
    let start = Instant::now();

    let status = Command::new("cargo")
        .args(["build", "--release"])
        .status()?;

    let elapsed = start.elapsed();

    if !status.success() {
        anyhow::bail!("Build failed with status: {}", status);
    }

    println!();
    println!(
        "  ✓ Release build complete in {:.1}s",
        elapsed.as_secs_f64()
    );

    // Try to find the binary
    if let Ok(metadata) = Command::new("cargo")
        .args(["metadata", "--format-version", "1", "--no-deps"])
        .output()
    {
        if let Ok(out) = String::from_utf8(metadata.stdout) {
            if let Some(name) = out
                .lines()
                .find(|l| l.contains("\"name\""))
                .and_then(|l| l.split('"').nth(3))
            {
                println!("  Binary: target/release/{}", name);
            }
        }
    }

    Ok(())
}
