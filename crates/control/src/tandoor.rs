use crate::auth::AuthUser;
use crate::caldav::host_of;
use crate::chat_tool_servers::StoredTool;
use crate::error::{ApiError, ApiResult};
use crate::integrations::{CallOutput, Credentials};
use crate::state::AppState;
use axum::Json;
use axum::extract::State;
use serde_json::{Value, json};
use std::time::Duration;

const CREATE_RECIPE: &str = "create_recipe";
const PLAN_MEAL: &str = "plan_meal";
const ADD_TO_SHOPPING_LIST: &str = "add_to_shopping_list";
const CHECK_OFF_ITEM: &str = "check_off_item";
const DEFAULT_LIMIT: u64 = 20;
const MAX_LIMIT: u64 = 100;
const TIMEOUT: Duration = Duration::from_secs(30);

enum Failure {
    Unreachable(String),
    Refused(String),
    Other(String),
}

impl Failure {
    fn message(self) -> String {
        match self {
            Failure::Unreachable(message) | Failure::Refused(message) | Failure::Other(message) => message,
        }
    }
}

#[derive(serde::Deserialize)]
pub struct TestRequest {
    url: String,
    #[serde(default)]
    token: String,
}

pub fn tools() -> Vec<StoredTool> {
    let limit = json!({"type": "integer", "description": format!("How many to return. Defaults to {DEFAULT_LIMIT}, at most {MAX_LIMIT}.")});
    let date = |description: &str| json!({"type": "string", "description": description});

    vec![
        StoredTool {
            name: "search_recipes".to_string(),
            description: "Search recipes by name. Leave the query out to list the newest recipes.".to_string(),
            input_schema: json!({"type": "object", "properties": {"query": {"type": "string", "description": "Words in the recipe's name."}, "limit": limit.clone()}}),
            read_only: None,
        },
        StoredTool {
            name: "get_recipe".to_string(),
            description: "Read a recipe with its ingredients and steps.".to_string(),
            input_schema: json!({"type": "object", "properties": {"recipe": {"type": "integer", "description": "The recipe's id, from search_recipes."}}, "required": ["recipe"]}),
            read_only: None,
        },
        StoredTool {
            name: "list_meal_plans".to_string(),
            description: "List planned meals between two dates, with their meal type and recipe.".to_string(),
            input_schema: json!({"type": "object", "properties": {"from_date": date("First day, as year-month-day."), "to_date": date("Last day, as year-month-day.")}, "required": ["from_date", "to_date"]}),
            read_only: None,
        },
        StoredTool {
            name: "get_shopping_list".to_string(),
            description: "List the shopping list entries, with whether each is checked off.".to_string(),
            input_schema: json!({"type": "object", "properties": {"include_checked": {"type": "boolean", "description": "Also return entries already checked off. Defaults to false."}}}),
            read_only: None,
        },
        StoredTool {
            name: CREATE_RECIPE.to_string(),
            description: "Add a recipe. Always needs the person's approval in chats, even when Always allow is on.".to_string(),
            input_schema: json!({"type": "object", "properties": {
                "name": {"type": "string"},
                "description": {"type": "string"},
                "servings": {"type": "integer"},
                "ingredients": {"type": "array", "items": {"type": "object", "properties": {"food": {"type": "string"}, "amount": {"type": "number"}, "unit": {"type": "string"}, "note": {"type": "string"}}, "required": ["food"]}},
                "instructions": {"type": "string", "description": "The method, as text."}
            }, "required": ["name"]}),
            read_only: None,
        },
        StoredTool {
            name: PLAN_MEAL.to_string(),
            description: "Plan a meal on a date. Needs a recipe id or a title. Always needs the person's approval in chats, even when Always allow is on.".to_string(),
            input_schema: json!({"type": "object", "properties": {
                "date": date("The day, as year-month-day."),
                "meal_type": {"type": "string", "description": "The name of a meal type from Tandoor, such as Dinner."},
                "recipe": {"type": "integer", "description": "The recipe's id."},
                "title": {"type": "string"},
                "servings": {"type": "number"}
            }, "required": ["date", "meal_type"]}),
            read_only: None,
        },
        StoredTool {
            name: ADD_TO_SHOPPING_LIST.to_string(),
            description: "Add an item to the shopping list. Always needs the person's approval in chats, even when Always allow is on.".to_string(),
            input_schema: json!({"type": "object", "properties": {"food": {"type": "string"}, "amount": {"type": "number"}, "unit": {"type": "string"}}, "required": ["food"]}),
            read_only: None,
        },
        StoredTool {
            name: CHECK_OFF_ITEM.to_string(),
            description: "Check a shopping list entry off, or put it back. Always needs the person's approval in chats, even when Always allow is on.".to_string(),
            input_schema: json!({"type": "object", "properties": {"entry": {"type": "integer", "description": "The entry's id, from get_shopping_list."}, "checked": {"type": "boolean", "description": "Defaults to true."}}, "required": ["entry"]}),
            read_only: None,
        },
    ]
}

