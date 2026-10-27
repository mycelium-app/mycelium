use crate::auth::AuthUser;
use crate::chat_tool_servers::{SERVER_COLUMNS, ServerRow, StoredTool};
use crate::error::{ApiError, ApiResult};
use crate::integrations::{CallOutput, Credentials};
use crate::mcp_oauth::page;
use crate::state::{AppState, PendingFreeAgent, new_token};
use axum::Json;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::Response;
use mycelium_proto::api::FreeAgentSignIn;
use mycelium_proto::stream::StreamMessage;
use serde_json::{Value, json};
use std::time::Duration;
use uuid::Uuid;

pub const PRODUCTION_ADDRESS: &str = "https://api.freeagent.com";
const WRITES: [&str; 2] = ["create_draft_invoice", "create_expense"];
const DEFAULT_PER_PAGE: u64 = 25;
const MAX_PER_PAGE: u64 = 100;
const DEFAULT_PAYMENT_TERMS_IN_DAYS: u64 = 30;
const REFRESH_MARGIN: chrono::Duration = chrono::Duration::seconds(60);
const TIMEOUT: Duration = Duration::from_secs(30);
const SIGN_IN_FIRST: &str = "Sign in to FreeAgent in Settings, Integrations.";

struct Resource {
    list: &'static str,
    get: &'static str,
    path: &'static str,
    singular: &'static str,
    noun: &'static str,
}

const RESOURCES: [Resource; 6] = [
    Resource { list: "list_contacts", get: "get_contact", path: "contacts", singular: "contact", noun: "contact" },
    Resource { list: "list_invoices", get: "get_invoice", path: "invoices", singular: "invoice", noun: "invoice" },
    Resource { list: "list_bills", get: "get_bill", path: "bills", singular: "bill", noun: "bill" },
    Resource { list: "list_bank_accounts", get: "get_bank_account", path: "bank_accounts", singular: "bank_account", noun: "bank account" },
    Resource { list: "list_bank_transactions", get: "get_bank_transaction", path: "bank_transactions", singular: "bank_transaction", noun: "bank transaction" },
    Resource { list: "list_expenses", get: "get_expense", path: "expenses", singular: "expense", noun: "expense" },
];

pub fn tools() -> Vec<StoredTool> {
    let paging = json!({
        "view": {"type": "string", "description": "FreeAgent's view filter for this kind, for example open, overdue, paid or draft for invoices and bills, and unexplained for bank transactions."},
        "from_date": {"type": "string", "description": "Only records dated on or after this day, as YYYY-MM-DD."},
        "to_date": {"type": "string", "description": "Only records dated on or before this day, as YYYY-MM-DD."},
        "page": {"type": "integer", "description": "Which page of results, from 1."},
        "per_page": {"type": "integer", "description": format!("How many to return. Defaults to {DEFAULT_PER_PAGE}, at most {MAX_PER_PAGE}.")}
    });
    let mut tools = Vec::new();

    for resource in &RESOURCES {
        let mut properties = paging.clone();
        let mut required: Vec<&str> = Vec::new();

        if resource.path == "bank_transactions" {
            properties["bank_account"] = json!({"type": "string", "description": "The bank account's id or url, from list_bank_accounts."});
            required.push("bank_account");
        }

        tools.push(StoredTool { name: resource.list.to_string(), description: format!("List {}s in FreeAgent.", resource.noun), input_schema: json!({"type": "object", "properties": properties, "required": required}), read_only: None });
        tools.push(StoredTool {
            name: resource.get.to_string(),
            description: format!("Read one {} in full.", resource.noun),
            input_schema: json!({"type": "object", "properties": {"id": {"type": "string", "description": format!("The {}'s id or url, as the list returned it.", resource.noun)}}, "required": ["id"]}),
            read_only: None,
        });
    }

    tools.push(StoredTool {
        name: "create_draft_invoice".to_string(),
        description: "Create a draft invoice. It is not sent: the person reviews and sends it in FreeAgent. Always needs the person's approval in chats, even when Always allow is on.".to_string(),
        input_schema: json!({"type": "object", "properties": {
            "contact": {"type": "string", "description": "The contact's id or url, from list_contacts."},
            "dated_on": {"type": "string", "description": "The invoice date, as YYYY-MM-DD."},
            "payment_terms_in_days": {"type": "integer", "description": format!("Days to pay. Defaults to {DEFAULT_PAYMENT_TERMS_IN_DAYS}.")},
            "reference": {"type": "string"},
            "items": {"type": "array", "items": {"type": "object", "properties": {
                "description": {"type": "string"},
                "price": {"type": "number", "description": "The price of one unit."},
                "quantity": {"type": "number"},
                "item_type": {"type": "string", "description": "Hours, Days, Products, Services or Comment. Leave out for no unit."}
            }, "required": ["description", "price"]}}
        }, "required": ["contact", "dated_on", "items"]}),
        read_only: None,
    });
    tools.push(StoredTool {
        name: "create_expense".to_string(),
        description: "Create an expense for the person. Always needs the person's approval in chats, even when Always allow is on.".to_string(),
        input_schema: json!({"type": "object", "properties": {
            "category": {"type": "string", "description": "The accounting category's nominal code, such as 285, or its url."},
            "dated_on": {"type": "string", "description": "The expense date, as YYYY-MM-DD."},
            "gross_value": {"type": "number", "description": "The total including tax, in the company's currency. Negative is a payment to the person."},
            "description": {"type": "string"}
        }, "required": ["category", "dated_on", "gross_value"]}),
        read_only: None,
    });

    tools
}

