use axum::Json;
use axum::Router;
use axum::body::Bytes;
use axum::extract::{Path, State};
use axum::http::{HeaderMap, Method, StatusCode, Uri};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, patch};
use base64::Engine;
use crate::common::{NodeLink, Server, serve};
use serde_json::{Value, json};
use std::sync::{Arc, Mutex};

const TOKEN: &str = "tandoor-token-1234";

#[derive(Default)]
struct FakeTandoor {
    posted: Mutex<Vec<(String, Value)>>,
    sent: Mutex<Vec<Sent>>,
}

#[derive(Clone, Debug)]
struct Sent {
    method: Method,
    path: String,
    content_type: String,
    body: Vec<u8>,
}

impl Sent {
    fn json(&self) -> Value {
        serde_json::from_slice(&self.body).unwrap_or(Value::Null)
    }
}

fn authorised(headers: &HeaderMap) -> bool {
    headers.get("authorization").and_then(|value| value.to_str().ok()) == Some(&format!("Bearer {TOKEN}"))
}

fn refuse() -> Response {
    (StatusCode::UNAUTHORIZED, Json(json!({"detail": "Authentication credentials were not provided."}))).into_response()
}

async fn space(headers: HeaderMap) -> Response {
    if !authorised(&headers) {
        return refuse();
    }

    Json(json!([{"id": 1, "name": "Home kitchen"}])).into_response()
}

async fn users(headers: HeaderMap) -> Response {
    if !authorised(&headers) {
        return refuse();
    }

    Json(json!([{"id": 1, "username": "sam", "display_name": "Sam"}])).into_response()
}

async fn recipes(State(fake): State<Arc<FakeTandoor>>, method: Method, uri: Uri, headers: HeaderMap) -> Response {
    if !authorised(&headers) {
        return refuse();
    }

    fake.sent.lock().unwrap().push(Sent { method, path: uri.to_string(), content_type: String::new(), body: Vec::new() });

    Json(json!({"count": 42, "results": [{"id": 3, "name": "Dal", "description": "", "servings": 4, "keywords": []}]})).into_response()
}

async fn create_recipe(State(fake): State<Arc<FakeTandoor>>, Json(body): Json<Value>) -> Json<Value> {
    fake.posted.lock().unwrap().push(("recipe".to_string(), body.clone()));

    Json(json!({"id": 9, "name": body["name"], "description": "", "servings": 1, "keywords": []}))
}

async fn meal_types() -> Json<Value> {
    Json(json!([{"id": 2, "name": "Dinner"}]))
}

async fn shopping() -> Json<Value> {
    Json(json!({"results": [{"id": 1, "food": {"name": "Rice"}, "amount": 1, "unit": null, "checked": false}, {"id": 2, "food": {"name": "Salt"}, "amount": 1, "unit": null, "checked": true}]}))
}

async fn check(State(fake): State<Arc<FakeTandoor>>, Path(id): Path<u64>, Json(body): Json<Value>) -> Json<Value> {
    fake.posted.lock().unwrap().push((format!("entry {id}"), body.clone()));

    Json(json!({"id": id, "food": {"name": "Rice"}, "amount": 1, "unit": null, "checked": body["checked"]}))
}

