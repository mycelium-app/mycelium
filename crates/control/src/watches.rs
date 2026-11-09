use crate::auth::AuthUser;
use crate::boot::run_while_ready;
use crate::error::ApiResult;
use crate::state::AppState;
use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use mycelium_proto::stream::StreamMessage;
use serde_json::{Value, json};
use uuid::Uuid;

pub const EVENT_ORIGIN: &str = "event";
const DUE_BATCH: i64 = 50;
const MOST_RESULT_CHARACTERS: usize = 4000;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Ending {
    Done,
    Failed,
    Waiting,
}

impl Ending {
    fn key(self) -> &'static str {
        match self {
            Ending::Done => "done",
            Ending::Failed => "failed",
            Ending::Waiting => "waiting",
        }
    }

    fn words(self) -> &'static str {
        match self {
            Ending::Done => "finished its turn",
            Ending::Failed => "failed",
            Ending::Waiting => "is waiting on the person",
        }
    }
}

pub async fn deliver(state: &AppState, owner: Uuid, watch: Uuid, watcher: Uuid, key: &str, text: String) -> ApiResult<bool> {
    let claimed = sqlx::query("insert into watch_notifications (watch_id, key) values ($1, $2) on conflict do nothing").bind(watch).bind(key).execute(&state.pool).await?;

    if claimed.rows_affected() == 0 {
        return Ok(false);
    }

    let (archived, chat) = sqlx::query_as::<_, (bool, bool)>("select archived_at is not null, agent = $2 from conversations where id = $1").bind(watcher).bind(crate::chats::CHAT_AGENT).fetch_one(&state.pool).await?;

    if archived {
        end(state, owner, watch).await?;

        return Ok(false);
    }

    let sent = if chat {
        crate::chats::send_or_queue(state, owner, watcher, &text).await
    } else {
        crate::prompts::send_from(state, owner, watcher, mycelium_proto::api::PromptRequest { text, ..Default::default() }, Some(EVENT_ORIGIN)).await.map(|_| ())
    };

    if let Err(error) = sent {
        sqlx::query("delete from watch_notifications where watch_id = $1 and key = $2").bind(watch).bind(key).execute(&state.pool).await?;

        return Err(error);
    }

    Ok(true)
}

pub async fn end(state: &AppState, owner: Uuid, watch: Uuid) -> ApiResult<()> {
    let watcher = sqlx::query_scalar::<_, Uuid>("update watches set ended_at = now() where id = $1 and owner = $2 and ended_at is null returning watcher_id").bind(watch).bind(owner).fetch_optional(&state.pool).await?;

    if let Some(watcher) = watcher {
        state.publish(Some(owner), StreamMessage::WatchesChanged { conversation_id: watcher });
    }

    Ok(())
}

pub async fn watch_conversation(state: &AppState, owner: Uuid, watcher: Uuid, target: Uuid, only_waiting: bool) -> ApiResult<Uuid> {
    let mut tx = state.pool.begin().await?;
    let (id, added) = record_conversation_watch(&mut tx, owner, watcher, target, only_waiting).await?;
    tx.commit().await?;

    if added {
        state.publish(Some(owner), StreamMessage::WatchesChanged { conversation_id: watcher });
    }

    Ok(id)
}

pub async fn record_conversation_watch(tx: &mut sqlx::PgConnection, owner: Uuid, watcher: Uuid, target: Uuid, only_waiting: bool) -> sqlx::Result<(Uuid, bool)> {
    let existing = sqlx::query_scalar::<_, Uuid>("select id from watches where watcher_id = $1 and target_id = $2 and kind = 'conversation' and ended_at is null").bind(watcher).bind(target).fetch_optional(&mut *tx).await?;

    if let Some(existing) = existing {
        sqlx::query("update watches set only_waiting = only_waiting and $2 where id = $1").bind(existing).bind(only_waiting).execute(&mut *tx).await?;

        return Ok((existing, false));
    }

    let id = sqlx::query_scalar::<_, Uuid>("insert into watches (owner, watcher_id, kind, target_id, only_waiting) values ($1, $2, 'conversation', $3, $4) returning id").bind(owner).bind(watcher).bind(target).bind(only_waiting).fetch_one(&mut *tx).await?;

    Ok((id, true))
}

pub async fn wake_at(state: &AppState, owner: Uuid, watcher: Uuid, due_at: chrono::DateTime<chrono::Utc>, note: &str) -> ApiResult<Uuid> {
    let id = sqlx::query_scalar::<_, Uuid>("insert into watches (owner, watcher_id, kind, due_at, note) values ($1, $2, 'timer', $3, $4) returning id").bind(owner).bind(watcher).bind(due_at).bind(note).fetch_one(&state.pool).await?;
    state.publish(Some(owner), StreamMessage::WatchesChanged { conversation_id: watcher });

    Ok(id)
}

pub async fn latest_result(state: &AppState, conversation: Uuid) -> ApiResult<Option<(i64, String)>> {
    let latest = sqlx::query_as::<_, (i64, String)>("select s.seq, s.text from search_entries s join events e on e.conversation_id = s.conversation_id and e.seq = s.seq where s.conversation_id = $1 and e.kind <> 'user_message' order by s.seq desc limit 1")
        .bind(conversation)
        .fetch_optional(&state.pool)
        .await?;

    Ok(latest)
}

async fn turn_came_from_an_event(state: &AppState, conversation: Uuid) -> ApiResult<bool> {
    let origin = sqlx::query_scalar::<_, Option<String>>("select payload->>'origin' from events where conversation_id = $1 and kind = 'user_message' order by seq desc limit 1").bind(conversation).fetch_optional(&state.pool).await?.flatten();

    Ok(origin.as_deref() == Some(EVENT_ORIGIN))
}

