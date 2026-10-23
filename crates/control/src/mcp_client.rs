use serde_json::{Value, json};
use std::time::Duration;

const MODERN_VERSION: &str = "2026-07-28";
const LEGACY_VERSION: &str = "2025-11-25";
const CALL_TIMEOUT: Duration = Duration::from_secs(120);
const LISTING_TIMEOUT: Duration = Duration::from_secs(20);
const MAX_LISTING_PAGES: usize = 20;
const HEADER_MISMATCH: i64 = -32020;

pub enum McpError {
    Unreachable,
    Rejected,
    Failed(String),
}

pub struct McpTool {
    pub name: String,
    pub description: String,
    pub input_schema: Value,
}

pub struct ToolOutput {
    pub text: String,
    pub is_error: bool,
}

enum Era {
    Unknown,
    Modern,
    Legacy { version: String, session: Option<String> },
}

pub struct McpConnection {
    http: reqwest::Client,
    url: String,
    token: Option<String>,
    era: Era,
    next_id: i64,
}

struct Reply {
    status: u16,
    session: Option<String>,
    body: String,
    event_stream: bool,
}

impl McpError {
    pub fn describe(&self, name: &str) -> String {
        match self {
            McpError::Unreachable => format!("Mycelium could not reach {name}. Check the address and that the server is running, then try again."),
            McpError::Rejected => format!("{name} did not accept this token. Check that it has not expired and is for this server, then try again."),
            McpError::Failed(reason) => format!("{name} answered with an error: {reason}"),
        }
    }
}

impl McpConnection {
    pub fn new(http: &reqwest::Client, url: &str, token: Option<&str>) -> McpConnection {
        McpConnection { http: http.clone(), url: url.to_string(), token: token.map(str::to_string), era: Era::Unknown, next_id: 0 }
    }

    pub async fn list_tools(&mut self) -> Result<Vec<McpTool>, McpError> {
        let mut tools = Vec::new();
        let mut cursor: Option<String> = None;

        for _ in 0..MAX_LISTING_PAGES {
            let params = cursor.as_ref().map(|cursor| json!({"cursor": cursor})).unwrap_or_else(|| json!({}));
            let result = self.request("tools/list", params, LISTING_TIMEOUT).await?;

            for tool in result["tools"].as_array().into_iter().flatten() {
                let Some(name) = tool["name"].as_str() else {
                    continue;
                };

                tools.push(McpTool {
                    name: name.to_string(),
                    description: tool["description"].as_str().unwrap_or_default().to_string(),
                    input_schema: tool.get("inputSchema").cloned().unwrap_or_else(|| json!({"type": "object"})),
                });
            }

            cursor = result["nextCursor"].as_str().map(str::to_string);

            if cursor.is_none() {
                break;
            }
        }

        Ok(tools)
    }

    pub async fn call_tool(&mut self, name: &str, arguments: &Value) -> Result<ToolOutput, McpError> {
        let result = self.request("tools/call", json!({"name": name, "arguments": arguments}), CALL_TIMEOUT).await?;

        if result.get("inputRequests").is_some() {
            return Ok(ToolOutput { text: "This tool asked for more input than Mycelium can give it from a chat.".to_string(), is_error: true });
        }

        let parts: Vec<String> = result["content"]
            .as_array()
            .into_iter()
            .flatten()
            .map(|part| match part["type"].as_str() {
                Some("text") => part["text"].as_str().unwrap_or_default().to_string(),
                Some("resource") => part["resource"]["text"].as_str().map(str::to_string).unwrap_or_else(|| format!("[Resource {}]", part["resource"]["uri"].as_str().unwrap_or_default())),
                Some(other) => format!("[{other} content left out]"),
                None => String::new(),
            })
            .collect();
        let text = match (parts.is_empty(), result.get("structuredContent")) {
            (true, Some(structured)) => structured.to_string(),
            _ => parts.join("\n"),
        };

        Ok(ToolOutput { text, is_error: result["isError"].as_bool().unwrap_or(false) })
    }

    async fn request(&mut self, method: &str, params: Value, timeout: Duration) -> Result<Value, McpError> {
        if matches!(self.era, Era::Unknown) {
            self.negotiate(timeout).await?;
        }

        let reply = self.send(method, &params, timeout).await?;

        Self::result_of(reply)
    }

    async fn negotiate(&mut self, timeout: Duration) -> Result<(), McpError> {
        self.era = Era::Modern;
        let reply = self.send("tools/list", &json!({}), timeout).await?;

        match Self::supported_versions(&reply) {
            Some(supported) if !supported.is_empty() && !supported.iter().any(|version| version.as_str() >= MODERN_VERSION) => {
                let version = supported.into_iter().max().unwrap_or_else(|| LEGACY_VERSION.to_string());

                self.initialise_legacy(&version, timeout).await
            }
            Some(_) => Ok(()),
            None if reply.status < 400 => Ok(()),
            None => self.initialise_legacy(LEGACY_VERSION, timeout).await,
        }
    }

