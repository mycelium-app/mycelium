use crate::caldav::host_of;
use crate::chat_tool_servers::StoredTool;
use crate::integrations::{CallOutput, Credentials, FoundImage};
use serde_json::{Value, json};
use std::time::Duration;

pub const BRAVE_ADDRESS: &str = "https://api.search.brave.com/res/v1/images/search";
const SEARCH_IMAGES: &str = "search_images";
const DEFAULT_COUNT: u64 = 8;
const MAX_COUNT: u64 = 20;
const TIMEOUT: Duration = Duration::from_secs(30);
const IMAGE_ADDRESS_LIMIT: usize = 2048;
const JSON_FORMAT_OFF: &str = "Turn on the JSON format in your SearXNG settings: add json under search, formats in settings.yml, then restart SearXNG.";

#[derive(Clone, Copy)]
enum Provider {
    SearXng,
    Brave,
}

fn provider_of(settings: &Value) -> Provider {
    match settings["provider"].as_str() {
        Some("brave") => Provider::Brave,
        _ => Provider::SearXng,
    }
}

pub fn normalise_settings(settings: &Value) -> Result<Value, String> {
    match settings["provider"].as_str().map(str::trim) {
        None | Some("") | Some("searxng") => Ok(json!({"provider": "searxng"})),
        Some("brave") => Ok(json!({"provider": "brave"})),
        Some(_) => Err("Choose SearXNG or Brave Search as the search provider.".to_string()),
    }
}

pub fn secret_required(settings: &Value) -> bool {
    matches!(provider_of(settings), Provider::Brave)
}

pub fn default_address(settings: &Value) -> Option<&'static str> {
    match provider_of(settings) {
        Provider::Brave => Some(BRAVE_ADDRESS),
        Provider::SearXng => None,
    }
}

pub fn tools() -> Vec<StoredTool> {
    vec![StoredTool {
        name: SEARCH_IMAGES.to_string(),
        description: "Search the web for images. Show the results with the Markdown block the tool returns, keeping every image address exactly as given: only addresses from this tool load at once, and any other image waits until the person chooses to load it.".to_string(),
        input_schema: json!({"type": "object", "properties": {"query": {"type": "string", "description": "What the images should show."}, "count": {"type": "integer", "description": format!("How many results to return. Defaults to {DEFAULT_COUNT}, at most {MAX_COUNT}.")}}, "required": ["query"]}),
        read_only: None,
    }]
}

pub fn always_asks(_tool: &str) -> bool {
    false
}

struct Found {
    title: String,
    image_url: String,
    page_url: Option<String>,
}

enum Failure {
    Unreachable(String),
    Refused(String),
}

impl Failure {
    fn message(self) -> String {
        match self {
            Failure::Unreachable(message) | Failure::Refused(message) => message,
        }
    }
}

fn shown_as_markdown(address: &str) -> bool {
    address.len() <= IMAGE_ADDRESS_LIMIT && !address.chars().any(|character| character.is_whitespace() || character.is_control() || "<>\"()".contains(character))
}

fn usable(image_url: &str) -> bool {
    shown_as_markdown(image_url) && url::Url::parse(image_url).is_ok_and(|parsed| parsed.scheme() == "https" && parsed.host_str().is_some() && parsed.username().is_empty() && parsed.password().is_none())
}

fn found_from(title: Option<&str>, image_url: Option<&str>, page_url: Option<&str>) -> Option<Found> {
    let image_url = image_url?.trim();

    if !usable(image_url) {
        return None;
    }

    let title: String = title.unwrap_or_default().chars().filter(|character| !matches!(character, '[' | ']') && !character.is_control()).collect::<String>().trim().to_string();
    let title = if title.is_empty() { host_of(image_url) } else { title };
    let page_url = page_url.map(str::trim).filter(|page| shown_as_markdown(page) && url::Url::parse(page).is_ok_and(|parsed| matches!(parsed.scheme(), "http" | "https"))).map(str::to_string);

    Some(Found { title, image_url: image_url.to_string(), page_url })
}