pub fn always_asks(tool: &str) -> bool {
    WRITES.contains(&tool)
}

fn failure(text: impl Into<String>) -> CallOutput {
    CallOutput { text: text.into(), is_error: true }
}

fn base(address: &str) -> &str {
    address.trim_end_matches('/')
}

fn locator(credentials: &Credentials, path: &str, entered: &str) -> String {
    let id = entered.trim().trim_end_matches('/').rsplit('/').next().unwrap_or_default();

    format!("{}/v2/{path}/{id}", base(&credentials.address))
}

fn tokens_request(http: &reqwest::Client, credentials: &Credentials, body: &[(&str, &str)]) -> reqwest::RequestBuilder {
    http.post(format!("{}/v2/token_endpoint", base(&credentials.address)))
        .basic_auth(&credentials.account, Some(&credentials.secret))
        .header("content-type", "application/x-www-form-urlencoded")
        .body(url::form_urlencoded::Serializer::new(String::new()).extend_pairs(body).finish())
        .timeout(TIMEOUT)
}

struct Tokens {
    access: String,
    refresh: Option<String>,
    expires_at: chrono::DateTime<chrono::Utc>,
}

async fn exchange(builder: reqwest::RequestBuilder) -> Option<Tokens> {
    let response = builder.send().await.ok()?;

    if !response.status().is_success() {
        return None;
    }

    let body: Value = response.json().await.ok()?;
    let lifetime = body["expires_in"].as_i64().unwrap_or(3600);

    Some(Tokens { access: body["access_token"].as_str()?.to_string(), refresh: body["refresh_token"].as_str().map(str::to_string), expires_at: chrono::Utc::now() + chrono::Duration::seconds(lifetime) })
}

pub async fn verify(_http: &reqwest::Client, _credentials: &Credentials) -> Result<(), String> {
    Ok(())
}

async fn sign_out_row(state: &AppState, owner: Uuid, id: Uuid) -> ApiResult<()> {
    sqlx::query("update chat_tool_servers set session_key = null, oauth_refresh_token = null, oauth_expires_at = null where id = $1").bind(id).execute(&state.pool).await?;
    state.hub.tools_changed(owner);
    state.publish(Some(owner), StreamMessage::IntegrationsChanged);

    Ok(())
}

