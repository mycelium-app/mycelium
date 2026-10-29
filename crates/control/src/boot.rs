use crate::config::Config;
use crate::error::ApiResult;
use crate::forge_cache::ForgeCache;
use crate::rate_limit::RateLimiter;
use crate::state::{AppState, Hub, Readiness};
use crate::versions::LatestVersions;
use anyhow::Result;
use axum::Json;
use axum::extract::State;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use mycelium_proto::api::ErrorBody;
use sqlx::migrate::Migrator;
use sqlx::postgres::PgPoolOptions;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::sync::broadcast;

const MIGRATION_LOCK_KEY: i64 = 7461001;
const VERSION: &str = match option_env!("MYCELIUM_VERSION") {
    Some(version) => version,
    None => env!("CARGO_PKG_VERSION"),
};
const RETRY_INTERVAL: Duration = Duration::from_secs(5);
static MIGRATOR: Migrator = sqlx::migrate!();

pub fn new_state(config: Config) -> Result<AppState> {
    let pool = PgPoolOptions::new().max_connections(config.database_pool_size).acquire_timeout(config.database_acquire_timeout).connect_lazy(&config.database_url)?;
    let (stream, _) = broadcast::channel(1024);
    let forge_cache = Arc::new(ForgeCache::new(config.forge_cache_ttl));

    Ok(AppState {
        pool,
        config: Arc::new(config),
        hub: Arc::new(Hub::default()),
        stream,
        rate_limiter: Arc::new(RateLimiter::default()),
        latest: Arc::new(LatestVersions::default()),
        readiness: Arc::new(Mutex::new(Readiness::Migrating)),
        http: crate::outbound::client()?,
        forge_cache,
    })
}

fn set_readiness(state: &AppState, readiness: Readiness) {
    if let Ok(mut current) = state.readiness.lock() {
        *current = readiness;
    }
}

pub async fn migrate_until_ready(state: AppState) {
    if state.config.skip_migrations {
        set_readiness(&state, Readiness::Ready);

        return;
    }

    let mut reported = false;

    loop {
        match apply_migrations(&state).await {
            Ok(()) => {
                if let Err(cause) = crate::imports::repair_activity_dates(&state).await {
                    tracing::error!("could not restore the activity dates of imported conversations: {cause:#}");
                }

                if let Err(cause) = crate::imports::repair_import_usage(&state).await {
                    tracing::error!("could not fill in the usage of imported conversations: {cause:#}");
                }

                if let Err(cause) = crate::imports::end_interrupted(&state).await {
                    tracing::error!("could not end the imports that were running when the control plane stopped: {cause:#}");
                }

                if let Err(cause) = crate::chats::end_interrupted(&state).await {
                    tracing::error!("could not end the chats that were replying when the control plane stopped: {cause:?}");
                }

                set_readiness(&state, Readiness::Ready);

                return;
            }
            Err(cause) => {
                let message = format!("{cause:#}");

                if !reported {
                    tracing::error!("migrations failed, the control plane stays up and reports not ready: {message}");
                    reported = true;
                }

                set_readiness(&state, Readiness::Failed);
                tokio::time::sleep(RETRY_INTERVAL).await;
            }
        }
    }
}

async fn apply_migrations(state: &AppState) -> Result<()> {
    let mut connection = state.pool.acquire().await?;

    sqlx::query("select pg_advisory_lock($1)").bind(MIGRATION_LOCK_KEY).execute(&mut *connection).await?;

    let migrated = MIGRATOR.run(&state.pool).await;
    let unlocked = sqlx::query("select pg_advisory_unlock($1)").bind(MIGRATION_LOCK_KEY).execute(&mut *connection).await;

    migrated?;
    unlocked?;

    Ok(())
}

pub fn is_ready(state: &AppState) -> bool {
    state.readiness.lock().is_ok_and(|current| *current == Readiness::Ready)
}

pub async fn run_while_ready<Tick, Ticked>(state: &AppState, name: &str, period: Duration, mut tick: Tick)
where
    Tick: FnMut() -> Ticked,
    Ticked: Future<Output = ApiResult<()>>,
{
    loop {
        if !is_ready(state) {
            tokio::time::sleep(period.min(RETRY_INTERVAL)).await;

            continue;
        }

        let ticked = tick().await;

        if let Err(cause) = ticked {
            tracing::error!("could not {name}: {}", cause.message());
        }

        tokio::time::sleep(period).await;
    }
}

pub async fn healthz() -> &'static str {
    "ok"
}

pub async fn version() -> Json<serde_json::Value> {
    Json(serde_json::json!({"version": VERSION}))
}

pub async fn readyz(State(state): State<AppState>) -> Response {
    let readiness = state.readiness.lock().map(|current| current.clone()).unwrap_or(Readiness::Migrating);

    let not_ready = |error: &str, message: String| (StatusCode::SERVICE_UNAVAILABLE, Json(ErrorBody { error: error.into(), message })).into_response();

    match readiness {
        Readiness::Migrating => not_ready("migrating", "The database is still being prepared.".into()),
        Readiness::Failed => not_ready("migrations_failed", "Migrations have not been applied.".into()),
        Readiness::Ready => match sqlx::query("select 1").execute(&state.pool).await {
            Ok(_) => Json(serde_json::json!({"status": "ready"})).into_response(),
            Err(cause) => {
                tracing::warn!("the readiness probe could not reach the database: {cause}");

                not_ready("database_unreachable", "The database is not reachable.".into())
            }
        },
    }
}
