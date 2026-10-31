use crate::apps::{self, DesktopApp};
use crate::browser_control;
use crate::rfb;
use crate::sway;
use crate::terminal::find_in_path;
use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use chrono::{DateTime, Utc};
use mycelium_proto::api::{EnvironmentCapability, EnvironmentController, EnvironmentState, EnvironmentVersions, RunningEnvironment};
use mycelium_proto::link::{BrowserAction, EnvironmentInputAction, NodeToControl};
use nix::sys::signal::{Signal, killpg};
use nix::unistd::Pid;
use std::collections::{HashMap, HashSet};
use std::ffi::{OsStr, OsString};
use std::os::unix::fs::DirBuilderExt;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::process::{Child, Command};
use tokio::sync::{mpsc, oneshot};
use uuid::Uuid;

const READY_TIMEOUT: Duration = Duration::from_secs(10);
const READY_POLL: Duration = Duration::from_millis(100);
const SHUTDOWN_WAIT: Duration = Duration::from_secs(5);
const SHUTDOWN_POLL: Duration = Duration::from_millis(50);
const CAGE_OUTPUT: &str = "HEADLESS-1";
const RESIZE_TIMEOUT: Duration = Duration::from_secs(3);
const VERSION_TIMEOUT: Duration = Duration::from_secs(3);
const SCREENSHOT_TIMEOUT: Duration = Duration::from_secs(15);
const BROWSERS: [&str; 4] = ["chromium", "chromium-browser", "google-chrome-stable", "google-chrome"];
const WINDOW_POLL: Duration = Duration::from_millis(1500);
const LAUNCH_WINDOW_WAIT: Duration = Duration::from_secs(5);
const LAUNCH_WINDOW_POLL: Duration = Duration::from_millis(200);
const PAGE_SIZE: u64 = 4096;
const NON_LINUX_NOTE: &str = "Environments need a Linux node";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompositorKind {
    Sway,
    Cage,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Compositor {
    pub kind: CompositorKind,
    pub program: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Tools {
    pub compositor: Option<Compositor>,
    pub wayvnc: Option<String>,
    pub grim: Option<String>,
    pub browser: Option<String>,
}

#[derive(Clone)]
struct SessionEnv {
    runtime_dir: PathBuf,
    display: String,
    debug_port: u16,
}

struct SessionDirectory {
    path: PathBuf,
}

struct Launched {
    children: Vec<Child>,
    pids: Vec<u32>,
    directory: SessionDirectory,
    session: SessionEnv,
    port: u16,
    browser: Option<String>,
}

struct Entry {
    generation: Uuid,
    started_at: DateTime<Utc>,
    ready: bool,
    port: u16,
    session: Option<SessionEnv>,
    browser: Option<String>,
    pids: Vec<u32>,
    apps: Vec<Child>,
    stop: Option<oneshot::Sender<()>>,
}

#[derive(Clone)]
pub struct EnvironmentManager {
    out: mpsc::UnboundedSender<NodeToControl>,
    path: OsString,
    data_dirs: Vec<PathBuf>,
    running: Arc<Mutex<HashMap<Uuid, Entry>>>,
    streams: Arc<Mutex<HashMap<Uuid, mpsc::UnboundedSender<Vec<u8>>>>>,
}

impl Tools {
    pub fn missing(&self) -> Vec<String> {
        let mut missing = Vec::new();

        if self.compositor.is_none() {
            missing.push("sway".to_string());
        }

        if self.wayvnc.is_none() {
            missing.push("wayvnc".to_string());
        }

        if self.grim.is_none() {
            missing.push("grim".to_string());
        }

        missing
    }
}

impl SessionDirectory {
    fn create() -> Result<SessionDirectory, String> {
        let path = std::env::temp_dir().join(format!("mycelium-environment-{}", Uuid::new_v4().simple()));
        let created = std::fs::DirBuilder::new().mode(0o700).create(&path);

        match created {
            Ok(()) => Ok(SessionDirectory { path }),
            Err(error) => Err(format!("The environment could not start. It needs a private folder under {} and could not make one: {error}.", std::env::temp_dir().display())),
        }
    }
}

impl Drop for SessionDirectory {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.path);
    }
}

impl Drop for Launched {
    fn drop(&mut self) {
        for pid in &self.pids {
            let _ = killpg(Pid::from_raw(*pid as i32), Signal::SIGKILL);
        }
    }
}

