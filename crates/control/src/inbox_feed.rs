use crate::auth::AuthUser;
use crate::chat_tool_servers::{ServerRow, bearer, list_owned, refreshable_after_rejection, renew};
use crate::error::ApiResult;
use crate::mcp_client::{McpConnection, McpError};
use crate::state::AppState;
use axum::Json;
use axum::extract::{Query, State};
use chrono::{DateTime, Utc};
use mycelium_proto::api::{InboxFeed, InboxPost};
use serde::Deserialize;
use serde_json::{Value, json};
use std::collections::HashMap;
use std::sync::{LazyLock, Mutex};
use std::time::{Duration, Instant};
use uuid::Uuid;

const FEED_LIMIT: u32 = 30;
const FRESH_FOR: Duration = Duration::from_secs(30);

struct Cached {
    posts: Vec<InboxPost>,
    fetched: Instant,
    fetched_at: DateTime<Utc>,
}

static CACHE: LazyLock<Mutex<HashMap<Uuid, Cached>>> = LazyLock::new(Default::default);

#[derive(Deserialize)]
pub struct FeedQuery {
    #[serde(default)]
    refresh: bool,
}

#[derive(Deserialize)]
struct Updates {
    #[serde(default)]
    posts: Vec<Update>,
}

#[derive(Deserialize)]
struct Update {
    id: String,
    author: String,
    kind: String,
    message: String,
    project: Option<String>,
    #[serde(default)]
    links: Vec<Link>,
    #[serde(rename = "createdAt")]
    created_at: DateTime<Utc>,
}

#[derive(Deserialize)]
struct Link {
    url: String,
}

impl Update {
    fn into_post(self) -> InboxPost {
        InboxPost { id: self.id, author: self.author, kind: self.kind, message: self.message, project: self.project, url: self.links.into_iter().next().map(|link| link.url), created_at: self.created_at }
    }
}

async fn fetch(state: &AppState, server: &ServerRow) -> Result<Vec<InboxPost>, String> {
    let token = bearer(state, server).await.map_err(|error| error.message().to_string())?;
    let mut connection = McpConnection::new(&state.http, &server.url, token.as_deref(), server.key_header());
    let arguments = json!({"limit": FEED_LIMIT});
    let mut called = connection.call_tool("list_updates", &arguments).await;

    if matches!(called, Err(McpError::Rejected)) && refreshable_after_rejection(server) {
        let renewed = renew(state, server).await.map_err(|error| error.message().to_string())?;
        connection.set_token(renewed.as_deref());
        called = connection.call_tool("list_updates", &arguments).await;
    }

    let output = called.map_err(|error| error.describe(&server.name))?;

    if output.is_error {
        return Err(output.text);
    }

    let parsed: Value = serde_json::from_str(&output.text).map_err(|_| "Murmur sent an answer Mycelium could not read.".to_string())?;
    let updates: Updates = serde_json::from_value(parsed).map_err(|_| "Murmur sent an answer Mycelium could not read.".to_string())?;

    Ok(updates.posts.into_iter().map(Update::into_post).collect())
}

pub async fn feed(State(state): State<AppState>, user: AuthUser, Query(query): Query<FeedQuery>) -> ApiResult<Json<InboxFeed>> {
    let servers = list_owned(&state, user.id).await?;
    let murmur = servers.iter().find(|server| server.enabled && server.settings["app"].as_str() == Some("murmur"));
    let Some(server) = murmur else {
        return Ok(Json(InboxFeed { configured: false, posts: Vec::new(), error: None, stale_since: None }));
    };

    let fresh = CACHE.lock().map_err(crate::error::ApiError::internal)?.get(&user.id).filter(|cached| !query.refresh && cached.fetched.elapsed() < FRESH_FOR).map(|cached| cached.posts.clone());

    if let Some(posts) = fresh {
        return Ok(Json(InboxFeed { configured: true, posts, error: None, stale_since: None }));
    }

    match fetch(&state, server).await {
        Ok(posts) => {
            CACHE.lock().map_err(crate::error::ApiError::internal)?.insert(user.id, Cached { posts: posts.clone(), fetched: Instant::now(), fetched_at: Utc::now() });

            Ok(Json(InboxFeed { configured: true, posts, error: None, stale_since: None }))
        }
        Err(cause) => {
            tracing::warn!(%cause, "Murmur feed failed");
            let cache = CACHE.lock().map_err(crate::error::ApiError::internal)?;
            let stale = cache.get(&user.id);

            Ok(Json(InboxFeed {
                configured: true,
                posts: stale.map(|cached| cached.posts.clone()).unwrap_or_default(),
                error: Some("Murmur did not answer.".to_string()),
                stale_since: stale.map(|cached| cached.fetched_at),
            }))
        }
    }
}
