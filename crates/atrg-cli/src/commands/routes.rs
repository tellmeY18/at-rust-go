//! `atrg routes` — print registered routes.
//!
//! Lists the built-in routes that atrg provides plus a note about user routes.

/// Print a row with fixed-width columns.
fn print_row(method: &str, path: &str, description: &str) {
    println!("  {method:<8} {path:<45} {description}");
}

/// Run the `atrg routes` command.
pub fn run() -> anyhow::Result<()> {
    println!("Built-in routes:");
    println!();
    print_row("METHOD", "PATH", "DESCRIPTION");
    print_row("------", "----", "-----------");
    print_row("GET", "/healthz", "Liveness probe");
    print_row("GET", "/readyz", "Readiness probe (DB + cache metrics)");
    print_row("GET", "/auth/login?handle=...", "Initiate OAuth login");
    print_row("GET", "/auth/callback", "OAuth callback");
    print_row("POST", "/auth/logout", "Clear session");
    print_row("GET", "/auth/session", "Current session info");
    print_row("GET", "/client-metadata.json", "OAuth client metadata");
    print_row(
        "GET",
        "/.well-known/oauth-protected-resource",
        "OAuth resource metadata",
    );
    println!();
    println!("User routes are defined in src/routes.rs. Mount them via AtrgApp::mount().");
    println!("XRPC routes under /xrpc/* use the AT Protocol error envelope automatically.");
    Ok(())
}