pub fn locate(path: &OsStr) -> Tools {
    let compositor = find_in_path("sway", path)
        .map(|program| Compositor { kind: CompositorKind::Sway, program })
        .or_else(|| find_in_path("cage", path).map(|program| Compositor { kind: CompositorKind::Cage, program }));

    Tools { compositor, wayvnc: find_in_path("wayvnc", path), grim: find_in_path("grim", path), browser: BROWSERS.iter().find_map(|name| find_in_path(name, path)) }
}

pub fn parse_version(output: &str, major_only: bool) -> Option<String> {
    let token = output.split_whitespace().find(|token| token.trim_start_matches('v').starts_with(|first: char| first.is_ascii_digit()))?;
    let number: String = token.trim_start_matches('v').chars().take_while(|character| character.is_ascii_digit() || *character == '.').collect();
    let number = number.trim_end_matches('.');

    if major_only {
        return number.split('.').next().map(str::to_string);
    }

    Some(number.to_string())
}

async fn version_of(program: &str, major_only: bool) -> Option<String> {
    let mut command = Command::new(program);
    command.arg("--version").stdin(Stdio::null()).stderr(Stdio::null()).kill_on_drop(true);
    let output = tokio::time::timeout(VERSION_TIMEOUT, command.output()).await.ok()?.ok()?;

    parse_version(&String::from_utf8_lossy(&output.stdout), major_only)
}

pub async fn probe(path: &OsStr, os: &str, data_dirs: &[PathBuf]) -> EnvironmentCapability {
    if os != "linux" {
        return EnvironmentCapability { available: false, missing: Vec::new(), versions: EnvironmentVersions::default(), note: Some(NON_LINUX_NOTE.to_string()), running: Vec::new(), apps: Vec::new() };
    }

    let tools = locate(path);
    let missing = tools.missing();
    let compositor = match &tools.compositor {
        Some(compositor) => version_of(&compositor.program, false).await,
        None => None,
    };
    let wayvnc = match &tools.wayvnc {
        Some(program) => version_of(program, false).await,
        None => None,
    };
    let browser = match &tools.browser {
        Some(program) => version_of(program, true).await,
        None => None,
    };

    EnvironmentCapability { available: missing.is_empty(), missing, versions: EnvironmentVersions { compositor, wayvnc, browser }, note: None, running: Vec::new(), apps: apps::discover(path, data_dirs).iter().map(DesktopApp::offer).collect() }
}

fn session_command(program: &str, runtime_dir: &Path, display: Option<&str>) -> Command {
    let mut command = Command::new(program);
    command
        .env_remove("DISPLAY")
        .env_remove("WAYLAND_DISPLAY")
        .env_remove("SWAYSOCK")
        .env_remove("I3SOCK")
        .env_remove("DBUS_SESSION_BUS_ADDRESS")
        .env("XDG_RUNTIME_DIR", runtime_dir)
        .env("XDG_CONFIG_HOME", runtime_dir.join("config"))
        .env("WLR_BACKENDS", "headless")
        .env("WLR_LIBINPUT_NO_DEVICES", "1")
        .env("WLR_RENDERER_ALLOW_SOFTWARE", "1")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .kill_on_drop(true)
        .process_group(0);

    if let Some(display) = display {
        command.env("WAYLAND_DISPLAY", display);
    }

    command
}

fn browser_arguments(runtime_dir: &Path, debug_port: u16) -> Vec<String> {
    vec![
        "--ozone-platform=wayland".to_string(),
        "--no-first-run".to_string(),
        "--disable-gpu".to_string(),
        "--start-maximized".to_string(),
        format!("--user-data-dir={}", runtime_dir.join("profile").display()),
        "--remote-debugging-address=127.0.0.1".to_string(),
        format!("--remote-debugging-port={debug_port}"),
    ]
}

fn sway_config(width: u32, height: u32) -> String {
    format!("output HEADLESS-1 resolution {width}x{height}\ndefault_border none\ndefault_floating_border none\n")
}

fn app_command(app: &DesktopApp, file: Option<&str>, session: &SessionEnv) -> Result<Command, String> {
    let words = app.command_with(file);
    let (Some(program), arguments) = (words.first(), words.get(1..).unwrap_or_default()) else {
        return Err(format!("{} has no command to run.", app.name));
    };
    let mut command = session_command(program, &session.runtime_dir, Some(&session.display));
    command.args(arguments);

    let chromium_based = Path::new(program).file_name().is_some_and(|name| BROWSERS.iter().any(|browser| name == *browser));

    if chromium_based {
        command.args(browser_arguments(&session.runtime_dir, session.debug_port));
    }

    Ok(command)
}