pub fn always_asks(tool: &str) -> bool {
    matches!(tool, CREATE_RECIPE | PLAN_MEAL | ADD_TO_SHOPPING_LIST | CHECK_OFF_ITEM)
}

fn request(http: &reqwest::Client, credentials: &Credentials, method: reqwest::Method, path: &str) -> reqwest::RequestBuilder {
    let base = credentials.address.trim_end_matches('/');

    http.request(method, format!("{base}/api/{path}")).header("authorization", format!("Bearer {}", credentials.secret)).header("accept", "application/json").timeout(TIMEOUT)
}

async fn send(builder: reqwest::RequestBuilder, address: &str) -> Result<Value, Failure> {
    let host = host_of(address);
    let response = builder.send().await.map_err(|_| Failure::Unreachable(format!("Could not reach {host}. Check the address and that Tandoor is running.")))?;
    let status = response.status();
    let body: Value = response.json().await.unwrap_or(Value::Null);

    if status == reqwest::StatusCode::UNAUTHORIZED || status == reqwest::StatusCode::FORBIDDEN {
        return Err(Failure::Refused(format!("{host} did not accept the API token. Check the token has the scope read write and has not expired, or create a new one in Tandoor under Settings, then API.")));
    }

    if status == reqwest::StatusCode::NOT_FOUND {
        return Err(Failure::Other("Tandoor has no entry with that id.".to_string()));
    }

    if !status.is_success() {
        let reason = body["detail"].as_str().map(str::to_string).unwrap_or_else(|| format!("status {}", status.as_u16()));

        return Err(Failure::Other(format!("Tandoor answered with an error: {reason}")));
    }

    Ok(body)
}

fn items(body: &Value) -> Vec<Value> {
    match body {
        Value::Array(entries) => entries.clone(),
        other => other["results"].as_array().cloned().unwrap_or_default(),
    }
}

async fn survey(http: &reqwest::Client, credentials: &Credentials) -> Result<Value, Failure> {
    let space = send(request(http, credentials, reqwest::Method::GET, "space/"), &credentials.address).await?;
    let users = send(request(http, credentials, reqwest::Method::GET, "user/"), &credentials.address).await?;
    let recipes = send(request(http, credentials, reqwest::Method::GET, "recipe/?page_size=1"), &credentials.address).await?;
    let space_name = items(&space).first().and_then(|space| space["name"].as_str().map(str::to_string)).unwrap_or_default();
    let user_name = items(&users).first().and_then(|user| user["display_name"].as_str().or(user["username"].as_str()).map(str::to_string)).unwrap_or_default();

    Ok(json!({"host": host_of(&credentials.address), "space": space_name, "user": user_name, "recipes": recipes["count"].as_u64().unwrap_or(0)}))
}

pub async fn verify(http: &reqwest::Client, credentials: &Credentials) -> Result<(), String> {
    survey(http, credentials).await.map(|_| ()).map_err(Failure::message)
}

pub async fn test(State(state): State<AppState>, _user: AuthUser, Json(request): Json<TestRequest>) -> ApiResult<Json<Value>> {
    let parsed = url::Url::parse(request.url.trim()).ok().filter(|parsed| matches!(parsed.scheme(), "http" | "https") && parsed.host_str().is_some());
    let Some(parsed) = parsed else {
        return Err(ApiError::bad_request("tandoor_address", "Enter the server address, like https://recipes.example.com"));
    };
    let credentials = Credentials { address: parsed.as_str().to_string(), account: String::new(), secret: request.token.trim().to_string(), settings: Value::Null, session: None };

    match survey(&state.http, &credentials).await {
        Ok(summary) => Ok(Json(summary)),
        Err(Failure::Unreachable(message)) => Err(ApiError::bad_request("tandoor_address", message)),
        Err(Failure::Refused(message)) => Err(ApiError::bad_request("tandoor_token", message)),
        Err(Failure::Other(message)) => Err(ApiError::bad_request("tandoor_address", message)),
    }
}

