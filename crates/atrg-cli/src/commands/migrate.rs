//! `atrg migrate` — run pending database migrations.

use std::path::Path;

/// Run all pending migrations (internal atrg tables first, then user-supplied).
pub async fn run() -> anyhow::Result<()> {
    let config = atrg_core::Config::load("atrg.toml")?;
    let pool = atrg_db::connect(&config.database.url).await?;

    atrg_db::run_internal_migrations(&pool).await?;
    atrg_db::run_user_migrations(&pool, Path::new("./migrations")).await?;

    println!("✓ Migrations complete");
    Ok(())
}