async fn renewed(state: &AppState, owner: Uuid, server: &ServerRow, credentials: &Credentials) -> Option<String> {
    let refresh = server.oauth_refresh_token.as_deref()?;
    let tokens = exchange(tokens_request(&state.http, credentials, &[("grant_type", "refresh_token"), ("refresh_token", refresh)])).await;
    let Some(tokens) = tokens else {
        let _ = sign_out_row(state, owner, server.id).await;

        return None;
    };
    let stored = sqlx::query("update chat_tool_servers set session_key = $2, oauth_refresh_token = coalesce($3, oauth_refresh_token), oauth_expires_at = $4 where id = $1").bind(server.id).bind(&tokens.access).bind(&tokens.refresh).bind(tokens.expires_at).execute(&state.pool).await;

    stored.ok().map(|_| tokens.access)
}

fn request(http: &reqwest::Client, token: &str, method: reqwest::Method, url: &str) -> reqwest::RequestBuilder {
    http.request(method, url).bearer_auth(token).header("accept", "application/json").header("user-agent", "Mycelium").timeout(TIMEOUT)
}

async fn send(builder: reqwest::RequestBuilder) -> Result<(reqwest::StatusCode, Value), String> {
    let response = builder.send().await.map_err(|_| "Mycelium could not reach FreeAgent. Try again in a moment.".to_string())?;
    let status = response.status();
    let location = response.headers().get("location").and_then(|value| value.to_str().ok()).map(str::to_string);
    let mut body: Value = response.json().await.unwrap_or(Value::Null);

    if let (Some(location), Some(body)) = (location, body.as_object_mut()) {
        body.insert("location".to_string(), json!(location));
    }

    Ok((status, body))
}

fn complaint(status: reqwest::StatusCode, body: &Value) -> String {
    let reason = body["errors"].as_array().and_then(|errors| errors.first()).and_then(|error| error["message"].as_str()).map(str::to_string).unwrap_or_else(|| format!("status {}", status.as_u16()));

    format!("FreeAgent answered with an error: {reason}")
}

fn query_of(arguments: &Value, bank_transactions: bool, credentials: &Credentials) -> String {
    let per_page = arguments["per_page"].as_u64().unwrap_or(DEFAULT_PER_PAGE).clamp(1, MAX_PER_PAGE);
    let mut query = url::form_urlencoded::Serializer::new(String::new());

    query.append_pair("per_page", &per_page.to_string());

    for key in ["view", "from_date", "to_date"] {
        if let Some(value) = arguments[key].as_str().map(str::trim).filter(|value| !value.is_empty()) {
            query.append_pair(key, value);
        }
    }

    if let Some(page) = arguments["page"].as_u64() {
        query.append_pair("page", &page.to_string());
    }

    if bank_transactions && let Some(account) = arguments["bank_account"].as_str().map(str::trim).filter(|account| !account.is_empty()) {
        query.append_pair("bank_account", &locator(credentials, "bank_accounts", account));
    }

    query.finish()
}