async fn record(State(fake): State<Arc<FakeTandoor>>, method: Method, uri: Uri, headers: HeaderMap, body: Bytes) -> Response {
    let content_type = headers.get("content-type").and_then(|value| value.to_str().ok()).unwrap_or_default().to_string();
    let sent = Sent { method: method.clone(), path: uri.to_string(), content_type, body: body.to_vec() };
    let given = sent.json();

    fake.sent.lock().unwrap().push(sent);

    if method == Method::DELETE {
        return StatusCode::NO_CONTENT.into_response();
    }

    if given["name"] == json!("Broken") {
        return (StatusCode::BAD_REQUEST, Json(json!({"steps": [{"ingredients": [{"unit": ["This field is required."]}]}]}))).into_response();
    }

    match uri.path() {
        "/api/recipe-from-source/" if given["url"] == json!("https://example.com/no-recipe") => Json(json!({"error": true, "msg": "No usable data could be found."})).into_response(),
        "/api/recipe-from-source/" => Json(json!({"error": false, "recipe": {"name": "Lentil soup", "steps": [{"instruction": "Simmer.", "ingredients": []}]}, "images": ["https://example.com/soup.jpg"], "duplicates": []})).into_response(),
        "/api/property-type/" => Json(json!({"results": [{"id": 5, "name": "Calories"}, {"id": 6, "name": "Protein"}]})).into_response(),
        "/api/food/" => Json(json!({"results": [{"id": 7, "name": "Oats", "properties": [{"property_amount": 389, "property_type": {"id": 5, "name": "Calories"}}], "properties_food_amount": 100, "properties_food_unit": {"name": "g"}}]})).into_response(),
        "/api/unit/" => Json(json!({"results": [{"id": 8, "name": "g", "description": "gram"}]})).into_response(),
        "/api/keyword/" => Json(json!({"results": [{"id": 1, "name": "Vegan", "description": ""}]})).into_response(),
        "/api/recipe/9/image/" => Json(json!({"image": "https://example.com/media/recipes/9.png", "image_url": null})).into_response(),
        "/api/meal-plan/4/" => Json(json!({"id": 4, "title": "", "recipe": null, "meal_type": given["meal_type"], "from_date": given["from_date"], "servings": given["servings"]})).into_response(),
        _ => {
            let mut answer = given;

            answer["id"] = json!(uri.path().trim_matches('/').rsplit('/').next().unwrap_or_default().parse::<u64>().unwrap_or(0));

            Json(answer).into_response()
        }
    }
}

fn sent(fake: &FakeTandoor, method: Method, prefix: &str) -> Vec<Sent> {
    fake.sent.lock().unwrap().iter().filter(|sent| sent.method == method && sent.path.starts_with(prefix)).cloned().collect()
}

async fn fake_tandoor() -> (String, Arc<FakeTandoor>) {
    let fake = Arc::new(FakeTandoor::default());
    let router = Router::new()
        .route("/api/space/", get(space))
        .route("/api/user/", get(users))
        .route("/api/recipe/", get(recipes).post(create_recipe))
        .route("/api/meal-type/", get(meal_types))
        .route("/api/shopping-list-entry/", get(shopping))
        .route("/api/shopping-list-entry/{id}/", patch(check).delete(record))
        .fallback(record)
        .with_state(fake.clone());

    (serve(router).await, fake)
}

async fn call(server: &Server, token: &str, tool: &str, arguments: Value) -> Value {
    let body = json!({"jsonrpc": "2.0", "id": 1, "method": "tools/call", "params": {"name": format!("tandoor__{tool}"), "arguments": arguments}});
    let asks = matches!(tool, "create_recipe" | "update_recipe" | "set_recipe_image" | "delete_recipe" | "update_food" | "plan_meal" | "update_meal_plan" | "delete_meal_plan" | "add_to_shopping_list" | "check_off_item" | "uncheck_item" | "remove_shopping_item");

    if asks {
        return server.approved_agent_mcp(token, body).await["result"].clone();
    }

    let anonymous = server.anonymous();
    let (status, reply) = anonymous.json(anonymous.post("/api/v1/mcp").header("authorization", format!("Bearer {token}")).json(&body)).await;
    assert_eq!(status, 200, "{reply}");

    reply["result"].clone()
}

fn text(result: &Value) -> String {
    result["content"][0]["text"].as_str().unwrap().to_string()
}

#[tokio::test]
async fn the_setup_test_names_the_space_the_user_and_the_recipe_count() {
    let server = Server::start().await;
    let (base, _) = fake_tandoor().await;

    let (status, found) = server.user.json(server.user.post("/api/v1/chat-tool-servers/tandoor-test").json(&json!({"url": base, "token": TOKEN}))).await;

    assert_eq!(status, 200, "{found}");
    assert_eq!((found["space"].clone(), found["user"].clone(), found["recipes"].clone()), (json!("Home kitchen"), json!("Sam"), json!(42)));
}

