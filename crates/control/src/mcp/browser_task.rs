use super::call::{ToolCall, ToolReply};
use super::environment::agent_environment;
use crate::decision_providers::{ask, provider_for};
use crate::environments::record_action;
use crate::error::ApiResult;
use crate::model_providers::{ChatMessage, list_owned};
use crate::nodes::node_offline;
use crate::state::{AppState, BrowserActedAnswer, BrowserControlsAnswer};
use mycelium_proto::api::{BrowserTask, BrowserTaskState, BrowserTaskStep, EnvironmentController};
use mycelium_proto::link::{BrowserAction, BrowserControl, ControlToNode};
use serde_json::{Value, json};
use std::time::{Duration, Instant};
use uuid::Uuid;

const MAX_STEPS: usize = 30;
const MAX_LISTED_CONTROLS: usize = 100;
const RECENT_STEPS: usize = 8;
const NODE_TIMEOUT: Duration = Duration::from_secs(15);
const WRITING_TIMEOUT: Duration = Duration::from_secs(20);
const SETTLE: Duration = Duration::from_millis(300);
const WAIT: Duration = Duration::from_secs(1);
const NO_BROWSER_PROVIDER: &str = "No decisions provider has Browser tasks switched on, so browser_task cannot run. Use environment_screenshot and environment_click instead, or ask the person to tick Browser tasks in Settings, Providers.";
const NOT_ANSWERED: &str = "The decisions server did not pick a step. Ask the person to check it answers choice questions, or use environment_screenshot and environment_click instead.";
const OPERATIONS: [(&str, &str); 7] = [
    ("click", "Click a control"),
    ("type", "Type text into a field"),
    ("select", "Choose an option in a list or menu"),
    ("scroll", "Scroll the page down to reveal more controls"),
    ("wait", "Wait for the page to finish loading or changing"),
    ("done", "The goal has been met"),
    ("blocked", "The goal cannot go on without the person, for example a sign-in, a code or a payment"),
];

pub fn definitions() -> Value {
    json!([
        {
            "name": "browser_task",
            "description": "Hand a goal to a fast loop that works the environment's browser page without a model turn per step. Mycelium reads the page's visible controls into a numbered table, a decisions model picks each operation and control, and Mycelium acts on it, up to 30 steps. Returns the result when the goal is met, or why it is blocked. The person watches the steps and can stop it by taking over. Needs a decisions provider with Browser tasks switched on; without one it returns an error and you should use environment_screenshot and environment_click.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "goal": {"type": "string", "description": "What the browser should achieve, in a sentence, for example \"Find the cheapest direct flight from London to Lisbon on Saturday 14 November.\""},
                },
                "required": ["goal"],
            },
        },
    ])
}

fn seconds(millis: u64) -> String {
    format!("{:.1} s", millis as f64 / 1000.0)
}

fn table_of(controls: &[BrowserControl]) -> String {
    controls.iter().take(MAX_LISTED_CONTROLS).map(|control| format!("{} | {} | {} | {}", control.index, control.role, control.name, control.value)).collect::<Vec<_>>().join("\n")
}

fn conclude(state: &AppState, conversation_id: Uuid, outcome: BrowserTaskState, result: Option<String>, why: Option<String>, logged: &str) {
    record_action(state, conversation_id, logged, |record| {
        if let Some(task) = record.environment.browser_task.as_mut() {
            task.state = outcome;
            task.result = result;
            task.why = why;
        }
    });
}

fn running(state: &AppState, conversation_id: Uuid) -> bool {
    state
        .hub
        .environment_of(conversation_id)
        .is_some_and(|record| record.is_live() && record.environment.controller == EnvironmentController::Agent && record.environment.browser_task.as_ref().is_some_and(|task| task.state == BrowserTaskState::Running))
}

async fn write(state: &AppState, owner: Uuid, system: &str, request: String) -> Option<String> {
    let providers = list_owned(state, owner).await.ok()?;
    let provider = providers.into_iter().find(|provider| provider.default_model.is_some())?;
    let model = provider.default_model.clone()?;
    let messages = [ChatMessage { role: "user", content: request, ..ChatMessage::default() }];
    let writing = async {
        let mut stream = provider.stream_reply(&state.http, &model, system, &messages, &[]).await.ok()?.ok()?;
        let mut written = String::new();

        while let Ok(Some(delta)) = stream.next().await {
            written.push_str(&delta.text);
        }

        Some(written.trim().to_string()).filter(|written| !written.is_empty())
    };

    tokio::time::timeout(WRITING_TIMEOUT, writing).await.ok().flatten()
}

