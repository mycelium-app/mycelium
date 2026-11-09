use crate::auth::AuthUser;
use crate::error::ApiResult;
use crate::state::AppState;
use axum::Json;
use axum::extract::State;
use chrono::{DateTime, Utc};
use mycelium_proto::api::{InboxFeed, InboxPost};
use mycelium_proto::stream::StreamMessage;
use serde_json::Value;
use uuid::Uuid;

const FEED_LIMIT: i64 = 30;
pub const LONGEST_POST: usize = 280;
const POST_SELECT: &str = "select f.id, a.name as assistant, c.agent, c.model, f.message, p.name as project, f.conversation_id, c.title as conversation_title, f.created_at
     from feed_posts f join conversations c on c.id = f.conversation_id left join assistants a on a.id = c.assistant_id left join projects p on p.id = c.project_id";

#[derive(sqlx::FromRow)]
struct PostRow {
    id: Uuid,
    assistant: Option<String>,
    agent: String,
    model: Option<String>,
    message: String,
    project: Option<String>,
    conversation_id: Uuid,
    conversation_title: String,
    created_at: DateTime<Utc>,
}

impl PostRow {
    fn into_post(self) -> InboxPost {
        let chat_model = self.model.filter(|_| self.agent == "chat");
        let author = self.assistant.or(chat_model).unwrap_or_else(|| crate::goals::agent_name(&self.agent).to_string());

        InboxPost { id: self.id, author, message: self.message, project: self.project, conversation_id: self.conversation_id, conversation_title: self.conversation_title, created_at: self.created_at }
    }
}

pub async fn post_from_agent(state: &AppState, owner: Uuid, conversation_id: Uuid, arguments: &Value) -> ApiResult<Result<(), String>> {
    let message = arguments.get("message").and_then(Value::as_str).map(str::trim).unwrap_or_default();

    if message.is_empty() {
        return Ok(Err("Enter a message to post to the feed.".to_string()));
    }

    if message.chars().count() > LONGEST_POST {
        return Ok(Err(format!("Message must be {LONGEST_POST} characters or fewer.")));
    }

    let id = sqlx::query_scalar::<_, Uuid>("insert into feed_posts (owner, conversation_id, message) values ($1, $2, $3) returning id").bind(owner).bind(conversation_id).bind(message).fetch_one(&state.pool).await?;
    let row = sqlx::query_as::<_, PostRow>(sqlx::AssertSqlSafe(format!("{POST_SELECT} where f.id = $1"))).bind(id).fetch_one(&state.pool).await?;

    state.publish(Some(owner), StreamMessage::FeedPost(row.into_post()));

    Ok(Ok(()))
}

pub async fn feed(State(state): State<AppState>, user: AuthUser) -> ApiResult<Json<InboxFeed>> {
    let rows = sqlx::query_as::<_, PostRow>(sqlx::AssertSqlSafe(format!("{POST_SELECT} where f.owner = $1 order by f.created_at desc, f.id desc limit $2"))).bind(user.id).bind(FEED_LIMIT).fetch_all(&state.pool).await?;

    Ok(Json(InboxFeed { posts: rows.into_iter().map(PostRow::into_post).collect() }))
}