#[tokio::test]
async fn the_setup_test_says_how_to_fix_a_bad_token_an_unreachable_server_and_a_bad_address() {
    let server = Server::start().await;
    let (base, _) = fake_tandoor().await;
    let test = |url: &str, token: &str| server.user.json(server.user.post("/api/v1/chat-tool-servers/tandoor-test").json(&json!({"url": url, "token": token})));

    let (refused, token) = test(&base, "wrong").await;
    let (unreachable, gone) = test("http://127.0.0.1:9", TOKEN).await;
    let (invalid, address) = test("recipes", TOKEN).await;

    assert_eq!((refused, token["error"].clone()), (400, json!("tandoor_token")));
    assert!(token["message"].as_str().unwrap().contains("did not accept the API token. Check the token has the scope read write and has not expired, or create a new one in Tandoor under Settings, then API."));
    assert_eq!((unreachable, gone["error"].clone()), (400, json!("tandoor_address")));
    assert!(gone["message"].as_str().unwrap().starts_with("Could not reach 127.0.0.1. Check the address and that Tandoor is running."));
    assert_eq!((invalid, address["message"].clone()), (400, json!("Enter the server address, like https://recipes.example.com")));
}

#[tokio::test]
async fn the_token_is_stored_but_never_returned() {
    let server = Server::start().await;
    let (base, _) = fake_tandoor().await;

    let (status, _) = server.user.json(server.user.post("/api/v1/chat-tool-servers").json(&json!({"name": "Tandoor", "kind": "tandoor", "url": base, "bearer_token": TOKEN}))).await;
    let stored = server.user.get("/api/v1/chat-tool-servers").send().await.unwrap().text().await.unwrap();

    assert_eq!(status, 201);
    assert!(!stored.contains(TOKEN));
}

#[tokio::test]
async fn the_agent_looks_recipes_and_the_shopping_list_up_without_asking() {
    let server = Server::start().await;
    let node = server.online_node("laptop").await;
    let (base, _) = fake_tandoor().await;
    server.user.json(server.user.post("/api/v1/chat-tool-servers").json(&json!({"name": "Tandoor", "kind": "tandoor", "url": base, "bearer_token": TOKEN}))).await;

    let found = text(&call(&server, &node.token, "search_recipes", json!({"query": "dal"})).await);
    let list = text(&call(&server, &node.token, "get_shopping_list", json!({})).await);

    assert!(found.contains("Dal") && found.contains("42"), "{found}");
    assert!(list.contains("Rice") && !list.contains("Salt"), "{list}");
}

#[tokio::test]
async fn changing_anything_asks_first_and_sends_what_was_approved() {
    let server = Server::start().await;
    let node = server.online_node("laptop").await;
    let (base, fake) = fake_tandoor().await;
    server.user.json(server.user.post("/api/v1/chat-tool-servers").json(&json!({"name": "Tandoor", "kind": "tandoor", "url": base, "bearer_token": TOKEN}))).await;

    let created = call(&server, &node.token, "create_recipe", json!({"name": "Flapjacks", "ingredients": [{"food": "Oats", "amount": 200, "unit": "g"}]})).await;
    let checked = call(&server, &node.token, "check_off_item", json!({"entry": 1})).await;
    let posted = fake.posted.lock().unwrap().clone();

    assert_ne!(created["isError"], true, "{created}");
    assert_ne!(checked["isError"], true, "{checked}");
    assert_eq!(posted[0].1["steps"][0]["ingredients"][0]["food"], json!({"name": "Oats"}));
    assert_eq!(posted[1], ("entry 1".to_string(), json!({"checked": true})));
}

async fn connected() -> (Server, NodeLink, Arc<FakeTandoor>) {
    let server = Server::start().await;
    let node = server.online_node("laptop").await;
    let (base, fake) = fake_tandoor().await;
    server.user.json(server.user.post("/api/v1/chat-tool-servers").json(&json!({"name": "Tandoor", "kind": "tandoor", "url": base, "bearer_token": TOKEN}))).await;

    (server, node, fake)
}

