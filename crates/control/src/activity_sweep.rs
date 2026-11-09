use crate::boot::run_while_ready;
use crate::error::ApiResult;
use crate::state::AppState;
use crate::store::{CONVERSATION_COLUMNS, ConversationRow, has_live_workflow_agent, turn_has_ended_of};
use mycelium_proto::stream::StreamMessage;
use std::time::Duration;

const SWEEP_INTERVAL: Duration = Duration::from_secs(10);
const AGENT_QUIET_SECONDS: i32 = 30;

pub async fn sweep_regularly(state: AppState) {
    run_while_ready(&state, "settle the conversations whose agent went quiet", state.config.scheduler_tick.min(SWEEP_INTERVAL), || sweep(&state)).await;
}

const CALL_RECORD_DAYS: i32 = 90;
const MANAGED_IDLE_DAYS: i32 = 3;

async fn sweep(state: &AppState) -> ApiResult<()> {
    sqlx::query("delete from integration_calls where at < now() - make_interval(days => $1)").bind(CALL_RECORD_DAYS).execute(&state.pool).await?;

    let quiet = sqlx::query_as::<_, ConversationRow>(sqlx::AssertSqlSafe(format!(
        "update conversations set agent_active = false
         where agent_active and (last_activity_at is null or last_activity_at < now() - make_interval(secs => {AGENT_QUIET_SECONDS})) and {}
         returning {CONVERSATION_COLUMNS}",
        turn_has_ended_of("conversations.id"),
    )))
    .fetch_all(&state.pool)
    .await?;

    let settled = sqlx::query_as::<_, ConversationRow>(sqlx::AssertSqlSafe(format!(
        "update conversations set status = 'review', updated_at = now()
         where status = 'working' and not agent_active
           and exists (select 1 from events where conversation_id = conversations.id and kind = 'user_message')
           and {} and not {}
         returning {CONVERSATION_COLUMNS}",
        turn_has_ended_of("conversations.id"),
        has_live_workflow_agent("conversations.id"),
    )))
    .fetch_all(&state.pool)
    .await?;

    for row in quiet.into_iter().chain(settled) {
        state.publish(Some(row.owner), StreamMessage::Conversation(row.to_proto()?));
    }

    let idle = sqlx::query_as::<_, ConversationRow>(sqlx::AssertSqlSafe(format!(
        "update conversations set archived_at = now(), agent_active = false where id in (
         select c.id from conversations c, lateral (select now() - make_interval(days => $1) as cutoff) since
         where c.managed_by is not null and c.archived_at is null and c.flagged_at is null and c.pinned_at is null and c.snoozed_until is null and c.resume_at is null
           and c.status not in ('working', 'input') and not c.agent_active
           and c.updated_at < since.cutoff and coalesce(c.last_activity_at, c.created_at) < since.cutoff
           and coalesce((select e.created_at from events e where e.conversation_id = c.id order by e.seq desc limit 1), c.created_at) < since.cutoff
           and not {}
           and not exists (
               select 1 from events request where request.conversation_id = c.id and request.kind in ('permission_request', 'review_request')
                 and not exists (select 1 from events resolved where resolved.conversation_id = c.id and resolved.kind = replace(request.kind, '_request', '_resolved') and resolved.payload->>'request_id' = request.payload->>'request_id'))
           and not exists (select 1 from queued_prompts where conversation_id = c.id)
           and not exists (select 1 from message_proposals where conversation_id = c.id and state = 'proposed')
           and not exists (select 1 from start_proposals where conversation_id = c.id and state = 'proposed')
           and not exists (select 1 from instruction_proposals where conversation_id = c.id and state = 'proposed')
           and not exists (select 1 from assistant_action_proposals where conversation_id = c.id and state = 'proposed')
           and not exists (select 1 from assistant_changes where conversation_id = c.id and state = 'proposed')
           and not exists (select 1 from api_access_requests where conversation_id = c.id and state = 'requested')
           and not exists (select 1 from ssh_access_requests where conversation_id = c.id and state = 'requested')
           and not exists (select 1 from artifact_access_requests where conversation_id = c.id and state = 'requested')
           and not exists (select 1 from workflows where conversation_id = c.id and status not in ('done', 'failed', 'stopped'))
           and not exists (select 1 from conversations child where child.parent_id = c.id and child.archived_at is null and (child.status in ('working', 'input') or child.agent_active))
           and not exists (select 1 from watches own where own.watcher_id = c.id and own.kind <> 'conversation' and own.ended_at is null)
           and not exists (
               select 1 from watches w join conversations watcher on watcher.id = w.watcher_id
               where w.target_id = c.id and w.kind = 'conversation' and w.ended_at is null
                 and watcher.archived_at is null and (watcher.status in ('working', 'input') or watcher.agent_active))
         for update of c skip locked)
         returning {CONVERSATION_COLUMNS}",
        has_live_workflow_agent("c.id"),
    )))
    .bind(MANAGED_IDLE_DAYS)
    .fetch_all(&state.pool)
    .await?;

    for row in idle {
        if let Err(error) = crate::conversations::put_away(state, &row).await {
            tracing::warn!("closing the session of archived conversation {} failed: {}", row.id, error.message());
        }

        state.publish(Some(row.owner), StreamMessage::Conversation(row.to_proto()?));
    }

    Ok(())
}