fn spawn_into(command: &mut Command, launched: &mut Launched, name: &str) -> Result<(), String> {
    match command.spawn() {
        Ok(child) => {
            launched.pids.extend(child.id());
            launched.children.push(child);

            Ok(())
        }
        Err(error) => Err(format!("{name} did not start. The node could not run it: {error}.")),
    }
}

fn find_socket(runtime_dir: &Path) -> Option<String> {
    let entries = std::fs::read_dir(runtime_dir).ok()?;

    entries.flatten().map(|entry| entry.file_name().to_string_lossy().into_owned()).find(|name| name.starts_with("wayland-") && !name.ends_with(".lock"))
}

fn exited(child: &mut Child) -> bool {
    !matches!(child.try_wait(), Ok(None))
}

async fn wait_for_socket(launched: &mut Launched, name: &str) -> Result<String, String> {
    let deadline = tokio::time::Instant::now() + READY_TIMEOUT;

    loop {
        if let Some(socket) = find_socket(&launched.directory.path) {
            return Ok(socket);
        }

        if exited(&mut launched.children[0]) || tokio::time::Instant::now() >= deadline {
            return Err(format!("{name} did not start. It stopped before its display was ready, so check that it can run headless on this node."));
        }

        tokio::time::sleep(READY_POLL).await;
    }
}

async fn wait_for_port(launched: &mut Launched) -> Result<(), String> {
    let deadline = tokio::time::Instant::now() + READY_TIMEOUT;
    let wayvnc = launched.children.len() - 1;

    loop {
        if TcpStream::connect(("127.0.0.1", launched.port)).await.is_ok() {
            return Ok(());
        }

        if exited(&mut launched.children[wayvnc]) || tokio::time::Instant::now() >= deadline {
            return Err("wayvnc did not start. It stopped before it could share the screen, so check that it supports this compositor.".to_string());
        }

        tokio::time::sleep(READY_POLL).await;
    }
}

fn free_loopback_port() -> Result<u16, String> {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").map_err(|error| format!("The environment could not start. The node found no free local port for wayvnc: {error}."))?;
    let address = listener.local_addr().map_err(|error| format!("The environment could not start. The node found no free local port for wayvnc: {error}."))?;

    Ok(address.port())
}

async fn launch(tools: &Tools, width: u32, height: u32, apps: &[DesktopApp]) -> Result<Launched, String> {
    let missing = tools.missing();

    if !missing.is_empty() {
        return Err(format!("This node is missing {}. Install them, then start the environment again.", missing.join(", ")));
    }

    let (Some(compositor), Some(wayvnc)) = (&tools.compositor, &tools.wayvnc) else {
        return Err("This node is missing tools the environment needs. Install them, then start the environment again.".to_string());
    };
    let directory = SessionDirectory::create()?;
    let runtime_dir = directory.path.clone();
    let port = free_loopback_port()?;
    let debug_port = free_loopback_port()?;
    let mut launched = Launched { children: Vec::new(), pids: Vec::new(), directory, session: SessionEnv { runtime_dir: runtime_dir.clone(), display: String::new(), debug_port }, port, browser: None };

    if let Some(browser) = &tools.browser {
        let browser_version = version_of(browser, true).await;
        launched.browser = Some(browser_version.map(|major| format!("Chromium {major}")).unwrap_or_else(|| "Chromium".to_string()));
    }

    let name = match compositor.kind {
        CompositorKind::Sway => "sway",
        CompositorKind::Cage => "cage",
    };
    let mut compositor_command = match compositor.kind {
        CompositorKind::Sway => {
            let config = runtime_dir.join("sway.conf");
            std::fs::write(&config, sway_config(width, height)).map_err(|error| format!("sway did not start. The node could not write its settings: {error}."))?;
            let mut command = session_command(&compositor.program, &runtime_dir, None);
            command.arg("-c").arg(&config);

            command
        }
        CompositorKind::Cage => {
            let [app] = apps else {
                return Err("This node runs cage, which shows one app at a time. Start with one app, or install sway.".to_string());
            };
            let session = SessionEnv { runtime_dir: runtime_dir.clone(), display: String::new(), debug_port };
            let process = app_command(app, None, &session)?;
            let mut command = session_command(&compositor.program, &runtime_dir, None);
            command.arg("--").arg(process.as_std().get_program()).args(process.as_std().get_args());

            command
        }
    };
    spawn_into(&mut compositor_command, &mut launched, name)?;
    launched.session.display = wait_for_socket(&mut launched, name).await?;

    if compositor.kind == CompositorKind::Cage {
        let mut resize = session_command("wlr-randr", &runtime_dir, Some(&launched.session.display));
        resize.arg("--output").arg(CAGE_OUTPUT).arg("--custom-mode").arg(format!("{width}x{height}"));
        let _ = tokio::time::timeout(RESIZE_TIMEOUT, resize.status()).await;
    }

    let mut wayvnc_command = session_command(wayvnc, &runtime_dir, Some(&launched.session.display));
    wayvnc_command.arg("127.0.0.1").arg(port.to_string());
    spawn_into(&mut wayvnc_command, &mut launched, "wayvnc")?;
    wait_for_port(&mut launched).await?;

    if compositor.kind == CompositorKind::Sway {
        for app in apps {
            let mut command = app_command(app, None, &launched.session)?;
            spawn_into(&mut command, &mut launched, &app.name)?;
        }
    }

    Ok(launched)
}

