use crate::error::{ApiError, ApiResult};
use mycelium_proto::api::ArtifactKind;
use serde_json::{Value, json};

const DRAWING_MEDIA_TYPE: &str = "application/vnd.excalidraw+json";
const TYPST_MEDIA_TYPE: &str = "text/vnd.typst";
const DOC_MEDIA_TYPE: &str = "application/vnd.mycelium.doc+json";
const SLIDES_MEDIA_TYPE: &str = "application/vnd.mycelium.slides+json";
const DASHBOARD_MEDIA_TYPE: &str = "application/vnd.grafana.dashboard+json";
const MOTION_MEDIA_TYPE: &str = "application/vnd.mycelium.motion+json";
const DESIGN_SYSTEM_MEDIA_TYPE: &str = "application/vnd.mycelium.design-system+json";
const DOC_BLOCK_TYPES: [&str; 6] = ["paragraph", "heading1", "heading2", "heading3", "bullet", "numbered"];
const MOTION_SHAPES: [&str; 3] = ["rect", "ellipse", "text"];

pub fn media_type(kind: ArtifactKind) -> Option<&'static str> {
    match kind {
        ArtifactKind::Drawing => Some(DRAWING_MEDIA_TYPE),
        ArtifactKind::Typst => Some(TYPST_MEDIA_TYPE),
        ArtifactKind::Doc => Some(DOC_MEDIA_TYPE),
        ArtifactKind::Slides => Some(SLIDES_MEDIA_TYPE),
        ArtifactKind::Dashboard => Some(DASHBOARD_MEDIA_TYPE),
        ArtifactKind::Motion => Some(MOTION_MEDIA_TYPE),
        ArtifactKind::DesignSystem => Some(DESIGN_SYSTEM_MEDIA_TYPE),
        ArtifactKind::Design | ArtifactKind::Page | ArtifactKind::Markdown | ArtifactKind::File => None,
    }
}

pub fn is_json_kind(kind: ArtifactKind) -> bool {
    media_type(kind).is_some_and(|media_type| media_type != TYPST_MEDIA_TYPE)
}

pub fn default_content(kind: ArtifactKind, title: &str) -> Option<Vec<u8>> {
    let value = match kind {
        ArtifactKind::Drawing => json!({"type": "excalidraw", "version": 2, "source": "mycelium", "elements": [], "appState": {}, "files": {}}),
        ArtifactKind::Typst => return Some(format!("= {title}\n").into_bytes()),
        ArtifactKind::Doc => json!({"blocks": [{"type": "paragraph", "spans": []}]}),
        ArtifactKind::Slides => json!({"slides": [{"id": "s1", "title": title, "body": [], "notes": ""}]}),
        ArtifactKind::Dashboard => json!({"title": title, "panels": []}),
        ArtifactKind::Motion => json!({"duration": 4000, "width": 1280, "height": 720, "background": "#101418", "layers": []}),
        ArtifactKind::DesignSystem => json!({"colors": [], "spacing": [], "radius": [], "type": [], "components": []}),
        ArtifactKind::Design | ArtifactKind::Page | ArtifactKind::Markdown | ArtifactKind::File => return None,
    };

    Some(value.to_string().into_bytes())
}

fn invalid(code: &'static str, message: &str) -> ApiError {
    ApiError::bad_request(code, message.to_string())
}

fn list<'a>(value: &'a Value, field: &str, code: &'static str, message: &str) -> ApiResult<&'a Vec<Value>> {
    value[field].as_array().ok_or_else(|| invalid(code, message))
}

fn parsed(bytes: &[u8], code: &'static str, message: &str) -> ApiResult<Value> {
    let value: Value = serde_json::from_slice(bytes).map_err(|_| invalid(code, message))?;

    if !value.is_object() {
        return Err(invalid(code, message));
    }

    Ok(value)
}

