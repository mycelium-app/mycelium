use crate::assistants::{self, StartOverrides};
use crate::auth::AuthUser;
use crate::error::{ApiError, ApiResult};
use crate::prompts;
use crate::schedules;
use crate::state::AppState;
use crate::wellness;
use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use chrono::{DateTime, Utc};
use chrono_tz::Tz;
use mycelium_proto::api::{ActionInput, ActionInputKind, ActionProposal, AssistantAction, CreateScheduleRequest, EachRun, ModelChoice, PromptRequest, RunActionRequest, SaveActionRequest};
use mycelium_proto::conversation::Conversation;
use mycelium_proto::stream::StreamMessage;
use serde_json::Value;
use sqlx::types::Json as Jsonb;
use std::collections::BTreeMap;
use uuid::Uuid;

pub const WELLNESS: &str = "wellness";
pub const WEEKLY_REFLECTION: &str = "weekly_reflection";
pub const END_OF_DAY: &str = "end_of_day";
pub const EXERCISE_REVIEW: &str = "exercise_review";
const NAME_LIMIT: usize = 60;
const PROMPT_LIMIT: usize = 20_000;
const INPUT_LIMIT: usize = 8;
const INPUT_NAME_LIMIT: usize = 40;
const INPUT_VALUE_LIMIT: usize = 60_000;
const TODAY: &str = "today";
const WELLNESS_INSTRUCTIONS: &str = "You are the Wellness assistant. You look after the person's weekly reflections, end-of-day wrap-ups and exercise reviews. Be brief, specific and kind. Write drafts in the first person, in British English. Only change files in your own storage.";
const ACTION_COLUMNS: &str = "id, assistant_id, name, prompt, inputs, model, node_id, pinned, position, builtin_key, created_at, updated_at";
const PROPOSAL_COLUMNS: &str = "id, assistant_id, conversation_id, name, prompt, inputs, reason, state, created_at, decided_at";

struct Builtin {
    key: &'static str,
    name: &'static str,
    prompt: &'static str,
    inputs: fn() -> Vec<ActionInput>,
    pinned: bool,
    cron: Option<&'static str>,
}

const BUILTINS: [Builtin; 3] = [
    Builtin {
        key: WEEKLY_REFLECTION,
        name: "Weekly reflection",
        prompt: "Draft my weekly reflection for the week containing {week} (Monday to Sunday). Use find_conversations and recall to look over that week, then call save_reflection once with four short sections in plain Markdown, written in the first person in British English: done (what was finished), learned (what I found out or got better at), waiting (what is blocked or waiting on someone) and next_week (what to pick up next). Leave a section empty rather than padding it. Do not change any files.",
        inputs: || vec![ActionInput { name: "week".to_string(), kind: ActionInputKind::Date, options: Vec::new(), required: false, default: Some(TODAY.to_string()) }],
        pinned: false,
        cron: Some("0 16 * * 5"),
    },
    Builtin {
        key: END_OF_DAY,
        name: "End-of-day wrap-up",
        prompt: "Wrap up my day. Use find_conversations and recall to look over what I worked on today, then write a short end-of-day summary in plain Markdown, in the first person in British English: what got done, what is still open and the first thing to pick up tomorrow. Do not change any files.",
        inputs: Vec::new,
        pinned: true,
        cron: Some("30 17 * * 1-5"),
    },
    Builtin {
        key: EXERCISE_REVIEW,
        name: "Review an exercise",
        prompt: "Review my solution to the practice exercise below. Comment on specific lines, say what is good as well as what to improve, ask a question where a hint will teach more than an answer, and do not give the full answer. Call save_exercise_review once with a short summary and your comments. Do not edit, create or delete any file.\n\n{exercise}",
        inputs: || vec![ActionInput { name: "exercise".to_string(), kind: ActionInputKind::Text, options: Vec::new(), required: true, default: None }],
        pinned: false,
        cron: None,
    },
];