fn tree_memory_bytes(roots: &[u32]) -> Option<u64> {
    let mut pending = roots.to_vec();
    let mut seen = HashSet::new();
    let mut total = 0u64;
    let mut measured = false;

    while let Some(pid) = pending.pop() {
        if !seen.insert(pid) {
            continue;
        }

        let statm = std::fs::read_to_string(format!("/proc/{pid}/statm")).ok();
        let pages = statm.as_deref().and_then(|text| text.split_whitespace().nth(1)).and_then(|value| value.parse::<u64>().ok());

        if let Some(pages) = pages {
            total += pages * PAGE_SIZE;
            measured = true;
        }

        let Ok(tasks) = std::fs::read_dir(format!("/proc/{pid}/task")) else {
            continue;
        };

        for task in tasks.flatten() {
            let children = std::fs::read_to_string(task.path().join("children")).unwrap_or_default();
            pending.extend(children.split_whitespace().filter_map(|child| child.parse::<u32>().ok()));
        }
    }

    measured.then_some(total)
}

async fn capture(grim: Option<String>, session: Option<SessionEnv>) -> Result<String, String> {
    let (Some(grim), Some(session)) = (grim, session) else {
        return Err("There is no environment running for this conversation. Start one, then take the screenshot.".to_string());
    };
    let mut command = session_command(&grim, &session.runtime_dir, Some(&session.display));
    command.arg("-").stdout(Stdio::piped()).stderr(Stdio::piped());
    let output = tokio::time::timeout(SCREENSHOT_TIMEOUT, command.output()).await.map_err(|_| "The screenshot took too long. Try again.".to_string())?.map_err(|error| format!("grim could not run: {error}."))?;

    if !output.status.success() || output.stdout.is_empty() {
        return Err(format!("grim could not capture the screen: {}", String::from_utf8_lossy(&output.stderr).trim()));
    }

    Ok(STANDARD.encode(&output.stdout))
}

impl EnvironmentManager {
    pub fn new(out: mpsc::UnboundedSender<NodeToControl>, path: OsString) -> EnvironmentManager {
        EnvironmentManager { out, path, data_dirs: apps::data_dirs(), running: Arc::new(Mutex::new(HashMap::new())), streams: Arc::new(Mutex::new(HashMap::new())) }
    }

    fn entries(&self) -> MutexGuard<'_, HashMap<Uuid, Entry>> {
        self.running.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn stream_senders(&self) -> MutexGuard<'_, HashMap<Uuid, mpsc::UnboundedSender<Vec<u8>>>> {
        self.streams.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn send_changed(&self, conversation_id: Uuid, state: EnvironmentState, failure: Option<String>, browser: Option<String>) {
        let _ = self.out.send(NodeToControl::EnvironmentChanged { conversation_id, state, failure, browser });
    }

    fn ready_port(&self, conversation_id: Uuid) -> Option<u16> {
        self.entries().get(&conversation_id).filter(|entry| entry.ready).map(|entry| entry.port)
    }

    fn ready_session(&self, conversation_id: Uuid) -> Option<SessionEnv> {
        self.entries().get(&conversation_id).filter(|entry| entry.ready).and_then(|entry| entry.session.clone())
    }

    fn running_list(&self) -> Vec<RunningEnvironment> {
        self.entries()
            .iter()
            .filter(|(_, entry)| entry.ready)
            .map(|(conversation_id, entry)| RunningEnvironment { conversation_id: *conversation_id, controller: EnvironmentController::Agent, started_at: entry.started_at, memory_bytes: tree_memory_bytes(&entry.pids) })
            .collect()
    }

