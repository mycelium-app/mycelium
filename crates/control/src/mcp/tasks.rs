use super::call::{ToolCall, ToolReply};
use crate::error::ApiResult;
use crate::schedules::{self, ProposedSchedule, preset_cron};
use crate::tasks;
use mycelium_proto::api::{EachRun, TaskStatus};
use serde_json::{Value, json};
use uuid::Uuid;

pub fn definitions() -> Value {
    json!([
        {
            "name": "propose_task",
            "description": "Propose a task for the person to approve. It waits in the Proposed column of their Tasks board until they approve or decline it. Use it for follow-up work you found but are not doing now.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "title": {"type": "string", "description": "A short title for the task."},
                    "body": {"type": "string", "description": "What the task involves. It becomes the prompt of the conversation that works on it."},
                    "project": {"type": "string", "description": "The name of the project the task belongs to. Defaults to this conversation's project."},
                    "depends_on": {"type": "array", "items": {"type": "string"}, "description": "Ids of tasks that must be done before this one can be started."},
                },
                "required": ["title"],
            },
        },
        {
            "name": "ask_question",
            "description": "Ask the person a question that you need answered before you can go on. It appears in the Questions column of their Tasks board. End your turn after asking: the answer arrives in this conversation as a new message.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "question": {"type": "string", "description": "The question."},
                    "options": {"type": "array", "items": {"type": "string"}, "description": "Suggested answers the person can pick."},
                },
                "required": ["question"],
            },
        },
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
    let ToolCall { caller, tool, arguments, .. } = call;
    let state = caller.state;

    Ok(match tool {
        "propose_task" | "ask_question" => {
            let (field, status) = if tool == "propose_task" { ("title", TaskStatus::Proposed) } else { ("question", TaskStatus::Question) };
            let Some(title) = arguments.get(field).and_then(Value::as_str).map(str::trim) else {
                return Ok(ToolReply::invalid(&format!("{tool} needs {field}, a string.")));
            };

            if title.is_empty() {
                return Ok(ToolReply::text(&format!("Nothing was stored. Give the {field}."), true));
            }

            let Some((conversation_id, _)) = caller.conversation().await? else {
                return Ok(ToolReply::text("Nothing was stored. A task belongs to a conversation, and this call did not come from one.", true));
            };
            let body = arguments.get("body").and_then(Value::as_str).map(str::trim).filter(|text| !text.is_empty());
            let options: Vec<String> = arguments.get("options").and_then(Value::as_array).into_iter().flatten().filter_map(Value::as_str).map(str::trim).filter(|option| !option.is_empty()).map(String::from).collect();
            let project = arguments.get("project").and_then(Value::as_str).map(str::trim).filter(|name| !name.is_empty());

            let mut depends_on = Vec::new();

            for value in arguments.get("depends_on").and_then(Value::as_array).into_iter().flatten() {
                let Some(task_id) = value.as_str().and_then(|text| Uuid::parse_str(text.trim()).ok()) else {
                    return Ok(ToolReply::text("Nothing was stored. depends_on must be a list of task ids.", true));
                };

                depends_on.push(task_id);
            }

            match tasks::record(state, conversation_id, status, title, body, &options, project, &depends_on).await? {
                Ok(task) if status == TaskStatus::Question => ToolReply::text(&format!("Question {} asked. End your turn: the answer arrives here as a new message.", task.id), false),
                Ok(task) => ToolReply::text(&format!("Task {} proposed. The person can approve it from their Tasks board.", task.id), false),
                Err(message) => ToolReply::text(&message, true),
            }
        }
        _ => {
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

            match schedules::propose(state, conversation_id, ProposedSchedule { cron, prompt, each_run, time_zone }).await? {
                Ok(schedule) => ToolReply::text(&format!("Schedule {} proposed ({}, {}). It waits for the person to confirm and will not run until they do.", schedule.id, schedule.cron, schedule.time_zone), false),
                Err(message) => ToolReply::text(&message, true),
            }
        }
    })
}
