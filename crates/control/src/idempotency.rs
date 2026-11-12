use crate::error::ApiResult;
use crate::state::AppState;
use sqlx::{Postgres, Transaction};
use uuid::Uuid;

pub struct Claim {
    tx: Transaction<'static, Postgres>,
    scope: Uuid,
    client_id: Uuid,
}

pub enum Entry {
    Fresh(Option<Claim>),
    Repeat(Option<Uuid>),
}

pub async fn enter(state: &AppState, scope: Uuid, client_id: Option<Uuid>) -> ApiResult<Entry> {
    let Some(client_id) = client_id else {
        return Ok(Entry::Fresh(None));
    };

    let mut tx = state.pool.begin().await?;
    sqlx::query("select pg_advisory_xact_lock(hashtextextended($1, 0))").bind(format!("{scope}/{client_id}")).execute(&mut *tx).await?;
    let earlier = sqlx::query_as::<_, (Option<Uuid>,)>("select result_id from client_requests where scope = $1 and client_id = $2").bind(scope).bind(client_id).fetch_optional(&mut *tx).await?;

    match earlier {
        Some((result_id,)) => Ok(Entry::Repeat(result_id)),
        None => Ok(Entry::Fresh(Some(Claim { tx, scope, client_id }))),
    }
}

impl Claim {
    pub async fn done(mut self, result_id: Option<Uuid>) -> ApiResult<()> {
        sqlx::query("insert into client_requests (scope, client_id, result_id) values ($1, $2, $3) on conflict (scope, client_id) do update set result_id = excluded.result_id")
            .bind(self.scope)
            .bind(self.client_id)
            .bind(result_id)
            .execute(&mut *self.tx)
            .await?;
        self.tx.commit().await?;

        Ok(())
    }
}
