use crate::auth::AuthUser;
use crate::error::{ApiError, ApiResult};
use crate::state::AppState;
use crate::store::enum_text;
use crate::wellness;
use axum::Json;
use axum::extract::State;
use axum::http::StatusCode;
use chrono::{Duration, NaiveDate, Utc};
use mycelium_proto::api::{HiddenSkillRequest, Skill};
use mycelium_proto::conversation::EventKind;
use serde_json::Value;
use sqlx::PgPool;
use std::collections::{BTreeMap, HashMap};
use uuid::Uuid;

const WINDOW_DAYS: i64 = 90;
const SERIES_WEEKS: usize = 8;
const KINDS: [&str; 3] = ["language", "area", "other"];
const LANGUAGES: [(&str, &str); 24] = [
    ("rs", "Rust"),
    ("kt", "Kotlin"),
    ("kts", "Kotlin"),
    ("java", "Java"),
    ("ts", "TypeScript"),
    ("tsx", "TypeScript"),
    ("js", "JavaScript"),
    ("jsx", "JavaScript"),
    ("svelte", "Svelte"),
    ("py", "Python"),
    ("go", "Go"),
    ("sql", "SQL"),
    ("css", "CSS"),
    ("html", "HTML"),
    ("sh", "Shell"),
    ("fish", "Shell"),
    ("swift", "Swift"),
    ("c", "C"),
    ("h", "C"),
    ("cpp", "C++"),
    ("rb", "Ruby"),
    ("nix", "Nix"),
    ("toml", "TOML"),
    ("md", "Markdown"),
];

#[derive(Default, Clone, Copy)]
struct Counts {
    agent: i32,
    own: i32,
}

pub fn language_of(path: &str) -> Option<&'static str> {
    let extension = path.rsplit_once('.')?.1.to_ascii_lowercase();

    LANGUAGES.iter().find(|(known, _)| *known == extension).map(|(_, name)| *name)
}

pub fn area_of(path: &str, cwd: &str) -> Option<String> {
    let relative = path.strip_prefix(cwd).map(|rest| rest.trim_start_matches('/')).unwrap_or_else(|| path.trim_start_matches('/'));
    let folders: Vec<&str> = relative.split('/').collect();
    let depth = folders.len().saturating_sub(1).min(2);

    match depth {
        0 => None,
        _ => Some(folders[..depth].join("/")),
    }
}

fn added_lines(new_text: &str, old_text: &str) -> i32 {
    let mut before: HashMap<&str, i32> = HashMap::new();

    for line in old_text.lines() {
        *before.entry(line).or_default() += 1;
    }

    let mut added = 0;

    for line in new_text.lines().filter(|line| !line.trim().is_empty()) {
        match before.get_mut(line) {
            Some(left) if *left > 0 => *left -= 1,
            _ => added += 1,
        }
    }

    added
}

async fn add(pool: &PgPool, owner: Uuid, day: NaiveDate, path: &str, cwd: &str, counts: Counts) -> ApiResult<()> {
    let mut places = Vec::new();

    if let Some(language) = language_of(path) {
        places.push(("language", language.to_string()));
    }

    if let Some(area) = area_of(path, cwd) {
        places.push(("area", area));
    }

    for (kind, name) in places {
        sqlx::query(
            "insert into skill_lines (owner, day, kind, name, agent_lines, own_lines) values ($1, $2, $3, $4, $5, $6)
             on conflict (owner, day, kind, name) do update set agent_lines = skill_lines.agent_lines + excluded.agent_lines, own_lines = skill_lines.own_lines + excluded.own_lines",
        )
        .bind(owner)
        .bind(day)
        .bind(kind)
        .bind(name)
        .bind(counts.agent)
        .bind(counts.own)
        .execute(pool)
        .await?;
    }

    Ok(())
}

pub async fn count_agent(pool: &PgPool, owner: Uuid, conversation_id: Uuid, cwd: &str, update: &Value, day: NaiveDate) -> ApiResult<()> {
    let Some(contents) = update["update"]["content"].as_array() else {
        return Ok(());
    };
    let diffs: Vec<&Value> = contents.iter().filter(|content| content["type"] == "diff").collect();

    if diffs.is_empty() {
        return Ok(());
    }

    if let Some(tool_call_id) = update["update"]["toolCallId"].as_str() {
        let first = sqlx::query("insert into counted_tool_calls (conversation_id, tool_call_id) values ($1, $2) on conflict do nothing").bind(conversation_id).bind(tool_call_id).execute(pool).await?.rows_affected();

        if first == 0 {
            return Ok(());
        }
    }

    for diff in diffs {
        let (Some(path), Some(new_text)) = (diff["path"].as_str(), diff["newText"].as_str()) else {
            continue;
        };
        let lines = added_lines(new_text, diff["oldText"].as_str().unwrap_or_default());

        add(pool, owner, day, path, cwd, Counts { agent: lines, own: 0 }).await?;
    }

    Ok(())
}

