use crate::auth::AuthUser;
use crate::chat_tool_servers::{DEFAULT_KEY_HEADER, fetch_tools};
use crate::error::{ApiError, ApiResult};
use crate::mcp_oauth::offers_sign_in;
use crate::state::AppState;
use axum::Json;
use axum::extract::State;

#[derive(serde::Deserialize)]
pub struct ProbeRequest {
    url: String,
}

#[derive(serde::Serialize)]
pub struct Probe {
    oauth: bool,
}

#[derive(serde::Deserialize)]
pub struct TestRequest {
    url: String,
    #[serde(default)]
    credential: Option<String>,
}

#[derive(serde::Serialize)]
pub struct TestedTool {
    name: String,
    kind: &'static str,
}

#[derive(serde::Serialize)]
pub struct Tested {
    tools: Vec<TestedTool>,
}

fn address(entered: &str) -> ApiResult<url::Url> {
    url::Url::parse(entered.trim()).ok().filter(|parsed| matches!(parsed.scheme(), "http" | "https") && parsed.host_str().is_some()).ok_or_else(|| ApiError::bad_request("url_invalid", "Enter the server's address, starting with http:// or https://, for example https://example.com/mcp."))
}

pub async fn probe(State(state): State<AppState>, _user: AuthUser, Json(request): Json<ProbeRequest>) -> ApiResult<Json<Probe>> {
    let parsed = address(&request.url)?;
    let oauth = offers_sign_in(&state, &parsed).await;

    Ok(Json(Probe { oauth }))
}

pub async fn test(State(state): State<AppState>, _user: AuthUser, Json(request): Json<TestRequest>) -> ApiResult<Json<Tested>> {
    let parsed = address(&request.url)?;
    let token = request.credential.as_deref().map(str::trim).filter(|token| !token.is_empty());
    let tools = fetch_tools(&state, parsed.as_str(), parsed.as_str(), token, DEFAULT_KEY_HEADER, None).await?;
    let tested = tools
        .into_iter()
        .map(|tool| TestedTool { kind: if tool.read_only == Some(true) { "read" } else { "write" }, name: tool.name })
        .collect();

    Ok(Json(Tested { tools: tested }))
}