    pub fn start(&self, conversation_id: Uuid, width: u32, height: u32, apps: Vec<String>) {
        let generation = Uuid::new_v4();

        {
            let mut running = self.entries();

            if let Some(existing) = running.get(&conversation_id) {
                if existing.ready {
                    self.send_changed(conversation_id, EnvironmentState::Running, None, existing.browser.clone());
                }

                return;
            }

            running.insert(conversation_id, Entry { generation, started_at: Utc::now(), ready: false, port: 0, session: None, browser: None, pids: Vec::new(), apps: Vec::new(), stop: None });
        }

        self.send_changed(conversation_id, EnvironmentState::Starting, None, None);
        let manager = self.clone();

        tokio::spawn(async move { manager.run(conversation_id, generation, width, height, apps).await });
    }

    async fn run(&self, conversation_id: Uuid, generation: Uuid, width: u32, height: u32, chosen: Vec<String>) {
        let tools = locate(&self.path);
        let installed = apps::discover(&self.path, &self.data_dirs);
        let mut apps = Vec::new();

        for id in &chosen {
            let Some(app) = installed.iter().find(|app| &app.id == id) else {
                self.forget(conversation_id, generation);
                self.send_changed(conversation_id, EnvironmentState::Failed, Some(format!("{id} is not installed on this node. Start the environment again with the apps it offers.")), None);

                return;
            };

            apps.push(app.clone());
        }

        let launched = launch(&tools, width, height, &apps).await;
        let mut launched = match launched {
            Ok(launched) => launched,
            Err(failure) => {
                self.forget(conversation_id, generation);
                self.send_changed(conversation_id, EnvironmentState::Failed, Some(failure), None);

                return;
            }
        };
        let (stop, mut stopped) = oneshot::channel();

        {
            let mut running = self.entries();

            match running.get_mut(&conversation_id) {
                Some(entry) if entry.generation == generation => {
                    entry.ready = true;
                    entry.port = launched.port;
                    entry.session = Some(launched.session.clone());
                    entry.browser = launched.browser.clone();
                    entry.pids = launched.pids.clone();
                    entry.stop = Some(stop);
                }
                _ => return,
            }
        }

        self.send_changed(conversation_id, EnvironmentState::Running, None, launched.browser.clone());

        let swaymsg = find_in_path("swaymsg", &self.path).filter(|_| tools.compositor.as_ref().is_some_and(|compositor| compositor.kind == CompositorKind::Sway));
        let mut ticker = tokio::time::interval(WINDOW_POLL);
        let mut reported = None;

        loop {
            tokio::select! {
                _ = launched.children[0].wait() => break,
                _ = &mut stopped => break,
                _ = ticker.tick(), if swaymsg.is_some() => {
                    let Some(swaymsg) = &swaymsg else {
                        continue;
                    };
                    let listed = sway::windows(swaymsg, &launched.directory.path).await;

                    if let Ok(windows) = listed && reported.as_ref() != Some(&windows) {
                        reported = Some(windows.clone());
                        let _ = self.out.send(NodeToControl::EnvironmentWindows { conversation_id, windows });
                    }
                }
            }
        }

        drop(launched);
        self.forget(conversation_id, generation);
        self.send_changed(conversation_id, EnvironmentState::Stopped, None, None);
    }

    fn forget(&self, conversation_id: Uuid, generation: Uuid) {
        let mut running = self.entries();
        let current = running.get(&conversation_id).is_some_and(|entry| entry.generation == generation);

        if current {
            running.remove(&conversation_id);
        }
    }

    pub fn report(&self) {
        let manager = self.clone();

        tokio::spawn(async move {
            let mut capability = probe(&manager.path, std::env::consts::OS, &manager.data_dirs).await;
            capability.running = manager.running_list();
            let _ = manager.out.send(NodeToControl::EnvironmentCapability { capability });
        });
    }

    pub fn input(&self, request_id: Uuid, conversation_id: Uuid, action: EnvironmentInputAction) {
        let port = self.ready_port(conversation_id);
        let out = self.out.clone();

        tokio::spawn(async move {
            let result = match port {
                Some(port) => rfb::send_input(port, &action).await.map_err(|error| error.to_string()),
                None => Err("There is no environment running for this conversation. Start one, then send the input.".to_string()),
            };
            let _ = out.send(NodeToControl::EnvironmentInputDone { request_id, error: result.err() });
        });
    }