async fn run(http: &reqwest::Client, token: &str, credentials: &Credentials, tool: &str, arguments: &Value) -> Result<Value, (String, bool)> {
    let unauthorised = |status: reqwest::StatusCode| status == reqwest::StatusCode::UNAUTHORIZED;
    let address = base(&credentials.address);

    if let Some(resource) = RESOURCES.iter().find(|resource| resource.list == tool) {
        if resource.path == "bank_transactions" && arguments["bank_account"].as_str().map(str::trim).is_none_or(str::is_empty) {
            return Err(("Give the bank account in bank_account.".to_string(), false));
        }

        let url = format!("{address}/v2/{}?{}", resource.path, query_of(arguments, resource.path == "bank_transactions", credentials));
        let (status, body) = send(request(http, token, reqwest::Method::GET, &url)).await.map_err(|reason| (reason, false))?;

        return match status.is_success() {
            true => Ok(body[resource.path].clone()),
            false => Err((complaint(status, &body), unauthorised(status))),
        };
    }

    if let Some(resource) = RESOURCES.iter().find(|resource| resource.get == tool) {
        let Some(id) = arguments["id"].as_str().map(str::trim).filter(|id| !id.is_empty()) else {
            return Err((format!("Give the {}'s id in id.", resource.noun), false));
        };
        let (status, body) = send(request(http, token, reqwest::Method::GET, &locator(credentials, resource.path, id))).await.map_err(|reason| (reason, false))?;

        return match status.is_success() {
            true => Ok(body[resource.singular].clone()),
            false => Err((complaint(status, &body), unauthorised(status))),
        };
    }

    match tool {
        "create_draft_invoice" => {
            let (Some(contact), Some(dated_on), Some(items)) = (arguments["contact"].as_str(), arguments["dated_on"].as_str(), arguments["items"].as_array().filter(|items| !items.is_empty())) else {
                return Err(("Give the contact, the invoice date and at least one item.".to_string(), false));
            };
            let items: Vec<Value> = items
                .iter()
                .map(|item| {
                    let mut line = json!({"description": item["description"], "price": item["price"].as_f64().unwrap_or_default().to_string(), "quantity": item["quantity"].as_f64().unwrap_or(1.0).to_string()});

                    if let Some(kind) = item["item_type"].as_str().filter(|kind| !kind.is_empty()) {
                        line["item_type"] = json!(kind);
                    }

                    line
                })
                .collect();
            let mut invoice = json!({"contact": locator(credentials, "contacts", contact), "dated_on": dated_on, "payment_terms_in_days": arguments["payment_terms_in_days"].as_u64().unwrap_or(DEFAULT_PAYMENT_TERMS_IN_DAYS), "invoice_items": items});

            if let Some(reference) = arguments["reference"].as_str().filter(|reference| !reference.is_empty()) {
                invoice["reference"] = json!(reference);
            }

            let (status, body) = send(request(http, token, reqwest::Method::POST, &format!("{address}/v2/invoices")).json(&json!({"invoice": invoice}))).await.map_err(|reason| (reason, false))?;

            match status.is_success() {
                true => Ok(json!({"created": true, "status": body["invoice"]["status"].as_str().unwrap_or("Draft"), "url": body["location"].as_str().or_else(|| body["invoice"]["url"].as_str())})),
                false => Err((complaint(status, &body), unauthorised(status))),
            }
        }
        "create_expense" => {
            let (Some(category), Some(dated_on), Some(gross_value)) = (arguments["category"].as_str(), arguments["dated_on"].as_str(), arguments["gross_value"].as_f64()) else {
                return Err(("Give the category, the date and the gross value.".to_string(), false));
            };
            let (status, me) = send(request(http, token, reqwest::Method::GET, &format!("{address}/v2/users/me"))).await.map_err(|reason| (reason, false))?;

            if !status.is_success() {
                return Err((complaint(status, &me), unauthorised(status)));
            }

            let mut expense = json!({"user": me["user"]["url"], "category": locator(credentials, "categories", category), "dated_on": dated_on, "gross_value": gross_value.to_string()});

            if let Some(description) = arguments["description"].as_str().filter(|description| !description.is_empty()) {
                expense["description"] = json!(description);
            }

            let (status, body) = send(request(http, token, reqwest::Method::POST, &format!("{address}/v2/expenses")).json(&json!({"expense": expense}))).await.map_err(|reason| (reason, false))?;

            match status.is_success() {
                true => Ok(json!({"created": true, "url": body["location"].as_str().or_else(|| body["expense"]["url"].as_str())})),
                false => Err((complaint(status, &body), unauthorised(status))),
            }
        }
        _ => Err((format!("FreeAgent has no tool called {tool}."), false)),
    }
}

pub async fn call(state: &AppState, owner: Uuid, server: &ServerRow, credentials: &Credentials, tool: &str, arguments: &Value) -> CallOutput {
    let Some(mut token) = credentials.session.clone() else {
        return failure(SIGN_IN_FIRST);
    };

    if server.oauth_expires_at.is_some_and(|expires_at| expires_at - chrono::Utc::now() < REFRESH_MARGIN) {
        match renewed(state, owner, server, credentials).await {
            Some(fresh) => token = fresh,
            None => return failure(SIGN_IN_FIRST),
        }
    }

    let mut outcome = run(&state.http, &token, credentials, tool, arguments).await;

    if matches!(outcome, Err((_, true))) {
        match renewed(state, owner, server, credentials).await {
            Some(fresh) => outcome = run(&state.http, &fresh, credentials, tool, arguments).await,
            None => return failure(SIGN_IN_FIRST),
        }
    }

    match outcome {
        Ok(value) => CallOutput { text: value.to_string(), is_error: false },
        Err((reason, _)) => failure(reason),
    }
}