fn limit(arguments: &Value) -> u64 {
    arguments["limit"].as_u64().unwrap_or(DEFAULT_LIMIT).clamp(1, MAX_LIMIT)
}

fn named(value: &str) -> Value {
    json!({"name": value})
}

fn recipe_summary(recipe: &Value) -> Value {
    json!({"id": recipe["id"], "name": recipe["name"], "description": recipe["description"], "servings": recipe["servings"], "keywords": recipe["keywords"].as_array().into_iter().flatten().map(|keyword| keyword["name"].clone()).collect::<Vec<_>>()})
}

fn recipe_detail(recipe: &Value) -> Value {
    let steps = recipe["steps"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|step| {
            let ingredients = step["ingredients"]
                .as_array()
                .into_iter()
                .flatten()
                .map(|ingredient| json!({"food": ingredient["food"]["name"], "amount": ingredient["amount"], "unit": ingredient["unit"]["name"], "note": ingredient["note"]}))
                .collect::<Vec<_>>();

            json!({"instruction": step["instruction"], "ingredients": ingredients})
        })
        .collect::<Vec<_>>();
    let mut detail = recipe_summary(recipe);

    detail["steps"] = Value::Array(steps);
    detail
}

fn meal_plan_summary(plan: &Value) -> Value {
    json!({"id": plan["id"], "title": plan["title"], "recipe": plan["recipe"]["name"], "recipe_id": plan["recipe"]["id"], "meal_type": plan["meal_type"]["name"], "from_date": plan["from_date"], "servings": plan["servings"]})
}

fn shopping_summary(entry: &Value) -> Value {
    json!({"id": entry["id"], "food": entry["food"]["name"], "amount": entry["amount"], "unit": entry["unit"]["name"], "checked": entry["checked"]})
}