pub fn normalise(kind: ArtifactKind, bytes: Vec<u8>) -> ApiResult<Vec<u8>> {
    match kind {
        ArtifactKind::Typst if std::str::from_utf8(&bytes).is_err() => Err(invalid("content_not_text", "A Typst document must be text in UTF-8.")),
        ArtifactKind::Drawing => {
            let message = "The drawing must be Excalidraw JSON with an elements list.";
            let value = parsed(&bytes, "drawing_invalid", message)?;

            list(&value, "elements", "drawing_invalid", message)?;

            Ok(bytes)
        }
        ArtifactKind::Doc => {
            let message = "The doc must be JSON with a blocks list, and each block needs a type (paragraph, heading1, heading2, heading3 or bullet) and a spans list.";
            let value = parsed(&bytes, "doc_invalid", message)?;
            let blocks = list(&value, "blocks", "doc_invalid", message)?;
            let well_formed = blocks.iter().all(|block| block["type"].as_str().is_some_and(|name| DOC_BLOCK_TYPES.contains(&name)) && block["spans"].is_array());

            if !well_formed {
                return Err(invalid("doc_invalid", message));
            }

            Ok(bytes)
        }
        ArtifactKind::Slides => {
            let message = "The deck must be JSON with a slides list, and each slide needs an id, a title and a body list.";
            let value = parsed(&bytes, "slides_invalid", message)?;
            let slides = list(&value, "slides", "slides_invalid", message)?;

            if slides.is_empty() || !slides.iter().all(|slide| slide["id"].is_string() && slide["title"].is_string() && slide["body"].is_array()) {
                return Err(invalid("slides_invalid", message));
            }

            Ok(bytes)
        }
        ArtifactKind::Dashboard => {
            let value = parsed(&bytes, "dashboard_invalid", "The dashboard must be Grafana dashboard JSON with a panels list.")?;

            serde_json::to_vec(&normalise_dashboard(value)?).map_err(ApiError::internal)
        }
        ArtifactKind::Motion => {
            let message = "The scene must be JSON with a duration in milliseconds and a layers list, and each layer needs an id, a name and a shape (rect, ellipse or text).";
            let value = parsed(&bytes, "motion_invalid", message)?;
            let layers = list(&value, "layers", "motion_invalid", message)?;
            let well_formed = value["duration"].as_f64().is_some_and(|duration| duration > 0.0) && layers.iter().all(|layer| layer["id"].is_string() && layer["name"].is_string() && layer["shape"].as_str().is_some_and(|shape| MOTION_SHAPES.contains(&shape)));

            if !well_formed {
                return Err(invalid("motion_invalid", message));
            }

            Ok(bytes)
        }
        ArtifactKind::DesignSystem => {
            let message = "The design system must be JSON with colors, spacing, radius, type and components lists, and each token needs a token name and a value.";
            let value = parsed(&bytes, "design_system_invalid", message)?;
            let tokens = tokens_of(&value);

            for field in ["colors", "spacing", "radius", "type", "components"] {
                if !value[field].is_null() && !value[field].is_array() {
                    return Err(invalid("design_system_invalid", message));
                }
            }

            if tokens.iter().any(|(name, value)| name.is_empty() || value.is_empty() || !name.chars().all(|character| character.is_ascii_alphanumeric() || character == '-' || character == '_')) {
                return Err(invalid("design_system_invalid", message));
            }

            Ok(bytes)
        }
        _ => Ok(bytes),
    }
}

