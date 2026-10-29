#![recursion_limit = "512"]

const PROXY_BODY_LIMIT: usize = 25 * 1024 * 1024;

mod account_usage;
mod agent_settings;
mod routing;
mod subscription_prices;
mod activity;
mod activity_sweep;
mod api_access;
mod artifacts;
mod artifact_kinds;
mod dashboards;
mod typst;
mod artifact_access;
mod attachments;
mod auth;
mod boot;
mod outbound;
mod caldav;
mod coolify;
mod chat_import;
mod chat_tool_servers;
mod chat_tool_setup;
mod chat_tools;
mod chats;
mod config;
mod continuation;
mod conversations;
mod git;
mod goals;
mod live_usage;
mod assistant_actions;
mod assistant_changes;
mod assistant_backup;
mod assistants;
mod groups;
mod device;
mod error;
mod experiments;
mod file_arguments;
mod forge_cache;
mod forges;
mod imports;
mod learned;
mod instruction_proposals;
mod instructions;
mod email;
mod environments;
mod exercises;
mod integration_policy;
mod integration_proxy;
mod integrations;
mod issues;
mod link;
mod mcp;
mod mcp_client;
mod mcp_oauth;
mod mcp_tokens;
mod memories;
mod message_proposals;
mod conversation_connections;
mod group_connections;
mod decision_providers;
mod model_providers;
mod provider_usage;
mod puzzles;
mod moves;
mod node_updates;
mod node_pick;
mod nodes;
mod novnc;
mod projects;
mod pricing;
mod prompts;
mod rate_limit;
mod reflections;
mod rfb;
mod response_editor;
mod response_filter;
mod schedules;
mod search;
mod sessions;
mod skills;
mod snapshots;
mod open_outline;
mod ssh_access;
mod start_proposals;
mod state;
mod store;
mod stream;
mod tasks;
mod terminals;
mod token_delivery;
mod transcripts;
mod transcriptions;
mod usage_history;
mod usage_limits;
mod versions;
mod walkthrough_diff;
mod walkthroughs;
mod voice;
mod wellness;
mod web;
mod worktrees;
mod workflows;
mod lastfm;
mod freeagent;
mod discord;
mod actual;
mod paperless;
mod stoat;
mod tandoor;
mod zulip;