pub async fn count_own(pool: &PgPool, owner: Uuid, cwd: &str, path: &str, new_text: &str, old_text: &str) -> ApiResult<()> {
    let lines = added_lines(new_text, old_text);

    add(pool, owner, Utc::now().date_naive(), path, cwd, Counts { agent: 0, own: lines }).await
}

async fn backfill(state: &AppState, owner: Uuid) -> ApiResult<()> {
    let claimed = sqlx::query("insert into user_settings (owner, skills_backfilled) values ($1, true) on conflict (owner) do update set skills_backfilled = true where not user_settings.skills_backfilled")
        .bind(owner)
        .execute(&state.pool)
        .await?
        .rows_affected();

    if claimed == 0 {
        return Ok(());
    }

    let events = sqlx::query_as::<_, (Uuid, String, Value, chrono::DateTime<Utc>)>(
        "select e.conversation_id, c.cwd, e.payload, e.created_at from events e join conversations c on c.id = e.conversation_id
         where c.owner = $1 and e.kind = $2 and e.created_at > now() - make_interval(days => $3) and e.payload::text like '%\"diff\"%' order by e.id",
    )
    .bind(owner)
    .bind(enum_text(&EventKind::AcpUpdate))
    .bind(WINDOW_DAYS as i32)
    .fetch_all(&state.pool)
    .await?;

    for (conversation_id, cwd, payload, created_at) in events {
        count_agent(&state.pool, owner, conversation_id, &cwd, &payload, created_at.date_naive()).await?;
    }

    Ok(())
}

pub async fn list(State(state): State<AppState>, user: AuthUser) -> ApiResult<Json<Vec<Skill>>> {
    backfill(&state, user.id).await?;

    let zone = wellness::zone_of(&state, user.id, None).await?;
    let today = Utc::now().with_timezone(&zone).date_naive();
    let since = today - Duration::days(WINDOW_DAYS);
    let rows = sqlx::query_as::<_, (NaiveDate, String, String, i32, i32)>(
        "select day, kind, name, agent_lines, own_lines from skill_lines s
         where owner = $1 and day > $2 and not exists (select 1 from hidden_skills h where h.owner = s.owner and h.kind = s.kind and h.name = s.name)",
    )
    .bind(user.id)
    .bind(since)
    .fetch_all(&state.pool)
    .await?;

    let mut skills: BTreeMap<(String, String), Skill> = BTreeMap::new();

    for (day, kind, name, agent, own) in rows {
        let skill = skills.entry((kind.clone(), name.clone())).or_insert_with(|| Skill { kind, name, own_lines: 0, agent_lines: 0, own_share: None, weekly_own_lines: vec![0; SERIES_WEEKS], last_practised: None });
        let weeks_ago = ((today - day).num_days().max(0) / 7) as usize;

        skill.own_lines += i64::from(own);
        skill.agent_lines += i64::from(agent);

        if weeks_ago < SERIES_WEEKS {
            skill.weekly_own_lines[SERIES_WEEKS - 1 - weeks_ago] += i64::from(own);
        }

        if own > 0 && skill.last_practised.as_deref().is_none_or(|last| last < day.to_string().as_str()) {
            skill.last_practised = Some(day.to_string());
        }
    }

    let mut skills: Vec<Skill> = skills.into_values().collect();

    for skill in &mut skills {
        let total = skill.own_lines + skill.agent_lines;

        skill.own_share = (total > 0).then(|| skill.own_lines as f64 / total as f64);
    }

    skills.sort_by(|a, b| (b.own_lines + b.agent_lines).cmp(&(a.own_lines + a.agent_lines)).then_with(|| a.name.cmp(&b.name)));

    Ok(Json(skills))
}

fn checked(request: &HiddenSkillRequest) -> ApiResult<(String, String)> {
    let name = request.name.trim().to_string();

    if !KINDS.contains(&request.kind.as_str()) || name.is_empty() {
        return Err(ApiError::bad_request("skill_invalid", "Give the skill's kind (language, area or other) and its name."));
    }

    Ok((request.kind.clone(), name))
}

pub async fn hide(State(state): State<AppState>, user: AuthUser, Json(request): Json<HiddenSkillRequest>) -> ApiResult<StatusCode> {
    user.require_full_access()?;

    let (kind, name) = checked(&request)?;

    sqlx::query("insert into hidden_skills (owner, kind, name) values ($1, $2, $3) on conflict do nothing").bind(user.id).bind(kind).bind(name).execute(&state.pool).await?;

    Ok(StatusCode::NO_CONTENT)
}

pub async fn unhide(State(state): State<AppState>, user: AuthUser, Json(request): Json<HiddenSkillRequest>) -> ApiResult<StatusCode> {
    user.require_full_access()?;

    let (kind, name) = checked(&request)?;

    sqlx::query("delete from hidden_skills where owner = $1 and kind = $2 and name = $3").bind(user.id).bind(kind).bind(name).execute(&state.pool).await?;

    Ok(StatusCode::NO_CONTENT)
}