fn normalise_dashboard(mut dashboard: Value) -> ApiResult<Value> {
    let message = "The dashboard must be Grafana dashboard JSON with a panels list, and each panel needs a type.";
    let object = dashboard.as_object_mut().ok_or_else(|| invalid("dashboard_invalid", message))?;

    for field in ["__inputs", "__requires", "__elements"] {
        object.remove(field);
    }

    object.entry("title").or_insert_with(|| json!("Dashboard"));
    object.entry("schemaVersion").or_insert_with(|| json!(39));
    object.entry("time").or_insert_with(|| json!({"from": "now-6h", "to": "now"}));

    let panels = object.get_mut("panels").and_then(Value::as_array_mut).ok_or_else(|| invalid("dashboard_invalid", message))?;

    if !panels.iter().all(|panel| panel["type"].is_string()) {
        return Err(invalid("dashboard_invalid", message));
    }

    let mut next_id = panels.iter().filter_map(|panel| panel["id"].as_i64()).max().unwrap_or(0) + 1;

    for (index, panel) in panels.iter_mut().enumerate() {
        let panel = panel.as_object_mut().ok_or_else(|| invalid("dashboard_invalid", message))?;

        if !panel.get("id").is_some_and(Value::is_i64) {
            panel.insert("id".into(), json!(next_id));
            next_id += 1;
        }

        panel.entry("title").or_insert_with(|| json!(""));
        panel.entry("gridPos").or_insert_with(|| json!({"x": (index * 8) % 24, "y": (index * 8 / 24) * 4, "w": 8, "h": 4}));
    }

    Ok(dashboard)
}

pub fn tokens_of(system: &Value) -> Vec<(String, String)> {
    let mut tokens = Vec::new();

    for field in ["colors", "spacing", "radius"] {
        for entry in system[field].as_array().into_iter().flatten() {
            tokens.push((entry["token"].as_str().unwrap_or_default().to_string(), entry["value"].as_str().unwrap_or_default().to_string()));
        }
    }

    for entry in system["type"].as_array().into_iter().flatten() {
        tokens.push((entry["token"].as_str().unwrap_or_default().to_string(), entry["size"].as_str().unwrap_or_default().to_string()));
    }

    tokens
}

pub fn design_system_style(system: &Value) -> String {
    let declarations: String = tokens_of(system).iter().map(|(name, value)| format!("--{name}:{value};")).collect();

    format!("<style data-design-system>:root{{{declarations}}}</style>")
}

pub fn with_design_system_style(html: &str, style: &str) -> String {
    let stripped = strip_design_system_style(html);

    match stripped.to_ascii_lowercase().find("</head>") {
        Some(position) => format!("{}{style}{}", &stripped[..position], &stripped[position..]),
        None => format!("{style}{stripped}"),
    }
}

fn strip_design_system_style(html: &str) -> String {
    let Some(start) = html.find("<style data-design-system>") else {
        return html.to_string();
    };
    let Some(length) = html[start..].find("</style>") else {
        return html.to_string();
    };

    format!("{}{}", &html[..start], &html[start + length + "</style>".len()..])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_dashboard_import_drops_grafana_export_inputs_and_fills_the_gaps() {
        let imported = normalise(ArtifactKind::Dashboard, json!({"__inputs": [{"name": "DS"}], "title": "Backups", "panels": [{"type": "stat", "title": "Last"}, {"type": "table", "id": 7}]}).to_string().into_bytes()).unwrap();
        let value: Value = serde_json::from_slice(&imported).unwrap();

        assert!(value.get("__inputs").is_none());
        assert_eq!(value["schemaVersion"], 39);
        assert_eq!(value["panels"][0]["id"], 8);
        assert_eq!(value["panels"][1]["id"], 7);
        assert_eq!(value["panels"][1]["gridPos"]["x"], 8);
    }

    #[test]
    fn normalising_a_dashboard_twice_changes_nothing() {
        let once = normalise(ArtifactKind::Dashboard, json!({"panels": [{"type": "timeseries"}]}).to_string().into_bytes()).unwrap();
        let twice = normalise(ArtifactKind::Dashboard, once.clone()).unwrap();

        assert_eq!(serde_json::from_slice::<Value>(&once).unwrap(), serde_json::from_slice::<Value>(&twice).unwrap());
    }

    #[test]
    fn the_design_system_style_replaces_the_one_already_in_the_artboard() {
        let system = json!({"colors": [{"token": "brand", "value": "#11305A"}], "type": [{"token": "body", "size": "15px"}]});
        let first = with_design_system_style("<html><head></head><body>x</body></html>", &design_system_style(&system));
        let second = with_design_system_style(&first, &design_system_style(&system));

        assert!(first.contains("--brand:#11305A;--body:15px;"));
        assert_eq!(first, second);
    }
}
