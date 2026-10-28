use futures_util::{SinkExt, StreamExt};
use mycelium_proto::link::{BrowserAction, BrowserControl};
use serde::Deserialize;
use serde_json::{Value, json};
use std::time::Duration;
use tokio_tungstenite::connect_async;
use tokio_tungstenite::tungstenite::Message;

const CDP_TIMEOUT: Duration = Duration::from_secs(10);
const NO_PAGE: &str = "The browser has no page open to read. Open a page in the environment, then try again.";

const READ_SCRIPT: &str = r#"(() => {
  const selector = 'a[href],button,input:not([type=hidden]),select,textarea,summary,[role=button],[role=link],[role=checkbox],[role=radio],[role=tab],[role=menuitem],[role=option],[role=combobox],[role=textbox],[role=switch],[contenteditable=""],[contenteditable=true]';
  const visible = (element) => {
    const box = element.getBoundingClientRect();
    const style = getComputedStyle(element);
    return box.width > 0 && box.height > 0 && style.visibility !== 'hidden' && style.display !== 'none' && box.bottom > 0 && box.right > 0 && box.top < innerHeight && box.left < innerWidth;
  };
  const nameOf = (element) => {
    const label = element.labels && element.labels[0] ? element.labels[0].innerText : '';
    const text = element.getAttribute('aria-label') || label || element.placeholder || element.innerText || element.value || element.title || element.alt || '';
    return text.trim().replace(/\s+/g, ' ').slice(0, 80);
  };
  const roleOf = (element) => {
    const given = element.getAttribute('role');
    if (given) return given;
    const byTag = { A: 'link', BUTTON: 'button', SELECT: 'select', TEXTAREA: 'textbox', SUMMARY: 'button' }[element.tagName];
    if (byTag) return byTag;
    return { checkbox: 'checkbox', radio: 'radio', submit: 'button', button: 'button' }[element.type] || 'textbox';
  };
  const found = [...document.querySelectorAll(selector)].filter(visible).slice(0, 150);
  window.__myceliumControls = found;
  return JSON.stringify({
    page: document.title + ' (' + location.href + ')',
    controls: found.map((element, position) => ({
      index: position + 1,
      role: roleOf(element),
      name: nameOf(element),
      value: 'value' in element && element.type !== 'password' ? String(element.value || '').slice(0, 80) : '',
    })),
  });
})()"#;

#[derive(Deserialize)]
struct ReadPage {
    page: String,
    controls: Vec<BrowserControl>,
}

fn act_script(action: &BrowserAction) -> String {
    match action {
        BrowserAction::Click { index } => format!(
            "(() => {{ const element = (window.__myceliumControls || [])[{index} - 1]; if (!element) return 'missing'; element.scrollIntoView({{block: 'center'}}); element.click(); return 'ok'; }})()"
        ),
        BrowserAction::Type { index, text } => format!(
            "(() => {{ const element = (window.__myceliumControls || [])[{index} - 1]; if (!element) return 'missing'; element.scrollIntoView({{block: 'center'}}); element.focus(); const text = {text}; if (element.isContentEditable) {{ element.textContent = text; }} else {{ const prototype = element.tagName === 'TEXTAREA' ? HTMLTextAreaElement.prototype : HTMLInputElement.prototype; Object.getOwnPropertyDescriptor(prototype, 'value').set.call(element, text); }} element.dispatchEvent(new Event('input', {{bubbles: true}})); element.dispatchEvent(new Event('change', {{bubbles: true}})); return 'ok'; }})()",
            text = json!(text)
        ),
        BrowserAction::Select { index, text } => format!(
            "(() => {{ const element = (window.__myceliumControls || [])[{index} - 1]; if (!element) return 'missing'; if (element.tagName !== 'SELECT') {{ element.scrollIntoView({{block: 'center'}}); element.click(); return 'ok'; }} const wanted = {text}.toLowerCase(); const option = [...element.options].find((candidate) => candidate.text.trim().toLowerCase() === wanted) || [...element.options].find((candidate) => candidate.text.toLowerCase().includes(wanted)); if (!option) return 'no option'; element.value = option.value; element.dispatchEvent(new Event('input', {{bubbles: true}})); element.dispatchEvent(new Event('change', {{bubbles: true}})); return 'ok'; }})()",
            text = json!(text)
        ),
        BrowserAction::Scroll { direction } => {
            let (across, down) = match direction.as_str() {
                "up" => (0.0, -0.8),
                "left" => (-0.8, 0.0),
                "right" => (0.8, 0.0),
                _ => (0.0, 0.8),
            };

            format!("(() => {{ window.scrollBy(innerWidth * {across}, innerHeight * {down}); return 'ok'; }})()")
        }
    }
}

async fn page_socket(debug_port: u16) -> Result<String, String> {
    let listed = reqwest::get(format!("http://127.0.0.1:{debug_port}/json/list")).await.map_err(|_| "The environment's browser is not answering. Try again in a moment.".to_string())?;
    let pages: Vec<Value> = listed.json().await.map_err(|_| "The environment's browser answered in a shape the node does not know.".to_string())?;

    pages
        .iter()
        .find(|page| page["type"] == "page" && page["webSocketDebuggerUrl"].is_string())
        .and_then(|page| page["webSocketDebuggerUrl"].as_str())
        .map(str::to_string)
        .ok_or_else(|| NO_PAGE.to_string())
}

