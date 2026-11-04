use super::approval;
use super::call::{ToolCall, ToolReply};
use crate::auth::AuthUser;
use crate::error::ApiResult;
use crate::schedules::{self, ProposedSchedule, preset_cron};
use crate::tasks;
use axum::Json;
use axum::extract::{Path, State};
use mycelium_proto::api::{EachRun, Task, TaskStatus};
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
            "name": "list_tasks",
            "description": "List the person's Tasks board: each item's id, column, title, age and the conversation it came from. Done, declined and closed items are left out unless asked for. It only reads.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "column": {"type": "string", "enum": ["question", "proposed", "open", "claimed", "done", "declined", "closed"], "description": "Only items in this column."},
                    "older_than_days": {"type": "number", "description": "Only items created more than this many days ago."},
                },
            },
        },
        {
            "name": "approve_task",
            "description": "Approve a proposed task so it moves to Open on the person's Tasks board. The person confirms it in a card in this conversation first.",
            "inputSchema": {"type": "object", "properties": {"id": {"type": "string", "description": "The id of the task, from list_tasks."}}, "required": ["id"]},
        },
        {
            "name": "decline_task",
            "description": "Decline a proposed task. The person confirms it in a card in this conversation first.",
            "inputSchema": {"type": "object", "properties": {"id": {"type": "string", "description": "The id of the task, from list_tasks."}}, "required": ["id"]},
        },
        {
            "name": "close_tasks",
            "description": "Close one or more questions, proposed, open or started tasks so they leave the person's Tasks board, for example the stale ones from list_tasks. The person confirms the whole batch in one card in this conversation first.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "ids": {"type": "array", "items": {"type": "string"}, "description": "The ids of the tasks to close, from list_tasks."},
                    "reason": {"type": "string", "description": "Why they should be closed, shown on the card."},
                },
                "required": ["ids"],
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
        "list_tasks" => {
            let owner = caller.owner().await?;
            let Json(all) = tasks::list(State(state.clone()), AuthUser::of(state, owner).await?).await?;
            let column = arguments.get("column").and_then(Value::as_str).map(str::to_string);
            let older_than = arguments.get("older_than_days").and_then(Value::as_f64);
            let now = chrono::Utc::now();
            let lines: Vec<String> = all
                .iter()
                .filter(|task| match &column {
                    Some(column) => crate::store::enum_text(&task.status) == *column,
                    None => matches!(task.status, TaskStatus::Question | TaskStatus::Proposed | TaskStatus::Open | TaskStatus::Claimed),
                })
                .filter(|task| older_than.is_none_or(|days| (now - task.created_at).num_hours() as f64 / 24.0 > days))
                .map(|task| listed(task, now))
                .collect();

            if lines.is_empty() { ToolReply::text("No tasks match.", false) } else { ToolReply::text(&lines.join("\n"), false) }
        }
        "approve_task" | "decline_task" => {
            let Some(id) = arguments.get("id").and_then(Value::as_str).and_then(|text| Uuid::parse_str(text.trim()).ok()) else {
                return Ok(ToolReply::invalid(&format!("{tool} needs id, a task id from list_tasks.")));
            };
            let owner = caller.owner().await?;
            let Some(task) = owned(state, owner, &[id]).await?.into_iter().next() else {
                return Ok(ToolReply::text("Nothing was changed. None of the person's tasks has that id. Look it up with list_tasks.", true));
            };

            if task.status != TaskStatus::Proposed {
                return Ok(ToolReply::text(&format!("Nothing was changed. \"{}\" is {}, and only proposed tasks are approved or declined.", task.title, crate::store::enum_text(&task.status)), true));
            }

            if let Err(refusal) = approval::approved(&caller, tool, &json!({"task": task.title})).await? {
                return Ok(ToolReply::text(&refusal, true));
            }

            let user = AuthUser::of(state, owner).await?;
            let moved = if tool == "approve_task" { tasks::approve(State(state.clone()), user, Path(id)).await } else { tasks::decline(State(state.clone()), user, Path(id)).await };

            match moved {
                Ok(Json(task)) => ToolReply::text(&format!("\"{}\" is now {}.", task.title, crate::store::enum_text(&task.status)), false),
                Err(error) => ToolReply::text(&format!("Nothing was changed. {}", error.message()), true),
            }
        }
        "close_tasks" => {
            let ids: Vec<Uuid> = arguments.get("ids").and_then(Value::as_array).into_iter().flatten().filter_map(|value| value.as_str().and_then(|text| Uuid::parse_str(text.trim()).ok())).collect();

            if ids.is_empty() {
                return Ok(ToolReply::invalid("close_tasks needs ids, a list of task ids from list_tasks."));
            }

            let owner = caller.owner().await?;
            let closable: Vec<Task> = owned(state, owner, &ids).await?.into_iter().filter(|task| matches!(task.status, TaskStatus::Question | TaskStatus::Proposed | TaskStatus::Open | TaskStatus::Claimed)).collect();

            if closable.is_empty() {
                return Ok(ToolReply::text("Nothing was closed. None of those ids is a question, proposed, open or started task of the person's. Look them up with list_tasks.", true));
            }

            let titles: Vec<&str> = closable.iter().map(|task| task.title.as_str()).collect();
            let card = json!({"close": titles, "reason": arguments.get("reason").and_then(Value::as_str).unwrap_or_default()});

            if let Err(refusal) = approval::approved(&caller, tool, &card).await? {
                return Ok(ToolReply::text(&refusal, true));
            }

            let wanted: Vec<Uuid> = closable.iter().map(|task| task.id).collect();
            let closed = tasks::close_owned(state, owner, &wanted).await?;
            let skipped = ids.len() - closed.len();

            ToolReply::text(&format!("Closed {} {}.{}", closed.len(), if closed.len() == 1 { "task" } else { "tasks" }, if skipped > 0 { format!(" {skipped} could not be closed: they are unknown, already finished or not the person's.") } else { String::new() }), false)
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

async fn owned(state: &crate::state::AppState, owner: Uuid, ids: &[Uuid]) -> ApiResult<Vec<Task>> {
    let Json(all) = tasks::list(State(state.clone()), AuthUser::of(state, owner).await?).await?;

    Ok(all.into_iter().filter(|task| ids.contains(&task.id)).collect())
}

fn listed(task: &Task, now: chrono::DateTime<chrono::Utc>) -> String {
    let days = (now - task.created_at).num_days();
    let age = match days {
        0 => "today".to_string(),
        1 => "1 day old".to_string(),
        days => format!("{days} days old"),
    };
    let source = task.source_title.as_deref().map(|title| format!(", from \"{title}\"")).unwrap_or_default();
    let waiting = if task.waits_on > 0 { format!(", waits on {}", task.waits_on) } else { String::new() };

    format!("{} [{}] {} ({age}{source}{waiting})", task.id, crate::store::enum_text(&task.status), task.title.replace('\n', " "))
}
