use crate::error::{ApiError, ApiResult};
use crate::forges::{self, ForgeApi, ForgeRow, Run, RunQuery};
use crate::secrets::Sealed;
use crate::state::AppState;
use crate::watches;
use axum::extract::{Path, State};
use axum::http::{HeaderMap, StatusCode};
use hmac::{Hmac, KeyInit, Mac};
use mycelium_proto::api::ForgeKind;
use serde_json::{Value, json};
use sha2::Sha256;
use subtle::ConstantTimeEq;
use uuid::Uuid;

const POLL_BATCH: i64 = 20;
const FINISHED_KEY: &str = "finished";
const WATCH_COLUMNS: &str = "id, owner, watcher_id, kind, forge_id, repository, reference";
const SHA_MARK: &str = " sha:";

pub enum Target {
    Run(String),
    Reference { reference: String, sha: Option<String> },
    Tag(String),
}

pub struct Place {
    forge: Option<ForgeRow>,
    repository: String,
}

enum ForgeEvent {
    RunFinished(Run),
    Released(String),
}

#[derive(sqlx::FromRow)]
struct OpenWatch {
    id: Uuid,
    owner: Uuid,
    watcher_id: Uuid,
    kind: String,
    forge_id: Option<Uuid>,
    repository: String,
    reference: String,
}

impl Target {
    pub fn encode(&self) -> String {
        match self {
            Target::Run(id) => format!("run:{id}"),
            Target::Reference { reference, sha: None } => format!("ref:{reference}"),
            Target::Reference { reference, sha: Some(sha) } => format!("ref:{reference}{SHA_MARK}{sha}"),
            Target::Tag(tag) => format!("tag:{tag}"),
        }
    }

    fn parse(text: &str) -> Option<Target> {
        let (kind, rest) = text.split_once(':')?;

        match kind {
            "run" => Some(Target::Run(rest.to_string())),
            "tag" => Some(Target::Tag(rest.to_string())),
            "ref" => match rest.split_once(SHA_MARK) {
                Some((reference, sha)) => Some(Target::Reference { reference: reference.to_string(), sha: Some(sha.to_string()) }),
                None => Some(Target::Reference { reference: rest.to_string(), sha: None }),
            },
            _ => None,
        }
    }

    fn matches(&self, run: &Run) -> bool {
        match self {
            Target::Run(id) => run.id == *id,
            Target::Reference { reference, sha } => run.reference == *reference && sha.as_ref().is_none_or(|sha| run.sha.starts_with(sha.as_str())),
            Target::Tag(_) => false,
        }
    }

    pub fn label(&self) -> String {
        match self {
            Target::Run(id) => format!("run {id}"),
            Target::Reference { reference, .. } => reference.clone(),
            Target::Tag(tag) => tag.clone(),
        }
    }
}

pub async fn resolve_place(state: &AppState, owner: Uuid, project: Option<&str>, forge: Option<&str>, repository: Option<&str>) -> ApiResult<Result<Place, String>> {
    let supported = |row: &ForgeRow| matches!(forges::parse_kind(&row.kind), Ok(ForgeKind::Github | ForgeKind::Gitlab));
    let (forge_row, repository) = match (project, forge, repository) {
        (Some(project), _, _) => {
            let found = sqlx::query_as::<_, (Option<Uuid>, Option<String>)>("select forge_id, repository from projects where owner = $1 and (id::text = lower($2) or name = $2)").bind(owner).bind(project).fetch_optional(&state.pool).await?;

            match found {
                None => return Ok(Err(format!("None of the person's projects is {project}."))),
                Some((Some(forge_id), Some(repository))) => (Some(forges::find_owned(state, forge_id, owner).await?), repository),
                Some(_) => return Ok(Err(format!("The project {project} is not linked to a repository on a forge. Give forge and repository instead."))),
            }
        }
        (None, Some(forge), Some(repository)) => {
            let found = sqlx::query_scalar::<_, Uuid>("select id from forges where owner = $1 and (id::text = lower($2) or kind = lower($2) or replace(replace(base_url, 'https://', ''), 'http://', '') = lower($2))").bind(owner).bind(forge).fetch_all(&state.pool).await?;

            match found.as_slice() {
                [only] => (Some(forges::find_owned(state, *only, owner).await?), repository.to_string()),
                [] if forge.eq_ignore_ascii_case("github.com") => (None, repository.to_string()),
                [] => return Ok(Err(format!("None of the person's forges is {forge}. Use github.com for a public GitHub repository."))),
                _ => return Ok(Err(format!("More than one forge is {forge}. Give its id."))),
            }
        }
        _ => return Ok(Err("Give a project, or a forge together with its repository.".to_string())),
    };

    if !repository.contains('/') {
        return Ok(Err(format!("Give the repository as owner/name, such as acme/app. {repository} has no owner.")));
    }

    if forge_row.as_ref().is_some_and(|row| !supported(row)) {
        return Ok(Err("Only GitHub and GitLab repositories can be watched.".to_string()));
    }

    Ok(Ok(Place { forge: forge_row, repository }))
}