pub use forge_cache::FORGE_CACHE_TTL;
pub use config::{Cidr, Config, DEVICE_CODE_LIFETIME, OidcConfig, SCHEDULER_TICK, STEER_TIMEOUT, STREAM_RECHECK_INTERVAL, VERSION_CHECK_INTERVAL};

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
    tokio::spawn(workflows::run_regularly(state.clone()));
    tokio::spawn(usage_limits::resume_regularly(state.clone()));
    tokio::spawn(activity_sweep::sweep_regularly(state.clone()));
    tokio::spawn(goals::nudge_regularly(state.clone()));
    tokio::spawn(conversations::wake_regularly(state.clone()));
    tokio::spawn(wellness::announce_boundaries(state.clone()));
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
        .route("/integrations/oauth/callback", get(mcp_oauth::callback))
        .route("/integrations/lastfm/callback", get(lastfm::callback))
        .route("/integrations/freeagent/callback", get(freeagent::callback))
        .route("/novnc/{*path}", get(novnc::serve))
        .route("/auth/logout", post(auth::logout))
        .route("/api/v1/me", get(auth::me))
        .route("/api/v1/workflows", get(workflows::list).post(workflows::create))
        .route("/api/v1/workflows/{id}", get(workflows::get))
        .route("/api/v1/workflows/{id}/control", post(workflows::control))
        .route("/api/v1/workflows/{id}/rerun", post(workflows::rerun))
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
        .route("/api/v1/nodes/any", get(node_pick::preview))
        .route("/api/v1/nodes/{id}", patch(nodes::update).delete(nodes::revoke))
        .route("/api/v1/nodes/{id}/directories", get(nodes::directories))
        .route("/api/v1/nodes/{id}/drain", post(nodes::drain))
        .route("/api/v1/nodes/{id}/update", post(node_updates::update))
        .route("/api/v1/nodes/{id}/instructions/take-over", post(instructions::take_over))
        .route("/api/v1/nodes/{id}/ssh-tokens", post(ssh_access::mint_for_person))
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
        .route("/api/v1/conversations/{id}/usage", get(activity::for_conversation))
        .route("/api/v1/conversations/{id}/goal", get(goals::get).put(goals::set).patch(goals::patch).delete(goals::clear))
        .route("/api/v1/conversations/{id}/goal/send", post(goals::send_again))
        .route("/api/v1/conversations/{id}/goal/nudge", post(goals::nudge_now))
        .route("/api/v1/conversations/{id}/goal/continue", post(goals::continue_goal))
        .route("/api/v1/conversations/{id}/walkthrough", get(walkthroughs::get))
        .route("/api/v1/walkthroughs/{id}/uncovered", get(walkthroughs::uncovered))
        .route("/api/v1/walkthroughs/{id}/notes", get(walkthroughs::notes).post(walkthroughs::add_note))
        .route("/api/v1/walkthroughs/{id}/notes/sent", post(walkthroughs::mark_notes_sent))
        .route("/api/v1/walkthroughs/{id}/notes/{note_id}", delete(walkthroughs::remove_note))
        .route("/api/v1/walkthroughs/{id}/reads", get(walkthroughs::reads))
        .route("/api/v1/walkthroughs/{id}/reads/{step}", put(walkthroughs::mark_read))
        .route("/api/v1/walkthroughs/{id}/answers", get(walkthroughs::answers))
        .route("/api/v1/walkthroughs/{id}/answers/{step}", put(walkthroughs::answer))
        .route("/api/v1/walkthroughs/{id}/dismissal", put(walkthroughs::dismiss).delete(walkthroughs::reopen))
        .route("/api/v1/conversations/{id}/continue", post(continuation::request))
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
        .route("/api/v1/conversations/{id}/git/hunk", post(git::hunk))
        .route("/api/v1/conversations/{id}/git/file", post(git::file))
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
        .route("/api/v1/conversations/{id}/environment", post(environments::start).get(environments::get).delete(environments::stop))
        .route("/api/v1/conversations/{id}/environment/take-over", post(environments::take_over))
        .route("/api/v1/conversations/{id}/environment/hand-back", post(environments::hand_back))
        .route("/api/v1/conversations/{id}/environment/browser-task/stop", post(environments::stop_browser_task))
        .route("/api/v1/conversations/{id}/environment/apps", post(environments::open_app))
        .route("/api/v1/conversations/{id}/environment/windows/{window}/focus", post(environments::focus_window))
        .route("/api/v1/conversations/{id}/environment/actions", get(environments::actions))
        .route("/api/v1/conversations/{id}/environment/stream", get(environments::stream))
        .route("/api/v1/conversations/{id}/environment/stream-ticket", post(environments::stream_ticket))
        .route("/api/v1/nodes/{id}/environment/check", post(environments::check))
        .route("/api/v1/environment/last-node", get(environments::last_node))
        .route("/api/v1/conversations/{id}/attachments", post(attachments::upload).layer(DefaultBodyLimit::max(attachments::MAX_ATTACHMENT_BYTES + 1024)))
        .route("/api/v1/attachments/{sha256}", get(attachments::get))
        .route("/api/v1/artifacts", get(artifacts::list).post(artifacts::create).layer(DefaultBodyLimit::max(artifacts::MAX_REQUEST_BYTES)))
        .route("/api/v1/artifacts/{id}", get(artifacts::get).patch(artifacts::patch).delete(artifacts::delete))
        .route("/api/v1/artifacts/{id}/links", get(artifact_access::links).post(artifact_access::link))
        .route("/api/v1/artifacts/{id}/links/{link_id}", delete(artifact_access::unlink))
        .route("/api/v1/artifact-access-requests", get(artifact_access::list_requests))
        .route("/api/v1/artifact-access-requests/{id}/approve", post(artifact_access::approve))
        .route("/api/v1/artifact-access-requests/{id}/decline", post(artifact_access::decline))
        .route("/api/v1/artifacts/{id}/versions", post(artifacts::create_version).layer(DefaultBodyLimit::max(artifacts::MAX_REQUEST_BYTES)))
        .route("/api/v1/artifacts/{id}/versions/{version}/content", get(artifacts::content))
        .route("/api/v1/artifacts/{id}/thumbnail", get(artifacts::thumbnail))
        .route("/api/v1/typst/compile", post(typst::preview))
        .route("/api/v1/artifacts/{id}/typst", get(typst::latest))
        .route("/api/v1/artifacts/{id}/pdf", get(typst::pdf))
        .route("/api/v1/artifacts/{id}/panels/{panel}/data", get(dashboards::panel_data))
        .route("/api/v1/artifacts/{id}/comments", get(artifacts::comments).post(artifacts::create_comment))
        .route("/api/v1/artifacts/{id}/comments/{comment_id}", patch(artifacts::patch_comment))
        .route("/api/v1/artifacts/{id}/data", get(artifacts::data))
        .route("/api/v1/artifacts/{id}/data/{key}", put(artifacts::replace_data))
        .route("/internal/artifacts/by-slug/{slug}", get(artifacts::host_artifact))
        .route("/internal/artifacts/by-slug/{slug}/content", get(artifacts::host_content))
        .route("/internal/artifacts/by-slug/{slug}/versions", post(artifacts::host_create_version).layer(DefaultBodyLimit::max(attachments::MAX_ATTACHMENT_BYTES + 1024)))
        .route("/internal/artifacts/by-slug/{slug}/data", get(artifacts::host_data))
        .route("/internal/artifacts/by-slug/{slug}/data/{key}", put(artifacts::host_replace_data))
        .route("/design", get(artifacts::editor))
        .route("/design/", get(artifacts::editor))
        .route("/design/{*path}", get(artifacts::editor))
        .route("/api/v1/conversations/{id}/prompt", post(prompts::prompt))
        .route("/api/v1/conversations/{id}/queue", get(prompts::queue))
        .route("/api/v1/conversations/{id}/queue/{queue_id}", patch(prompts::edit_queued).delete(prompts::remove_queued))
        .route("/api/v1/conversations/{id}/queue/{queue_id}/move", post(prompts::move_queued))
        .route("/api/v1/conversations/{id}/queue/{queue_id}/send-now", post(prompts::send_now))
        .route("/api/v1/conversations/{id}/queue/send-all-now", post(prompts::send_all_now))
        .route("/api/v1/conversations/{id}/retry", post(prompts::retry))
        .route("/api/v1/conversations/{id}/dismiss", post(prompts::dismiss))
        .route("/api/v1/conversations/{id}/resume", post(usage_limits::resume).delete(usage_limits::cancel_resume))
        .route("/api/v1/conversations/{id}/cancel", post(prompts::cancel))
        .route("/api/v1/conversations/{id}/permission", post(prompts::permission))
        .route("/api/v1/conversations/{id}/responses/{request_id}", get(response_editor::show).post(response_editor::send))
        .route("/api/v1/conversations/{id}/responses/{request_id}/filter", post(response_editor::filter))
        .route("/api/v1/conversations/{id}/config", post(prompts::config))
        .route("/api/v1/memories", get(memories::list))
        .route("/api/v1/memories/{id}", patch(memories::update).delete(memories::delete))
        .route("/api/v1/memories/{id}/bank", put(memories::move_to_bank))
        .route("/api/v1/memory-banks", get(memories::list_banks).post(memories::create_bank))
        .route("/api/v1/memory-banks/{id}", patch(memories::update_bank).delete(memories::delete_bank))
        .route("/api/v1/mcp", get(mcp::stream).post(mcp::handle))
        .route("/api/v1/forges", get(forges::list).post(forges::connect))
        .route("/api/v1/forges/{id}", delete(forges::disconnect))
        .route("/api/v1/forges/{id}/mcp", post(forges::link_mcp))
        .route("/api/v1/repositories", get(forges::repositories))
        .route("/api/v1/projects", get(projects::list).post(projects::create))
        .route("/api/v1/projects/{id}", patch(projects::patch))
        .route("/api/v1/projects/{id}/checkout", post(projects::checkout))
        .route("/api/v1/projects/{id}/find-icon", post(projects::find_icon))
        .route("/api/v1/projects/{id}/avatar", post(projects::upload_avatar).layer(DefaultBodyLimit::max(projects::MAX_AVATAR_BYTES + 1024)))
        .route("/api/v1/privacy", get(voice::get_privacy).put(voice::save_privacy))
        .route("/api/v1/speech", post(voice::speak))
        .route("/api/v1/speech/voices", get(voice::voices))
        .route("/api/v1/transcriptions", post(transcriptions::create).layer(DefaultBodyLimit::max(transcriptions::MAX_AUDIO_BYTES + 1024)))
        .route("/api/v1/decision-providers", get(decision_providers::list).post(decision_providers::create))
        .route("/api/v1/decision-providers/test", post(decision_providers::test))
        .route("/api/v1/decision-providers/{id}", patch(decision_providers::update).delete(decision_providers::delete))
        .route("/api/v1/model-providers", get(model_providers::list).post(model_providers::create))
        .route("/api/v1/model-provider-usage", get(provider_usage::list))
        .route("/api/v1/usage/resets", post(account_usage::use_reset))
        .route("/api/v1/usage/history", get(usage_history::get))
        .route("/api/v1/model-providers/{id}", patch(model_providers::update).delete(model_providers::delete))
        .route("/api/v1/model-providers/{id}/refresh", post(model_providers::refresh))
        .route("/api/v1/model-providers/{id}/favourites", put(model_providers::set_favourite))
        .route("/api/v1/chat-tool-servers", get(chat_tool_servers::list).post(chat_tool_servers::create))
        .route("/api/v1/chat-tool-servers/oauth", post(mcp_oauth::start))
        .route("/api/v1/chat-tool-servers/probe", post(chat_tool_setup::probe))
        .route("/api/v1/chat-tool-servers/test", post(chat_tool_setup::test))
        .route("/api/v1/chat-tool-servers/caldav-test", post(caldav::test))
        .route("/api/v1/chat-tool-servers/coolify-test", post(coolify::test))
        .route("/api/v1/chat-tool-servers/tandoor-test", post(tandoor::test))
        .route("/api/v1/chat-tool-servers/{id}/coolify-options", get(coolify::target_options))
        .route("/api/v1/chat-tool-servers/{id}/coolify-target", put(coolify::set_target))
        .route("/api/v1/apps", get(coolify::list_apps))
        .route("/api/v1/apps/{uuid}", get(coolify::get_app).delete(coolify::delete_app))
        .route("/api/v1/apps/{uuid}/deployments", get(coolify::deployments))
        .route("/api/v1/apps/{uuid}/logs", get(coolify::logs))
        .route("/api/v1/apps/{uuid}/envs", get(coolify::envs))
        .route("/api/v1/apps/{uuid}/envs/{key}", put(coolify::put_env).delete(coolify::delete_env))
        .route("/api/v1/apps/{uuid}/redeploy", post(coolify::redeploy))
        .route("/api/v1/apps/{uuid}/start", post(coolify::start))
        .route("/api/v1/apps/{uuid}/stop", post(coolify::stop))
        .route("/api/v1/app-deployments/{uuid}", get(coolify::deployment))
        .route("/api/v1/chat-tool-servers/{id}", patch(chat_tool_servers::patch).delete(chat_tool_servers::delete))
        .route("/api/v1/chat-tool-servers/{id}/refresh", post(chat_tool_servers::refresh))
        .route("/api/v1/chat-tool-servers/{id}/groups", put(chat_tool_servers::set_groups))
        .route("/api/v1/chat-tool-servers/{id}/lastfm-sign-in", post(lastfm::start_sign_in).delete(lastfm::sign_out))
        .route("/api/v1/chat-tool-servers/{id}/freeagent-sign-in", post(freeagent::start_sign_in).delete(freeagent::sign_out))
        .route("/api/v1/chat-tool-servers/{id}/api-access", get(integration_proxy::grants))
        .route("/api/v1/integrations/{server_id}/calendars", get(caldav::calendars))
        .route("/api/v1/integrations/{server_id}/proxy/{*path}", axum::routing::any(integration_proxy::proxy).layer(axum::extract::DefaultBodyLimit::max(PROXY_BODY_LIMIT)))
        .route("/api/v1/integration-calls", get(chat_tool_servers::calls))
        .route("/api/v1/chats", post(chats::create))
        .route("/api/v1/chats/import", post(chat_import::create).layer(DefaultBodyLimit::max(chat_import::MAX_EXPORT_BYTES)))
        .route("/api/v1/assistants", get(assistants::list).post(assistants::create))
        .route("/api/v1/assistant-changes", get(assistant_changes::list))
        .route("/api/v1/assistant-changes/{id}/approve", post(assistant_changes::approve))
        .route("/api/v1/assistant-changes/{id}/decline", post(assistant_changes::decline))
        .route("/api/v1/assistant-changes/{id}/undo", post(assistant_changes::undo))
        .route("/api/v1/assistants/builtin/wellness", get(assistant_actions::wellness_assistant))
        .route("/api/v1/assistants/builtin/voice", get(voice::voice_assistant))
        .route("/api/v1/assistants/{id}/instructions/reset", post(voice::reset_instructions))
        .route("/api/v1/assistants/{id}", get(assistants::get).patch(assistants::patch))
        .route("/api/v1/assistants/{id}/actions", get(assistant_actions::list).post(assistant_actions::create))
        .route("/api/v1/assistants/{id}/actions/{action_id}", put(assistant_actions::update).delete(assistant_actions::delete))
        .route("/api/v1/assistants/{id}/actions/{action_id}/run", post(assistant_actions::run_handler))
        .route("/api/v1/assistants/{id}/actions/{action_id}/reset", post(assistant_actions::reset))
        .route("/api/v1/assistants/{id}/action-proposals", get(assistant_actions::list_proposals))
        .route("/api/v1/assistants/{id}/action-proposals/{proposal_id}/approve", post(assistant_actions::approve))
        .route("/api/v1/assistants/{id}/action-proposals/{proposal_id}/decline", post(assistant_actions::decline))
        .route("/api/v1/assistants/{id}/prompt", post(assistants::prompt))
        .route("/api/v1/assistants/{id}/clear", post(assistants::clear))
        .route("/api/v1/assistants/{id}/files", get(assistants::list_files))
        .route("/api/v1/assistants/{id}/backup", put(assistant_backup::set).delete(assistant_backup::clear))
        .route("/api/v1/assistants/{id}/backup/pull", post(assistant_backup::pull))
        .route("/api/v1/assistants/{id}/secrets", get(assistants::list_secrets))
        .route("/api/v1/assistants/{id}/secrets/{name}", put(assistants::put_secret).delete(assistants::delete_secret))
        .route("/api/v1/assistants/{id}/files/{*path}", get(assistants::read_file).put(assistants::write_file).delete(assistants::delete_file))
        .route("/api/v1/groups", get(groups::list).post(groups::create))
        .route("/api/v1/groups/{id}", patch(groups::rename).delete(groups::delete))
        .route("/api/v1/groups/{id}/hours", put(groups::save_hours))
        .route("/api/v1/groups/{id}/open", post(groups::open).delete(groups::close_now))
        .route("/api/v1/groups/{id}/instructions", get(instructions::layer).put(instructions::save))
        .route("/api/v1/groups/{id}/folders", post(groups::add_folder))
        .route("/api/v1/groups/{id}/folders/{folder_id}", delete(groups::remove_folder))
        .route("/api/v1/instructions", get(instructions::get))
        .route("/api/v1/instructions/pull", post(instructions::pull))
        .route("/api/v1/instructions/{layer}", put(instructions::save))
        .route("/api/v1/instruction-proposals", get(instruction_proposals::list))
        .route("/api/v1/instruction-proposals/{id}/approve", post(instruction_proposals::approve))
        .route("/api/v1/instruction-proposals/{id}/decline", post(instruction_proposals::decline))
        .route("/api/v1/conversation-connections", get(conversation_connections::list).post(conversation_connections::create))
        .route("/api/v1/conversation-connections/{id}", delete(conversation_connections::revoke))
        .route("/api/v1/group-connections", get(group_connections::list).post(group_connections::create))
        .route("/api/v1/group-connections/{id}", delete(group_connections::revoke))
        .route("/api/v1/message-proposals/{id}/connect", post(message_proposals::approve_and_connect))
        .route("/api/v1/message-proposals/{id}/connect-groups", post(message_proposals::approve_and_connect_groups))
        .route("/api/v1/message-proposals", get(message_proposals::list))
        .route("/api/v1/message-proposals/{id}/approve", post(message_proposals::approve))
        .route("/api/v1/message-proposals/{id}/decline", post(message_proposals::decline))
        .route("/api/v1/start-proposals", get(start_proposals::list))
        .route("/api/v1/start-proposals/{id}/approve", post(start_proposals::approve))
        .route("/api/v1/start-proposals/{id}/connect", post(start_proposals::approve_and_connect))
        .route("/api/v1/start-proposals/{id}/decline", post(start_proposals::decline))
        .route("/api/v1/api-access-requests", get(api_access::list))
        .route("/api/v1/api-access-requests/{id}/approve", post(api_access::approve))
        .route("/api/v1/api-access-requests/{id}/decline", post(api_access::decline))
        .route("/api/v1/ssh-access-requests", get(ssh_access::list))
        .route("/api/v1/ssh-access-requests/{id}/approve", post(ssh_access::approve))
        .route("/api/v1/ssh-access-requests/{id}/decline", post(ssh_access::decline))
        .route("/api/v1/ssh-access-requests/{id}/revoke", post(ssh_access::revoke))
        .route("/api/v1/nodes/{id}/ssh", get(ssh_access::connect))
        .route("/api/v1/instruction-source", put(instructions::set_source).delete(instructions::clear_source))
        .route("/api/v1/extensions", get(instructions::get_extensions).put(instructions::save_extensions))
        .route("/api/v1/experiments", get(experiments::get).put(experiments::save))
        .route("/api/v1/reflections", get(reflections::list))
        .route("/api/v1/reflections/{id}", patch(reflections::patch))
        .route("/api/v1/reflections/{id}/redraft", post(reflections::redraft))
        .route("/api/v1/exercises", get(exercises::list))
        .route("/api/v1/exercises/suggested", get(exercises::suggested))
        .route("/api/v1/exercises/propose", post(exercises::propose))
        .route("/api/v1/exercises/{id}/start", post(exercises::start))
        .route("/api/v1/exercises/{id}/not-now", post(exercises::not_now))
        .route("/api/v1/exercises/{id}/tests", post(exercises::run_tests))
        .route("/api/v1/exercises/{id}/review", post(exercises::review))
        .route("/api/v1/exercises/{id}/done", post(exercises::done))
        .route("/api/v1/exercises/{id}/solution", get(exercises::solution))
        .route("/api/v1/skills", get(skills::list))
        .route("/api/v1/skills/hidden", put(skills::hide).delete(skills::unhide))
        .route("/api/v1/wellness/settings", get(wellness::get_settings).put(wellness::save_settings))
        .route("/api/v1/wellness/heartbeat", post(wellness::heartbeat))
        .route("/api/v1/wellness/breaks", post(wellness::start_break))
        .route("/api/v1/wellness/breaks/current", delete(wellness::end_break))
        .route("/api/v1/wellness/breaks/{id}/note", put(wellness::note_break))
        .route("/api/v1/wellness/overview", get(wellness::overview))
        .route("/api/v1/wellness/activity/day", get(wellness::activity_day))
        .route("/api/v1/wellness/activity/hours", get(wellness::activity_hours))
        .route("/api/v1/wellness/learned", get(learned::list))
        .route("/api/v1/wellness/puzzles", get(wellness::puzzles))
        .route("/api/v1/wellness/holds", post(wellness::hold))
        .route("/api/v1/wellness/puzzles/{puzzle}/today", put(wellness::mark_puzzle).delete(wellness::unmark_puzzle))
        .route("/api/v1/wellness/puzzles/wordle/game", get(puzzles::wordle_game))
        .route("/api/v1/wellness/puzzles/wordle/guesses", post(puzzles::wordle_guess))
        .route("/api/v1/wellness/puzzles/connections/game", get(puzzles::connections_game))
        .route("/api/v1/wellness/puzzles/connections/guesses", post(puzzles::connections_guess))
        .route("/api/v1/usage-limit", get(usage_limits::get_setting).put(usage_limits::save_setting))
        .route("/api/v1/agent-settings", get(agent_settings::get).put(agent_settings::save))
        .route("/api/v1/agent-settings/account-mode", put(agent_settings::set_account_mode))
        .route("/api/v1/account-prices", get(subscription_prices::list).put(subscription_prices::set))
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
        .route_layer(axum::middleware::from_fn_with_state(state.clone(), auth::refuse_cross_site_session_writes))
        .route_layer(axum::middleware::from_fn_with_state(state.clone(), experiments::gate))
        .route_layer(axum::middleware::from_fn_with_state(state.clone(), api_access::gate_scoped_tokens))
        .fallback_service(web::single_page_app(&web_dir))
        .with_state(state)
}
