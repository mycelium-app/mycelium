use mycelium_proto::link::{BrowserAction, BrowserControl};
use nix::fcntl::{FcntlArg, OFlag, fcntl};
use serde::Deserialize;
use serde_json::{Value, json};
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::unix::pipe;
use tokio::process::Command;

const CDP_TIMEOUT: Duration = Duration::from_secs(10);
const CDP_INPUT_FD: i32 = 3;
const CDP_OUTPUT_FD: i32 = 4;
const FIRST_PRIVATE_FD: i32 = 10;
const NO_BROWSER: &str = "No Chromium-based browser is answering in the environment. Open Chromium or Chrome from the app launcher, then try again.";
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

enum Failure {
    Closed,
    Refused(String),
}

struct Ends {
    sender: pipe::Sender,
    receiver: pipe::Receiver,
    buffer: Vec<u8>,
    next_id: u64,
}

pub struct BrowserEnds {
    input: OwnedFd,
    output: OwnedFd,
}

pub struct CdpPipe {
    ends: tokio::sync::Mutex<Ends>,
}

#[derive(Clone, Default)]
pub struct BrowserPipes {
    open: Arc<Mutex<Vec<Arc<CdpPipe>>>>,
}

impl CdpPipe {
    pub fn open() -> Result<(CdpPipe, BrowserEnds), String> {
        let unavailable = |error: std::io::Error| format!("The environment could not open a private channel to the browser: {error}.");
        let (browser_input, node_output) = nix::unistd::pipe2(OFlag::O_CLOEXEC).map_err(|error| unavailable(error.into()))?;
        let (node_input, browser_output) = nix::unistd::pipe2(OFlag::O_CLOEXEC).map_err(|error| unavailable(error.into()))?;
        let sender = pipe::Sender::from_owned_fd(node_output).map_err(unavailable)?;
        let receiver = pipe::Receiver::from_owned_fd(node_input).map_err(unavailable)?;
        let ends = Ends { sender, receiver, buffer: Vec::new(), next_id: 0 };

        Ok((CdpPipe { ends: tokio::sync::Mutex::new(ends) }, BrowserEnds { input: browser_input, output: browser_output }))
    }

    async fn evaluate(&self, expression: &str) -> Result<String, Failure> {
        let mut ends = self.ends.lock().await;
        let targets = ends.call("Target.getTargets", json!({}), None).await?;
        let target = targets["targetInfos"]
            .as_array()
            .and_then(|infos| infos.iter().find(|info| info["type"] == "page"))
            .and_then(|info| info["targetId"].as_str())
            .map(str::to_string)
            .ok_or_else(|| Failure::Refused(NO_PAGE.to_string()))?;
        let attached = ends.call("Target.attachToTarget", json!({"targetId": target, "flatten": true}), None).await?;
        let session = attached["sessionId"].as_str().unwrap_or_default().to_string();
        let evaluated = ends.call("Runtime.evaluate", json!({"expression": expression, "returnByValue": true}), Some(&session)).await;
        let _ = ends.call("Target.detachFromTarget", json!({"sessionId": session}), None).await;
        let result = evaluated?;

        if let Some(exception) = result["exceptionDetails"]["text"].as_str() {
            return Err(Failure::Refused(format!("The page refused the instruction: {exception}.")));
        }

        Ok(result["result"]["value"].as_str().unwrap_or_default().to_string())
    }
}

impl Ends {
    async fn call(&mut self, method: &str, params: Value, session: Option<&str>) -> Result<Value, Failure> {
        self.next_id += 1;
        let id = self.next_id;
        let mut request = json!({"id": id, "method": method, "params": params});

        if let Some(session) = session {
            request["sessionId"] = json!(session);
        }

        let mut frame = request.to_string().into_bytes();
        frame.push(0);
        self.sender.write_all(&frame).await.map_err(|_| Failure::Closed)?;

        loop {
            let reply = self.next_frame().await?;

            if reply["id"] != id {
                continue;
            }

            if let Some(message) = reply["error"]["message"].as_str() {
                return Err(Failure::Refused(format!("The environment's browser refused the request: {message}.")));
            }

            return Ok(reply["result"].clone());
        }
    }

    async fn next_frame(&mut self) -> Result<Value, Failure> {
        loop {
            if let Some(end) = self.buffer.iter().position(|byte| *byte == 0) {
                let frame: Vec<u8> = self.buffer.drain(..=end).collect();

                if let Ok(value) = serde_json::from_slice::<Value>(&frame[..end]) {
                    return Ok(value);
                }

                continue;
            }

            let mut chunk = [0u8; 16 * 1024];
            let count = self.receiver.read(&mut chunk).await.map_err(|_| Failure::Closed)?;

            if count == 0 {
                return Err(Failure::Closed);
            }

            self.buffer.extend_from_slice(&chunk[..count]);
        }
    }
}

impl BrowserPipes {
    fn lock(&self) -> MutexGuard<'_, Vec<Arc<CdpPipe>>> {
        self.open.lock().unwrap_or_else(PoisonError::into_inner)
    }

    pub fn attach(&self, command: &mut Command) -> Result<(), String> {
        let (pipe, ends) = CdpPipe::open()?;
        let input = move_clear_of_inherited(ends.input)?;
        let output = move_clear_of_inherited(ends.output)?;
        let (input_fd, output_fd) = (input.as_raw_fd(), output.as_raw_fd());

        unsafe {
            command.pre_exec(move || {
                let _keep = (&input, &output);

                if libc::dup2(input_fd, CDP_INPUT_FD) < 0 || libc::dup2(output_fd, CDP_OUTPUT_FD) < 0 {
                    return Err(std::io::Error::last_os_error());
                }

                Ok(())
            });
        }

        self.lock().push(Arc::new(pipe));

        Ok(())
    }

    async fn evaluate(&self, expression: &str) -> Result<String, String> {
        let candidates: Vec<Arc<CdpPipe>> = self.lock().iter().rev().cloned().collect();

        for pipe in candidates {
            match pipe.evaluate(expression).await {
                Ok(value) => return Ok(value),
                Err(Failure::Refused(message)) => return Err(message),
                Err(Failure::Closed) => self.lock().retain(|open| !Arc::ptr_eq(open, &pipe)),
            }
        }

        Err(NO_BROWSER.to_string())
    }
}

