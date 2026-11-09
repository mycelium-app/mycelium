use super::call::{ToolCall, ToolReply};
use crate::error::ApiResult;
use crate::schedules::{self, ProposedSchedule, preset_cron};
use mycelium_proto::api::EachRun;
use serde_json::{Value, json};

pub fn definitions() -> Value {
    json!([
        {
            "name": "propose_schedule",
            "description": "Set up a recurring run of a prompt when the person asks you to schedule something, run it every day, every weekday, every hour and so on. Always use this for Mycelium work, never crontab, systemd timers or sleep loops. It waits for the person to confirm in a card in this conversation and never runs until they do. By default each run is queued in this conversation.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "prompt": {"type": "string", "description": "The prompt to run on each occurrence. Write it so it makes sense without the earlier conversation."},
                    "preset": {"type": "string", "description": "When to run, as \"weekdays at 20:00\", \"daily at 09:00\", \"every 15 minutes\" or \"every 2 hours\". Give this or cron."},
                    "cron": {"type": "string", "description": "When to run, as five cron fields, for example 0 20 * * 1-5. Evaluated in time_zone. Used when preset is not given."},
                    "each_run": {"type": "string", "enum": ["same", "new"], "description": "same (default) queues each run in this conversation. new starts a new conversation for each run."},
                    "time_zone": {"type": "string", "description": "An IANA time zone such as Europe/London. Defaults to the person's own."},
                },
                "required": ["prompt"],
            },
        },
    ])
}

pub async fn call(call: ToolCall<'_>) -> ApiResult<ToolReply> {
    let ToolCall { caller, arguments, .. } = call;
    let state = caller.state;

    let prompt = arguments.get("prompt").and_then(Value::as_str).map(str::trim).unwrap_or_default();

    if prompt.is_empty() {
        return Ok(ToolReply::text("Nothing was proposed. Give the prompt to run.", true));
    }

    let preset = arguments.get("preset").and_then(Value::as_str).map(str::trim).filter(|text| !text.is_empty());
    let cron = match (preset, arguments.get("cron").and_then(Value::as_str).map(str::trim).filter(|text| !text.is_empty())) {
        (Some(preset), _) => match preset_cron(preset) {
            Some(cron) => cron,
            None => return Ok(ToolReply::text("Nothing was proposed. Use a preset such as \"weekdays at 20:00\", \"daily at 09:00\", \"every 15 minutes\" or \"every 2 hours\", or give cron.", true)),
        },
        (None, Some(cron)) => cron.to_string(),
        (None, None) => return Ok(ToolReply::text("Nothing was proposed. Give a preset or a cron expression.", true)),
    };
    let each_run = match arguments.get("each_run").and_then(Value::as_str) {
        None | Some("same") => EachRun::Same,
        Some("new") => EachRun::New,
        Some(_) => return Ok(ToolReply::text("Nothing was proposed. each_run is same or new.", true)),
    };
    let Some((conversation_id, _)) = caller.conversation().await? else {
        return Ok(ToolReply::text("Nothing was proposed. A schedule belongs to a conversation, and this call did not come from one.", true));
    };
    let time_zone = arguments.get("time_zone").and_then(Value::as_str).map(str::trim).filter(|text| !text.is_empty());

    Ok(match schedules::propose(state, conversation_id, ProposedSchedule { cron, prompt, each_run, time_zone }).await? {
        Ok(schedule) => ToolReply::text(&format!("Schedule {} proposed ({}, {}). It waits for the person to confirm and will not run until they do.", schedule.id, schedule.cron, schedule.time_zone), false),
        Err(message) => ToolReply::text(&message, true),
    })
}
