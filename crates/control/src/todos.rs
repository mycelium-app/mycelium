use crate::auth::AuthUser;
use crate::error::{ApiError, ApiResult};
use crate::state::AppState;
use crate::store::find_owned;
use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use mycelium_proto::api::{PromptRequest, SaveTodosRequest};
use mycelium_proto::conversation::EventKind;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sqlx::PgConnection;
use uuid::Uuid;

pub const STATUSES: [&str; 3] = ["pending", "in_progress", "completed"];
const BY_PERSON: &str = "person";
const LATEST_PLAN: &str = "select payload from events where conversation_id = $1 and kind = 'acp_update' and payload->'update'->>'sessionUpdate' = 'plan' order by seq desc limit 1";
const LATEST_AGENT_PLAN: &str = "select payload from events where conversation_id = $1 and kind = 'acp_update' and payload->'update'->>'sessionUpdate' = 'plan' and payload->>'by' is distinct from 'person' order by seq desc limit 1";

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct Edits {
    #[serde(default)]
    added: Vec<Value>,
    #[serde(default)]
    dismissed: Vec<String>,
    #[serde(default)]
    described: Vec<Value>,
}

impl Edits {
    fn between(before: &[Value], after: &[Value]) -> Edits {
        let holds = |list: &[Value], content: &str| list.iter().any(|entry| content_of(entry) == content);

        Edits {
            added: after.iter().filter(|entry| !holds(before, content_of(entry))).cloned().collect(),
            dismissed: before.iter().map(content_of).filter(|content| !holds(after, content)).map(str::to_string).collect(),
            described: after.iter().filter(|entry| before.iter().any(|old| content_of(old) == content_of(entry) && old.get("description") != entry.get("description"))).cloned().collect(),
        }
    }

    fn is_empty(&self) -> bool {
        self.added.is_empty() && self.dismissed.is_empty() && self.described.is_empty()
    }

    fn extend(&mut self, earlier: Edits) {
        for entry in earlier.added {
            if !self.dismissed.iter().any(|content| content == content_of(&entry)) && !self.added.iter().any(|added| content_of(added) == content_of(&entry)) {
                self.added.push(entry);
            }
        }

        for content in earlier.dismissed {
            if !self.added.iter().any(|added| content_of(added) == content) && !self.dismissed.contains(&content) {
                self.dismissed.push(content);
            }
        }

        for entry in earlier.described {
            if !self.described.iter().any(|described| content_of(described) == content_of(&entry)) {
                self.described.push(entry);
            }
        }
    }

    fn apply(&self, incoming: Vec<Value>) -> Vec<Value> {
        let mut merged: Vec<Value> = incoming
            .into_iter()
            .filter(|entry| !self.dismissed.iter().any(|content| content == content_of(entry)))
            .map(|mut entry| {
                if let Some(described) = self.described.iter().find(|described| content_of(described) == content_of(&entry)) {
                    match described.get("description") {
                        Some(description) => entry["description"] = description.clone(),
                        None => {
                            entry.as_object_mut().map(|fields| fields.remove("description"));
                        }
                    }
                }

                entry
            })
            .collect();

        for added in &self.added {
            if !merged.iter().any(|entry| content_of(entry) == content_of(added)) {
                merged.push(added.clone());
            }
        }

        merged
    }

    fn lines(&self) -> Vec<String> {
        let mut lines: Vec<String> = self.added.iter().map(|entry| format!("- Added: {}", content_of(entry))).collect();
        lines.extend(self.dismissed.iter().map(|content| format!("- Dismissed: {content}")));
        lines.extend(self.described.iter().filter(|entry| !self.added.iter().any(|added| content_of(added) == content_of(entry))).map(|entry| match entry["description"].as_str() {
            Some(description) => format!("- Changed the details of {}: {description}", content_of(entry)),
            None => format!("- Removed the details of {}", content_of(entry)),
        }));

        lines
    }
}

fn content_of(entry: &Value) -> &str {
    entry["content"].as_str().unwrap_or_default()
}

fn entries_of(payload: &Value) -> Vec<Value> {
    payload["update"]["entries"].as_array().cloned().unwrap_or_default()
}

pub fn entry(content: &str, status: &str, description: Option<&str>) -> Option<Value> {
    let content = content.trim();

    if content.is_empty() || !STATUSES.contains(&status) {
        return None;
    }

    let mut entry = json!({"content": content, "status": status});

    if let Some(description) = description.map(str::trim).filter(|description| !description.is_empty()) {
        entry["description"] = json!(description);
    }

    Some(entry)
}

fn plan(entries: &[Value]) -> Value {
    json!({"update": {"sessionUpdate": "plan", "entries": entries}})
}

async fn latest(conn: &mut PgConnection, query: &'static str, conversation_id: Uuid) -> ApiResult<Option<Value>> {
    let found = sqlx::query_scalar::<_, sqlx::types::Json<Value>>(query).bind(conversation_id).fetch_optional(conn).await?;

    Ok(found.map(|found| found.0))
}

