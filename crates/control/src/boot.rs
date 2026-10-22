use crate::config::Config;
use crate::state::{AppState, Hub, Readiness};
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
const RETRY_INTERVAL: Duration = Duration::from_secs(5);
static MIGRATOR: Migrator = sqlx::migrate!();

pub fn new_state(config: Config) -> Result<AppState> {
    let pool = PgPoolOptions::new().max_connections(10).acquire_timeout(Duration::from_secs(3)).connect_lazy(&config.database_url)?;
    let (stream, _) = broadcast::channel(1024);

    Ok(AppState {
        pool,
        config: Arc::new(config),
        hub: Arc::new(Hub::default()),
        stream,
        readiness: Arc::new(Mutex::new(Readiness::Migrating)),
        http: reqwest::Client::builder().timeout(Duration::from_secs(10)).build()?,
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

pub async fn healthz() -> &'static str {
    "ok"
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
