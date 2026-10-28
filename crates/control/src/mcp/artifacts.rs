use super::call::{ToolCall, ToolReply};
use crate::artifacts;
use crate::error::ApiResult;
use serde_json::{Value, json};
use uuid::Uuid;

pub fn definitions() -> Value {
    json!([
        {
            "name": "find_artifacts",
            "description": "Find artefacts this conversation or its project can view or edit. Returns ids, slugs, titles and access. Use request_artifact_access for another known artefact.",
            "inputSchema": {"type": "object", "properties": {"query": {"type": "string"}}, "additionalProperties": false}
        },
        {
            "name": "request_artifact_access",
            "description": "Ask the person to grant this conversation view or edit access to an artefact. A durable approval card appears in Artefacts. Access is granted only after approval.",
            "inputSchema": {"type": "object", "properties": {"artifact": {"type": "string"}, "permission": {"type": "string", "enum": ["view", "edit"]}, "reason": {"type": "string"}}, "required": ["artifact", "permission", "reason"], "additionalProperties": false}
        },
        {
            "name": "create_artifact",
            "description": "Save something the person should look at and come back to as an artifact: a design (artboards on a canvas), a page (one HTML file, which may keep state in the artifact's data store), a markdown document, a drawing (Excalidraw JSON), a typst document (Typst source, shown beside its rendered pages), a doc (basic rich text as JSON {\"blocks\": [{\"type\": \"paragraph|heading1|heading2|heading3|bullet|numbered\", \"spans\": [{\"text\", \"bold\"?, \"italic\"?, \"link\"?}]}]}), slides (JSON {\"slides\": [{\"id\", \"title\", \"body\": [lines], \"notes\"}]} with a presenter view), a dashboard (Grafana dashboard JSON with stat, timeseries and table panels, importable into Grafana as it is), a motion scene (JSON {\"duration\": ms, \"width\", \"height\", \"background\", \"layers\": [{\"id\", \"name\", \"shape\": \"rect|ellipse|text\", \"text\"?, \"fill\", \"keyframes\": [{\"t\": ms, \"x\", \"y\", \"w\", \"h\", \"opacity\", \"rotation\"}]}]}), a design_system (JSON {\"colors\": [{\"name\", \"token\", \"value\"}], \"spacing\": [{\"token\", \"value\"}], \"radius\": [...], \"type\": [{\"token\", \"size\", \"name\"}], \"components\": [{\"name\", \"html\"}]} that designs use), or any other file such as a PDF. A kind other than design or file without content starts empty. Give the content as text, or the path of a file in this conversation's folder. A design without content starts with one empty page. The person opens it in Mycelium, comments on it and edits it.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "kind": {"type": "string", "enum": ["design", "page", "markdown", "file", "drawing", "typst", "doc", "slides", "dashboard", "motion", "design_system"], "description": "design, page (HTML), markdown, file (anything else), drawing, typst, doc, slides, dashboard, motion or design_system."},
                    "title": {"type": "string", "description": "A short title."},
                    "content": {"type": "string", "description": "The content as text. A design is JSON, {\"pages\": [{\"id\", \"name\", \"items\": [...]}]}."},
                    "path": {"type": "string", "description": "A file in this conversation's folder to store instead of content. Use it for binary files."},
                    "media_type": {"type": "string", "description": "The media type of a file. Defaults to one guessed from the path."},
                    "design_system": {"type": "string", "description": "For a design: the id or slug of a design_system artifact whose tokens its artboards use."},
                },
                "required": ["kind", "title"],
            },
        },
        {
            "name": "update_artifact",
            "description": "Save a new version of an artifact with the complete new content, as text or from a file in this conversation's folder. Earlier versions are kept. To change one artboard of a design use design_set_artboard instead.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "artifact": {"type": "string", "description": "The artifact's id or slug."},
                    "content": {"type": "string", "description": "The complete new content as text."},
                    "path": {"type": "string", "description": "A file in this conversation's folder to store instead of content."},
                    "note": {"type": "string", "description": "A short note saying what changed."},
                },
                "required": ["artifact"],
            },
        },
        {
            "name": "design_set_artboard",
            "description": "Replace or add one artboard in the latest version of a design, and save a new version. Everything else in the design stays as it is, including what the person drew. Give artboard to replace an existing one, or leave it out to add a new one beside the others. The html is a complete document, shown in a sandboxed frame.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "artifact": {"type": "string", "description": "The design's id or slug."},
                    "page": {"type": "string", "description": "The id of the page. Defaults to the first page."},
                    "artboard": {"type": "string", "description": "The id of the artboard to replace. A new artboard is added when it is left out."},
                    "name": {"type": "string", "description": "The artboard's name."},
                    "x": {"type": "number", "description": "Left edge on the canvas."},
                    "y": {"type": "number", "description": "Top edge on the canvas."},
                    "w": {"type": "number", "description": "Width. Defaults to 1440 for a new artboard."},
                    "h": {"type": "number", "description": "Height. Defaults to 900 for a new artboard."},
                    "html": {"type": "string", "description": "The artboard's complete HTML document."},
                },
                "required": ["artifact", "html"],
            },
        },
        {
            "name": "design_remove_item",
            "description": "Remove one item (an artboard, shape, text, note or drawing) from a design by its id, and save a new version.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "artifact": {"type": "string", "description": "The design's id or slug."},
                    "item": {"type": "string", "description": "The id of the item to remove. read_artifact lists them."},
                },
                "required": ["artifact", "item"],
            },
        },
        {
            "name": "read_artifact",
            "description": "Read the latest version of an artifact: the text of a page or document, a summary of a file, or for a design its pages and items with ids, names and sizes. Give artboard to read the full HTML of one artboard of a design.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "artifact": {"type": "string", "description": "The artifact's id or slug."},
                    "artboard": {"type": "string", "description": "The id or name of an artboard of a design."},
                },
                "required": ["artifact"],
            },
        },
        {
            "name": "read_artifact_feedback",
            "description": "Read what people left on an artifact: its unresolved comments with where they point, and the keys and values its data store holds. Use it to read back the choices a person made on an interactive page.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "artifact": {"type": "string", "description": "The artifact's id or slug."},
                },
                "required": ["artifact"],
            },
        },
    ])
}

pub async fn call(call: ToolCall<'_>) -> ApiResult<ToolReply> {
    let ToolCall { caller, tool, arguments, .. } = call;
    let state = caller.state;

    let Some((conversation_id, _)) = caller.conversation().await? else {
        return Ok(ToolReply::text("An artifact belongs to a conversation, and this call did not come from one.", true));
    };
    let owner = sqlx::query_scalar::<_, Uuid>("select owner from conversations where id = $1").bind(conversation_id).fetch_one(&state.pool).await?;

    Ok(match artifacts::tool(state, owner, conversation_id, tool, &arguments).await {
        Ok(text) => ToolReply::text(&text, false),
        Err(error) => ToolReply::text(error.message(), true),
    })
}
