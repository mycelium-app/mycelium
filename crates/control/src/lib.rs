#![recursion_limit = "512"]

mod account_usage;
mod activity;
mod api_access;
mod artifacts;
mod attachments;
mod auth;
mod boot;
mod chat_import;
mod chat_tool_servers;
mod chat_tools;
mod chats;
mod config;
mod conversations;
mod git;
mod goals;
mod groups;
mod device;
mod error;
mod forge_cache;
mod forges;
mod imports;
mod instruction_proposals;
mod instructions;
mod issues;
mod link;
mod mcp;
mod mcp_client;
mod memories;
mod message_proposals;
mod model_providers;
mod moves;
mod nodes;
mod projects;
mod pricing;
mod prompts;
mod rate_limit;
mod schedules;
mod search;
mod sessions;
mod state;
mod store;
mod stream;
mod tasks;
mod terminals;
mod transcripts;
mod usage_limits;
mod versions;
mod walkthrough_diff;
mod walkthroughs;
mod web;
mod worktrees;

pub use forge_cache::FORGE_CACHE_TTL;
pub use config::{Cidr, Config, DEVICE_CODE_LIFETIME, OidcConfig, SCHEDULER_TICK, VERSION_CHECK_INTERVAL};

use anyhow::Result;
use axum::Router;
use axum::extract::DefaultBodyLimit;
use axum::routing::{any, delete, get, patch, post, put};
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
    tokio::spawn(versions::refresh_regularly(state.clone()));
    tokio::spawn(schedules::run_regularly(state.clone()));
    tokio::spawn(usage_limits::resume_regularly(state.clone()));
    axum::serve(listener, router(state).into_make_service_with_connect_info::<SocketAddr>()).await?;

    Ok(())
}