pub async fn conversation_ended(state: &AppState, conversation: Uuid, ending: Ending) -> ApiResult<()> {
    if turn_came_from_an_event(state, conversation).await? {
        return Ok(());
    }

    let watches = sqlx::query_as::<_, (Uuid, Uuid, Uuid, bool)>(
        "select w.id, w.owner, w.watcher_id, w.only_waiting from watches w join conversations target on target.id = w.target_id join conversations watcher on watcher.id = w.watcher_id
         where w.target_id = $1 and w.kind = 'conversation' and w.ended_at is null and target.archived_at is null and watcher.archived_at is null",
    )
    .bind(conversation)
    .fetch_all(&state.pool)
    .await?;

    if watches.is_empty() {
        return Ok(());
    }

    let title = sqlx::query_scalar::<_, String>("select title from conversations where id = $1").bind(conversation).fetch_one(&state.pool).await?;
    let at = sqlx::query_scalar::<_, Option<i64>>("select max(seq) from events where conversation_id = $1").bind(conversation).fetch_one(&state.pool).await?.unwrap_or_default();
    let label = crate::message_proposals::label(conversation, &title);
    let result = latest_result(state, conversation).await?.map(|(_, text)| text.chars().take(MOST_RESULT_CHARACTERS).collect::<String>());
    let text = match result {
        Some(result) => format!("From {label}:\n\n{label} ({conversation}) {}.\n\nLatest result:\n{result}", ending.words()),
        None => format!("From {label}:\n\n{label} ({conversation}) {}.", ending.words()),
    };

    for (watch, owner, watcher, only_waiting) in watches {
        if only_waiting && ending != Ending::Waiting {
            continue;
        }

        if let Err(error) = deliver(state, owner, watch, watcher, &format!("{at}:{}", ending.key()), text.clone()).await {
            tracing::warn!("could not tell {watcher} that {conversation} {}: {}", ending.words(), error.message());
        }
    }

    Ok(())
}

pub async fn deliver_regularly(state: AppState) {
    run_while_ready(&state, "wake the conversations whose timers are due and end the watches of archived conversations", state.config.scheduler_tick, || tick(&state)).await;
}

async fn tick(state: &AppState) -> ApiResult<()> {
    let ended = sqlx::query_as::<_, (Uuid, Uuid)>(
        "update watches w set ended_at = now() from conversations watcher where watcher.id = w.watcher_id and w.ended_at is null
         and (watcher.archived_at is not null or exists (select 1 from conversations target where target.id = w.target_id and target.archived_at is not null))
         returning w.owner, w.watcher_id",
    )
    .fetch_all(&state.pool)
    .await?;

    for (owner, watcher) in ended {
        state.publish(Some(owner), StreamMessage::WatchesChanged { conversation_id: watcher });
    }

    let due = sqlx::query_as::<_, (Uuid, Uuid, Uuid, Option<String>, chrono::DateTime<chrono::Utc>)>(sqlx::AssertSqlSafe(format!(
        "update watches set ended_at = now() where id in (select id from watches where kind = 'timer' and ended_at is null and due_at <= now() order by due_at limit {DUE_BATCH} for update skip locked) returning id, owner, watcher_id, note, created_at"
    )))
    .fetch_all(&state.pool)
    .await?;

    for (watch, owner, watcher, note, set_at) in due {
        let text = format!("From the wake-up this conversation set at {}:\n\n{}", set_at.format("%Y-%m-%d %H:%M UTC"), note.unwrap_or_default());

        if let Err(error) = deliver(state, owner, watch, watcher, "due", text).await {
            tracing::warn!("could not wake {watcher}: {}", error.message());
            sqlx::query("update watches set ended_at = null where id = $1").bind(watch).execute(&state.pool).await?;
        }

        state.publish(Some(owner), StreamMessage::WatchesChanged { conversation_id: watcher });
    }

    crate::forge_watches::poll(state).await
}

pub async fn listed(state: &AppState, owner: Uuid, watcher: Uuid) -> ApiResult<Vec<Value>> {
    let rows = sqlx::query_as::<_, (Uuid, String, Option<Uuid>, Option<String>, Option<String>, Option<String>, Option<String>, Option<chrono::DateTime<chrono::Utc>>, bool)>(
        "select w.id, w.kind, w.target_id, t.title, w.repository, w.reference, w.note, w.due_at, w.only_waiting from watches w left join conversations t on t.id = w.target_id
         where w.owner = $1 and w.watcher_id = $2 and w.ended_at is null order by coalesce(w.due_at, w.created_at)",
    )
    .bind(owner)
    .bind(watcher)
    .fetch_all(&state.pool)
    .await?;

    Ok(rows
        .into_iter()
        .map(|(id, kind, target, title, repository, reference, note, due_at, only_waiting)| {
            let reference = reference.map(|reference| crate::forge_watches::Target::parse(&reference).map_or(reference, |target| target.label()));

            json!({"id": id, "kind": kind, "conversation_id": target, "title": title, "repository": repository, "reference": reference, "note": note, "due_at": due_at, "only_waiting": only_waiting})
        })
        .collect())
}

pub async fn list(State(state): State<AppState>, user: AuthUser, Path(id): Path<Uuid>) -> ApiResult<Json<Vec<Value>>> {
    crate::store::find_owned(&state.pool, id, user.id).await?;

    Ok(Json(listed(&state, user.id, id).await?))
}

pub async fn delete(State(state): State<AppState>, user: AuthUser, Path(id): Path<Uuid>) -> ApiResult<StatusCode> {
    end(&state, user.id, id).await?;

    Ok(StatusCode::NO_CONTENT)
}