pub async fn call(http: &reqwest::Client, credentials: &Credentials, tool: &str, arguments: &Value) -> CallOutput {
    let address = credentials.address.as_str();
    let outcome: Result<Value, Failure> = async {
        match tool {
            "search_recipes" => {
                let path = {
                    let mut query = url::form_urlencoded::Serializer::new(String::new());

                    query.append_pair("page_size", &limit(arguments).to_string());

                    if let Some(words) = arguments["query"].as_str().map(str::trim).filter(|words| !words.is_empty()) {
                        query.append_pair("query", words);
                    }

                    format!("recipe/?{}", query.finish())
                };
                let body = send(request(http, credentials, reqwest::Method::GET, &path), address).await?;

                Ok(json!({"total": body["count"], "recipes": items(&body).iter().map(recipe_summary).collect::<Vec<_>>()}))
            }
            "get_recipe" => {
                let Some(id) = arguments["recipe"].as_u64() else {
                    return Err(Failure::Other("Give the recipe's id in recipe.".to_string()));
                };
                let recipe = send(request(http, credentials, reqwest::Method::GET, &format!("recipe/{id}/")), address).await?;

                Ok(recipe_detail(&recipe))
            }
            "list_meal_plans" => {
                let (Some(from), Some(to)) = (arguments["from_date"].as_str(), arguments["to_date"].as_str()) else {
                    return Err(Failure::Other("Give from_date and to_date as year-month-day.".to_string()));
                };
                let path = {
                    let mut query = url::form_urlencoded::Serializer::new(String::new());

                    query.append_pair("from_date", from);
                    query.append_pair("to_date", to);

                    format!("meal-plan/?{}", query.finish())
                };
                let body = send(request(http, credentials, reqwest::Method::GET, &path), address).await?;

                Ok(Value::Array(items(&body).iter().map(meal_plan_summary).collect()))
            }
            "get_shopping_list" => {
                let body = send(request(http, credentials, reqwest::Method::GET, "shopping-list-entry/"), address).await?;
                let include_checked = arguments["include_checked"].as_bool().unwrap_or(false);

                Ok(Value::Array(items(&body).iter().filter(|entry| include_checked || entry["checked"] != json!(true)).map(shopping_summary).collect()))
            }
            CREATE_RECIPE => {
                let Some(name) = arguments["name"].as_str().map(str::trim).filter(|name| !name.is_empty()) else {
                    return Err(Failure::Other("Give the recipe a name.".to_string()));
                };
                let ingredients = arguments["ingredients"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(|ingredient| {
                        let food = ingredient["food"].as_str()?;

                        Some(json!({"food": named(food), "unit": ingredient["unit"].as_str().map(named), "amount": ingredient["amount"].as_f64().unwrap_or(0.0), "note": ingredient["note"].as_str().unwrap_or_default()}))
                    })
                    .collect::<Vec<_>>();
                let body = json!({
                    "name": name,
                    "description": arguments["description"].as_str().unwrap_or_default(),
                    "servings": arguments["servings"].as_u64().unwrap_or(1),
                    "keywords": [],
                    "steps": [{"instruction": arguments["instructions"].as_str().unwrap_or_default(), "ingredients": ingredients, "order": 0}],
                });
                let created = send(request(http, credentials, reqwest::Method::POST, "recipe/").json(&body), address).await?;

                Ok(recipe_summary(&created))
            }
            PLAN_MEAL => {
                let (Some(date), Some(meal_type)) = (arguments["date"].as_str(), arguments["meal_type"].as_str()) else {
                    return Err(Failure::Other("Give date and meal_type.".to_string()));
                };

                if arguments["recipe"].as_u64().is_none() && arguments["title"].as_str().is_none() {
                    return Err(Failure::Other("Give a recipe id or a title for the meal.".to_string()));
                }

                let types = send(request(http, credentials, reqwest::Method::GET, "meal-type/"), address).await?;
                let Some(found) = items(&types).into_iter().find(|candidate| candidate["name"].as_str().is_some_and(|name| name.eq_ignore_ascii_case(meal_type))) else {
                    let names = items(&types).iter().filter_map(|candidate| candidate["name"].as_str().map(str::to_string)).collect::<Vec<_>>().join(", ");

                    return Err(Failure::Other(format!("Tandoor has no meal type called {meal_type}. Use one of: {names}.")));
                };
                let recipe = arguments["recipe"].as_u64().map(|id| json!({"id": id, "name": ""}));
                let body = json!({
                    "title": arguments["title"].as_str().unwrap_or_default(),
                    "recipe": recipe,
                    "servings": arguments["servings"].as_f64().unwrap_or(1.0),
                    "from_date": date,
                    "to_date": date,
                    "meal_type": found,
                    "note": "",
                });
                let created = send(request(http, credentials, reqwest::Method::POST, "meal-plan/").json(&body), address).await?;

                Ok(meal_plan_summary(&created))
            }
            ADD_TO_SHOPPING_LIST => {
                let Some(food) = arguments["food"].as_str().map(str::trim).filter(|food| !food.is_empty()) else {
                    return Err(Failure::Other("Give the food to add in food.".to_string()));
                };
                let body = json!({"food": named(food), "unit": arguments["unit"].as_str().map(named), "amount": arguments["amount"].as_f64().unwrap_or(1.0)});
                let created = send(request(http, credentials, reqwest::Method::POST, "shopping-list-entry/").json(&body), address).await?;

                Ok(shopping_summary(&created))
            }
            CHECK_OFF_ITEM => {
                let Some(id) = arguments["entry"].as_u64() else {
                    return Err(Failure::Other("Give the entry's id in entry.".to_string()));
                };
                let body = json!({"checked": arguments["checked"].as_bool().unwrap_or(true)});
                let updated = send(request(http, credentials, reqwest::Method::PATCH, &format!("shopping-list-entry/{id}/")).json(&body), address).await?;

                Ok(shopping_summary(&updated))
            }
            _ => Err(Failure::Other(format!("Tandoor has no tool called {tool}."))),
        }
    }
    .await;

    match outcome {
        Ok(value) => CallOutput { text: value.to_string(), is_error: false },
        Err(failure) => CallOutput { text: failure.message(), is_error: true },
    }
}
