use crate::boot::is_ready;
use crate::error::ApiResult;
use crate::state::AppState;
use crate::store::{CONVERSATION_COLUMNS, ConversationRow, has_live_workflow_agent, turn_has_ended_of};
use mycelium_proto::stream::StreamMessage;
use std::time::Duration;

const SWEEP_INTERVAL: Duration = Duration::from_secs(10);
const AGENT_QUIET_SECONDS: i32 = 30;

pub async fn sweep_regularly(state: AppState) {
    loop {
        tokio::time::sleep(state.config.scheduler_tick.min(SWEEP_INTERVAL)).await;

        if !is_ready(&state) {
            continue;
        }

        let swept = sweep(&state).await;

        if let Err(cause) = swept {
            tracing::error!("could not settle the conversations whose agent went quiet: {}", cause.message());
        }
    }
}

const CALL_RECORD_DAYS: i32 = 90;

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

    Ok(())
}
