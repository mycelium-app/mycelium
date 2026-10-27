use crate::chat_tool_servers::{ServerRow, bearer, list_owned};
use crate::error::ApiResult;
use crate::integration_proxy::api_base;
use crate::state::AppState;
use crate::store::append_event;
use mycelium_proto::conversation::EventKind;
use mycelium_proto::stream::StreamMessage;
use serde_json::json;
use std::time::Duration;
use uuid::Uuid;

const LOOKUP_TIMEOUT: Duration = Duration::from_secs(30);

fn document_id(given: &str) -> &str {
    let last = given.trim().trim_end_matches('/').rsplit('/').next().unwrap_or_default();

    if Uuid::parse_str(last).is_ok() {
        return last;
    }

    last.rsplit('-').next().unwrap_or(last)
}

fn choose<'a>(outlines: &[&'a ServerRow], integration: Option<&str>) -> Result<&'a ServerRow, String> {
    let names = || outlines.iter().map(|server| server.name.as_str()).collect::<Vec<_>>().join(", ");

    match (outlines, integration) {
        ([], _) => Err("This conversation has no Outline integration. Add one in Settings, Integrations.".to_string()),
        (_, Some(wanted)) => outlines
            .iter()
            .copied()
            .find(|server| server.id.to_string() == wanted || server.name.eq_ignore_ascii_case(wanted))
            .ok_or_else(|| format!("This conversation has no Outline integration called {wanted}. Use one of: {}.", names())),
        ([only], None) => Ok(only),
        (_, None) => Err(format!("This conversation has more than one Outline integration, so give integration as one of: {}.", names())),
    }
}

pub async fn open(state: &AppState, owner: Uuid, conversation_id: Uuid, group: Option<Uuid>, document: &str, integration: Option<&str>) -> ApiResult<Result<String, String>> {
    let servers = list_owned(state, owner).await?;
    let outlines: Vec<&ServerRow> = servers.iter().filter(|server| server.enabled && server.in_group(group) && api_base(server).is_some()).collect();
    let server = match choose(&outlines, integration) {
        Ok(server) => server,
        Err(message) => return Ok(Err(message)),
    };
    let Some(base) = api_base(server) else {
        return Ok(Err(format!("{} does not offer API access.", server.name)));
    };
    let credential = bearer(state, server).await?.unwrap_or_default();
    let looked_up = state.http.post(format!("{base}documents.info")).timeout(LOOKUP_TIMEOUT).bearer_auth(credential).json(&json!({"id": document_id(document)})).send().await;
    let response = match looked_up {
        Ok(response) if response.status().is_success() => response,
        Ok(_) => return Ok(Err(format!("{} has no document {document}. Give a document's id, urlId or address.", server.name))),
        Err(_) => return Ok(Err(format!("Mycelium could not reach {}.", server.name))),
    };
    let body: serde_json::Value = response.json().await.unwrap_or_default();
    let data = &body["data"];
    let (Some(title), Some(url_id)) = (data["title"].as_str(), data["urlId"].as_str()) else {
        return Ok(Err(format!("{} answered without a title for {document}.", server.name)));
    };
    let Ok(mut page) = url::Url::parse(&server.url) else {
        return Ok(Err(format!("{} has no usable address.", server.name)));
    };
    page.set_path(&format!("/doc/{url_id}"));
    page.set_query(None);
    page.set_fragment(None);

    let mut tx = state.pool.begin().await?;
    let event = append_event(&mut tx, conversation_id, EventKind::OpenOutline, &json!({"title": title, "url": page.as_str(), "integration_id": server.id, "integration_name": server.name}), None).await?;
    tx.commit().await?;

    if let Some(event) = event {
        state.publish(Some(owner), StreamMessage::Event(event));
    }

    Ok(Ok(format!("Opened {title} beside the conversation: {page}")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_document_is_found_by_id_url_id_or_address() {
        let id = "6a1b2c3d-0000-4000-8000-000000000001";

        assert_eq!(document_id(id), id);
        assert_eq!(document_id("aB3dE5fG"), "aB3dE5fG");
        assert_eq!(document_id("https://docs.example/doc/release-notes-aB3dE5fG/"), "aB3dE5fG");
        assert_eq!(document_id(&format!("https://docs.example/doc/{id}")), id);
    }
}