#[derive(sqlx::FromRow)]
struct ActionRow {
    id: Uuid,
    assistant_id: Uuid,
    name: String,
    prompt: String,
    inputs: Jsonb<Vec<ActionInput>>,
    model: Option<Jsonb<ModelChoice>>,
    node_id: Option<Uuid>,
    pinned: bool,
    position: i32,
    builtin_key: Option<String>,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

impl ActionRow {
    fn to_proto(self) -> AssistantAction {
        AssistantAction {
            id: self.id,
            assistant_id: self.assistant_id,
            name: self.name,
            prompt: self.prompt,
            inputs: self.inputs.0,
            model: self.model.map(|model| model.0),
            node_id: self.node_id,
            pinned: self.pinned,
            position: self.position,
            builtin_key: self.builtin_key,
            created_at: self.created_at,
            updated_at: self.updated_at,
        }
    }
}

#[derive(sqlx::FromRow)]
struct ProposalRow {
    id: Uuid,
    assistant_id: Uuid,
    conversation_id: Option<Uuid>,
    name: String,
    prompt: String,
    inputs: Jsonb<Vec<ActionInput>>,
    reason: String,
    state: String,
    created_at: DateTime<Utc>,
    decided_at: Option<DateTime<Utc>>,
}

impl ProposalRow {
    fn to_proto(self) -> ActionProposal {
        ActionProposal {
            id: self.id,
            assistant_id: self.assistant_id,
            conversation_id: self.conversation_id,
            name: self.name,
            prompt: self.prompt,
            inputs: self.inputs.0,
            reason: self.reason,
            state: self.state,
            created_at: self.created_at,
            decided_at: self.decided_at,
        }
    }
}

fn valid_name(name: &str) -> ApiResult<String> {
    let name = name.trim();

    if name.is_empty() {
        return Err(ApiError::bad_request("name_required", "Enter a name for the action."));
    }

    if name.chars().count() > NAME_LIMIT {
        return Err(ApiError::bad_request("name_invalid", "Use up to 60 characters in the action name."));
    }

    Ok(name.to_string())
}

fn valid_prompt(prompt: &str) -> ApiResult<String> {
    let prompt = prompt.trim();

    if prompt.is_empty() {
        return Err(ApiError::bad_request("prompt_required", "Enter the prompt the action sends."));
    }

    if prompt.len() > PROMPT_LIMIT {
        return Err(ApiError::bad_request("prompt_too_long", "Shorten the prompt to 20,000 characters or fewer."));
    }

    Ok(prompt.to_string())
}

fn valid_inputs(inputs: Vec<ActionInput>) -> ApiResult<Vec<ActionInput>> {
    if inputs.len() > INPUT_LIMIT {
        return Err(ApiError::bad_request("inputs_too_many", "An action can ask for up to 8 inputs. Remove one."));
    }

    let mut checked: Vec<ActionInput> = Vec::new();

    for mut input in inputs {
        input.name = input.name.trim().to_string();

        let spelled_well = !input.name.is_empty() && input.name.chars().count() <= INPUT_NAME_LIMIT && input.name.chars().all(|character| character.is_ascii_alphanumeric() || character == '_' || character == '-');

        if !spelled_well {
            return Err(ApiError::bad_request("input_name_invalid", "Name each input with letters, numbers, hyphens or underscores, for example meeting."));
        }

        if checked.iter().any(|earlier| earlier.name == input.name) {
            return Err(ApiError::bad_request("input_name_taken", format!("Two inputs are called {}. Give each input its own name.", input.name)));
        }

        input.options = input.options.into_iter().map(|option| option.trim().to_string()).filter(|option| !option.is_empty()).collect();

        if input.kind == ActionInputKind::Choice && input.options.is_empty() {
            return Err(ApiError::bad_request("input_options_required", format!("Add at least one option to {}, or change its kind.", input.name)));
        }

        if input.kind != ActionInputKind::Choice {
            input.options.clear();
        }

        input.default = input.default.map(|default| default.trim().to_string()).filter(|default| !default.is_empty());
        checked.push(input);
    }

    Ok(checked)
}

fn name_taken(name: &str) -> ApiError {
    ApiError::conflict("name_taken", format!("This assistant already has an action called {name}. Choose another name."))
}

async fn validate_place(state: &AppState, owner: Uuid, model: &Option<ModelChoice>, node_id: Option<Uuid>) -> ApiResult<()> {
    let preferences = mycelium_proto::api::AssistantPreferences { models: model.iter().cloned().collect(), nodes: node_id.into_iter().collect(), ..Default::default() };

    assistants::validate_preferences(state, owner, &preferences).await
}

pub fn expand(action: &AssistantAction, values: &BTreeMap<String, String>, zone: Tz, now: DateTime<Utc>) -> ApiResult<String> {
    let today = now.with_timezone(&zone).date_naive().to_string();
    let mut text = action.prompt.clone();

    for input in &action.inputs {
        let given = values.get(&input.name).map(|value| value.trim()).filter(|value| !value.is_empty());
        let chosen = given.or(input.default.as_deref());
        let value = match (chosen, input.kind) {
            (Some(value), _) => value.to_string(),
            (None, ActionInputKind::Date) => TODAY.to_string(),
            (None, _) => String::new(),
        };

        if value.is_empty() {
            if input.required {
                return Err(ApiError::bad_request("input_required", format!("Enter a value for {}.", input.name)));
            }

            text = text.replace(&format!("{{{}}}", input.name), "");

            continue;
        }

        if value.len() > INPUT_VALUE_LIMIT {
            return Err(ApiError::bad_request("input_too_long", format!("Shorten {} and run the action again.", input.name)));
        }

        let value = match input.kind {
            ActionInputKind::Text => value,
            ActionInputKind::Date if value.eq_ignore_ascii_case(TODAY) => today.clone(),
            ActionInputKind::Date => match chrono::NaiveDate::parse_from_str(&value, "%Y-%m-%d") {
                Ok(date) => date.to_string(),
                Err(_) => return Err(ApiError::bad_request("input_date_invalid", format!("Enter {} as a date like 2026-10-26.", input.name))),
            },
            ActionInputKind::Choice => {
                if !input.options.contains(&value) {
                    return Err(ApiError::bad_request("input_choice_invalid", format!("Choose {} from: {}.", input.name, input.options.join(", "))));
                }

                value
            }
        };

        text = text.replace(&format!("{{{}}}", input.name), &value);
    }

    Ok(text)
}

pub async fn find(state: &AppState, assistant_id: Uuid, action_id: Uuid) -> ApiResult<AssistantAction> {
    let row = sqlx::query_as::<_, ActionRow>(sqlx::AssertSqlSafe(format!("select {ACTION_COLUMNS} from assistant_actions where id = $1 and assistant_id = $2"))).bind(action_id).bind(assistant_id).fetch_optional(&state.pool).await?;

    row.map(ActionRow::to_proto).ok_or_else(|| ApiError::not_found("That action"))
}

fn changed(state: &AppState, owner: Uuid, assistant_id: Uuid) {
    state.publish(Some(owner), StreamMessage::AssistantActionsChanged { assistant_id });
}

pub async fn list(State(state): State<AppState>, user: AuthUser, Path(assistant_id): Path<Uuid>) -> ApiResult<Json<Vec<AssistantAction>>> {
    assistants::find_owned(&state, assistant_id, user.id).await?;

    let rows = sqlx::query_as::<_, ActionRow>(sqlx::AssertSqlSafe(format!("select {ACTION_COLUMNS} from assistant_actions where assistant_id = $1 order by position, created_at, id"))).bind(assistant_id).fetch_all(&state.pool).await?;

    Ok(Json(rows.into_iter().map(ActionRow::to_proto).collect()))
}

async fn insert(state: &AppState, owner: Uuid, assistant_id: Uuid, request: SaveActionRequest) -> ApiResult<AssistantAction> {
    let name = valid_name(&request.name)?;
    let prompt = valid_prompt(&request.prompt)?;
    let inputs = valid_inputs(request.inputs)?;

    validate_place(state, owner, &request.model, request.node_id).await?;

    let inserted = sqlx::query_as::<_, ActionRow>(sqlx::AssertSqlSafe(format!(
        "insert into assistant_actions (assistant_id, name, prompt, inputs, model, node_id, pinned, position)
         values ($1, $2, $3, $4, $5, $6, $7, (select coalesce(max(position) + 1, 0) from assistant_actions where assistant_id = $1)) returning {ACTION_COLUMNS}",
    )))
    .bind(assistant_id)
    .bind(&name)
    .bind(prompt)
    .bind(Jsonb(&inputs))
    .bind(request.model.as_ref().map(Jsonb))
    .bind(request.node_id)
    .bind(request.pinned)
    .fetch_one(&state.pool)
    .await;
    let row = match inserted {
        Ok(row) => row,
        Err(sqlx::Error::Database(cause)) if cause.is_unique_violation() => return Err(name_taken(&name)),
        Err(cause) => return Err(cause.into()),
    };

    changed(state, owner, assistant_id);

    Ok(row.to_proto())
}

pub async fn create(State(state): State<AppState>, user: AuthUser, Path(assistant_id): Path<Uuid>, Json(request): Json<SaveActionRequest>) -> ApiResult<(StatusCode, Json<AssistantAction>)> {
    assistants::find_owned(&state, assistant_id, user.id).await?;

    Ok((StatusCode::CREATED, Json(insert(&state, user.id, assistant_id, request).await?)))
}

pub async fn update(State(state): State<AppState>, user: AuthUser, Path((assistant_id, action_id)): Path<(Uuid, Uuid)>, Json(request): Json<SaveActionRequest>) -> ApiResult<Json<AssistantAction>> {
    assistants::find_owned(&state, assistant_id, user.id).await?;
    find(&state, assistant_id, action_id).await?;

    let name = valid_name(&request.name)?;
    let prompt = valid_prompt(&request.prompt)?;
    let inputs = valid_inputs(request.inputs)?;

    validate_place(&state, user.id, &request.model, request.node_id).await?;

    let updated = sqlx::query_as::<_, ActionRow>(sqlx::AssertSqlSafe(format!(
        "update assistant_actions set name = $3, prompt = $4, inputs = $5, model = $6, node_id = $7, pinned = $8, updated_at = now() where id = $1 and assistant_id = $2 returning {ACTION_COLUMNS}",
    )))
    .bind(action_id)
    .bind(assistant_id)
    .bind(&name)
    .bind(prompt)
    .bind(Jsonb(&inputs))
    .bind(request.model.as_ref().map(Jsonb))
    .bind(request.node_id)
    .bind(request.pinned)
    .fetch_one(&state.pool)
    .await;
    let row = match updated {
        Ok(row) => row,
        Err(sqlx::Error::Database(cause)) if cause.is_unique_violation() => return Err(name_taken(&name)),
        Err(cause) => return Err(cause.into()),
    };

    changed(&state, user.id, assistant_id);

    Ok(Json(row.to_proto()))
}

pub async fn delete(State(state): State<AppState>, user: AuthUser, Path((assistant_id, action_id)): Path<(Uuid, Uuid)>) -> ApiResult<StatusCode> {
    assistants::find_owned(&state, assistant_id, user.id).await?;

    let action = find(&state, assistant_id, action_id).await?;

    if action.builtin_key.is_some() {
        return Err(ApiError::conflict("action_built_in", format!("{} is built in, so it cannot be deleted. Edit it, or reset it to its default.", action.name)));
    }

    sqlx::query("delete from assistant_actions where id = $1").bind(action_id).execute(&state.pool).await?;
    changed(&state, user.id, assistant_id);

    Ok(StatusCode::NO_CONTENT)
}

pub async fn reset(State(state): State<AppState>, user: AuthUser, Path((assistant_id, action_id)): Path<(Uuid, Uuid)>) -> ApiResult<Json<AssistantAction>> {
    assistants::find_owned(&state, assistant_id, user.id).await?;

    let action = find(&state, assistant_id, action_id).await?;
    let Some(builtin) = BUILTINS.iter().find(|builtin| Some(builtin.key) == action.builtin_key.as_deref()) else {
        return Err(ApiError::conflict("action_not_built_in", format!("{} is not built in, so it has no default to reset to.", action.name)));
    };
    let row = sqlx::query_as::<_, ActionRow>(sqlx::AssertSqlSafe(format!(
        "update assistant_actions set name = $2, prompt = $3, inputs = $4, model = null, node_id = null, pinned = $5, updated_at = now() where id = $1 returning {ACTION_COLUMNS}",
    )))
    .bind(action_id)
    .bind(builtin.name)
    .bind(builtin.prompt)
    .bind(Jsonb((builtin.inputs)()))
    .bind(builtin.pinned)
    .fetch_one(&state.pool)
    .await?;

    changed(&state, user.id, assistant_id);

    Ok(Json(row.to_proto()))
}

pub async fn run(state: &AppState, owner: Uuid, assistant_id: Uuid, action_id: Uuid, values: &BTreeMap<String, String>, zone: Tz) -> ApiResult<Conversation> {
    let action = find(state, assistant_id, action_id).await?;
    let text = expand(&action, values, zone, Utc::now())?;
    let overrides = StartOverrides { model: action.model, node_id: action.node_id };

    assistants::wake(state, owner, assistant_id, PromptRequest { text, ..Default::default() }, &overrides).await
}

pub async fn run_handler(State(state): State<AppState>, user: AuthUser, Path((assistant_id, action_id)): Path<(Uuid, Uuid)>, request: Option<Json<RunActionRequest>>) -> ApiResult<Json<Conversation>> {
    let request = request.map(|Json(request)| request).unwrap_or_default();
    let zone = wellness::zone_of(&state, user.id, None).await?;

    Ok(Json(run(&state, user.id, assistant_id, action_id, &request.inputs, zone).await?))
}

fn publish_proposal(state: &AppState, owner: Uuid, row: ProposalRow) -> ActionProposal {
    let proposal = row.to_proto();

    state.publish(Some(owner), StreamMessage::ActionProposal(proposal.clone()));

    proposal
}

async fn find_proposal(state: &AppState, assistant_id: Uuid, id: Uuid) -> ApiResult<ProposalRow> {
    let row = sqlx::query_as::<_, ProposalRow>(sqlx::AssertSqlSafe(format!("select {PROPOSAL_COLUMNS} from assistant_action_proposals where id = $1 and assistant_id = $2"))).bind(id).bind(assistant_id).fetch_optional(&state.pool).await?;

    row.ok_or_else(|| ApiError::not_found("That proposed action"))
}

pub async fn list_proposals(State(state): State<AppState>, user: AuthUser, Path(assistant_id): Path<Uuid>) -> ApiResult<Json<Vec<ActionProposal>>> {
    assistants::find_owned(&state, assistant_id, user.id).await?;

    let rows = sqlx::query_as::<_, ProposalRow>(sqlx::AssertSqlSafe(format!("select {PROPOSAL_COLUMNS} from assistant_action_proposals where assistant_id = $1 order by (state = 'proposed') desc, created_at desc limit 50"))).bind(assistant_id).fetch_all(&state.pool).await?;

    Ok(Json(rows.into_iter().map(ProposalRow::to_proto).collect()))
}

pub async fn propose_from_agent(state: &AppState, conversation_id: Uuid, arguments: &Value) -> ApiResult<Result<ActionProposal, String>> {
    let session = sqlx::query_as::<_, (Option<Uuid>, Uuid)>("select assistant_id, owner from conversations where id = $1").bind(conversation_id).fetch_one(&state.pool).await?;
    let (Some(assistant_id), owner) = session else {
        return Ok(Err("Nothing was proposed. Only an assistant's own conversation can propose an action.".to_string()));
    };
    let text = |field: &str| arguments.get(field).and_then(Value::as_str).map(str::trim).filter(|text| !text.is_empty());
    let (Some(name), Some(prompt)) = (text("name"), text("prompt")) else {
        return Ok(Err("Nothing was proposed. Give the action a name and a prompt.".to_string()));
    };
    let inputs: Vec<ActionInput> = match arguments.get("inputs") {
        Some(given) => match serde_json::from_value(given.clone()) {
            Ok(inputs) => inputs,
            Err(_) => return Ok(Err("Nothing was proposed. Each input needs a name, a kind (text, date or choice) and optionally options, required and default.".to_string())),
        },
        None => Vec::new(),
    };
    let checked = valid_name(name).and_then(|name| valid_prompt(prompt).and_then(|prompt| valid_inputs(inputs).map(|inputs| (name, prompt, inputs))));
    let (name, prompt, inputs) = match checked {
        Ok(checked) => checked,
        Err(error) => return Ok(Err(format!("Nothing was proposed. {}", error.message()))),
    };
    let taken = sqlx::query_scalar::<_, bool>("select exists (select 1 from assistant_actions where assistant_id = $1 and lower(name) = lower($2))").bind(assistant_id).bind(&name).fetch_one(&state.pool).await?;

    if taken {
        return Ok(Err(format!("Nothing was proposed. This assistant already has an action called {name}. Choose another name.")));
    }

    let row = sqlx::query_as::<_, ProposalRow>(sqlx::AssertSqlSafe(format!(
        "insert into assistant_action_proposals (owner, assistant_id, conversation_id, name, prompt, inputs, reason) values ($1, $2, $3, $4, $5, $6, $7) returning {PROPOSAL_COLUMNS}",
    )))
    .bind(owner)
    .bind(assistant_id)
    .bind(conversation_id)
    .bind(name)
    .bind(prompt)
    .bind(Jsonb(&inputs))
    .bind(text("reason").unwrap_or_default())
    .fetch_one(&state.pool)
    .await?;

    Ok(Ok(publish_proposal(state, owner, row)))
}

pub async fn approve(State(state): State<AppState>, user: AuthUser, Path((assistant_id, proposal_id)): Path<(Uuid, Uuid)>, edited: Option<Json<SaveActionRequest>>) -> ApiResult<Json<AssistantAction>> {
    assistants::find_owned(&state, assistant_id, user.id).await?;

    let found = find_proposal(&state, assistant_id, proposal_id).await?;

    if found.state != "proposed" {
        return Err(ApiError::conflict("proposal_not_proposed", "This proposal has already been decided."));
    }

    let request = match edited {
        Some(Json(request)) => request,
        None => SaveActionRequest { name: found.name.clone(), prompt: found.prompt.clone(), inputs: found.inputs.0.clone(), model: None, node_id: None, pinned: false },
    };
    let action = insert(&state, user.id, assistant_id, request).await?;
    let decided = sqlx::query_as::<_, ProposalRow>(sqlx::AssertSqlSafe(format!("update assistant_action_proposals set state = 'approved', decided_at = now() where id = $1 returning {PROPOSAL_COLUMNS}"))).bind(proposal_id).fetch_one(&state.pool).await?;

    publish_proposal(&state, user.id, decided);

    Ok(Json(action))
}

pub async fn decline(State(state): State<AppState>, user: AuthUser, Path((assistant_id, proposal_id)): Path<(Uuid, Uuid)>) -> ApiResult<Json<ActionProposal>> {
    assistants::find_owned(&state, assistant_id, user.id).await?;

    let found = find_proposal(&state, assistant_id, proposal_id).await?;

    if found.state != "proposed" {
        return Err(ApiError::conflict("proposal_not_proposed", "This proposal has already been decided."));
    }

    let decided = sqlx::query_as::<_, ProposalRow>(sqlx::AssertSqlSafe(format!("update assistant_action_proposals set state = 'declined', decided_at = now() where id = $1 returning {PROPOSAL_COLUMNS}"))).bind(proposal_id).fetch_one(&state.pool).await?;

    if let Some(conversation_id) = found.conversation_id {
        let text = format!("I declined the action you proposed, {}. Do not propose it again unless I ask.", found.name);

        let _ = prompts::continue_unless_queued(&state, user.id, conversation_id, &text).await;
    }

    Ok(Json(publish_proposal(&state, user.id, decided)))
}

pub async fn ensure_wellness(state: &AppState, owner: Uuid) -> ApiResult<Uuid> {
    let existing = sqlx::query_scalar::<_, Uuid>("select id from assistants where owner = $1 and builtin_key = $2").bind(owner).bind(WELLNESS).fetch_optional(&state.pool).await?;
    let id = match existing {
        Some(id) => id,
        None => {
            let mut created = None;

            for name in ["Wellness", "Wellness (built in)"] {
                created = sqlx::query_scalar::<_, Uuid>(
                    "insert into assistants (owner, name, description, builtin_key) values ($1, $3, 'Drafts your reflections, wraps up your day and reviews your exercises.', $2)
                     on conflict do nothing returning id",
                )
                .bind(owner)
                .bind(WELLNESS)
                .bind(name)
                .fetch_optional(&state.pool)
                .await?;

                if created.is_some() {
                    break;
                }
            }

            match created {
                Some(id) => id,
                None => sqlx::query_scalar::<_, Uuid>("select id from assistants where owner = $1 and builtin_key = $2").bind(owner).bind(WELLNESS).fetch_one(&state.pool).await?,
            }
        }
    };

    let instructions = assistants::instructions_of(state, id).await?;

    if instructions.trim().is_empty() {
        assistants::store_file(state, id, assistants::INSTRUCTIONS_PATH, WELLNESS_INSTRUCTIONS.as_bytes()).await?;
    }

    for builtin in &BUILTINS {
        let action_id = sqlx::query_scalar::<_, Uuid>(
            "insert into assistant_actions (assistant_id, name, prompt, inputs, pinned, position, builtin_key)
             values ($1, $2, $3, $4, $5, (select coalesce(max(position) + 1, 0) from assistant_actions where assistant_id = $1), $6)
             on conflict do nothing returning id",
        )
        .bind(id)
        .bind(builtin.name)
        .bind(builtin.prompt)
        .bind(Jsonb((builtin.inputs)()))
        .bind(builtin.pinned)
        .bind(builtin.key)
        .fetch_optional(&state.pool)
        .await?;

        if let (Some(action_id), Some(cron)) = (action_id, builtin.cron) {
            let request = CreateScheduleRequest {
                node_id: None,
                agent: None,
                cwd: String::new(),
                assistant_id: Some(id),
                action_id: Some(action_id),
                action_inputs: BTreeMap::new(),
                project_id: None,
                prompt: builtin.name.to_string(),
                cron: cron.to_string(),
                each_run: EachRun::Same,
                time_zone: None,
                account: None, routing: None,
            };

            schedules::insert(state, owner, request).await?;
        }
    }

    assistants::publish(state, owner, id).await?;
    changed(state, owner, id);

    Ok(id)
}

pub async fn wellness_assistant(State(state): State<AppState>, user: AuthUser) -> ApiResult<Json<mycelium_proto::api::Assistant>> {
    let id = ensure_wellness(&state, user.id).await?;

    Ok(Json(assistants::find_owned(&state, id, user.id).await?))
}

pub async fn action_of_key(state: &AppState, owner: Uuid, key: &str) -> ApiResult<(Uuid, Uuid)> {
    let assistant_id = ensure_wellness(state, owner).await?;
    let action_id = sqlx::query_scalar::<_, Uuid>("select id from assistant_actions where assistant_id = $1 and builtin_key = $2").bind(assistant_id).bind(key).fetch_one(&state.pool).await?;

    Ok((assistant_id, action_id))
}

pub async fn is_wellness_conversation(state: &AppState, conversation_id: Uuid) -> ApiResult<bool> {
    let wellness = sqlx::query_scalar::<_, bool>("select exists (select 1 from conversations c join assistants a on a.id = c.assistant_id where c.id = $1 and a.builtin_key = $2)").bind(conversation_id).bind(WELLNESS).fetch_one(&state.pool).await?;

    Ok(wellness)
}
