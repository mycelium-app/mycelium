use crate::boot::run_while_ready;
use crate::conversations;
use crate::error::ApiResult;
use crate::forges::{self, ForgeApi};
use crate::state::AppState;
use mycelium_proto::stream::StreamMessage;
use std::collections::HashMap;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;
use uuid::Uuid;

const SYNC_INTERVAL: Duration = Duration::from_secs(60);

type Signatures = Arc<Mutex<HashMap<(Uuid, String), u64>>>;

pub async fn sync_regularly(state: AppState) {
    let signatures = Signatures::default();

    run_while_ready(&state, "sync issues and pull requests from the forges", state.config.scheduler_tick.min(SYNC_INTERVAL), || sync(&state, &signatures)).await;
}

fn signature<Item: serde::Serialize>(items: &[Item]) -> u64 {
    let mut hasher = DefaultHasher::new();

    serde_json::to_string(items).unwrap_or_default().hash(&mut hasher);

    hasher.finish()
}

async fn sync(state: &AppState, signatures: &Signatures) -> ApiResult<()> {
    let repositories = sqlx::query_as::<_, (Uuid, Uuid, String)>("select distinct owner, forge_id, repository from projects where forge_id is not null and repository is not null").fetch_all(&state.pool).await?;

    for (owner, forge_id, repository) in repositories {
        let Ok(forge) = forges::find_owned(state, forge_id, owner).await else {
            continue;
        };
        let Ok(api) = ForgeApi::from_row(&forge) else {
            continue;
        };
        let api = api.cached(&state.forge_cache).refreshing(true);
        let (Ok(issues), Ok(pull_requests)) = (api.issues(&state.http, &repository, "").await, api.pull_requests(&state.http, &repository).await) else {
            continue;
        };
        let current = signature(&issues) ^ signature(&pull_requests).rotate_left(1);
        let previous = signatures.lock().unwrap_or_else(PoisonError::into_inner).insert((forge_id, repository.clone()), current);

        if previous.is_none_or(|known| known == current) {
            continue;
        }

        state.forge_cache.invalidate(forge_id);
        state.publish(Some(owner), StreamMessage::ForgeChanged);

        let tracking = sqlx::query_scalar::<_, Uuid>(
            "select id from conversations where archived_at is null and branch is not null
               and project_id in (select id from projects where owner = $1 and forge_id = $2 and repository = $3)",
        )
        .bind(owner)
        .bind(forge_id)
        .bind(&repository)
        .fetch_all(&state.pool)
        .await?;

        for conversation_id in tracking {
            conversations::spawn_branch_refresh(state, conversation_id);
        }
    }

    Ok(())
}
