use crate::auth::AuthUser;
use crate::error::ApiResult;
use crate::state::AppState;
use axum::Json;
use axum::extract::{MatchedPath, Query, Request, State};
use axum::http::HeaderValue;
use axum::middleware::Next;
use axum::response::Response;
use chrono::{DateTime, Utc};
use serde::Deserialize;
use serde_json::{Map, Value, json};
use std::fmt::Debug;
use std::sync::OnceLock;
use std::time::{Duration, Instant};
use tokio::sync::mpsc;
use tracing::field::{Empty, Field, Visit};
use tracing::span::{Attributes, Id, Record};
use tracing::{Event, Instrument, Subscriber};
use tracing_subscriber::layer::{Context, Layer};
use tracing_subscriber::registry::LookupSpan;
use uuid::Uuid;

pub const RETENTION_HOURS: i32 = 48;
pub const LARGEST_STORE: i64 = 200_000;
const QUEUE: usize = 4096;
const BATCH: usize = 200;
const BATCH_WAIT: Duration = Duration::from_secs(1);
const LONGEST_STATEMENT: usize = 500;
const DEFAULT_READ: i64 = 20;
const LARGEST_READ: i64 = 100;
const RECORDED: [&str; 4] = ["http", "mcp_tool", "node", "external"];
const KEPT_FIELDS: [&str; 12] = ["method", "route", "status", "owner", "request_id", "tool", "error", "answer", "node", "host", "path", "conversation"];
const REQUEST_ID: &str = "x-request-id";
const UNTRACED_ROUTES: [&str; 1] = ["/api/v1/stream"];

static SINK: OnceLock<mpsc::Sender<Finished>> = OnceLock::new();

struct Open {
    started: Instant,
    started_at: DateTime<Utc>,
    fields: Map<String, Value>,
    children: Vec<Finished>,
}

struct Finished {
    name: String,
    started_at: DateTime<Utc>,
    duration_ms: f64,
    fields: Map<String, Value>,
    children: Vec<Finished>,
}

struct KeptFields<'a>(&'a mut Map<String, Value>);

impl Visit for KeptFields<'_> {
    fn record_str(&mut self, field: &Field, value: &str) {
        self.keep(field, Value::String(value.to_string()));
    }

    fn record_u64(&mut self, field: &Field, value: u64) {
        self.keep(field, json!(value));
    }

    fn record_i64(&mut self, field: &Field, value: i64) {
        self.keep(field, json!(value));
    }

    fn record_bool(&mut self, field: &Field, value: bool) {
        self.keep(field, json!(value));
    }

    fn record_debug(&mut self, field: &Field, value: &dyn Debug) {
        self.keep(field, Value::String(format!("{value:?}")));
    }
}

impl KeptFields<'_> {
    fn keep(&mut self, field: &Field, value: Value) {
        if KEPT_FIELDS.contains(&field.name()) {
            self.0.insert(field.name().to_string(), value);
        }
    }
}

struct QueryFields<'a> {
    fields: &'a mut Map<String, Value>,
    elapsed_secs: f64,
}

impl Visit for QueryFields<'_> {
    fn record_str(&mut self, field: &Field, value: &str) {
        match field.name() {
            "summary" => {
                self.fields.insert("statement".into(), Value::String(value.to_string()));
            }
            "db.statement" if !value.trim().is_empty() => {
                self.fields.insert("sql".into(), Value::String(value.trim().chars().take(LONGEST_STATEMENT).collect()));
            }
            _ => {}
        }
    }

    fn record_u64(&mut self, field: &Field, value: u64) {
        if matches!(field.name(), "rows_affected" | "rows_returned") {
            self.fields.insert(field.name().into(), json!(value));
        }
    }

    fn record_f64(&mut self, field: &Field, value: f64) {
        if field.name() == "elapsed_secs" {
            self.elapsed_secs = value;
        }
    }

    fn record_debug(&mut self, _: &Field, _: &dyn Debug) {}
}

pub struct Recorder;

pub fn recorder<S>() -> tracing_subscriber::filter::Filtered<Recorder, tracing_subscriber::filter::Targets, S>
where
    S: Subscriber + for<'a> LookupSpan<'a>,
{
    Recorder.with_filter(tracing_subscriber::filter::Targets::new().with_target("mycelium_control", tracing::Level::INFO).with_target("sqlx::query", tracing::Level::DEBUG))
}