    fn supported_versions(reply: &Reply) -> Option<Vec<String>> {
        let body: Value = serde_json::from_str(&reply.body).ok()?;
        let error = body.get("error")?;
        let supported: Vec<String> = error["data"]["supported"].as_array().into_iter().flatten().filter_map(Value::as_str).map(str::to_string).collect();

        (error["code"].as_i64() == Some(HEADER_MISMATCH) || !supported.is_empty()).then_some(supported)
    }

    async fn initialise_legacy(&mut self, version: &str, timeout: Duration) -> Result<(), McpError> {
        self.era = Era::Legacy { version: version.to_string(), session: None };
        let params = json!({"protocolVersion": version, "capabilities": {}, "clientInfo": {"name": "mycelium", "version": env!("CARGO_PKG_VERSION")}});
        let reply = self.send("initialize", &params, timeout).await?;
        let session = reply.session.clone();
        let result = Self::result_of(reply)?;
        let agreed = result["protocolVersion"].as_str().unwrap_or(version).to_string();
        self.era = Era::Legacy { version: agreed, session };
        self.notify("notifications/initialized", timeout).await;

        Ok(())
    }

    async fn notify(&self, method: &str, timeout: Duration) {
        let body = json!({"jsonrpc": "2.0", "method": method});

        if let Ok(request) = self.prepare(method, None, timeout) {
            request.json(&body).send().await.ok();
        }
    }

    fn prepare(&self, method: &str, name: Option<&str>, timeout: Duration) -> Result<reqwest::RequestBuilder, McpError> {
        let mut request = self
            .http
            .post(&self.url)
            .timeout(timeout)
            .header("user-agent", "mycelium")
            .header("accept", "application/json, text/event-stream");

        if let Some(token) = &self.token {
            request = request.bearer_auth(token);
        }

        request = match &self.era {
            Era::Legacy { version, session } => {
                let request = request.header("mcp-protocol-version", version);

                match session {
                    Some(session) => request.header("mcp-session-id", session),
                    None => request,
                }
            }
            _ => {
                let request = request.header("mcp-protocol-version", MODERN_VERSION).header("mcp-method", method);

                match name {
                    Some(name) => request.header("mcp-name", header_value(name)),
                    None => request,
                }
            }
        };

        Ok(request)
    }

    async fn send(&mut self, method: &str, params: &Value, timeout: Duration) -> Result<Reply, McpError> {
        self.next_id += 1;
        let id = self.next_id;
        let mut params = params.clone();

        if matches!(self.era, Era::Modern) {
            params["_meta"] = json!({
                "io.modelcontextprotocol/protocolVersion": MODERN_VERSION,
                "io.modelcontextprotocol/clientInfo": {"name": "mycelium", "version": env!("CARGO_PKG_VERSION")},
                "io.modelcontextprotocol/clientCapabilities": {},
            });
        }

        let body = json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params});
        let name = params["name"].as_str().filter(|_| method == "tools/call");
        let request = self.prepare(method, name, timeout)?;
        let Ok(response) = request.json(&body).send().await else {
            return Err(McpError::Unreachable);
        };
        let status = response.status().as_u16();
        let session = response.headers().get("mcp-session-id").and_then(|value| value.to_str().ok()).map(str::to_string);
        let event_stream = response.headers().get("content-type").and_then(|value| value.to_str().ok()).is_some_and(|value| value.starts_with("text/event-stream"));
        let Ok(text) = response.text().await else {
            return Err(McpError::Failed("the server stopped replying part way through.".to_string()));
        };

        if status == 401 || status == 403 {
            return Err(McpError::Rejected);
        }

        let body = if event_stream { Self::answer_in_events(&text, id) } else { text };

        Ok(Reply { status, session, body, event_stream })
    }

    fn answer_in_events(text: &str, id: i64) -> String {
        let mut answer = String::new();

        for event in text.replace("\r\n", "\n").split("\n\n") {
            let data: Vec<&str> = event.lines().filter_map(|line| line.strip_prefix("data:")).map(str::trim_start).collect();
            let joined = data.join("\n");

            if let Ok(message) = serde_json::from_str::<Value>(&joined)
                && message["id"].as_i64() == Some(id)
                && (message.get("result").is_some() || message.get("error").is_some())
            {
                answer = joined;
            }
        }

        answer
    }

    fn result_of(reply: Reply) -> Result<Value, McpError> {
        let parsed = serde_json::from_str::<Value>(&reply.body);

        match parsed {
            Ok(body) if body.get("result").is_some() => Ok(body["result"].clone()),
            Ok(body) if body.get("error").is_some() => Err(McpError::Failed(body["error"]["message"].as_str().unwrap_or("it answered with an error.").to_string())),
            _ if reply.status >= 400 => Err(McpError::Failed(format!("it answered with status {}.", reply.status))),
            _ if reply.event_stream => Err(McpError::Failed("it ended its reply without an answer.".to_string())),
            _ => Err(McpError::Failed("it did not answer in a form Mycelium understands.".to_string())),
        }
    }
}

fn header_value(value: &str) -> String {
    let plain = value.bytes().all(|byte| (0x21..=0x7e).contains(&byte)) && !(value.starts_with("=?base64?") && value.ends_with("?="));

    if plain {
        return value.to_string();
    }

    use base64::Engine;

    format!("=?base64?{}?=", base64::engine::general_purpose::STANDARD.encode(value))
}