pub async fn start(state: &AppState, owner: Uuid, watcher: Uuid, kind: &str, place: Place, target: Target) -> ApiResult<(Uuid, String)> {
    let reference = target.encode();
    let forge_id = place.forge.as_ref().map(|forge| forge.id);
    let existing = sqlx::query_scalar::<_, Uuid>("select id from watches where watcher_id = $1 and kind = $2 and forge_id is not distinct from $3 and repository = $4 and reference = $5 and ended_at is null")
        .bind(watcher)
        .bind(kind)
        .bind(forge_id)
        .bind(&place.repository)
        .bind(&reference)
        .fetch_optional(&state.pool)
        .await?;
    let watch = match existing {
        Some(existing) => existing,
        None => {
            let created = sqlx::query_scalar::<_, Uuid>("insert into watches (owner, watcher_id, kind, forge_id, repository, reference) values ($1, $2, $3, $4, $5, $6) returning id")
                .bind(owner)
                .bind(watcher)
                .bind(kind)
                .bind(forge_id)
                .bind(&place.repository)
                .bind(&reference)
                .fetch_one(&state.pool)
                .await?;
            state.publish(Some(owner), mycelium_proto::stream::StreamMessage::WatchesChanged { conversation_id: watcher });

            created
        }
    };

    let Some(forge) = &place.forge else {
        return Ok((watch, "Mycelium checks it every minute.".to_string()));
    };

    if !ensure_webhook(state, forge, &place.repository).await? {
        return Ok((watch, format!("Mycelium could not register a webhook on {} for {}, which needs admin rights on the repository, so it checks every minute instead.", forges::display_name(forges::parse_kind(&forge.kind)?), place.repository)));
    }

    let open = sqlx::query_as::<_, OpenWatch>(sqlx::AssertSqlSafe(format!("select {WATCH_COLUMNS} from watches where id = $1"))).bind(watch).fetch_one(&state.pool).await?;

    if let Err(error) = check(state, &open).await {
        tracing::warn!("could not check watch {watch} after creating it: {}", error.message());
    }

    Ok((watch, format!("{} tells Mycelium when it finishes.", forges::display_name(forges::parse_kind(&forge.kind)?))))
}

async fn ensure_webhook(state: &AppState, forge: &ForgeRow, repository: &str) -> ApiResult<bool> {
    let known = sqlx::query_scalar::<_, i64>("select count(*) from forge_webhooks where forge_id = $1 and lower(repository) = lower($2)").bind(forge.id).bind(repository).fetch_one(&state.pool).await?;

    if known > 0 {
        return Ok(true);
    }

    let fresh = format!("{}{}", Uuid::new_v4().simple(), Uuid::new_v4().simple());
    let secret = sqlx::query_scalar::<_, Sealed>("update forges set webhook_secret = coalesce(webhook_secret, $2) where id = $1 returning webhook_secret").bind(forge.id).bind(Sealed::from(fresh)).fetch_one(&state.pool).await?;
    let receiver = format!("{}/api/v1/forges/{}/webhook", state.config.public_url, forge.id);
    let created = ForgeApi::from_row(forge)?.create_webhook(&state.http, repository, &receiver, &secret).await;

    if created.is_err() {
        return Ok(false);
    }

    sqlx::query("insert into forge_webhooks (forge_id, repository) values ($1, $2) on conflict do nothing").bind(forge.id).bind(repository).execute(&state.pool).await?;

    Ok(true)
}