pub async fn call(call: ToolCall<'_>) -> ApiResult<ToolReply> {
    let ToolCall { caller, arguments, .. } = call;
    let state = caller.state;
    let Some(goal) = arguments.get("goal").and_then(Value::as_str).map(str::trim).filter(|goal| !goal.is_empty()).map(String::from) else {
        return Ok(ToolReply::invalid("browser_task needs a goal, a sentence saying what the browser should achieve."));
    };
    let owner = caller.owner().await?;
    let Some(provider) = provider_for(state, owner, "browser").await? else {
        return Ok(ToolReply::text(NO_BROWSER_PROVIDER, true));
    };
    let Some(conversation_id) = caller.conversation_id().await? else {
        return Ok(ToolReply::text("The environment belongs to a conversation, so use this tool from one.", true));
    };
    let record = match agent_environment(state, conversation_id) {
        Ok(record) => record,
        Err(reply) => return Ok(reply),
    };

    if running(state, conversation_id) {
        return Ok(ToolReply::text("A browser task is already running in this environment. Wait for it to finish.", true));
    }

    let node_id = record.environment.node_id;
    let person = record.person.clone();

    record_action(state, conversation_id, "Started a browser task", |record| {
        record.environment.browser_task = Some(BrowserTask { goal: goal.clone(), state: BrowserTaskState::Running, provider: provider.name.clone(), steps: Vec::new(), result: None, why: None, stopped_by: None });
    });

    let mut steps: Vec<BrowserTaskStep> = Vec::new();

    while steps.len() < MAX_STEPS {
        if !running(state, conversation_id) {
            break;
        }

        let started = Instant::now();
        let read = state.ask_node::<BrowserControlsAnswer>(node_id, NODE_TIMEOUT, |request_id| ControlToNode::EnvironmentBrowserRead { request_id, conversation_id }).await;
        let Some(read) = read else {
            conclude(state, conversation_id, BrowserTaskState::Failed, None, Some(node_offline().message().to_string()), "Browser task failed");

            return Ok(ToolReply::text(node_offline().message(), true));
        };

        if let Some(error) = read.error {
            conclude(state, conversation_id, BrowserTaskState::Failed, None, Some(error.clone()), "Browser task failed");

            return Ok(ToolReply::text(&error, true));
        }

        let table = table_of(&read.controls);
        let recent: Vec<&str> = steps.iter().rev().take(RECENT_STEPS).rev().map(|step| step.label.as_str()).collect();
        let subject = json!({"goal": goal, "page": read.page, "controls": table, "steps_so_far": recent});
        let operations: serde_json::Map<String, Value> = OPERATIONS.iter().map(|(name, meaning)| (name.to_string(), json!(meaning))).collect();
        let mut targets: serde_json::Map<String, Value> = read.controls.iter().take(MAX_LISTED_CONTROLS).map(|control| (control.index.to_string(), json!(format!("{}: {}", control.role, control.name)))).collect();

        if targets.is_empty() {
            targets.insert("none".to_string(), json!("There is no control to act on"));
        }

        let questions = json!({
            "operation": {"type": "choice", "instructions": "Which operation moves the browser closest to the goal, given the page and the steps so far?", "criteria": operations},
            "target": {"type": "choice", "instructions": "Which control should the operation act on? Ignore this for scroll, wait, done and blocked.", "criteria": targets},
        });
        let asked = ask(state, &provider.base_url, provider.api_key.as_deref(), &provider.model, &subject, &questions, None).await;
        let answers = match asked {
            Ok(answers) => answers,
            Err(message) => {
                conclude(state, conversation_id, BrowserTaskState::Failed, None, Some(message.clone()), "Browser task failed");

                return Ok(ToolReply::text(&message, true));
            }
        };
        let Some(operation) = answers["operation"]["choice"].as_str() else {
            conclude(state, conversation_id, BrowserTaskState::Failed, None, Some(NOT_ANSWERED.to_string()), "Browser task failed");

            return Ok(ToolReply::text(NOT_ANSWERED, true));
        };
        let control = answers["target"]["choice"].as_str().and_then(|choice| choice.parse::<u32>().ok()).and_then(|index| read.controls.iter().find(|control| control.index == index));

        if !running(state, conversation_id) {
            break;
        }

        let elapsed = |started: Instant| u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX);
        let context = format!("Goal: {goal}\nPage: {}\nControls:\n{table}", read.page);
        let total: u64 = steps.iter().map(|step| step.millis).sum::<u64>() + elapsed(started);

        match operation {
            "done" => {
                let written = write(state, owner, "Reply with one or two plain sentences saying what the goal found or did. No preamble.", format!("{context}\n\nThe goal has been met. State the result.")).await;
                let result = written.unwrap_or_else(|| format!("The goal is met. The browser is on {}.", read.page));

                conclude(state, conversation_id, BrowserTaskState::Done, Some(result.clone()), None, "Browser task done");

                return Ok(ToolReply::text(&format!("Browser task done after {} steps in {}. Result: {result}", steps.len(), seconds(total)), false));
            }
            "blocked" => {
                let written = write(state, owner, "Reply with one plain sentence saying what stops the goal and what the person must do. No preamble.", format!("{context}\n\nThe goal cannot go on without the person. Say why.")).await;
                let why = written.unwrap_or_else(|| format!("The page needs something the task cannot do. The browser is on {}.", read.page));

                conclude(state, conversation_id, BrowserTaskState::Blocked, None, Some(why.clone()), "Browser task blocked");

                return Ok(ToolReply::text(&format!("Browser task blocked after {} steps. Why: {why} Take a screenshot to see the page. The person can take over, then hand back.", steps.len()), false));
            }
            _ => {}
        }

        let acted = match (operation, control) {
            ("wait", _) => {
                tokio::time::sleep(WAIT).await;

                Ok(("Wait".to_string(), "Waited for the page".to_string()))
            }
            ("scroll", _) => {
                let action = BrowserAction::Scroll { direction: "down".to_string() };

                act(state, node_id, conversation_id, action).await.map(|()| ("Scroll down".to_string(), "Scrolled down".to_string()))
            }
            ("click", Some(control)) => act(state, node_id, conversation_id, BrowserAction::Click { index: control.index }).await.map(|()| (format!("Click: {}", control.name), format!("Clicked “{}”", control.name))),
            ("type", Some(control)) => {
                let written = write(state, owner, "Reply with only the text to type into the field, nothing else.", format!("{context}\n\nField: {}\n\nWrite the text to type into this field to move toward the goal.", control.name)).await;
                let text = written.unwrap_or_else(|| goal.clone());
                let label = format!("Type: {} {text}", control.name);

                act(state, node_id, conversation_id, BrowserAction::Type { index: control.index, text }).await.map(|()| (label, format!("Typed into “{}”", control.name)))
            }
            ("select", Some(control)) => {
                let text = if control.role == "select" {
                    write(state, owner, "Reply with only the option text to choose, nothing else.", format!("{context}\n\nList: {}\n\nWhich option should be chosen to move toward the goal?", control.name)).await.unwrap_or_default()
                } else {
                    String::new()
                };

                act(state, node_id, conversation_id, BrowserAction::Select { index: control.index, text }).await.map(|()| (format!("Select: {}", control.name), format!("Chose “{}”", control.name)))
            }
            _ => Err(format!("The decisions server picked {operation} on a control that is not on the page. Take a screenshot and carry on by hand.")),
        };
        let (label, logged) = match acted {
            Ok(acted) => acted,
            Err(message) => {
                conclude(state, conversation_id, BrowserTaskState::Failed, None, Some(message.clone()), "Browser task failed");

                return Ok(ToolReply::text(&message, true));
            }
        };
        let step = BrowserTaskStep { label, millis: elapsed(started) };

        steps.push(step.clone());
        record_action(state, conversation_id, &logged, |record| {
            if let Some(task) = record.environment.browser_task.as_mut() {
                task.steps.push(step);
            }
        });
        tokio::time::sleep(SETTLE).await;
    }

    let finished = state.hub.environment_of(conversation_id).and_then(|record| record.environment.browser_task);
    let total: u64 = steps.iter().map(|step| step.millis).sum();

    match finished {
        Some(task) if task.state == BrowserTaskState::Stopped => {
            record_action(state, conversation_id, "Browser task stopped", |_| {});

            let who = if task.stopped_by.as_deref() == Some("took_over") { format!("{person} took over") } else { format!("{person} stopped it") };

            Ok(ToolReply::text(&format!("Browser task stopped when {who}, after {} steps in {}. Take a screenshot before carrying on, and wait for them to hand back if they took over.", steps.len(), seconds(total)), false))
        }
        _ if steps.len() >= MAX_STEPS => {
            let why = format!("Used all {MAX_STEPS} steps without meeting the goal.");

            conclude(state, conversation_id, BrowserTaskState::Blocked, None, Some(why.clone()), "Browser task blocked");

            Ok(ToolReply::text(&format!("Browser task blocked after {} steps. Why: {why} Take a screenshot to see where it got to.", steps.len()), false))
        }
        _ => {
            let message = "The environment stopped before the browser task finished.";

            conclude(state, conversation_id, BrowserTaskState::Failed, None, Some(message.to_string()), "Browser task failed");

            Ok(ToolReply::text(message, true))
        }
    }
}

async fn act(state: &AppState, node_id: Uuid, conversation_id: Uuid, action: BrowserAction) -> Result<(), String> {
    let answered = state.ask_node::<BrowserActedAnswer>(node_id, NODE_TIMEOUT, |request_id| ControlToNode::EnvironmentBrowserAct { request_id, conversation_id, action }).await;

    match answered {
        None => Err(node_offline().message().to_string()),
        Some(answer) => answer.error.map_or(Ok(()), Err),
    }
}
