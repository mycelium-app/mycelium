use super::call::{ToolCall, ToolReply};
use crate::error::{ApiError, ApiResult};
use base64::Engine;
use serde_json::{Value, json};

pub fn definitions() -> Value {
    json!([
        {
            "name": "set_project_icon",
            "description": "Set this conversation's project logo from an image file inside its working folder. Accepts PNG, JPEG or WebP up to 256 KB. Convert SVG to PNG first so the logo works on every client. This updates the project immediately.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "path": {"type": "string", "description": "The image file inside this conversation's working folder."},
                },
                "required": ["path"],
            },
        },
    ])
}

pub async fn call(call: ToolCall<'_>) -> ApiResult<ToolReply> {
    let ToolCall { caller, arguments, .. } = call;
    let state = caller.state;
    let path = arguments.get("path").and_then(Value::as_str).map(str::trim).filter(|path| !path.is_empty());
    let Some(path) = path else {
        return Ok(ToolReply::invalid("set_project_icon needs the image file's path."));
    };
    let Some((conversation_id, _)) = caller.conversation().await? else {
        return Ok(ToolReply::text("Set the logo from a conversation linked to a project.", true));
    };
    let owner = caller.owner().await?;
    let result = async {
        let conversation = crate::store::find_owned(&state.pool, conversation_id, owner).await?;
        let project = conversation.project_id.ok_or_else(|| ApiError::bad_request("project_required", "This conversation has no project. Link it to a project before setting its logo."))?;
        crate::projects::find_owned(state, project, owner).await?;
        let answer = crate::git::relay(state, conversation.node_id, conversation.cwd, mycelium_proto::link::GitCommand::ReadFile { path: path.to_string() }).await?;
        let encoded = answer["data"].as_str().unwrap_or_default();
        let bytes = base64::engine::general_purpose::STANDARD.decode(encoded).map_err(ApiError::internal)?;
        let media_type = if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
            "image/png"
        } else if bytes.starts_with(b"\xff\xd8\xff") {
            "image/jpeg"
        } else if bytes.starts_with(b"RIFF") && bytes.get(8..12) == Some(b"WEBP") {
            "image/webp"
        } else {
            return Err(ApiError::bad_request("avatar_not_image", "Choose a PNG, JPEG or WebP image. Convert SVG to PNG before setting the project logo."));
        };

        crate::projects::set_avatar(state, owner, project, media_type, &bytes).await
    }.await;

    Ok(match result {
        Ok(project) => ToolReply::text(&format!("Set the logo for project {} ({}).", project.name, project.id), false),
        Err(error) => ToolReply::text(error.message(), true),
    })
}