    pub fn screenshot(&self, request_id: Uuid, conversation_id: Uuid) {
        let grim = find_in_path("grim", &self.path);
        let session = self.ready_session(conversation_id);
        let out = self.out.clone();

        tokio::spawn(async move {
            let (png, error) = match capture(grim, session).await {
                Ok(png) => (Some(png), None),
                Err(error) => (None, Some(error)),
            };
            let _ = out.send(NodeToControl::EnvironmentScreenshotTaken { request_id, png, error });
        });
    }

    pub fn browser_read(&self, request_id: Uuid, conversation_id: Uuid) {
        let debug_port = self.ready_session(conversation_id).map(|session| session.debug_port);
        let out = self.out.clone();

        tokio::spawn(async move {
            let read = match debug_port {
                Some(debug_port) => browser_control::read(debug_port).await,
                None => Err("There is no environment running for this conversation. Start one, then read the page.".to_string()),
            };
            let message = match read {
                Ok((page, controls)) => NodeToControl::EnvironmentBrowserControls { request_id, page, controls, error: None },
                Err(error) => NodeToControl::EnvironmentBrowserControls { request_id, page: String::new(), controls: Vec::new(), error: Some(error) },
            };
            let _ = out.send(message);
        });
    }

    pub fn browser_act(&self, request_id: Uuid, conversation_id: Uuid, action: BrowserAction) {
        let debug_port = self.ready_session(conversation_id).map(|session| session.debug_port);
        let out = self.out.clone();

        tokio::spawn(async move {
            let result = match debug_port {
                Some(debug_port) => browser_control::act(debug_port, &action).await,
                None => Err("There is no environment running for this conversation. Start one, then act on the page.".to_string()),
            };
            let _ = out.send(NodeToControl::EnvironmentBrowserActed { request_id, error: result.err() });
        });
    }

    pub fn launch(&self, request_id: Uuid, conversation_id: Uuid, app: String, file: Option<String>) {
        let manager = self.clone();

        tokio::spawn(async move {
            let launched = manager.launch_app(conversation_id, &app, file.as_deref()).await;
            let (window, error) = match launched {
                Ok(window) => (window, None),
                Err(error) => (None, Some(error)),
            };
            let _ = manager.out.send(NodeToControl::EnvironmentLaunched { request_id, window, error });
        });
    }

    async fn launch_app(&self, conversation_id: Uuid, id: &str, file: Option<&str>) -> Result<Option<i64>, String> {
        let Some(session) = self.ready_session(conversation_id) else {
            return Err("There is no environment running for this conversation. Start one, then open the app.".to_string());
        };
        let installed = apps::discover(&self.path, &self.data_dirs);
        let Some(app) = installed.iter().find(|app| app.id == id) else {
            return Err(format!("{id} is not installed on this node."));
        };
        let swaymsg = find_in_path("swaymsg", &self.path);
        let before = match &swaymsg {
            Some(swaymsg) => sway::windows(swaymsg, &session.runtime_dir).await.unwrap_or_default(),
            None => Vec::new(),
        };
        let mut command = app_command(app, file, &session)?;
        let child = command.spawn().map_err(|error| format!("{} did not start. The node could not run it: {error}.", app.name))?;

        if let Some(entry) = self.entries().get_mut(&conversation_id) {
            entry.apps.push(child);
        }

        let Some(swaymsg) = swaymsg else {
            return Ok(None);
        };
        let deadline = tokio::time::Instant::now() + LAUNCH_WINDOW_WAIT;

        while tokio::time::Instant::now() < deadline {
            tokio::time::sleep(LAUNCH_WINDOW_POLL).await;

            let now = sway::windows(&swaymsg, &session.runtime_dir).await.unwrap_or_default();
            let opened = now.iter().find(|window| !before.iter().any(|earlier| earlier.id == window.id));

            if let Some(opened) = opened {
                sway::focus(&swaymsg, &session.runtime_dir, opened.id).await?;

                return Ok(Some(opened.id));
            }
        }

        Ok(None)
    }

    pub fn focus(&self, request_id: Uuid, conversation_id: Uuid, window: i64) {
        let session = self.ready_session(conversation_id);
        let swaymsg = find_in_path("swaymsg", &self.path);
        let out = self.out.clone();

        tokio::spawn(async move {
            let result = match (swaymsg, session) {
                (Some(swaymsg), Some(session)) => sway::focus(&swaymsg, &session.runtime_dir, window).await,
                _ => Err("There is no environment running for this conversation. Start one, then switch window.".to_string()),
            };
            let _ = out.send(NodeToControl::EnvironmentInputDone { request_id, error: result.err() });
        });
    }