#[tokio::test]
async fn update_recipe_puts_the_whole_recipe_with_its_steps_and_ingredients() {
    let (server, node, fake) = connected().await;

    let updated = call(&server, &node.token, "update_recipe", json!({
        "recipe": 9,
        "name": "Flapjacks",
        "servings": 12,
        "servings_text": "squares",
        "working_time": 10,
        "waiting_time": 25,
        "keywords": ["Baking"],
        "internal": true,
        "show_ingredient_overview": true,
        "source_url": "https://example.com/flapjacks",
        "steps": [
            {"name": "Melt", "show_as_header": true, "instruction": "Melt the butter and syrup.", "time": 5, "ingredients": [{"food": "Butter", "amount": 125, "unit": "g"}, {"food": "Golden syrup", "amount": 3, "unit": "tbsp", "note": "warmed"}]},
            {"instruction": "Stir in the oats and bake.", "order": 4, "ingredients": [{"food": "Oats", "amount": 250, "unit": "g", "order": 2}, {"food": "Salt"}]}
        ]
    })).await;
    let puts = sent(&fake, Method::PUT, "/api/recipe/9/");

    assert_ne!(updated["isError"], true, "{updated}");
    assert_eq!(puts.len(), 1);
    assert_eq!(puts[0].json(), json!({
        "name": "Flapjacks",
        "servings": 12,
        "servings_text": "squares",
        "working_time": 10,
        "waiting_time": 25,
        "keywords": [{"name": "Baking"}],
        "internal": true,
        "show_ingredient_overview": true,
        "source_url": "https://example.com/flapjacks",
        "steps": [
            {"name": "Melt", "show_as_header": true, "instruction": "Melt the butter and syrup.", "time": 5, "order": 0, "ingredients": [
                {"food": {"name": "Butter"}, "unit": {"name": "g"}, "amount": 125.0, "note": "", "order": 0},
                {"food": {"name": "Golden syrup"}, "unit": {"name": "tbsp"}, "amount": 3.0, "note": "warmed", "order": 1}
            ]},
            {"name": "", "show_as_header": false, "instruction": "Stir in the oats and bake.", "time": 0, "order": 4, "ingredients": [
                {"food": {"name": "Oats"}, "unit": {"name": "g"}, "amount": 250.0, "note": "", "order": 2},
                {"food": {"name": "Salt"}, "unit": null, "amount": 0.0, "note": "", "order": 1}
            ]}
        ]
    }));
}

#[tokio::test]
async fn update_recipe_without_steps_is_refused_before_anything_is_sent() {
    let (server, node, fake) = connected().await;

    let refused = call(&server, &node.token, "update_recipe", json!({"recipe": 9, "name": "Flapjacks"})).await;

    assert_eq!(refused["isError"], true, "{refused}");
    assert!(text(&refused).contains("Give every step of the recipe in steps."));
    assert!(sent(&fake, Method::PUT, "/api/recipe/").is_empty());
}

#[tokio::test]
async fn a_rejected_recipe_says_which_field_tandoor_refused() {
    let (server, node, _) = connected().await;

    let refused = call(&server, &node.token, "update_recipe", json!({"recipe": 9, "name": "Broken", "steps": []})).await;

    assert_eq!(refused["isError"], true, "{refused}");
    assert!(text(&refused).contains("This field is required."), "{refused}");
}

#[tokio::test]
async fn create_recipe_posts_its_steps_and_recipe_fields_as_an_internal_recipe() {
    let (server, node, fake) = connected().await;

    let created = call(&server, &node.token, "create_recipe", json!({
        "name": "Lentil soup",
        "working_time": 15,
        "keywords": ["Soup", "Vegan"],
        "steps": [{"name": "Soften", "instruction": "Soften the onion.", "time": 8, "ingredients": [{"food": "Onion", "amount": 1, "unit": "piece"}]}, {"instruction": "Add the lentils and simmer."}]
    })).await;
    let posted = fake.posted.lock().unwrap()[0].1.clone();

    assert_ne!(created["isError"], true, "{created}");
    assert_eq!(posted, json!({
        "name": "Lentil soup",
        "description": "",
        "servings": 1,
        "working_time": 15,
        "keywords": [{"name": "Soup"}, {"name": "Vegan"}],
        "internal": true,
        "steps": [
            {"name": "Soften", "show_as_header": false, "instruction": "Soften the onion.", "time": 8, "order": 0, "ingredients": [{"food": {"name": "Onion"}, "unit": {"name": "piece"}, "amount": 1.0, "note": "", "order": 0}]},
            {"name": "", "show_as_header": false, "instruction": "Add the lentils and simmer.", "time": 0, "order": 1, "ingredients": []}
        ]
    }));
}

