use super::call::{ToolCall, ToolReply};
use crate::chat_tool_servers::{bearer, list_owned, refreshable_after_rejection, renew, slug};
use crate::chat_tools;
use crate::error::ApiResult;
use crate::mcp_client::{McpConnection, McpError};
use crate::response_editor;
use mycelium_proto::api::IntegrationStatus;
use uuid::Uuid;

pub async fn call(call: ToolCall<'_>) -> ApiResult<ToolReply> {
    let ToolCall { caller, tool: name, mut arguments, .. } = call;
    let state = caller.state;
    let owner = caller.owner().await?;
    let servers = list_owned(state, owner).await?;
    let found = servers.iter().find_map(|server| name.strip_prefix(&format!("{}__", slug(&server.name))).map(|tool| (server, tool)));
    let Some((server, tool)) = found else {
        return Ok(ToolReply::invalid("Unknown tool. Use a tool from tools/list."));
    };

    let group = caller.group_id().await?;

    if !server.in_group(group) {
        return Ok(ToolReply::text(&format!("{} is not available in this group. Change its availability in Settings, Integrations.", server.name), true));
    }

    if !server.enabled {
        return Ok(ToolReply::text(&format!("{} is switched off in Mycelium. Switch it on in the tool server settings, then try again.", server.name), true));
    }

    if server.status() == IntegrationStatus::Expired {
        return Ok(ToolReply::text(&chat_tools::expired_text(&server.name), true));
    }

    let conversation = caller.conversation_id().await?;
    let mut decision = "auto";
    let mut answer = None;
    let filter = response_editor::take_filter(&mut arguments);
    let effective = server.effective(tool);
    let asks = effective.ask && (server.integration().is_some() || effective.ask_source != crate::integration_policy::Source::Preset);

    if asks || effective.edit {
        let Some(conversation_id) = conversation else {
            let refusal = if asks { "This tool needs a person's approval, and this call did not come from a conversation. Nothing was done." } else { response_editor::NO_CONVERSATION_TEXT };

            return Ok(ToolReply::text(refusal, true));
        };

        if asks {
            let request_id = Uuid::new_v4().to_string();
            let approval = chat_tools::Approval { title: name, input: &arguments, server: &server.name, account: server.account.as_deref(), filter: filter.as_deref(), edits_by_setting: effective.edit };
            let answered = chat_tools::request_approval(state, owner, conversation_id, &request_id, &approval).await?;

            if answered == chat_tools::ALWAYS {
                chat_tools::stop_asking(state, owner, server, tool).await?;
            }

            if !chat_tools::is_approved(&answered) {
                chat_tools::record_call(state, owner, server, tool, conversation, "workspace", None, "declined", false, None).await;

                return Ok(ToolReply::text(chat_tools::DECLINED_TEXT, true));
            }

            decision = "approved";
            answer = Some(answered);
        }
    }

    let arguments = match crate::file_arguments::substitute(state, owner, conversation, tool, arguments).await {
        Ok(arguments) => arguments,
        Err(message) => return Ok(ToolReply::text(&message, true)),
    };
    let edits = conversation.is_some() && chat_tools::edits_response(answer.as_deref(), effective.edit);
    let (mut text, is_error) = if let Some(integration) = server.integration() {
        let output = integration.call(state, owner, server, conversation, tool, &arguments).await;

        (output.text, output.is_error)
    } else {
        let token = match bearer(state, server).await {
            Ok(token) => token,
            Err(error) => return Ok(ToolReply::text(error.message(), true)),
        };
        let mut connection = McpConnection::new(&state.http, &server.url, token.as_deref(), server.key_header());

        let mut called = connection.call_tool(tool, &arguments).await;

        if matches!(called, Err(McpError::Rejected)) && refreshable_after_rejection(server) {
            match renew(state, server).await {
                Ok(token) => {
                    connection.set_token(token.as_deref());
                    called = connection.call_tool(tool, &arguments).await;
                }
                Err(error) => return Ok(ToolReply::text(error.message(), true)),
            }
        }

        match called {
            Ok(output) => (output.text, output.is_error),
            Err(error) => (error.describe(&server.name), true),
        }
    };
    let mut review = None;

    if let (true, false, Some(conversation_id)) = (edits, is_error, conversation) {
        let held = response_editor::held_from(tool, &server.name, server.account.as_deref(), filter, &text);
        let sent = response_editor::hold(state, owner, conversation_id, &Uuid::new_v4().to_string(), held, text).await?;
        text = sent.text;
        review = Some(sent.outcome);
    }

    chat_tools::record_call(state, owner, server, tool, conversation, "workspace", None, decision, is_error, review).await;

    Ok(ToolReply::text(&text, is_error))
}