fn move_clear_of_inherited(end: OwnedFd) -> Result<OwnedFd, String> {
    let moved = fcntl(&end, FcntlArg::F_DUPFD_CLOEXEC(FIRST_PRIVATE_FD)).map_err(|error| format!("The environment could not open a private channel to the browser: {error}."))?;

    Ok(unsafe { OwnedFd::from_raw_fd(moved) })
}

pub async fn read(pipes: &BrowserPipes) -> Result<(String, Vec<BrowserControl>), String> {
    let work = async {
        let value = pipes.evaluate(READ_SCRIPT).await?;
        let page: ReadPage = serde_json::from_str(&value).map_err(|_| "The page's controls could not be read. Try again.".to_string())?;

        Ok((page.page, page.controls))
    };

    tokio::time::timeout(CDP_TIMEOUT, work).await.map_err(|_| "The page took too long to answer. Try again.".to_string())?
}

pub async fn act(pipes: &BrowserPipes, action: &BrowserAction) -> Result<(), String> {
    let work = async {
        let outcome = pipes.evaluate(&act_script(action)).await?;

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

    fn fake_browser(pipes: &BrowserPipes, answers: Vec<&'static str>) {
        let (pipe, ends) = CdpPipe::open().unwrap();
        pipes.lock().push(Arc::new(pipe));
        let mut requests = pipe::Receiver::from_owned_fd(ends.input).unwrap();
        let mut replies = pipe::Sender::from_owned_fd(ends.output).unwrap();

        tokio::spawn(async move {
            let mut answers = answers.into_iter();
            let mut buffer = Vec::new();
            let mut chunk = [0u8; 4096];

            loop {
                let Some(end) = buffer.iter().position(|byte| *byte == 0) else {
                    match requests.read(&mut chunk).await {
                        Ok(count) if count > 0 => buffer.extend_from_slice(&chunk[..count]),
                        _ => return,
                    }

                    continue;
                };
                let frame: Vec<u8> = buffer.drain(..=end).collect();
                let request: Value = serde_json::from_slice(&frame[..end]).unwrap();
                let result = match request["method"].as_str().unwrap() {
                    "Target.getTargets" => json!({"targetInfos": [{"type": "service_worker", "targetId": "w"}, {"type": "page", "targetId": "p"}]}),
                    "Target.attachToTarget" => json!({"sessionId": "s"}),
                    "Runtime.evaluate" => {
                        assert_eq!(request["sessionId"], "s");
                        json!({"result": {"type": "string", "value": answers.next().unwrap_or("")}})
                    }
                    _ => json!({}),
                };
                let mut reply = json!({"id": request["id"], "result": result}).to_string().into_bytes();
                reply.push(0);

                if replies.write_all(&reply).await.is_err() {
                    return;
                }
            }
        });
    }

    #[tokio::test]
    async fn reading_returns_the_pages_visible_controls_in_order() {
        let page = r#"{"page":"Book a flight (https://flights.example/)","controls":[{"index":1,"role":"textbox","name":"From","value":""},{"index":2,"role":"button","name":"Search flights","value":""}]}"#;
        let pipes = BrowserPipes::default();
        fake_browser(&pipes, vec![page]);

        let (title, controls) = read(&pipes).await.unwrap();

        assert_eq!(title, "Book a flight (https://flights.example/)");
        assert_eq!(controls.iter().map(|control| (control.index, control.name.as_str())).collect::<Vec<_>>(), [(1, "From"), (2, "Search flights")]);
    }

    #[tokio::test]
    async fn acting_on_a_control_the_page_no_longer_has_says_to_read_again() {
        let pipes = BrowserPipes::default();
        fake_browser(&pipes, vec!["missing"]);

        let outcome = act(&pipes, &BrowserAction::Click { index: 9 }).await;

        assert_eq!(outcome, Err("That control is no longer on the page. Read the page again.".to_string()));
    }

    #[tokio::test]
    async fn an_environment_without_a_browser_is_reported_plainly() {
        let outcome = read(&BrowserPipes::default()).await;

        assert_eq!(outcome.unwrap_err(), NO_BROWSER);
    }

    #[tokio::test]
    async fn a_browser_that_has_exited_is_dropped_and_the_next_one_answers() {
        let pipes = BrowserPipes::default();
        let page = r#"{"page":"Home (https://home.example/)","controls":[]}"#;
        fake_browser(&pipes, vec![page]);
        let (gone, ends) = CdpPipe::open().unwrap();
        drop(ends);
        pipes.lock().push(Arc::new(gone));

        let (title, _) = read(&pipes).await.unwrap();

        assert_eq!(title, "Home (https://home.example/)");
        assert_eq!(pipes.lock().len(), 1);
    }

    #[test]
    fn typed_text_is_quoted_so_it_cannot_leave_the_script() {
        let script = act_script(&BrowserAction::Type { index: 1, text: "a\"; alert(1); \"".to_string() });

        assert!(script.contains(r#"const text = "a\"; alert(1); \"";"#), "{script}");
    }
}