    pub fn open_stream(&self, stream_id: Uuid, conversation_id: Uuid) {
        let Some(port) = self.ready_port(conversation_id) else {
            let _ = self.out.send(NodeToControl::EnvironmentStreamClosed { stream_id });

            return;
        };
        let (input, mut queued) = mpsc::unbounded_channel::<Vec<u8>>();
        self.stream_senders().insert(stream_id, input);
        let manager = self.clone();

        tokio::spawn(async move {
            let Ok(connection) = TcpStream::connect(("127.0.0.1", port)).await else {
                manager.stream_senders().remove(&stream_id);
                let _ = manager.out.send(NodeToControl::EnvironmentStreamClosed { stream_id });

                return;
            };
            let (mut reader, mut writer) = connection.into_split();
            let mut buffer = vec![0u8; 16 * 1024];

            loop {
                tokio::select! {
                    read = reader.read(&mut buffer) => match read {
                        Ok(count) if count > 0 => {
                            let data = STANDARD.encode(&buffer[..count]);
                            let _ = manager.out.send(NodeToControl::EnvironmentStreamData { stream_id, data });
                        }
                        _ => {
                            let _ = manager.out.send(NodeToControl::EnvironmentStreamClosed { stream_id });

                            break;
                        }
                    },
                    bytes = queued.recv() => match bytes {
                        Some(bytes) => {
                            if writer.write_all(&bytes).await.is_err() {
                                let _ = manager.out.send(NodeToControl::EnvironmentStreamClosed { stream_id });

                                break;
                            }
                        }
                        None => break,
                    },
                }
            }

            manager.stream_senders().remove(&stream_id);
        });
    }

    pub fn stream_input(&self, stream_id: Uuid, data: &str) {
        let Ok(bytes) = STANDARD.decode(data) else {
            return;
        };

        if let Some(input) = self.stream_senders().get(&stream_id) {
            let _ = input.send(bytes);
        }
    }

    pub fn close_stream(&self, stream_id: Uuid) {
        self.stream_senders().remove(&stream_id);
    }

    pub fn stop(&self, conversation_id: Uuid) {
        let mut running = self.entries();
        let stop = running.get_mut(&conversation_id).and_then(|entry| entry.stop.take());

        match stop {
            Some(stop) => {
                let _ = stop.send(());
            }
            None => {
                running.remove(&conversation_id);
                self.send_changed(conversation_id, EnvironmentState::Stopped, None, None);
            }
        }
    }

    pub async fn stop_all(&self) {
        let conversations: Vec<Uuid> = self.entries().keys().copied().collect();

        for conversation_id in conversations {
            self.stop(conversation_id);
        }

        let deadline = tokio::time::Instant::now() + SHUTDOWN_WAIT;

        while !self.entries().is_empty() && tokio::time::Instant::now() < deadline {
            tokio::time::sleep(SHUTDOWN_POLL).await;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    fn stub(directory: &Path, name: &str, output: &str) {
        let file = directory.join(name);
        std::fs::write(&file, format!("#!/bin/sh\necho '{output}'\n")).unwrap();
        std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o755)).unwrap();
    }

    fn stocked_path() -> (tempfile::TempDir, OsString) {
        let directory = tempfile::tempdir().unwrap();
        stub(directory.path(), "sway", "sway version 1.10");
        stub(directory.path(), "wayvnc", "wayvnc: 0.9.1");
        stub(directory.path(), "grim", "grim 1.4");
        stub(directory.path(), "chromium", "Chromium 141.0.7390.54 snap");
        let path = directory.path().as_os_str().to_os_string();

        (directory, path)
    }

    #[tokio::test]
    async fn a_node_with_every_tool_is_available_and_reports_versions() {
        let (_directory, path) = stocked_path();

        let capability = probe(&path, "linux", &[]).await;

        assert!(capability.available);
        assert!(capability.missing.is_empty());
        assert_eq!(capability.versions.compositor.as_deref(), Some("1.10"));
        assert_eq!(capability.versions.wayvnc.as_deref(), Some("0.9.1"));
        assert_eq!(capability.versions.browser.as_deref(), Some("141"));
    }

    #[tokio::test]
    async fn an_empty_path_lists_every_tool_as_missing_in_order() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().as_os_str().to_os_string();

        let capability = probe(&path, "linux", &[]).await;