fn unseen_of(payload: &Value) -> Option<Edits> {
    payload.get("unseen").and_then(|unseen| serde_json::from_value(unseen.clone()).ok())
}

async fn pending_edits(conn: &mut PgConnection, conversation_id: Uuid) -> ApiResult<Option<(Edits, bool)>> {
    let Some(newest) = latest(conn, LATEST_PLAN, conversation_id).await? else {
        return Ok(None);
    };

    if newest["by"] != BY_PERSON {
        return Ok(unseen_of(&newest).map(|unseen| (unseen, false)));
    }

    let base = latest(conn, LATEST_AGENT_PLAN, conversation_id).await?.unwrap_or(Value::Null);
    let mut edits = Edits::between(&entries_of(&base), &entries_of(&newest));

    if let Some(earlier) = unseen_of(&base) {
        edits.extend(earlier);
    }

    Ok(Some((edits, newest["told"] == true)))
}

pub async fn from_agent(conn: &mut PgConnection, conversation_id: Uuid, entries: Vec<Value>) -> ApiResult<(Vec<Value>, Vec<String>)> {
    match pending_edits(conn, conversation_id).await? {
        Some((edits, _)) if !edits.is_empty() => Ok((edits.apply(entries), edits.lines())),
        _ => Ok((entries, Vec::new())),
    }
}

pub async fn from_harness(conn: &mut PgConnection, conversation_id: Uuid, mut update: Value) -> ApiResult<Value> {
    let Some((edits, told)) = pending_edits(conn, conversation_id).await? else {
        return Ok(update);
    };

    if edits.is_empty() {
        return Ok(update);
    }

    update["update"]["entries"] = json!(edits.apply(entries_of(&update)));

    if !told {
        update["unseen"] = json!(edits);
    }

    Ok(update)
}

pub async fn carry(state: &AppState, owner: Uuid, assistant_id: Uuid, conversation_id: Uuid) -> ApiResult<()> {
    let previous = sqlx::query_scalar::<_, Uuid>("select id from conversations where assistant_id = $1 and id <> $2 order by created_at desc limit 1").bind(assistant_id).bind(conversation_id).fetch_optional(&state.pool).await?;
    let Some(previous) = previous else {
        return Ok(());
    };
    let mut conn = state.pool.acquire().await?;
    let Some(payload) = latest(&mut conn, LATEST_PLAN, previous).await? else {
        return Ok(());
    };
    let entries = entries_of(&payload);

    if entries.iter().all(|entry| entry["status"] == "completed") {
        return Ok(());
    }

    let mut carried = plan(&entries);
    carried["carried"] = json!(true);

    if let Some((edits, false)) = pending_edits(&mut conn, previous).await?
        && !edits.is_empty()
    {
        carried["unseen"] = json!(edits);
    }

    drop(conn);
    crate::chats::record(state, owner, conversation_id, EventKind::AcpUpdate, &carried).await
}

pub async fn carried(state: &AppState, conversation_id: Uuid) -> ApiResult<bool> {
    let found = sqlx::query_scalar::<_, bool>("select exists(select 1 from events where conversation_id = $1 and kind = 'acp_update' and payload->>'carried' = 'true')").bind(conversation_id).fetch_one(&state.pool).await?;

    Ok(found)
}

pub async fn carried_and_unsent(state: &AppState, conversation_id: Uuid) -> ApiResult<bool> {
    let sent = sqlx::query_scalar::<_, bool>("select exists(select 1 from events where conversation_id = $1 and kind = 'user_message')").bind(conversation_id).fetch_one(&state.pool).await?;

    Ok(!sent && carried(state, conversation_id).await?)
}

pub async fn save(State(state): State<AppState>, user: AuthUser, Path(id): Path<Uuid>, Json(request): Json<SaveTodosRequest>) -> ApiResult<StatusCode> {
    let row = find_owned(&state.pool, id, user.id).await?;
    let mut entries = Vec::with_capacity(request.todos.len());

    for item in &request.todos {
        let Some(entry) = entry(&item.content, &item.status, item.description.as_deref()) else {
            return Err(ApiError::bad_request("todo_content_required", "Enter what to do for every item on the to-do list, or dismiss the empty ones."));
        };

        entries.push(entry);
    }

    let mut conn = state.pool.acquire().await?;
    let before = latest(&mut conn, LATEST_PLAN, id).await?.map(|payload| entries_of(&payload)).unwrap_or_default();
    drop(conn);
    let mut saved = plan(&entries);
    saved["by"] = json!(BY_PERSON);
    saved["told"] = json!(request.tell_agent);
    crate::chats::record(&state, user.id, id, EventKind::AcpUpdate, &saved).await?;

    let lines = Edits::between(&before, &entries).lines();

    if request.tell_agent && !lines.is_empty() {
        let text = format!("I changed the to-do list:\n{}", lines.join("\n"));

        if crate::chats::is_chat(&row) {
            crate::chats::send_or_queue(&state, user.id, id, &text).await?;
        } else {
            crate::prompts::send(&state, user.id, id, PromptRequest { text, ..Default::default() }).await?;
        }
    }

    Ok(StatusCode::NO_CONTENT)
}