fn pipeline_text(repository: &str, run: &Run) -> String {
    let label = if run.reference.is_empty() { format!("run {}", run.id) } else { run.reference.clone() };

    format!("From {repository} {label}:\n\nThe {} {}.\n{}", run.title, run.outcome(), run.url)
}

fn release_text(repository: &str, tag: &str, runs: &[Run], release: Option<&str>) -> String {
    let outcomes = runs.iter().map(|run| format!("The {} {}", run.title, run.outcome())).collect::<Vec<_>>().join("; ");
    let release_line = release.map(|url| format!("\nRelease: {url}")).unwrap_or_default();

    if runs.is_empty() {
        return format!("From {repository} {tag}:\n\nThe release for {tag} was published.{release_line}");
    }

    format!("From {repository} {tag}:\n\nAll runs for {tag} have finished. {outcomes}.{release_line}")
}

async fn finish(state: &AppState, watch: &OpenWatch, text: String) -> ApiResult<()> {
    watches::deliver(state, watch.owner, watch.id, watch.watcher_id, FINISHED_KEY, text).await?;

    watches::end(state, watch.owner, watch.id).await
}

async fn check(state: &AppState, watch: &OpenWatch) -> ApiResult<()> {
    let Some(target) = Target::parse(&watch.reference) else {
        return Ok(());
    };
    let api = match watch.forge_id {
        Some(forge_id) => ForgeApi::from_row(&forges::find_owned(state, forge_id, watch.owner).await?)?,
        None => ForgeApi::public_github(&state.config.github_api_url),
    };
    let failed = |error: forges::ForgeError| error.into_api(api.kind, &api.base_url);

    match &target {
        Target::Run(id) => {
            let runs = api.runs(&state.http, &watch.repository, RunQuery::Id(id)).await.map_err(failed)?;

            if let Some(run) = runs.iter().find(|run| run.finished) {
                finish(state, watch, pipeline_text(&watch.repository, run)).await?;
            }
        }
        Target::Reference { reference, sha } => {
            let runs = api.runs(&state.http, &watch.repository, RunQuery::Reference { reference, sha: sha.as_deref() }).await.map_err(failed)?;

            if let Some(run) = runs.first().filter(|run| run.finished) {
                finish(state, watch, pipeline_text(&watch.repository, run)).await?;
            }
        }
        Target::Tag(tag) => {
            let runs = api.runs(&state.http, &watch.repository, RunQuery::Reference { reference: tag, sha: None }).await.map_err(failed)?;
            let release = api.release_url(&state.http, &watch.repository, tag).await.map_err(failed)?;
            let settled = runs.iter().all(|run| run.finished) && (!runs.is_empty() || release.is_some());

            if settled {
                finish(state, watch, release_text(&watch.repository, tag, &runs, release.as_deref())).await?;
            }
        }
    }

    Ok(())
}

pub async fn poll(state: &AppState) -> ApiResult<()> {
    let due = sqlx::query_as::<_, OpenWatch>(sqlx::AssertSqlSafe(format!(
        "update watches set checked_at = now() where id in (
            select w.id from watches w
            where w.kind in ('pipeline', 'release') and w.ended_at is null
              and (w.checked_at is null or w.checked_at <= now() - make_interval(secs => $1::float8))
              and not exists (select 1 from forge_webhooks h where h.forge_id = w.forge_id and lower(h.repository) = lower(w.repository))
            order by w.checked_at nulls first limit {POLL_BATCH} for update skip locked)
         returning {WATCH_COLUMNS}"
    )))
    .bind(state.config.forge_watch_interval.as_secs_f64())
    .fetch_all(&state.pool)
    .await?;

    for watch in due {
        if let Err(error) = check(state, &watch).await {
            tracing::warn!("could not check {} {} for watch {}: {}", watch.repository, watch.reference, watch.id, error.message());
        }
    }

    Ok(())
}