impl<S> Layer<S> for Recorder
where
    S: Subscriber + for<'a> LookupSpan<'a>,
{
    fn on_new_span(&self, attributes: &Attributes<'_>, id: &Id, context: Context<'_, S>) {
        let metadata = attributes.metadata();

        if !RECORDED.contains(&metadata.name()) || !metadata.target().starts_with("mycelium_control") {
            return;
        }

        let Some(span) = context.span(id) else {
            return;
        };
        let mut fields = Map::new();
        attributes.record(&mut KeptFields(&mut fields));
        span.extensions_mut().insert(Open { started: Instant::now(), started_at: Utc::now(), fields, children: Vec::new() });
    }

    fn on_record(&self, id: &Id, values: &Record<'_>, context: Context<'_, S>) {
        if let Some(span) = context.span(id)
            && let Some(open) = span.extensions_mut().get_mut::<Open>()
        {
            values.record(&mut KeptFields(&mut open.fields));
        }
    }

    fn on_event(&self, event: &Event<'_>, context: Context<'_, S>) {
        if event.metadata().target() != "sqlx::query" {
            return;
        }

        let Some(scope) = context.event_scope(event) else {
            return;
        };

        for span in scope {
            let mut extensions = span.extensions_mut();

            if let Some(open) = extensions.get_mut::<Open>() {
                let mut fields = Map::new();
                let mut query = QueryFields { fields: &mut fields, elapsed_secs: 0.0 };
                event.record(&mut query);
                let elapsed_secs = query.elapsed_secs;
                let started_at = Utc::now() - chrono::Duration::microseconds((elapsed_secs * 1_000_000.0) as i64);
                open.children.push(Finished { name: "db".into(), started_at, duration_ms: elapsed_secs * 1000.0, fields, children: Vec::new() });

                return;
            }
        }
    }

    fn on_close(&self, id: Id, context: Context<'_, S>) {
        let Some(span) = context.span(&id) else {
            return;
        };
        let Some(open) = span.extensions_mut().remove::<Open>() else {
            return;
        };
        let finished = Finished { name: span.name().to_string(), started_at: open.started_at, duration_ms: open.started.elapsed().as_secs_f64() * 1000.0, fields: open.fields, children: open.children };
        let mut ancestor = span.parent();

        while let Some(parent) = ancestor {
            if let Some(parent_open) = parent.extensions_mut().get_mut::<Open>() {
                parent_open.children.push(finished);

                return;
            }

            ancestor = parent.parent();
        }

        if let Some(sink) = SINK.get() {
            let _ = sink.try_send(finished);
        }
    }
}

pub fn store_regularly(state: AppState) {
    let (sender, mut receiver) = mpsc::channel(QUEUE);

    if SINK.set(sender).is_err() {
        return;
    }

    tokio::spawn(async move {
        loop {
            let Some(first) = receiver.recv().await else {
                return;
            };
            let mut batch = vec![first];
            let deadline = tokio::time::Instant::now() + BATCH_WAIT;

            while batch.len() < BATCH {
                match tokio::time::timeout_at(deadline, receiver.recv()).await {
                    Ok(Some(trace)) => batch.push(trace),
                    _ => break,
                }
            }

            if let Err(cause) = store(&state, batch).await {
                tracing::warn!("could not store traces: {}", cause.message());
            }
        }
    });
}

struct Row {
    trace_id: Uuid,
    owner: Option<Uuid>,
    position: i32,
    parent: Option<i32>,
    name: String,
    label: String,
    started_at: DateTime<Utc>,
    duration_ms: f64,
    fields: Value,
}

async fn store(state: &AppState, traces: Vec<Finished>) -> ApiResult<()> {
    let mut rows = Vec::new();

    for root in traces {
        let trace_id = root.fields.get("request_id").and_then(Value::as_str).and_then(|id| id.parse().ok()).unwrap_or_else(Uuid::new_v4);
        let owner = owner_of(&root);
        let failed = root.fields.get("status").and_then(Value::as_u64).is_some_and(|status| status >= 500) || root.fields.get("error").and_then(Value::as_bool) == Some(true);
        let in_full = root.duration_ms >= state.config.trace_kept_in_full.as_secs_f64() * 1000.0 || failed;

        flatten(root, trace_id, owner, None, in_full, &mut rows);
    }

    let mut query = sqlx::QueryBuilder::new("insert into trace_spans (trace_id, owner, position, parent, name, label, started_at, duration_ms, fields) ");
    query.push_values(rows, |mut row, span| {
        row.push_bind(span.trace_id).push_bind(span.owner).push_bind(span.position).push_bind(span.parent).push_bind(span.name).push_bind(span.label).push_bind(span.started_at).push_bind(span.duration_ms).push_bind(span.fields);
    });
    query.build().execute(&state.pool).await?;

    Ok(())
}

fn owner_of(span: &Finished) -> Option<Uuid> {
    span.fields.get("owner").and_then(Value::as_str).and_then(|owner| owner.parse().ok()).or_else(|| span.children.iter().find_map(owner_of))
}

fn flatten(span: Finished, trace_id: Uuid, owner: Option<Uuid>, parent: Option<i32>, in_full: bool, rows: &mut Vec<Row>) {
    let position = rows.iter().filter(|row| row.trace_id == trace_id).count() as i32;
    let label = label_of(&span);
    rows.push(Row { trace_id, owner, position, parent, name: span.name, label, started_at: span.started_at, duration_ms: span.duration_ms, fields: Value::Object(span.fields) });

    if in_full {
        for child in span.children {
            flatten(child, trace_id, owner, Some(position), true, rows);
        }
    }
}