fn router(state: AppState) -> Router {
    let web_dir = state.config.web_dir.clone();

    Router::new()
        .route("/healthz", get(boot::healthz))
        .route("/readyz", get(boot::readyz))
        .route("/api/v1/version", get(boot::version))
        .route("/auth/login", get(auth::login))
        .route("/auth/callback", get(auth::callback))
        .route("/auth/logout", post(auth::logout))
        .route("/api/v1/me", get(auth::me))
        .route("/api/v1/activity", get(activity::get))
        .route("/api/v1/activity/conversations", get(activity::per_conversation))
        .route("/api/v1/sessions", get(sessions::list))
        .route("/api/v1/sessions/{id}", delete(sessions::end))
        .route("/api/v1/device", post(device::start))
        .route("/api/v1/device/token", post(device::token))
        .route("/api/v1/device/{user_code}", get(device::pending))
        .route("/api/v1/device/{user_code}/approve", post(device::approve))
        .route("/api/v1/device/{user_code}/deny", post(device::deny))
        .route("/api/v1/nodes", get(nodes::list))
        .route("/api/v1/nodes/{id}", patch(nodes::update).delete(nodes::revoke))
        .route("/api/v1/nodes/{id}/drain", post(nodes::drain))
        .route("/api/v1/nodes/{id}/instructions/take-over", post(instructions::take_over))
        .route("/api/v1/nodes/{id}/watch", post(nodes::watch))
        .route("/api/v1/nodes/{id}/processes/{pid}/stop", post(nodes::stop_process))
        .route("/api/v1/nodes/{id}/logins", post(nodes::start_login))
        .route("/api/v1/logins/{login_id}/input", post(nodes::login_input))
        .route("/api/v1/logins/{login_id}", delete(nodes::stop_login))
        .route("/api/v1/conversations", get(conversations::list).post(conversations::create))
        .route("/api/v1/conversations/{id}", get(conversations::get).patch(conversations::patch).delete(conversations::delete))
        .route("/api/v1/conversations/{id}/close", post(conversations::close))
        .route("/api/v1/conversations/{id}/opened", post(conversations::opened))
        .route("/api/v1/conversations/{id}/side", post(conversations::side))
        .route("/api/v1/conversations/{id}/goal", get(goals::get).put(goals::set).patch(goals::patch).delete(goals::clear))
        .route("/api/v1/conversations/{id}/walkthrough", get(walkthroughs::get))
        .route("/api/v1/walkthroughs/{id}/uncovered", get(walkthroughs::uncovered))
        .route("/api/v1/walkthroughs/{id}/notes", get(walkthroughs::notes).post(walkthroughs::add_note))
        .route("/api/v1/walkthroughs/{id}/notes/sent", post(walkthroughs::mark_notes_sent))
        .route("/api/v1/walkthroughs/{id}/notes/{note_id}", delete(walkthroughs::remove_note))
        .route("/api/v1/walkthroughs/{id}/reads", get(walkthroughs::reads))
        .route("/api/v1/walkthroughs/{id}/reads/{step}", put(walkthroughs::mark_read))
        .route("/api/v1/walkthroughs/{id}/dismissal", put(walkthroughs::dismiss).delete(walkthroughs::reopen))
        .route("/api/v1/conversations/{id}/move-plan", get(moves::plan))
        .route("/api/v1/conversations/{id}/move", get(moves::current).post(moves::request).delete(moves::cancel))
        .route("/api/v1/conversations/{id}/events", get(conversations::events))
        .route("/api/v1/conversations/{id}/session", get(conversations::session))
        .route("/api/v1/conversations/{id}/files", get(conversations::files))
        .route("/api/v1/conversations/{id}/git/status", get(git::status))
        .route("/api/v1/conversations/{id}/git/log", get(git::log))
        .route("/api/v1/conversations/{id}/git/graph", get(git::graph))
        .route("/api/v1/conversations/{id}/git/show", get(git::show))
        .route("/api/v1/conversations/{id}/git/branches", get(git::branches))
        .route("/api/v1/conversations/{id}/git/stack", get(git::stack))
        .route("/api/v1/conversations/{id}/git/switch", post(git::switch))
        .route("/api/v1/conversations/{id}/git/diff", get(git::diff))
        .route("/api/v1/conversations/{id}/git/changes-layout", get(git::changes_layout))
        .route("/api/v1/conversations/{id}/git/changes-rows", post(git::changes_rows))
        .route("/api/v1/conversations/{id}/git/stage", post(git::stage))
        .route("/api/v1/conversations/{id}/git/unstage", post(git::unstage))
        .route("/api/v1/conversations/{id}/git/stage-all", post(git::stage_all))
        .route("/api/v1/conversations/{id}/git/commit", post(git::commit))
        .route("/api/v1/conversations/{id}/git/fetch", post(git::fetch))
        .route("/api/v1/conversations/{id}/git/pull", post(git::pull))
        .route("/api/v1/conversations/{id}/git/push", post(git::push))
        .route("/api/v1/conversations/{id}/git/undo", post(git::undo))
        .route("/api/v1/conversations/{id}/git/town/sync", post(git::town_sync))
        .route("/api/v1/conversations/{id}/git/town/propose", post(git::town_propose))
        .route("/api/v1/conversations/{id}/terminal", post(terminals::start))
        .route("/api/v1/terminals/{terminal_id}/input", post(terminals::input))
        .route("/api/v1/terminals/{terminal_id}/resize", post(terminals::resize))
        .route("/api/v1/terminals/{terminal_id}", delete(terminals::stop))
        .route("/api/v1/conversations/{id}/attachments", post(attachments::upload).layer(DefaultBodyLimit::max(attachments::MAX_ATTACHMENT_BYTES + 1024)))
        .route("/api/v1/attachments/{sha256}", get(attachments::get))
        .route("/api/v1/artifacts", get(artifacts::list).post(artifacts::create).layer(DefaultBodyLimit::max(artifacts::MAX_REQUEST_BYTES)))
        .route("/api/v1/artifacts/{id}", get(artifacts::get).patch(artifacts::patch).delete(artifacts::delete))
        .route("/api/v1/artifacts/{id}/versions", post(artifacts::create_version).layer(DefaultBodyLimit::max(artifacts::MAX_REQUEST_BYTES)))
        .route("/api/v1/artifacts/{id}/versions/{version}/content", get(artifacts::content))
        .route("/api/v1/artifacts/{id}/comments", get(artifacts::comments).post(artifacts::create_comment))
        .route("/api/v1/artifacts/{id}/comments/{comment_id}", patch(artifacts::patch_comment))
        .route("/api/v1/artifacts/{id}/data", get(artifacts::data))
        .route("/api/v1/artifacts/{id}/data/{key}", put(artifacts::replace_data))
        .route("/internal/artifacts/by-slug/{slug}", get(artifacts::host_artifact))
        .route("/internal/artifacts/by-slug/{slug}/content", get(artifacts::host_content))
        .route("/internal/artifacts/by-slug/{slug}/data", get(artifacts::host_data))
        .route("/internal/artifacts/by-slug/{slug}/data/{key}", put(artifacts::host_replace_data))
        .route("/design", get(artifacts::editor))
        .route("/design/", get(artifacts::editor))
        .route("/design/{*path}", get(artifacts::editor))
        .route("/api/v1/conversations/{id}/prompt", post(prompts::prompt))
        .route("/api/v1/conversations/{id}/queue", get(prompts::queue))
        .route("/api/v1/conversations/{id}/queue/{queue_id}", patch(prompts::edit_queued).delete(prompts::remove_queued))
        .route("/api/v1/conversations/{id}/queue/{queue_id}/send-now", post(prompts::send_now))
        .route("/api/v1/conversations/{id}/retry", post(prompts::retry))
        .route("/api/v1/conversations/{id}/resume", post(usage_limits::resume).delete(usage_limits::cancel_resume))
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
        .route("/api/v1/projects/{id}", patch(projects::patch))
        .route("/api/v1/projects/{id}/avatar", post(projects::upload_avatar).layer(DefaultBodyLimit::max(projects::MAX_AVATAR_BYTES + 1024)))
        .route("/api/v1/model-providers", get(model_providers::list).post(model_providers::create))
        .route("/api/v1/model-providers/{id}", delete(model_providers::delete))
        .route("/api/v1/model-providers/{id}/refresh", post(model_providers::refresh))
        .route("/api/v1/chat-tool-servers", get(chat_tool_servers::list).post(chat_tool_servers::create))
        .route("/api/v1/chat-tool-servers/{id}", patch(chat_tool_servers::patch).delete(chat_tool_servers::delete))
        .route("/api/v1/chat-tool-servers/{id}/refresh", post(chat_tool_servers::refresh))
        .route("/api/v1/chats", post(chats::create))
        .route("/api/v1/chats/import", post(chat_import::create).layer(DefaultBodyLimit::max(chat_import::MAX_EXPORT_BYTES)))
        .route("/api/v1/groups", get(groups::list).post(groups::create))
        .route("/api/v1/groups/{id}", patch(groups::rename).delete(groups::delete))
        .route("/api/v1/groups/{id}/folders", post(groups::add_folder))
        .route("/api/v1/groups/{id}/folders/{folder_id}", delete(groups::remove_folder))
        .route("/api/v1/instructions", get(instructions::get))
        .route("/api/v1/instructions/pull", post(instructions::pull))
        .route("/api/v1/instructions/{layer}", put(instructions::save))
        .route("/api/v1/instruction-proposals", get(instruction_proposals::list))
        .route("/api/v1/instruction-proposals/{id}/approve", post(instruction_proposals::approve))
        .route("/api/v1/instruction-proposals/{id}/decline", post(instruction_proposals::decline))
        .route("/api/v1/message-proposals", get(message_proposals::list))
        .route("/api/v1/message-proposals/{id}/approve", post(message_proposals::approve))
        .route("/api/v1/message-proposals/{id}/decline", post(message_proposals::decline))
        .route("/api/v1/api-access-requests", get(api_access::list))
        .route("/api/v1/api-access-requests/{id}/approve", post(api_access::approve))
        .route("/api/v1/api-access-requests/{id}/decline", post(api_access::decline))
        .route("/api/v1/instruction-source", put(instructions::set_source).delete(instructions::clear_source))
        .route("/api/v1/extensions", get(instructions::get_extensions).put(instructions::save_extensions))
        .route("/api/v1/usage-limit", get(usage_limits::get_setting).put(usage_limits::save_setting))
        .route("/api/v1/worktrees", get(worktrees::list))
        .route("/api/v1/tasks", get(tasks::list).post(tasks::create))
        .route("/api/v1/tasks/{id}", delete(tasks::delete))
        .route("/api/v1/tasks/{id}/start", post(tasks::start))
        .route("/api/v1/tasks/{id}/approve", post(tasks::approve))
        .route("/api/v1/tasks/{id}/decline", post(tasks::decline))
        .route("/api/v1/tasks/{id}/answer", post(tasks::answer))
        .route("/api/v1/tasks/{id}/done", post(tasks::done))
        .route("/api/v1/schedules", get(schedules::list).post(schedules::create))
        .route("/api/v1/schedules/{id}", patch(schedules::patch).delete(schedules::delete))
        .route("/api/v1/schedules/{id}/run", post(schedules::run_now))
        .route("/api/v1/schedules/{id}/confirm", post(schedules::confirm))
        .route("/api/v1/schedules/{id}/decline", post(schedules::decline))
        .route("/api/v1/me/time-zone", put(schedules::set_time_zone))
        .route("/api/v1/schedule-runs", get(schedules::runs))
        .route("/api/v1/issues", get(issues::list))
        .route("/api/v1/projects/{id}/issues/{number}/workspace", post(issues::start_workspace))
        .route("/api/v1/imports", get(imports::list).post(imports::create))
        .route("/api/v1/imports/jobs", get(imports::jobs))
        .route("/api/v1/indexing", get(search::indexing))
        .route("/api/v1/search", get(search::search))
        .route("/api/v1/stream", get(stream::connect))
        .route("/api/v1/node/connect", get(link::connect))
        .route("/api/{*unknown}", any(web::unknown_api))
        .route_layer(axum::middleware::from_fn_with_state(state.clone(), api_access::gate_scoped_tokens))
        .fallback_service(web::single_page_app(&web_dir))
        .with_state(state)
}