async fn find_owned(state: &AppState, id: Uuid, owner: Uuid) -> ApiResult<ServerRow> {
    let row = sqlx::query_as::<_, ServerRow>(sqlx::AssertSqlSafe(format!("select {SERVER_COLUMNS} from chat_tool_servers where id = $1 and owner = $2 and kind = 'freeagent'"))).bind(id).bind(owner).fetch_optional(&state.pool).await?;

    row.ok_or_else(|| ApiError::not_found("That FreeAgent integration"))
}

fn redirect_address(state: &AppState) -> String {
    format!("{}/integrations/freeagent/callback", state.config.public_url)
}

pub async fn start_sign_in(State(state): State<AppState>, user: AuthUser, Path(id): Path<Uuid>) -> ApiResult<Json<FreeAgentSignIn>> {
    let row = find_owned(&state, id, user.id).await?;
    let sign_in = new_token();
    let mut authorise = url::Url::parse(&format!("{}/v2/approve_app", base(&row.url))).map_err(ApiError::internal)?;

    authorise.query_pairs_mut().append_pair("client_id", row.account.as_deref().unwrap_or_default()).append_pair("response_type", "code").append_pair("redirect_uri", &redirect_address(&state)).append_pair("state", &sign_in);
    state.hub.track_freeagent(sign_in, PendingFreeAgent { owner: user.id, server: id, created: std::time::Instant::now() });

    Ok(Json(FreeAgentSignIn { authorize_url: authorise.to_string() }))
}

#[derive(serde::Deserialize)]
pub struct CallbackQuery {
    state: Option<String>,
    code: Option<String>,
}

pub async fn callback(State(state): State<AppState>, Query(query): Query<CallbackQuery>) -> Response {
    let Some(pending) = query.state.as_deref().and_then(|sign_in| state.hub.take_freeagent(sign_in)) else {
        return page(StatusCode::BAD_REQUEST, "This sign-in has expired. Start it again from Settings, Integrations.");
    };
    let Some(code) = query.code else {
        return page(StatusCode::BAD_REQUEST, "FreeAgent did not sign you in. Start it again from Settings, Integrations.");
    };
    let Ok(row) = find_owned(&state, pending.server, pending.owner).await else {
        return page(StatusCode::BAD_REQUEST, "This FreeAgent integration no longer exists.");
    };
    let credentials = row.credentials();
    let redirect = redirect_address(&state);
    let tokens = exchange(tokens_request(&state.http, &credentials, &[("grant_type", "authorization_code"), ("code", &code), ("redirect_uri", &redirect)])).await;
    let Some(tokens) = tokens else {
        return page(StatusCode::BAD_GATEWAY, "FreeAgent did not give Mycelium access. Check the client id and secret, then start the sign-in again from Settings, Integrations.");
    };
    let stored = sqlx::query("update chat_tool_servers set session_key = $2, oauth_refresh_token = $3, oauth_expires_at = $4 where id = $1").bind(row.id).bind(&tokens.access).bind(&tokens.refresh).bind(tokens.expires_at).execute(&state.pool).await;

    if stored.is_err() {
        return page(StatusCode::INTERNAL_SERVER_ERROR, "Mycelium could not save the sign-in. Try again.");
    }

    state.hub.tools_changed(pending.owner);
    state.publish(Some(pending.owner), StreamMessage::IntegrationsChanged);

    page(StatusCode::OK, &format!("{} is signed in to FreeAgent. You can close this tab and return to Mycelium.", row.name))
}

pub async fn sign_out(State(state): State<AppState>, user: AuthUser, Path(id): Path<Uuid>) -> ApiResult<StatusCode> {
    find_owned(&state, id, user.id).await?;
    sign_out_row(&state, user.id, id).await?;

    Ok(StatusCode::NO_CONTENT)
}