async fn search(http: &reqwest::Client, credentials: &Credentials, query: &str, count: u64) -> Result<Vec<Found>, Failure> {
    let provider = provider_of(&credentials.settings);
    let request = match provider {
        Provider::SearXng => http.get(format!("{}/search", credentials.address.trim_end_matches('/'))).query(&[("q", query), ("categories", "images"), ("format", "json")]),
        Provider::Brave => http.get(&credentials.address).query(&[("q", query), ("count", &count.to_string())]).header("x-subscription-token", &credentials.secret).header("accept", "application/json"),
    };
    let product = match provider {
        Provider::SearXng => "SearXNG",
        Provider::Brave => "Brave Search",
    };
    let host = host_of(&credentials.address);
    let unreachable = || Failure::Unreachable(format!("Mycelium could not reach {host}. Check the address and that {product} is running."));
    let response = request.timeout(TIMEOUT).send().await.map_err(|_| unreachable())?;
    let status = response.status();

    match (provider, status.as_u16()) {
        (Provider::SearXng, 403) => return Err(Failure::Refused(JSON_FORMAT_OFF.to_string())),
        (Provider::Brave, 401 | 403 | 422) => return Err(Failure::Refused("Brave Search did not accept the API key. Check the key is a Search API key and has not expired, or create a new one on brave.com/search/api.".to_string())),
        (Provider::Brave, 429) => return Err(Failure::Refused("Brave Search says the plan's rate limit or quota is used up. Try again later, or check the plan on brave.com/search/api.".to_string())),
        _ if !status.is_success() => return Err(Failure::Refused(format!("{product} answered {status}."))),
        _ => {}
    }

    let body: Value = response.json().await.map_err(|_| Failure::Refused(format!("{product} did not answer with JSON. Check the address is the {product} server's own.")))?;
    let results = body["results"].as_array().cloned().unwrap_or_default();
    let found = results.iter().filter_map(|result| match provider {
        Provider::SearXng => found_from(result["title"].as_str(), result["img_src"].as_str(), result["url"].as_str()),
        Provider::Brave => found_from(result["title"].as_str(), result["properties"]["url"].as_str(), result["url"].as_str()),
    });

    Ok(found.take(count as usize).collect())
}

pub async fn verify(http: &reqwest::Client, credentials: &Credentials) -> Result<(), String> {
    search(http, credentials, "test", 1).await.map(|_| ()).map_err(Failure::message)
}

fn present(query: &str, found: &[Found]) -> String {
    if found.is_empty() {
        return format!("No images were found for {query}. Try different words.");
    }

    let listed: Vec<String> = found
        .iter()
        .enumerate()
        .map(|(position, image)| {
            let page = image.page_url.as_deref().map(|page| format!("\n   page_url: {page}")).unwrap_or_default();

            format!("{}. {}\n   image_url: {}{page}\n   host: {}", position + 1, image.title, image.image_url, host_of(&image.image_url))
        })
        .collect();
    let markdown: Vec<String> = found
        .iter()
        .map(|image| match &image.page_url {
            Some(page) => format!("![{}]({} \"{page}\")", image.title, image.image_url),
            None => format!("![{}]({})", image.title, image.image_url),
        })
        .collect();

    format!("{}\n\nShow the images with this Markdown, keeping every address exactly as given:\n\n{}", listed.join("\n"), markdown.join("\n"))
}

pub async fn call(http: &reqwest::Client, credentials: &Credentials, tool: &str, arguments: &Value) -> CallOutput {
    if tool != SEARCH_IMAGES {
        return CallOutput { text: format!("Web search has no tool called {tool}."), is_error: true, binary: None, images: Vec::new() };
    }

    let Some(query) = arguments["query"].as_str().map(str::trim).filter(|query| !query.is_empty()) else {
        return CallOutput { text: "Give the query.".to_string(), is_error: true, binary: None, images: Vec::new() };
    };
    let count = arguments["count"].as_u64().unwrap_or(DEFAULT_COUNT).clamp(1, MAX_COUNT);

    match search(http, credentials, query, count).await {
        Ok(found) => CallOutput { text: present(query, &found), is_error: false, binary: None, images: found.into_iter().map(|image| FoundImage { url: image.image_url, page_url: image.page_url }).collect() },
        Err(failure) => CallOutput { text: failure.message(), is_error: true, binary: None, images: Vec::new() },
    }
}
