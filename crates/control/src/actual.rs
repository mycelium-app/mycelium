use crate::chat_tool_servers::StoredTool;
use crate::integrations::{CallOutput, Credentials};
use serde_json::{Value, json};
use std::time::Duration;

const ADD_TRANSACTION: &str = "add_transaction";
const TIMEOUT: Duration = Duration::from_secs(60);
const MINOR_UNITS: f64 = 100.0;
const ACCOUNT_ID_LENGTH: usize = 64;

pub fn normalise_settings(settings: &Value) -> Result<Value, String> {
    let Some(sync_id) = settings["budget_sync_id"].as_str().map(str::trim).filter(|id| !id.is_empty()) else {
        return Err("Enter your budget's sync id, from Settings, Show advanced settings in Actual Budget.".to_string());
    };
    let mut normalised = json!({"budget_sync_id": sync_id});

    if let Some(password) = settings["encryption_password"].as_str().map(str::trim).filter(|password| !password.is_empty()) {
        normalised["encryption_password"] = json!(password);
    }

    Ok(normalised)
}

pub fn tools() -> Vec<StoredTool> {
    let account = json!({"type": "string", "description": "The account's id, from list_accounts."});
    let tool = |name: &str, description: &str, input_schema: Value| StoredTool { name: name.to_string(), description: description.to_string(), input_schema, read_only: None };

    vec![
        tool("list_accounts", "List the budget's accounts with their ids, balances and whether they are closed or off budget. Amounts are in the budget's currency.", json!({"type": "object", "properties": {}})),
        tool("list_categories", "List the budget's categories with their ids.", json!({"type": "object", "properties": {}})),
        tool(
            "get_transactions",
            "List transactions between two dates, newest first. Give an account, or leave it out for every open account. Amounts are negative for money out.",
            json!({"type": "object", "properties": {"account": account.clone(), "since": {"type": "string", "description": "The first day, as YYYY-MM-DD."}, "until": {"type": "string", "description": "The last day, as YYYY-MM-DD. Defaults to today."}}, "required": ["since"]}),
        ),
        tool(
            "get_budget_month",
            "Get what each category had budgeted, spent and left in a month.",
            json!({"type": "object", "properties": {"month": {"type": "string", "description": "The month, as YYYY-MM."}}, "required": ["month"]}),
        ),
        tool(
            ADD_TRANSACTION,
            "Add a transaction to an account. Always needs the person's approval in chats, even when Always allow is on.",
            json!({"type": "object", "properties": {
                "account": account,
                "date": {"type": "string", "description": "The day, as YYYY-MM-DD."},
                "amount": {"type": "number", "description": "In the budget's currency, negative for money out, for example -12.50."},
                "payee": {"type": "string", "description": "Who was paid or paid you."},
                "category": {"type": "string", "description": "A category id, from list_categories."},
                "notes": {"type": "string"}
            }, "required": ["account", "date", "amount"]}),
        ),
    ]
}

pub fn always_asks(tool: &str) -> bool {
    tool == ADD_TRANSACTION
}

fn failure(text: impl Into<String>) -> CallOutput {
    CallOutput { text: text.into(), is_error: true }
}

fn request(http: &reqwest::Client, credentials: &Credentials, method: reqwest::Method, path: &str) -> reqwest::RequestBuilder {
    let base = credentials.address.trim_end_matches('/');
    let budget = credentials.settings["budget_sync_id"].as_str().unwrap_or_default();
    let builder = http.request(method, format!("{base}/v1/budgets/{budget}/{path}")).header("x-api-key", &credentials.secret).timeout(TIMEOUT);

    match credentials.settings["encryption_password"].as_str() {
        Some(password) => builder.header("budget-encryption-password", password),
        None => builder,
    }
}

async fn send(builder: reqwest::RequestBuilder) -> Result<Value, String> {
    let response = builder.send().await.map_err(|_| "Mycelium could not reach the Actual HTTP API. Check its address and that it is running, then try again.".to_string())?;
    let status = response.status();
    let body: Value = response.json().await.unwrap_or(Value::Null);

    if status == reqwest::StatusCode::UNAUTHORIZED || status == reqwest::StatusCode::FORBIDDEN {
        return Err("The Actual HTTP API did not accept this API key. Check the API_KEY it was started with, then try again.".to_string());
    }

    if !status.is_success() {
        let reason = body["error"].as_str().or_else(|| body["message"].as_str()).map(str::to_string).unwrap_or_else(|| format!("status {}", status.as_u16()));

        return Err(format!("The Actual HTTP API answered with an error: {reason}. Check the budget sync id and the encryption password, if the budget has one."));
    }

    Ok(body["data"].clone())
}

pub async fn verify(http: &reqwest::Client, credentials: &Credentials) -> Result<(), String> {
    send(request(http, credentials, reqwest::Method::GET, "accounts")).await.map(|_| ())
}

fn text<'a>(arguments: &'a Value, key: &str) -> Option<&'a str> {
    arguments[key].as_str().map(str::trim).filter(|value| !value.is_empty())
}

