use crate::auth::AuthUser;
use crate::caldav::host_of;
use crate::chat_tool_servers::StoredTool;
use crate::error::{ApiError, ApiResult};
use crate::integrations::{CallOutput, Credentials};
use crate::state::AppState;
use axum::Json;
use axum::extract::State;
use base64::Engine;
use serde_json::{Value, json};
use std::time::Duration;

const CREATE_RECIPE: &str = "create_recipe";
const UPDATE_RECIPE: &str = "update_recipe";
const DELETE_RECIPE: &str = "delete_recipe";
const SET_RECIPE_IMAGE: &str = "set_recipe_image";
const UPDATE_FOOD: &str = "update_food";
const PLAN_MEAL: &str = "plan_meal";
const UPDATE_MEAL_PLAN: &str = "update_meal_plan";
const DELETE_MEAL_PLAN: &str = "delete_meal_plan";
const ADD_TO_SHOPPING_LIST: &str = "add_to_shopping_list";
const CHECK_OFF_ITEM: &str = "check_off_item";
const UNCHECK_ITEM: &str = "uncheck_item";
const REMOVE_SHOPPING_ITEM: &str = "remove_shopping_item";
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
    let entry = json!({"type": "object", "properties": {"entry": {"type": "integer", "description": "The entry's id, from get_shopping_list."}}, "required": ["entry"]});
    let ids = |description: &str| json!({"type": "array", "items": {"type": "integer"}, "description": description});
    let lookup = |what: &str| json!({"type": "object", "properties": {"query": {"type": "string", "description": format!("Words in the {what}'s name.")}, "limit": limit.clone()}, "required": ["query"]});
    let ingredient = json!({"type": "object", "properties": {
        "food": {"type": "string"},
        "amount": {"type": "number"},
        "unit": {"type": "string", "description": "The unit's name, such as g. Leave it out for none."},
        "note": {"type": "string"},
        "order": {"type": "integer"}
    }, "required": ["food"]});
    let step = json!({"type": "object", "properties": {
        "name": {"type": "string"},
        "show_as_header": {"type": "boolean"},
        "instruction": {"type": "string", "description": "The method for this step, as Markdown."},
        "time": {"type": "integer", "description": "Minutes this step takes."},
        "order": {"type": "integer", "description": "Defaults to the step's place in the list."},
        "ingredients": {"type": "array", "items": ingredient.clone()}
    }});
    let recipe_properties = json!({
        "name": {"type": "string"},
        "description": {"type": "string"},
        "servings": {"type": "integer"},
        "servings_text": {"type": "string", "description": "What a serving is, such as pieces."},
        "working_time": {"type": "integer", "description": "Minutes of work."},
        "waiting_time": {"type": "integer", "description": "Minutes of waiting."},
        "keywords": {"type": "array", "items": {"type": "string"}, "description": "Keyword names. Ones Tandoor does not have yet are created."},
        "internal": {"type": "boolean", "description": "Defaults to true."},
        "show_ingredient_overview": {"type": "boolean"},
        "source_url": {"type": "string"},
        "steps": {"type": "array", "items": step}
    });
    let mut create_properties = recipe_properties.clone();
    let mut update_properties = recipe_properties;

    create_properties["ingredients"] = json!({"type": "array", "items": ingredient, "description": "Used with instructions as a single step when steps is left out."});
    create_properties["instructions"] = json!({"type": "string", "description": "The method, as text. Used when steps is left out."});
    update_properties["recipe"] = json!({"type": "integer", "description": "The recipe's id."});

    vec![
        StoredTool {
            name: "search_recipes".to_string(),
            description: "Search recipes by name, keyword, food and rating. Leave the query out to list the newest recipes.".to_string(),
            input_schema: json!({"type": "object", "properties": {
                "query": {"type": "string", "description": "Words in the recipe's name."},
                "keywords": ids("Keyword ids, from find_keyword. Recipes have all of them."),
                "foods": ids("Food ids, from find_food. Recipes have all of them."),
                "rating_gte": {"type": "integer", "description": "Lowest rating, 0 to 5."},
                "makenow": {"type": "boolean", "description": "Only recipes that can be made with food on hand."},
                "random": {"type": "boolean", "description": "Return the results in a random order."},
                "new": {"type": "boolean", "description": "Return new recipes first."},
                "sort_order": {"type": "string", "enum": ["score", "-score", "name", "-name", "lastcooked", "-lastcooked", "rating", "-rating", "times_cooked", "-times_cooked", "created_at", "-created_at", "lastviewed", "-lastviewed"]},
                "page": {"type": "integer", "description": "Which page of limit results, from 1."},
                "limit": limit.clone()
            }}),
            read_only: None,
        },
        StoredTool {
            name: "get_recipe".to_string(),
            description: "Read a recipe with its ingredients and steps.".to_string(),
            input_schema: json!({"type": "object", "properties": {"recipe": {"type": "integer", "description": "The recipe's id, from search_recipes."}}, "required": ["recipe"]}),
            read_only: None,
        },
        StoredTool {
            name: "import_recipe_from_url".to_string(),
            description: "Read a recipe from a web page into Tandoor's recipe format, without saving it. Pass the result to create_recipe to save it.".to_string(),
            input_schema: json!({"type": "object", "properties": {"url": {"type": "string", "description": "The recipe page's address."}}, "required": ["url"]}),
            read_only: None,
        },
        StoredTool {
            name: "find_food".to_string(),
            description: "Find foods by name, with their ids and nutrition properties.".to_string(),
            input_schema: lookup("food"),
            read_only: None,
        },
        StoredTool {
            name: "find_unit".to_string(),
            description: "Find units by name, with their ids.".to_string(),
            input_schema: lookup("unit"),
            read_only: None,
        },
        StoredTool {
            name: "find_keyword".to_string(),
            description: "Find keywords by name, with their ids.".to_string(),
            input_schema: lookup("keyword"),
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
            description: "Add a recipe, with its steps and their ingredients. Always needs the person's approval in chats, even when Always allow is on.".to_string(),
            input_schema: json!({"type": "object", "properties": create_properties, "required": ["name"]}),
            read_only: None,
        },
        StoredTool {
            name: UPDATE_RECIPE.to_string(),
            description: "Replace a whole recipe, including its steps. Anything left out is cleared, so read it with get_recipe first and send everything to keep. Always needs the person's approval in chats, even when Always allow is on.".to_string(),
            input_schema: json!({"type": "object", "properties": update_properties, "required": ["recipe", "name", "steps"]}),
            read_only: None,
        },
        StoredTool {
            name: SET_RECIPE_IMAGE.to_string(),
            description: "Set a recipe's image from a JPEG, PNG, WebP or GIF file, or from an image address Tandoor downloads. Always needs the person's approval in chats, even when Always allow is on.".to_string(),
            input_schema: json!({"type": "object", "properties": {
                "recipe": {"type": "integer", "description": "The recipe's id."},
                "image": {"type": "string", "description": "The image as base64. Give {\"file\": \"<path>\", \"encoding\": \"base64\"} to send a file from the conversation folder."},
                "image_url": {"type": "string", "description": "An image address, instead of image."}
            }, "required": ["recipe"]}),
            read_only: None,
        },
        StoredTool {
            name: DELETE_RECIPE.to_string(),
            description: "Delete a recipe. Always needs the person's approval in chats, even when Always allow is on.".to_string(),
            input_schema: json!({"type": "object", "properties": {"recipe": {"type": "integer", "description": "The recipe's id."}}, "required": ["recipe"]}),
            read_only: None,
        },
        StoredTool {
            name: UPDATE_FOOD.to_string(),
            description: "Set a food's nutrition properties. Replaces all of them, so include the ones to keep. Always needs the person's approval in chats, even when Always allow is on.".to_string(),
            input_schema: json!({"type": "object", "properties": {
                "food": {"type": "integer", "description": "The food's id, from find_food."},
                "properties": {"type": "array", "items": {"type": "object", "properties": {"name": {"type": "string", "description": "The name of a property type from Tandoor, such as Calories."}, "amount": {"type": "number"}}, "required": ["name", "amount"]}},
                "per_amount": {"type": "number", "description": "How much of the food the amounts are for. Defaults to 100."},
                "per_unit": {"type": "string", "description": "The unit of per_amount, such as g."}
            }, "required": ["food", "properties"]}),
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
            name: UPDATE_MEAL_PLAN.to_string(),
            description: "Change a planned meal and leave what is not given alone. Always needs the person's approval in chats, even when Always allow is on.".to_string(),
            input_schema: json!({"type": "object", "properties": {
                "plan": {"type": "integer", "description": "The planned meal's id, from list_meal_plans."},
                "date": date("The day, as year-month-day."),
                "meal_type": {"type": "string", "description": "The name of a meal type from Tandoor, such as Dinner."},
                "recipe": {"type": "integer", "description": "The recipe's id."},
                "title": {"type": "string"},
                "servings": {"type": "number"},
                "note": {"type": "string"}
            }, "required": ["plan"]}),
            read_only: None,
        },
        StoredTool {
            name: DELETE_MEAL_PLAN.to_string(),
            description: "Delete a planned meal. Always needs the person's approval in chats, even when Always allow is on.".to_string(),
            input_schema: json!({"type": "object", "properties": {"plan": {"type": "integer", "description": "The planned meal's id, from list_meal_plans."}}, "required": ["plan"]}),
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
            description: "Check a shopping list entry off. Always needs the person's approval in chats, even when Always allow is on.".to_string(),
            input_schema: entry.clone(),
            read_only: None,
        },
        StoredTool {
            name: UNCHECK_ITEM.to_string(),
            description: "Put a checked off shopping list entry back on the list. Always needs the person's approval in chats, even when Always allow is on.".to_string(),
            input_schema: entry.clone(),
            read_only: None,
        },
        StoredTool {
            name: REMOVE_SHOPPING_ITEM.to_string(),
            description: "Remove an entry from the shopping list. Always needs the person's approval in chats, even when Always allow is on.".to_string(),
            input_schema: entry,
            read_only: None,
        },
    ]
}

