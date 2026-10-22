mod auth;
mod boot;
mod config;
mod conversations;
mod git;
mod device;
mod error;
mod forges;
mod imports;
mod link;
mod mcp;
mod memories;
mod nodes;
mod projects;
mod prompts;
mod rate_limit;
mod search;
mod state;
mod store;
mod stream;
mod terminals;
mod transcripts;
mod web;
mod worktrees;

pub use config::{Cidr, Config, DEVICE_CODE_LIFETIME, OidcConfig};

use anyhow::Result;
use axum::Router;
use axum::routing::{any, delete, get, patch, post};
use state::AppState;
use std::net::SocketAddr;
use tokio::net::TcpListener;
use tracing_subscriber::EnvFilter;

pub async fn serve() -> Result<()> {
    tracing_subscriber::fmt().with_env_filter(EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"))).init();

    let config = Config::from_env()?;
    let listener = TcpListener::bind(config.listen).await?;
    tracing::info!("control plane listening on {}", config.listen);

    run(config, listener).await
}

pub async fn run(config: Config, listener: TcpListener) -> Result<()> {
    if config.oidc.is_some() && config.allowed_emails.is_empty() {
        tracing::warn!("MYCELIUM_ALLOWED_EMAILS is empty, so every account at the identity provider can sign in and run agents on every node.");
    }

    let state = boot::new_state(config)?;

    tokio::spawn(boot::migrate_until_ready(state.clone()));
    axum::serve(listener, router(state).into_make_service_with_connect_info::<SocketAddr>()).await?;

    Ok(())
}

fn router(state: AppState) -> Router {
    let web_dir = state.config.web_dir.clone();

    Router::new()
        .route("/healthz", get(boot::healthz))
        .route("/readyz", get(boot::readyz))
        .route("/auth/login", get(auth::login))
        .route("/auth/callback", get(auth::callback))
        .route("/auth/logout", post(auth::logout))
        .route("/api/v1/me", get(auth::me))
        .route("/api/v1/device", post(device::start))
        .route("/api/v1/device/token", post(device::token))
        .route("/api/v1/device/{user_code}", get(device::pending))
        .route("/api/v1/device/{user_code}/approve", post(device::approve))
        .route("/api/v1/device/{user_code}/deny", post(device::deny))
        .route("/api/v1/nodes", get(nodes::list))
        .route("/api/v1/nodes/{id}", patch(nodes::update).delete(nodes::revoke))
        .route("/api/v1/nodes/{id}/drain", post(nodes::drain))
        .route("/api/v1/nodes/{id}/logins", post(nodes::start_login))
        .route("/api/v1/logins/{login_id}/input", post(nodes::login_input))
        .route("/api/v1/logins/{login_id}", delete(nodes::stop_login))
        .route("/api/v1/conversations", get(conversations::list).post(conversations::create))
        .route("/api/v1/conversations/{id}", get(conversations::get).patch(conversations::patch))
        .route("/api/v1/conversations/{id}/opened", post(conversations::opened))
        .route("/api/v1/conversations/{id}/events", get(conversations::events))
        .route("/api/v1/conversations/{id}/files", get(conversations::files))
        .route("/api/v1/conversations/{id}/git/status", get(git::status))
        .route("/api/v1/conversations/{id}/git/log", get(git::log))
        .route("/api/v1/conversations/{id}/git/diff", get(git::diff))
        .route("/api/v1/conversations/{id}/git/stage", post(git::stage))
        .route("/api/v1/conversations/{id}/git/unstage", post(git::unstage))
        .route("/api/v1/conversations/{id}/git/stage-all", post(git::stage_all))
        .route("/api/v1/conversations/{id}/git/commit", post(git::commit))
        .route("/api/v1/conversations/{id}/git/fetch", post(git::fetch))
        .route("/api/v1/conversations/{id}/git/undo", post(git::undo))
        .route("/api/v1/conversations/{id}/terminal", post(terminals::start))
        .route("/api/v1/terminals/{terminal_id}/input", post(terminals::input))
        .route("/api/v1/terminals/{terminal_id}/resize", post(terminals::resize))
        .route("/api/v1/terminals/{terminal_id}", delete(terminals::stop))
        .route("/api/v1/conversations/{id}/prompt", post(prompts::prompt))
        .route("/api/v1/conversations/{id}/queue", get(prompts::queue))
        .route("/api/v1/conversations/{id}/queue/{queue_id}", patch(prompts::edit_queued).delete(prompts::remove_queued))
        .route("/api/v1/conversations/{id}/queue/{queue_id}/send-now", post(prompts::send_now))
        .route("/api/v1/conversations/{id}/cancel", post(prompts::cancel))
        .route("/api/v1/conversations/{id}/permission", post(prompts::permission))
        .route("/api/v1/conversations/{id}/config", post(prompts::config))
        .route("/api/v1/memories", get(memories::list))
        .route("/api/v1/memories/{id}", patch(memories::update).delete(memories::delete))
        .route("/api/v1/mcp", post(mcp::handle))
        .route("/api/v1/forges", get(forges::list).post(forges::connect))
        .route("/api/v1/forges/{id}", delete(forges::disconnect))
        .route("/api/v1/repositories", get(forges::repositories))
        .route("/api/v1/projects", get(projects::list).post(projects::create))
        .route("/api/v1/worktrees", get(worktrees::list))
        .route("/api/v1/imports", get(imports::list).post(imports::create))
        .route("/api/v1/indexing", get(search::indexing))
        .route("/api/v1/search", get(search::search))
        .route("/api/v1/stream", get(stream::connect))
        .route("/api/v1/node/connect", get(link::connect))
        .route("/api/{*unknown}", any(web::unknown_api))
        .fallback_service(web::single_page_app(&web_dir))
        .with_state(state)
}