fn label_of(span: &Finished) -> String {
    let field = |name: &str| span.fields.get(name).and_then(Value::as_str).unwrap_or_default();

    match span.name.as_str() {
        "http" => {
            let tools: Vec<&str> = span.children.iter().filter(|child| child.name == "mcp_tool").filter_map(|child| child.fields.get("tool").and_then(Value::as_str)).collect();

            format!("{} {} {}", field("method"), field("route"), tools.join(" ")).trim_end().to_string()
        }
        "mcp_tool" => field("tool").to_string(),
        "db" => field("statement").to_string(),
        "node" => field("answer").to_string(),
        "external" => format!("{} {}{}", field("method"), field("host"), field("path")),
        other => other.to_string(),
    }
}

pub async fn http(request: Request, next: Next) -> Response {
    let route = request.extensions().get::<MatchedPath>().map(|path| path.as_str().to_string());
    let Some(route) = route.filter(|route| (route.starts_with("/api/") || route.starts_with("/auth/")) && !UNTRACED_ROUTES.contains(&route.as_str())) else {
        return next.run(request).await;
    };
    let request_id = request.headers().get(REQUEST_ID).and_then(|value| value.to_str().ok()).and_then(|value| value.parse::<Uuid>().ok()).unwrap_or_else(Uuid::new_v4);
    let span = tracing::info_span!("http", method = %request.method(), route = %route, request_id = %request_id, status = Empty, owner = Empty);
    let mut response = next.run(request).instrument(span.clone()).await;

    span.record("status", response.status().as_u16());

    if let Ok(value) = HeaderValue::from_str(&request_id.to_string()) {
        response.headers_mut().insert(REQUEST_ID, value);
    }

    response
}

pub fn record_owner(owner: Uuid) {
    tracing::Span::current().record("owner", tracing::field::display(owner));
}

pub async fn send(request: reqwest::RequestBuilder) -> reqwest::Result<reqwest::Response> {
    let (client, built) = request.build_split();
    let built = built?;
    let span = tracing::info_span!("external", method = %built.method(), host = built.url().host_str().unwrap_or_default(), path = built.url().path(), status = Empty);
    let response = client.execute(built).instrument(span.clone()).await;

    if let Ok(response) = &response {
        span.record("status", response.status().as_u16());
    }

    response
}

#[derive(Deserialize, Default)]
pub struct Filter {
    pub since: Option<DateTime<Utc>>,
    pub name: Option<String>,
    pub min_duration_ms: Option<f64>,
    pub limit: Option<i64>,
}

pub async fn list(State(state): State<AppState>, user: AuthUser, Query(filter): Query<Filter>) -> ApiResult<Json<Value>> {
    Ok(Json(read(&state, user.id, &filter).await?))
}

pub async fn read(state: &AppState, owner: Uuid, filter: &Filter) -> ApiResult<Value> {
    let limit = filter.limit.unwrap_or(DEFAULT_READ).clamp(1, LARGEST_READ);
    let pattern = filter.name.as_deref().map(|name| format!("%{}%", name.replace('\\', "\\\\").replace('%', "\\%").replace('_', "\\_")));
    let roots = sqlx::query_as::<_, (Uuid,)>(
        "select trace_id from trace_spans where owner = $1 and position = 0 and ($2::timestamptz is null or started_at >= $2) and ($3::text is null or label ilike $3) and ($4::float8 is null or duration_ms >= $4)
         order by started_at desc limit $5",
    )
    .bind(owner)
    .bind(filter.since)
    .bind(pattern)
    .bind(filter.min_duration_ms)
    .bind(limit)
    .fetch_all(&state.pool)
    .await?;
    let ids: Vec<Uuid> = roots.into_iter().map(|(id,)| id).collect();
    let spans = sqlx::query_as::<_, (Uuid, i32, Option<i32>, String, String, DateTime<Utc>, f64, Value)>(
        "select trace_id, position, parent, name, label, started_at, duration_ms, fields from trace_spans where owner = $1 and trace_id = any($2) order by trace_id, position",
    )
    .bind(owner)
    .bind(&ids)
    .fetch_all(&state.pool)
    .await?;
    let traces: Vec<Value> = ids
        .iter()
        .filter_map(|id| {
            let spans: Vec<_> = spans.iter().filter(|span| span.0 == *id).collect();

            spans.iter().find(|span| span.1 == 0).map(|root| tree(root, &spans))
        })
        .collect();

    Ok(json!({"traces": traces}))
}

type SpanRow = (Uuid, i32, Option<i32>, String, String, DateTime<Utc>, f64, Value);

fn tree(span: &SpanRow, all: &[&SpanRow]) -> Value {
    let children: Vec<Value> = all.iter().filter(|child| child.2 == Some(span.1)).map(|child| tree(child, all)).collect();
    let mut node = json!({"name": span.3, "label": span.4, "started_at": span.5, "duration_ms": (span.6 * 10.0).round() / 10.0, "fields": span.7});

    if span.1 == 0 {
        node["trace_id"] = json!(span.0);
    }

    if !children.is_empty() {
        node["children"] = Value::Array(children);
    }

    node
}