pub fn always_asks(tool: &str) -> bool {
    matches!(tool, CREATE_RECIPE | UPDATE_RECIPE | SET_RECIPE_IMAGE | DELETE_RECIPE | UPDATE_FOOD | PLAN_MEAL | UPDATE_MEAL_PLAN | DELETE_MEAL_PLAN | ADD_TO_SHOPPING_LIST | CHECK_OFF_ITEM | UNCHECK_ITEM | REMOVE_SHOPPING_ITEM)
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
        let reason = match (&body["detail"], &body) {
            (Value::String(detail), _) => detail.clone(),
            (_, Value::Object(fields)) if !fields.is_empty() => body.to_string(),
            _ => format!("status {}", status.as_u16()),
        };

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

fn step_body(step: &Value, position: usize) -> Value {
    let ingredients = step["ingredients"]
        .as_array()
        .into_iter()
        .flatten()
        .enumerate()
        .filter_map(|(position, ingredient)| {
            let food = ingredient["food"].as_str()?;
            let unit = ingredient["unit"].as_str().map(str::trim).filter(|unit| !unit.is_empty());

            Some(json!({"food": named(food), "unit": unit.map(named), "amount": ingredient["amount"].as_f64().unwrap_or(0.0), "note": ingredient["note"].as_str().unwrap_or_default(), "order": ingredient["order"].as_u64().unwrap_or(position as u64)}))
        })
        .collect::<Vec<_>>();

    json!({
        "name": step["name"].as_str().unwrap_or_default(),
        "show_as_header": step["show_as_header"].as_bool().unwrap_or(false),
        "instruction": step["instruction"].as_str().unwrap_or_default(),
        "time": step["time"].as_u64().unwrap_or(0),
        "order": step["order"].as_u64().unwrap_or(position as u64),
        "ingredients": ingredients,
    })
}

fn recipe_body(name: &str, arguments: &Value) -> Value {
    let steps = match arguments["steps"].as_array() {
        Some(steps) => steps.iter().enumerate().map(|(position, step)| step_body(step, position)).collect::<Vec<_>>(),
        None => vec![step_body(&json!({"instruction": arguments["instructions"], "ingredients": arguments["ingredients"]}), 0)],
    };
    let mut body = json!({"name": name, "steps": steps});

    if let Some(keywords) = arguments["keywords"].as_array() {
        body["keywords"] = keywords.iter().filter_map(Value::as_str).map(named).collect();
    }

    for field in ["description", "servings", "servings_text", "working_time", "waiting_time", "internal", "show_ingredient_overview", "source_url"] {
        if !arguments[field].is_null() {
            body[field] = arguments[field].clone();
        }
    }

    body
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
                .map(|ingredient| json!({"food": ingredient["food"]["name"], "amount": ingredient["amount"], "unit": ingredient["unit"]["name"], "note": ingredient["note"], "order": ingredient["order"]}))
                .collect::<Vec<_>>();

            json!({"name": step["name"], "show_as_header": step["show_as_header"], "instruction": step["instruction"], "time": step["time"], "order": step["order"], "ingredients": ingredients})
        })
        .collect::<Vec<_>>();
    let mut detail = recipe_summary(recipe);

    for field in ["servings_text", "working_time", "waiting_time", "internal", "show_ingredient_overview", "source_url", "image"] {
        detail[field] = recipe[field].clone();
    }

    detail["steps"] = Value::Array(steps);
    detail
}

