//! Social API scaffold example — a micro-social-network API on AT Protocol.

use atrg_core::AtrgApp;

mod handlers;
mod routes;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    AtrgApp::new()
        .with_auth_routes(atrg_auth::routes::auth_router())
        .with_cleanup_task(atrg_auth::routes::spawn_cleanup_task)
        .mount(routes::api())
        .run()
        .await
}