async fn evaluate(socket_url: &str, expression: &str) -> Result<String, String> {
    let (mut socket, _) = connect_async(socket_url).await.map_err(|error| format!("The node could not reach the environment's browser: {error}."))?;
    let request = json!({"id": 1, "method": "Runtime.evaluate", "params": {"expression": expression, "returnByValue": true}});

    socket.send(Message::text(request.to_string())).await.map_err(|error| format!("The node could not talk to the environment's browser: {error}."))?;

    while let Some(message) = socket.next().await {
        let Ok(Message::Text(text)) = message else {
            continue;
        };
        let Ok(reply) = serde_json::from_str::<Value>(text.as_str()) else {
            continue;
        };

        if reply["id"] != 1 {
            continue;
        }

        if let Some(exception) = reply["result"]["exceptionDetails"]["text"].as_str() {
            return Err(format!("The page refused the instruction: {exception}."));
        }

        return Ok(reply["result"]["result"]["value"].as_str().unwrap_or_default().to_string());
    }

    Err("The environment's browser closed the connection before it answered.".to_string())
}

pub async fn read(debug_port: u16) -> Result<(String, Vec<BrowserControl>), String> {
    let work = async {
        let socket_url = page_socket(debug_port).await?;
        let value = evaluate(&socket_url, READ_SCRIPT).await?;
        let page: ReadPage = serde_json::from_str(&value).map_err(|_| "The page's controls could not be read. Try again.".to_string())?;

        Ok((page.page, page.controls))
    };

    tokio::time::timeout(CDP_TIMEOUT, work).await.map_err(|_| "The page took too long to answer. Try again.".to_string())?
}

pub async fn act(debug_port: u16, action: &BrowserAction) -> Result<(), String> {
    let work = async {
        let socket_url = page_socket(debug_port).await?;
        let outcome = evaluate(&socket_url, &act_script(action)).await?;

        match outcome.as_str() {
            "ok" => Ok(()),
            "missing" => Err("That control is no longer on the page. Read the page again.".to_string()),
            "no option" => Err("That list has no option with that text.".to_string()),
            other => Err(format!("The page did not do it: {other}.")),
        }
    };

    tokio::time::timeout(CDP_TIMEOUT, work).await.map_err(|_| "The page took too long to answer. Try again.".to_string())?
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

    async fn fake_browser(answers: Vec<&'static str>) -> u16 {
        let sockets = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let socket_port = sockets.local_addr().unwrap().port();
        let listing = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let debug_port = listing.local_addr().unwrap().port();

        tokio::spawn(async move {
            loop {
                let Ok((mut connection, _)) = listing.accept().await else {
                    return;
                };
                let mut buffer = [0u8; 1024];
                let _ = connection.read(&mut buffer).await;
                let body = json!([{"type": "page", "webSocketDebuggerUrl": format!("ws://127.0.0.1:{socket_port}/page")}]).to_string();
                let response = format!("HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}", body.len());
                let _ = connection.write_all(response.as_bytes()).await;
            }
        });
        tokio::spawn(async move {
            let mut answers = answers.into_iter();

            while let Ok((connection, _)) = sockets.accept().await {
                let Ok(mut socket) = tokio_tungstenite::accept_async(connection).await else {
                    continue;
                };
                let Some(Ok(Message::Text(request))) = socket.next().await else {
                    continue;
                };
                let request: Value = serde_json::from_str(request.as_str()).unwrap();
                let value = answers.next().unwrap_or("");
                let reply = json!({"id": request["id"], "result": {"result": {"type": "string", "value": value}}});
                let _ = socket.send(Message::text(reply.to_string())).await;
            }
        });

        debug_port
    }

    #[tokio::test]
    async fn reading_returns_the_pages_visible_controls_in_order() {
        let page = r#"{"page":"Book a flight (https://flights.example/)","controls":[{"index":1,"role":"textbox","name":"From","value":""},{"index":2,"role":"button","name":"Search flights","value":""}]}"#;
        let port = fake_browser(vec![page]).await;

        let (title, controls) = read(port).await.unwrap();

        assert_eq!(title, "Book a flight (https://flights.example/)");
        assert_eq!(controls.iter().map(|control| (control.index, control.name.as_str())).collect::<Vec<_>>(), [(1, "From"), (2, "Search flights")]);
    }

    #[tokio::test]
    async fn acting_on_a_control_the_page_no_longer_has_says_to_read_again() {
        let port = fake_browser(vec!["missing"]).await;

        let outcome = act(port, &BrowserAction::Click { index: 9 }).await;

        assert_eq!(outcome, Err("That control is no longer on the page. Read the page again.".to_string()));
    }

    #[tokio::test]
    async fn a_browser_that_is_not_listening_is_reported_plainly() {
        let free = TcpListener::bind("127.0.0.1:0").await.unwrap().local_addr().unwrap().port();

        let outcome = read(free).await;

        assert_eq!(outcome.unwrap_err(), "The environment's browser is not answering. Try again in a moment.");
    }

    #[test]
    fn typed_text_is_quoted_so_it_cannot_leave_the_script() {
        let script = act_script(&BrowserAction::Type { index: 1, text: "a\"; alert(1); \"".to_string() });

        assert!(script.contains(r#"const text = "a\"; alert(1); \"";"#), "{script}");
    }
}