fn github_event(name: &str, payload: &Value) -> Option<(String, ForgeEvent)> {
    let repository = payload["repository"]["full_name"].as_str()?.to_string();

    match (name, payload["action"].as_str()?) {
        ("workflow_run", "completed") => Run::from_github(&payload["workflow_run"]).filter(|run| run.finished).map(|run| (repository, ForgeEvent::RunFinished(run))),
        ("release", "published") => Some((repository, ForgeEvent::Released(payload["release"]["tag_name"].as_str()?.to_string()))),
        _ => None,
    }
}

fn gitlab_event(payload: &Value) -> Option<(String, ForgeEvent)> {
    let repository = payload["project"]["path_with_namespace"].as_str()?.to_string();

    match payload["object_kind"].as_str()? {
        "pipeline" => {
            let attributes = &payload["object_attributes"];
            let pipeline = json!({"id": attributes["id"], "ref": attributes["ref"], "sha": attributes["sha"], "status": attributes["status"], "web_url": attributes["url"]});

            Run::from_gitlab(&pipeline).filter(|run| run.finished).map(|run| (repository, ForgeEvent::RunFinished(run)))
        }
        "release" if payload["action"].as_str() == Some("create") => Some((repository, ForgeEvent::Released(payload["tag"].as_str()?.to_string()))),
        _ => None,
    }
}

pub async fn webhook(State(state): State<AppState>, Path(id): Path<Uuid>, headers: HeaderMap, body: axum::body::Bytes) -> ApiResult<StatusCode> {
    let found = sqlx::query_as::<_, (String, Option<Sealed>)>("select kind, webhook_secret from forges where id = $1").bind(id).fetch_optional(&state.pool).await?;
    let Some((kind, Some(secret))) = found else {
        return Err(ApiError::unauthorised());
    };
    let kind = forges::parse_kind(&kind)?;
    let header = |name: &str| headers.get(name).and_then(|value| value.to_str().ok()).unwrap_or_default().to_string();
    let genuine = match kind {
        ForgeKind::Github => {
            let mut mac = Hmac::<Sha256>::new_from_slice(secret.as_bytes()).map_err(ApiError::internal)?;

            mac.update(&body);

            let digest: String = mac.finalize().into_bytes().iter().map(|byte| format!("{byte:02x}")).collect();

            bool::from(header("x-hub-signature-256").as_bytes().ct_eq(format!("sha256={digest}").as_bytes()))
        }
        ForgeKind::Gitlab => bool::from(header("x-gitlab-token").as_bytes().ct_eq(secret.as_bytes())),
        ForgeKind::Forgejo => false,
    };

    if !genuine {
        return Err(ApiError::unauthorised());
    }

    let payload: Value = serde_json::from_slice(&body).unwrap_or(Value::Null);
    let event = match kind {
        ForgeKind::Github => github_event(&header("x-github-event"), &payload),
        _ => gitlab_event(&payload),
    };
    let Some((repository, event)) = event else {
        return Ok(StatusCode::NO_CONTENT);
    };
    let open = sqlx::query_as::<_, OpenWatch>(sqlx::AssertSqlSafe(format!("select {WATCH_COLUMNS} from watches where forge_id = $1 and kind in ('pipeline', 'release') and ended_at is null and lower(repository) = lower($2)")))
        .bind(id)
        .bind(&repository)
        .fetch_all(&state.pool)
        .await?;

    for watch in open {
        let Some(target) = Target::parse(&watch.reference) else {
            continue;
        };
        let reacted = match (&event, watch.kind.as_str(), &target) {
            (ForgeEvent::RunFinished(run), "pipeline", target) if target.matches(run) => finish(&state, &watch, pipeline_text(&repository, run)).await,
            (ForgeEvent::RunFinished(run), "release", Target::Tag(tag)) if *tag == run.reference => check(&state, &watch).await,
            (ForgeEvent::Released(released), "release", Target::Tag(tag)) if tag == released => check(&state, &watch).await,
            _ => Ok(()),
        };

        if let Err(error) = reacted {
            tracing::warn!("could not react to a {} webhook for watch {}: {}", forges::display_name(kind), watch.id, error.message());
        }
    }

    Ok(StatusCode::NO_CONTENT)
}