        assert!(!capability.available);
        assert_eq!(capability.missing, vec!["sway", "wayvnc", "grim"]);
        assert_eq!(capability.versions, EnvironmentVersions::default());
    }

    #[tokio::test]
    async fn one_absent_tool_is_the_only_one_listed() {
        let (directory, path) = stocked_path();
        std::fs::remove_file(directory.path().join("grim")).unwrap();

        let capability = probe(&path, "linux", &[]).await;

        assert!(!capability.available);
        assert_eq!(capability.missing, vec!["grim"]);
    }

    #[tokio::test]
    async fn cage_stands_in_for_sway() {
        let (directory, path) = stocked_path();
        std::fs::remove_file(directory.path().join("sway")).unwrap();
        stub(directory.path(), "cage", "cage version 0.2.0");

        let capability = probe(&path, "linux", &[]).await;

        assert!(capability.available);
        assert_eq!(capability.versions.compositor.as_deref(), Some("0.2.0"));
    }

    #[tokio::test]
    async fn a_node_without_a_browser_is_available() {
        let (directory, path) = stocked_path();
        std::fs::remove_file(directory.path().join("chromium")).unwrap();

        let capability = probe(&path, "linux", &[]).await;

        assert!(capability.available);
        assert!(capability.missing.is_empty());
        assert_eq!(capability.versions.browser, None);
    }

    #[tokio::test]
    async fn other_browsers_satisfy_the_browser_requirement() {
        let (directory, path) = stocked_path();
        std::fs::remove_file(directory.path().join("chromium")).unwrap();
        stub(directory.path(), "google-chrome-stable", "Google Chrome 130.0.1");

        let capability = probe(&path, "linux", &[]).await;

        assert!(capability.available);
        assert_eq!(capability.versions.browser.as_deref(), Some("130"));
    }

    #[tokio::test]
    async fn a_tool_that_fails_its_version_check_is_still_present() {
        let (directory, path) = stocked_path();
        stub(directory.path(), "wayvnc", "no number here");

        let capability = probe(&path, "linux", &[]).await;

        assert!(capability.available);
        assert_eq!(capability.versions.wayvnc, None);
    }

    #[tokio::test]
    async fn a_node_that_is_not_linux_is_unavailable_with_a_note() {
        let (_directory, path) = stocked_path();

        let capability = probe(&path, "macos", &[]).await;

        assert!(!capability.available);
        assert!(capability.missing.is_empty());
        assert_eq!(capability.note.as_deref(), Some("Environments need a Linux node"));
    }

    #[test]
    fn versions_are_reduced_to_the_bare_number() {
        assert_eq!(parse_version("sway version 1.10", false).as_deref(), Some("1.10"));
        assert_eq!(parse_version("wayvnc: 0.9.1", false).as_deref(), Some("0.9.1"));
        assert_eq!(parse_version("sway version 1.10-dirty", false).as_deref(), Some("1.10"));
        assert_eq!(parse_version("Chromium 141.0.7390.54", true).as_deref(), Some("141"));
        assert_eq!(parse_version("nothing", false), None);
    }

    #[test]
    fn the_sway_settings_set_the_requested_headless_resolution() {
        assert!(sway_config(1280, 800).starts_with("output HEADLESS-1 resolution 1280x800\n"));
    }

    #[tokio::test]
    async fn a_stream_for_an_unknown_conversation_closes_at_once() {
        let (out, mut messages) = mpsc::unbounded_channel();
        let manager = EnvironmentManager::new(out, OsString::new());
        let stream_id = Uuid::new_v4();

        manager.open_stream(stream_id, Uuid::new_v4());

        assert_eq!(messages.recv().await, Some(NodeToControl::EnvironmentStreamClosed { stream_id }));
    }

    #[tokio::test]
    async fn starting_without_the_tools_fails_naming_what_is_missing() {
        let directory = tempfile::tempdir().unwrap();
        let (out, mut messages) = mpsc::unbounded_channel();
        let manager = EnvironmentManager::new(out, directory.path().as_os_str().to_os_string());
        let conversation_id = Uuid::new_v4();

        manager.start(conversation_id, 1280, 800, Vec::new());

        assert!(matches!(messages.recv().await, Some(NodeToControl::EnvironmentChanged { state: EnvironmentState::Starting, .. })));
        let Some(NodeToControl::EnvironmentChanged { state, failure, .. }) = messages.recv().await else {
            panic!("expected a state change");
        };
        assert_eq!(state, EnvironmentState::Failed);
        assert!(failure.unwrap().contains("sway, wayvnc, grim"));
    }
}