#[tokio::test]
async fn set_recipe_image_uploads_the_file_as_a_multipart_image_field() {
    let (server, node, fake) = connected().await;
    let png = [0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A, 1, 2, 3];

    let set = call(&server, &node.token, "set_recipe_image", json!({"recipe": 9, "image": base64::engine::general_purpose::STANDARD.encode(png)})).await;
    let puts = sent(&fake, Method::PUT, "/api/recipe/9/image/");
    let body = String::from_utf8_lossy(&puts[0].body).to_string();

    assert_ne!(set["isError"], true, "{set}");
    assert!(text(&set).contains("https://example.com/media/recipes/9.png"), "{set}");
    assert!(puts[0].content_type.starts_with("multipart/form-data; boundary="), "{}", puts[0].content_type);
    assert!(body.contains("Content-Disposition: form-data; name=\"image\"; filename=\"image.png\""), "{body}");
    assert!(body.contains("Content-Type: image/png"), "{body}");
    assert!(puts[0].body.windows(png.len()).any(|window| window == png));
}

#[tokio::test]
async fn set_recipe_image_from_an_address_lets_tandoor_fetch_it() {
    let (server, node, fake) = connected().await;

    let set = call(&server, &node.token, "set_recipe_image", json!({"recipe": 9, "image_url": "https://example.org/flapjacks.jpg"})).await;
    let puts = sent(&fake, Method::PUT, "/api/recipe/9/image/");
    let body = String::from_utf8_lossy(&puts[0].body).to_string();

    assert_ne!(set["isError"], true, "{set}");
    assert!(body.contains("name=\"image_url\"") && body.contains("https://example.org/flapjacks.jpg"), "{body}");
    assert!(!body.contains("name=\"image\""), "{body}");
}

#[tokio::test]
async fn set_recipe_image_refuses_a_file_that_is_not_an_image() {
    let (server, node, fake) = connected().await;

    let refused = call(&server, &node.token, "set_recipe_image", json!({"recipe": 9, "image": base64::engine::general_purpose::STANDARD.encode(b"%PDF-1.7")})).await;

    assert_eq!(text(&refused), "The image must be a JPEG, PNG, WebP or GIF file.");
    assert!(sent(&fake, Method::PUT, "/api/recipe/").is_empty());
}

#[tokio::test]
async fn deleting_asks_first_and_sends_a_delete() {
    let (server, node, fake) = connected().await;

    let recipe = call(&server, &node.token, "delete_recipe", json!({"recipe": 9})).await;
    let plan = call(&server, &node.token, "delete_meal_plan", json!({"plan": 4})).await;
    let entry = call(&server, &node.token, "remove_shopping_item", json!({"entry": 1})).await;
    let deleted = sent(&fake, Method::DELETE, "/api/").iter().map(|sent| sent.path.clone()).collect::<Vec<_>>();

    assert_eq!((text(&recipe), text(&plan), text(&entry)), (json!({"recipe": 9, "deleted": true}).to_string(), json!({"plan": 4, "deleted": true}).to_string(), json!({"entry": 1, "deleted": true}).to_string()));
    assert_eq!(deleted, ["/api/recipe/9/", "/api/meal-plan/4/", "/api/shopping-list-entry/1/"]);
}

#[tokio::test]
async fn search_recipes_passes_every_filter_to_tandoor() {
    let (server, node, fake) = connected().await;

    let found = call(&server, &node.token, "search_recipes", json!({"query": "dal", "keywords": [1, 2], "foods": [7], "rating_gte": 4, "makenow": true, "random": false, "new": true, "sort_order": "-rating", "page": 2, "limit": 5})).await;
    let searched = sent(&fake, Method::GET, "/api/recipe/?").last().unwrap().path.clone();

    assert_ne!(found["isError"], true, "{found}");
    assert_eq!(searched, "/api/recipe/?page_size=5&query=dal&keywords=1&keywords=2&foods=7&rating_gte=4&page=2&makenow=true&random=false&new=true&sort_order=-rating");
}