fn food_summary(food: &Value) -> Value {
    let properties = food["properties"].as_array().into_iter().flatten().map(|property| json!({"name": property["property_type"]["name"], "amount": property["property_amount"]})).collect::<Vec<_>>();

    json!({"id": food["id"], "name": food["name"], "properties": properties, "per_amount": food["properties_food_amount"], "per_unit": food["properties_food_unit"]["name"]})
}

fn image_type(bytes: &[u8]) -> Option<(&'static str, &'static str)> {
    match bytes {
        [0xFF, 0xD8, 0xFF, ..] => Some(("jpg", "image/jpeg")),
        [0x89, b'P', b'N', b'G', ..] => Some(("png", "image/png")),
        [b'G', b'I', b'F', b'8', ..] => Some(("gif", "image/gif")),
        [b'R', b'I', b'F', b'F', _, _, _, _, b'W', b'E', b'B', b'P', ..] => Some(("webp", "image/webp")),
        _ => None,
    }
}

async fn find_meal_type(http: &reqwest::Client, credentials: &Credentials, name: &str) -> Result<Value, Failure> {
    let types = send(request(http, credentials, reqwest::Method::GET, "meal-type/"), &credentials.address).await?;
    let Some(found) = items(&types).into_iter().find(|candidate| candidate["name"].as_str().is_some_and(|candidate| candidate.eq_ignore_ascii_case(name))) else {
        let names = items(&types).iter().filter_map(|candidate| candidate["name"].as_str().map(str::to_string)).collect::<Vec<_>>().join(", ");

        return Err(Failure::Other(format!("Tandoor has no meal type called {name}. Use one of: {names}.")));
    };

    Ok(found)
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

                    for field in ["keywords", "foods"] {
                        for id in arguments[field].as_array().into_iter().flatten().filter_map(Value::as_u64) {
                            query.append_pair(field, &id.to_string());
                        }
                    }

                    for field in ["rating_gte", "page"] {
                        if let Some(number) = arguments[field].as_u64() {
                            query.append_pair(field, &number.to_string());
                        }
                    }

                    for field in ["makenow", "random", "new"] {
                        if let Some(flag) = arguments[field].as_bool() {
                            query.append_pair(field, &flag.to_string());
                        }
                    }

                    if let Some(order) = arguments["sort_order"].as_str() {
                        query.append_pair("sort_order", order);
                    }

                    format!("recipe/?{}", query.finish())
                };
                let body = send(request(http, credentials, reqwest::Method::GET, &path), address).await?;

                Ok(json!({"total": body["count"], "recipes": items(&body).iter().map(recipe_summary).collect::<Vec<_>>()}))
            }
            "import_recipe_from_url" => {
                let Some(url) = arguments["url"].as_str().map(str::trim).filter(|url| !url.is_empty()) else {
                    return Err(Failure::Other("Give the recipe page's address in url.".to_string()));
                };
                let body = send(request(http, credentials, reqwest::Method::POST, "recipe-from-source/").json(&json!({"url": url})), address).await?;

                if body["error"] == json!(true) {
                    return Err(Failure::Other(format!("Tandoor could not read a recipe from that page: {}", body["msg"].as_str().unwrap_or_default())));
                }

                Ok(json!({"recipe": body["recipe"], "images": body["images"], "duplicates": body["duplicates"]}))
            }
            "find_food" | "find_unit" | "find_keyword" => {
                let Some(words) = arguments["query"].as_str().map(str::trim).filter(|words| !words.is_empty()) else {
                    return Err(Failure::Other("Give words in the name in query.".to_string()));
                };
                let resource = tool.trim_start_matches("find_");
                let path = {
                    let mut query = url::form_urlencoded::Serializer::new(String::new());

                    query.append_pair("query", words);
                    query.append_pair("page_size", &limit(arguments).to_string());

                    format!("{resource}/?{}", query.finish())
                };
                let body = send(request(http, credentials, reqwest::Method::GET, &path), address).await?;
                let found = items(&body)
                    .iter()
                    .map(|entry| match resource {
                        "food" => food_summary(entry),
                        _ => json!({"id": entry["id"], "name": entry["name"], "description": entry["description"]}),
                    })
                    .collect();

                Ok(Value::Array(found))
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
                let mut body = recipe_body(name, arguments);

                for (field, default) in [("description", json!("")), ("servings", json!(1)), ("keywords", json!([])), ("internal", json!(true))] {
                    if body[field].is_null() {
                        body[field] = default;
                    }
                }

                let created = send(request(http, credentials, reqwest::Method::POST, "recipe/").json(&body), address).await?;

                Ok(recipe_summary(&created))
            }
            UPDATE_RECIPE => {
                let Some(id) = arguments["recipe"].as_u64() else {
                    return Err(Failure::Other("Give the recipe's id in recipe.".to_string()));
                };
                let Some(name) = arguments["name"].as_str().map(str::trim).filter(|name| !name.is_empty()) else {
                    return Err(Failure::Other("Give the recipe a name.".to_string()));
                };

                if !arguments["steps"].is_array() {
                    return Err(Failure::Other("Give every step of the recipe in steps. Read them with get_recipe first.".to_string()));
                }

                let updated = send(request(http, credentials, reqwest::Method::PUT, &format!("recipe/{id}/")).json(&recipe_body(name, arguments)), address).await?;

                Ok(recipe_detail(&updated))
            }
            SET_RECIPE_IMAGE => {
                let Some(id) = arguments["recipe"].as_u64() else {
                    return Err(Failure::Other("Give the recipe's id in recipe.".to_string()));
                };
                let form = match (arguments["image"].as_str(), arguments["image_url"].as_str()) {
                    (Some(encoded), _) => {
                        let Ok(bytes) = base64::engine::general_purpose::STANDARD.decode(encoded.trim()) else {
                            return Err(Failure::Other("Give image as base64, or as {\"file\": \"<path>\", \"encoding\": \"base64\"}.".to_string()));
                        };
                        let Some((extension, mime)) = image_type(&bytes) else {
                            return Err(Failure::Other("The image must be a JPEG, PNG, WebP or GIF file.".to_string()));
                        };
                        let part = reqwest::multipart::Part::bytes(bytes).file_name(format!("image.{extension}")).mime_str(mime).map_err(|error| Failure::Other(error.to_string()))?;

                        reqwest::multipart::Form::new().part("image", part)
                    }
                    (None, Some(url)) => reqwest::multipart::Form::new().text("image_url", url.trim().to_string()),
                    (None, None) => return Err(Failure::Other("Give the image in image, or its address in image_url.".to_string())),
                };
                let updated = send(request(http, credentials, reqwest::Method::PUT, &format!("recipe/{id}/image/")).multipart(form), address).await?;

                Ok(json!({"recipe": id, "image": updated["image"]}))
            }
            DELETE_RECIPE => {
                let Some(id) = arguments["recipe"].as_u64() else {
                    return Err(Failure::Other("Give the recipe's id in recipe.".to_string()));
                };
                send(request(http, credentials, reqwest::Method::DELETE, &format!("recipe/{id}/")), address).await?;

                Ok(json!({"recipe": id, "deleted": true}))
            }
            UPDATE_FOOD => {
                let (Some(id), Some(given)) = (arguments["food"].as_u64(), arguments["properties"].as_array()) else {
                    return Err(Failure::Other("Give the food's id in food and its nutrition in properties.".to_string()));
                };
                let found_types = send(request(http, credentials, reqwest::Method::GET, "property-type/?page_size=100"), address).await?;
                let types = items(&found_types);
                let mut properties = Vec::new();

                for property in given {
                    let (Some(name), Some(amount)) = (property["name"].as_str(), property["amount"].as_f64()) else {
                        return Err(Failure::Other("Give each property a name and an amount.".to_string()));
                    };
                    let Some(found) = types.iter().find(|candidate| candidate["name"].as_str().is_some_and(|candidate| candidate.eq_ignore_ascii_case(name))) else {
                        let names = types.iter().filter_map(|candidate| candidate["name"].as_str()).collect::<Vec<_>>().join(", ");

                        return Err(Failure::Other(format!("Tandoor has no property type called {name}. Use one of: {names}.")));
                    };

                    properties.push(json!({"property_amount": amount, "property_type": {"id": found["id"], "name": found["name"]}}));
                }

                let mut body = json!({"properties": properties});

                if let Some(amount) = arguments["per_amount"].as_f64() {
                    body["properties_food_amount"] = json!(amount);
                }

                if let Some(unit) = arguments["per_unit"].as_str() {
                    body["properties_food_unit"] = named(unit);
                }

                let updated = send(request(http, credentials, reqwest::Method::PATCH, &format!("food/{id}/")).json(&body), address).await?;

                Ok(food_summary(&updated))
            }
            PLAN_MEAL => {
                let (Some(date), Some(meal_type)) = (arguments["date"].as_str(), arguments["meal_type"].as_str()) else {
                    return Err(Failure::Other("Give date and meal_type.".to_string()));
                };

                if arguments["recipe"].as_u64().is_none() && arguments["title"].as_str().is_none() {
                    return Err(Failure::Other("Give a recipe id or a title for the meal.".to_string()));
                }

                let found = find_meal_type(http, credentials, meal_type).await?;
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
            UPDATE_MEAL_PLAN => {
                let Some(id) = arguments["plan"].as_u64() else {
                    return Err(Failure::Other("Give the planned meal's id in plan.".to_string()));
                };
                let mut body = json!({});

                if let Some(date) = arguments["date"].as_str() {
                    body["from_date"] = json!(date);
                    body["to_date"] = json!(date);
                }

                if let Some(name) = arguments["meal_type"].as_str() {
                    body["meal_type"] = find_meal_type(http, credentials, name).await?;
                }

                if let Some(recipe) = arguments["recipe"].as_u64() {
                    body["recipe"] = json!({"id": recipe, "name": ""});
                }

                for field in ["title", "servings", "note"] {
                    if !arguments[field].is_null() {
                        body[field] = arguments[field].clone();
                    }
                }

                if body.as_object().is_some_and(serde_json::Map::is_empty) {
                    return Err(Failure::Other("Give at least one of date, meal_type, recipe, title, servings or note to change.".to_string()));
                }

                let updated = send(request(http, credentials, reqwest::Method::PATCH, &format!("meal-plan/{id}/")).json(&body), address).await?;

                Ok(meal_plan_summary(&updated))
            }
            DELETE_MEAL_PLAN => {
                let Some(id) = arguments["plan"].as_u64() else {
                    return Err(Failure::Other("Give the planned meal's id in plan.".to_string()));
                };
                send(request(http, credentials, reqwest::Method::DELETE, &format!("meal-plan/{id}/")), address).await?;

                Ok(json!({"plan": id, "deleted": true}))
            }
            ADD_TO_SHOPPING_LIST => {
                let Some(food) = arguments["food"].as_str().map(str::trim).filter(|food| !food.is_empty()) else {
                    return Err(Failure::Other("Give the food to add in food.".to_string()));
                };
                let body = json!({"food": named(food), "unit": arguments["unit"].as_str().map(named), "amount": arguments["amount"].as_f64().unwrap_or(1.0)});
                let created = send(request(http, credentials, reqwest::Method::POST, "shopping-list-entry/").json(&body), address).await?;

                Ok(shopping_summary(&created))
            }
            CHECK_OFF_ITEM | UNCHECK_ITEM => {
                let Some(id) = arguments["entry"].as_u64() else {
                    return Err(Failure::Other("Give the entry's id in entry.".to_string()));
                };
                let body = json!({"checked": tool == CHECK_OFF_ITEM});
                let updated = send(request(http, credentials, reqwest::Method::PATCH, &format!("shopping-list-entry/{id}/")).json(&body), address).await?;

                Ok(shopping_summary(&updated))
            }
            REMOVE_SHOPPING_ITEM => {
                let Some(id) = arguments["entry"].as_u64() else {
                    return Err(Failure::Other("Give the entry's id in entry.".to_string()));
                };
                send(request(http, credentials, reqwest::Method::DELETE, &format!("shopping-list-entry/{id}/")), address).await?;

                Ok(json!({"entry": id, "deleted": true}))
            }
            _ => Err(Failure::Other(format!("Tandoor has no tool called {tool}."))),
        }
    }
    .await;

    match outcome {
        Ok(value) => CallOutput { text: value.to_string(), is_error: false, binary: None },
        Err(failure) => CallOutput { text: failure.message(), is_error: true, binary: None },
    }
}