fn currency(minor: &Value) -> Value {
    match minor.as_f64() {
        Some(minor) => json!(minor / MINOR_UNITS),
        None => Value::Null,
    }
}

fn account_of(account: &Value) -> Value {
    json!({"id": account["id"], "name": account["name"], "balance": currency(&account["balance"]), "closed": account["closed"], "off_budget": account["offbudget"]})
}

fn transaction_of(transaction: &Value) -> Value {
    json!({"id": transaction["id"], "account": transaction["account"], "date": transaction["date"], "amount": currency(&transaction["amount"]), "payee": transaction["payee_name"].as_str().or_else(|| transaction["payee"].as_str()), "category": transaction["category"], "notes": transaction["notes"], "cleared": transaction["cleared"]})
}

fn account_id<'a>(arguments: &'a Value) -> Option<&'a str> {
    text(arguments, "account").filter(|account| account.len() <= ACCOUNT_ID_LENGTH && account.bytes().all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_'))
}

fn month_of<'a>(arguments: &'a Value) -> Option<&'a str> {
    let month = text(arguments, "month")?;
    let (year, number) = month.split_once('-')?;

    (year.len() == 4 && number.len() == 2 && year.bytes().chain(number.bytes()).all(|byte| byte.is_ascii_digit())).then_some(month)
}

fn collect(list: Result<Value, String>, convert: fn(&Value) -> Value) -> Result<Value, String> {
    list.map(|data| Value::Array(data.as_array().into_iter().flatten().map(convert).collect()))
}

pub async fn call(http: &reqwest::Client, credentials: &Credentials, tool: &str, arguments: &Value) -> CallOutput {
    let outcome = match tool {
        "list_accounts" => collect(send(request(http, credentials, reqwest::Method::GET, "accounts")).await, account_of),
        "list_categories" => collect(send(request(http, credentials, reqwest::Method::GET, "categories")).await, |category| json!({"id": category["id"], "name": category["name"], "group": category["group_id"], "income": category["is_income"], "hidden": category["hidden"]})),
        "get_transactions" => {
            let Some(since) = text(arguments, "since") else {
                return failure("Give the first day in since, as YYYY-MM-DD.");
            };
            let until = text(arguments, "until").map(str::to_string).unwrap_or_else(|| chrono::Utc::now().format("%Y-%m-%d").to_string());
            if text(arguments, "account").is_some() && account_id(arguments).is_none() {
                return failure("Give the account's id in account, as listed by list_accounts.");
            }

            let accounts = match account_id(arguments) {
                Some(account) => Ok(vec![account.to_string()]),
                None => send(request(http, credentials, reqwest::Method::GET, "accounts")).await.map(|data| data.as_array().into_iter().flatten().filter(|account| account["closed"] != true).filter_map(|account| account["id"].as_str().map(str::to_string)).collect()),
            };
            let mut found: Vec<Value> = Vec::new();
            let mut failed = None;

            for account in accounts.unwrap_or_else(|reason| {
                failed = Some(reason);
                Vec::new()
            }) {
                let path = format!("accounts/{account}/transactions?{}", url::form_urlencoded::Serializer::new(String::new()).append_pair("since_date", since).append_pair("until_date", &until).finish());

                match send(request(http, credentials, reqwest::Method::GET, &path)).await {
                    Ok(data) => found.extend(data.as_array().into_iter().flatten().map(transaction_of)),
                    Err(reason) => {
                        failed = Some(reason);
                        break;
                    }
                }
            }

            found.sort_by(|left, right| right["date"].as_str().cmp(&left["date"].as_str()));

            match failed {
                Some(reason) => Err(reason),
                None => Ok(Value::Array(found)),
            }
        }
        "get_budget_month" => {
            let Some(month) = month_of(arguments) else {
                return failure("Give the month in month, as YYYY-MM.");
            };

            collect(send(request(http, credentials, reqwest::Method::GET, &format!("months/{month}/categories"))).await, |category| {
                json!({"id": category["id"], "name": category["name"], "budgeted": currency(&category["budgeted"]), "spent": currency(&category["spent"]), "balance": currency(&category["balance"])})
            })
        }
        ADD_TRANSACTION => {
            let (Some(account), Some(date), Some(amount)) = (account_id(arguments), text(arguments, "date"), arguments["amount"].as_f64()) else {
                return failure("Give the account's id from list_accounts, the date and the amount.");
            };
            let mut transaction = json!({"date": date, "amount": (amount * MINOR_UNITS).round() as i64});

            for (argument, field) in [("payee", "payee_name"), ("category", "category"), ("notes", "notes")] {
                if let Some(value) = text(arguments, argument) {
                    transaction[field] = json!(value);
                }
            }

            send(request(http, credentials, reqwest::Method::POST, &format!("accounts/{account}/transactions")).json(&json!({"transaction": transaction}))).await.map(|_| json!({"done": true}))
        }
        _ => Err(format!("The Actual HTTP API has no tool called {tool}.")),
    };

    match outcome {
        Ok(value) => CallOutput { text: value.to_string(), is_error: false },
        Err(reason) => failure(reason),
    }
}