#[tokio::test]
async fn foods_units_and_keywords_are_found_by_name_without_asking() {
    let (server, node, fake) = connected().await;

    let food = call(&server, &node.token, "find_food", json!({"query": "oats"})).await;
    let unit = call(&server, &node.token, "find_unit", json!({"query": "g"})).await;
    let keyword = call(&server, &node.token, "find_keyword", json!({"query": "vegan", "limit": 3})).await;
    let paths = sent(&fake, Method::GET, "/api/").into_iter().map(|sent| sent.path).filter(|path| !path.starts_with("/api/recipe/")).collect::<Vec<_>>();

    assert_eq!(serde_json::from_str::<Value>(&text(&food)).unwrap(), json!([{"id": 7, "name": "Oats", "properties": [{"name": "Calories", "amount": 389}], "per_amount": 100, "per_unit": "g"}]));
    assert!(text(&unit).contains("\"id\":8") && text(&keyword).contains("\"id\":1"));
    assert_eq!(paths, ["/api/food/?query=oats&page_size=20", "/api/unit/?query=g&page_size=20", "/api/keyword/?query=vegan&page_size=3"]);
}

#[tokio::test]
async fn update_food_patches_its_nutrition_with_tandoors_property_types() {
    let (server, node, fake) = connected().await;

    let updated = call(&server, &node.token, "update_food", json!({"food": 7, "properties": [{"name": "calories", "amount": 389}, {"name": "Protein", "amount": 13.2}], "per_amount": 100, "per_unit": "g"})).await;
    let patches = sent(&fake, Method::PATCH, "/api/food/7/");

    assert_ne!(updated["isError"], true, "{updated}");
    assert_eq!(patches[0].json(), json!({
        "properties": [{"property_amount": 389.0, "property_type": {"id": 5, "name": "Calories"}}, {"property_amount": 13.2, "property_type": {"id": 6, "name": "Protein"}}],
        "properties_food_amount": 100.0,
        "properties_food_unit": {"name": "g"}
    }));
}

#[tokio::test]
async fn update_food_names_the_property_types_when_one_is_unknown() {
    let (server, node, fake) = connected().await;

    let refused = call(&server, &node.token, "update_food", json!({"food": 7, "properties": [{"name": "Fibre", "amount": 10}]})).await;

    assert_eq!(text(&refused), "Tandoor has no property type called Fibre. Use one of: Calories, Protein.");
    assert!(sent(&fake, Method::PATCH, "/api/food/").is_empty());
}

#[tokio::test]
async fn update_meal_plan_patches_only_what_was_given() {
    let (server, node, fake) = connected().await;

    let updated = call(&server, &node.token, "update_meal_plan", json!({"plan": 4, "date": "2026-11-12", "meal_type": "dinner", "servings": 2})).await;
    let patches = sent(&fake, Method::PATCH, "/api/meal-plan/4/");

    assert_ne!(updated["isError"], true, "{updated}");
    assert_eq!(patches[0].json(), json!({"from_date": "2026-11-12", "to_date": "2026-11-12", "meal_type": {"id": 2, "name": "Dinner"}, "servings": 2}));
}

#[tokio::test]
async fn uncheck_item_puts_an_entry_back_on_the_list() {
    let (server, node, fake) = connected().await;

    let unchecked = call(&server, &node.token, "uncheck_item", json!({"entry": 1})).await;
    let posted = fake.posted.lock().unwrap().clone();

    assert_ne!(unchecked["isError"], true, "{unchecked}");
    assert_eq!(posted, [("entry 1".to_string(), json!({"checked": false}))]);
}

#[tokio::test]
async fn import_recipe_from_url_returns_the_parsed_recipe_without_saving_it() {
    let (server, node, fake) = connected().await;

    let imported = call(&server, &node.token, "import_recipe_from_url", json!({"url": "https://example.com/lentil-soup"})).await;
    let failed = call(&server, &node.token, "import_recipe_from_url", json!({"url": "https://example.com/no-recipe"})).await;
    let posts = sent(&fake, Method::POST, "/api/recipe-from-source/");

    assert!(text(&imported).contains("Lentil soup") && text(&imported).contains("https://example.com/soup.jpg"), "{imported}");
    assert_eq!(text(&failed), "Tandoor could not read a recipe from that page: No usable data could be found.");
    assert_eq!(posts[0].json(), json!({"url": "https://example.com/lentil-soup"}));
    assert!(fake.posted.lock().unwrap().is_empty());
}
