use crate::error::ApiResult;
use crate::state::AppState;
use crate::store::{CONVERSATION_GROUP_SQL, parse_enum};
use axum::http::HeaderMap;
use mycelium_proto::api::ModelClass;
use serde_json::Value;
use uuid::Uuid;

#[derive(Clone, Copy)]
pub struct Caller<'a> {
    pub state: &'a AppState,
    pub node_id: Uuid,
    pub headers: &'a HeaderMap,
}

pub struct ToolCall<'a> {
    pub caller: Caller<'a>,
    pub tool: &'a str,
    pub params: &'a Value,
    pub arguments: Value,
}

pub enum ToolReply {
    Text { text: String, is_error: bool },
    Image { png: String },
    Invalid(String),
}

impl ToolReply {
    pub fn text(text: &str, is_error: bool) -> Self {
        ToolReply::Text { text: text.to_string(), is_error }
    }

    pub fn invalid(message: &str) -> Self {
        ToolReply::Invalid(message.to_string())
    }
}

pub async fn node_owner(state: &AppState, node_id: Uuid) -> ApiResult<Uuid> {
    Ok(sqlx::query_scalar::<_, Uuid>("select created_by from nodes where id = $1").bind(node_id).fetch_one(&state.pool).await?)
}

impl Caller<'_> {
    pub async fn owner(&self) -> ApiResult<Uuid> {
        node_owner(self.state, self.node_id).await
    }

    pub async fn conversation(&self) -> ApiResult<Option<(Uuid, String)>> {
        let claimed = self.headers.get("x-mycelium-conversation").and_then(|value| value.to_str().ok()).and_then(|value| value.parse::<Uuid>().ok());

        match claimed {
            Some(conversation_id) => Ok(sqlx::query_as::<_, (Uuid, String)>("select id, title from conversations where id = $1 and node_id = $2").bind(conversation_id).bind(self.node_id).fetch_optional(&self.state.pool).await?),
            None => Ok(None),
        }
    }

    pub async fn conversation_id(&self) -> ApiResult<Option<Uuid>> {
        Ok(self.conversation().await?.map(|(conversation_id, _)| conversation_id))
    }

    pub async fn group(&self) -> ApiResult<Option<(Uuid, String)>> {
        let Some((conversation_id, _)) = self.conversation().await? else {
            return Ok(None);
        };

        Ok(sqlx::query_as::<_, (Uuid, String)>(sqlx::AssertSqlSafe(format!("select g.id, g.name from conversations c join groups g on g.id = {CONVERSATION_GROUP_SQL} where c.id = $1"))).bind(conversation_id).fetch_optional(&self.state.pool).await?)
    }

    pub async fn group_id(&self) -> ApiResult<Option<Uuid>> {
        Ok(self.group().await?.map(|(group_id, _)| group_id))
    }

    pub async fn model_class(&self) -> ApiResult<ModelClass> {
        let Some((conversation_id, _)) = self.conversation().await? else {
            return Ok(ModelClass::Cloud);
        };
        let stored = sqlx::query_scalar::<_, String>("select p.model_class from conversations c join model_providers p on p.id = c.provider_id where c.id = $1").bind(conversation_id).fetch_optional(&self.state.pool).await?;

        match stored {
            Some(class) => parse_enum(&class),
            None => Ok(ModelClass::Cloud),
        }
    }
}
