//! `atrg routes` — print registered routes.
//!
//! Lists the built-in routes that atrg provides plus a note about user routes.

/// Run the `atrg routes` command.
pub fn run() -> anyhow::Result<()> {
    println!("Built-in routes:");
    println!();
    println!("  {:<8} {:<45} {}", "METHOD", "PATH", "DESCRIPTION");
    println!("  {:<8} {:<45} {}", "------", "----", "-----------");
    println!("  {:<8} {:<45} {}", "GET", "/healthz", "Liveness probe");
    println!(
        "  {:<8} {:<45} {}",
        "GET", "/readyz", "Readiness probe (DB + cache metrics)"
    );
    println!(
        "  {:<8} {:<45} {}",
        "GET", "/auth/login?handle=...", "Initiate OAuth login"
    );
    println!(
        "  {:<8} {:<45} {}",
        "GET", "/auth/callback", "OAuth callback"
    );
    println!("  {:<8} {:<45} {}", "POST", "/auth/logout", "Clear session");
    println!(
        "  {:<8} {:<45} {}",
        "GET", "/auth/session", "Current session info"
    );
    println!(
        "  {:<8} {:<45} {}",
        "GET", "/client-metadata.json", "OAuth client metadata"
    );
    println!(
        "  {:<8} {:<45} {}",
        "GET", "/.well-known/oauth-protected-resource", "OAuth resource metadata"
    );
    println!();
    println!("User routes are defined in src/routes.rs. Mount them via AtrgApp::mount().");
    println!("XRPC routes under /xrpc/* use the AT Protocol error envelope automatically.");
    Ok(())
}
