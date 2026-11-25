use anyhow::{Context, Result, bail};
use serde_json::{Value, json};
use std::fs::OpenOptions;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
#[cfg(unix)]
use tokio::net::{UnixListener, UnixStream};
#[cfg(windows)]
pub mod windows;

#[cfg(unix)]
pub type Client = UnixStream;
#[cfg(windows)]
pub type Client = tokio::net::windows::named_pipe::NamedPipeClient;

pub async fn connect(dir: &Path) -> std::io::Result<Client> {
    #[cfg(unix)]
    return UnixStream::connect(socket_path(dir)).await;

    #[cfg(windows)]
    {
        let marker = std::fs::read_to_string(socket_path(dir))?;
        let id = uuid::Uuid::parse_str(&marker).map_err(|error| std::io::Error::new(std::io::ErrorKind::InvalidData, error))?;

        tokio::net::windows::named_pipe::ClientOptions::new().open(windows::pipe_name(id))
    }
}
use tokio::process::Command;
use tokio::sync::{mpsc, watch};

const SESSION_MARK: &str = "MYCELIUM_KEEPER_SESSION";
#[cfg(target_os = "linux")]
const STRAY_GRACE_STEPS: u32 = 20;
#[cfg(target_os = "linux")]
const STRAY_GRACE_STEP: std::time::Duration = std::time::Duration::from_millis(100);
const EXIT_DRAIN_TIMEOUT: Duration = Duration::from_secs(2);

struct Journal {
    written: usize,
    file: std::fs::File,
}

struct Shared {
    journal: Mutex<Journal>,
    journal_path: PathBuf,
    head: watch::Sender<usize>,
    exit_code: watch::Sender<Option<i32>>,
    generation: watch::Sender<u64>,
    attached: watch::Sender<usize>,
    close: mpsc::UnboundedSender<()>,
}

impl Shared {
    fn append(&self, direction: &str, frame: Value) -> Result<()> {
        let mut journal = self.journal.lock().map_err(|_| anyhow::anyhow!("journal lock poisoned"))?;
        let seq = journal.written + 1;
        let line = json!({"seq": seq, "dir": direction, "frame": frame}).to_string();
        writeln!(journal.file, "{line}")?;
        journal.file.sync_data()?;
        journal.written = seq;
        self.head.send_replace(seq);

        Ok(())
    }
}

pub async fn run(dir: PathBuf, command: Vec<String>) -> Result<()> {
    let Some((program, args)) = command.split_first() else {
        bail!("Give the agent command after --.");
    };

    std::fs::create_dir_all(&dir).with_context(|| format!("create {}", dir.display()))?;
    let dir = dir.canonicalize().with_context(|| format!("resolve {}", dir.display()))?;
    let _session = hold_session(&dir)?;
    stop_strays(&dir).await;
    let socket_path = socket_path(&dir);
    let _ = std::fs::remove_file(&socket_path);
    let _ = std::fs::remove_file(dir.join("exit.json"));
    #[cfg(unix)]
    let listener = UnixListener::bind(&socket_path).with_context(|| format!("bind {}", socket_path.display()))?;

    #[cfg(windows)]
    let (mut listener, pipe_name, pipe_id) = {
        let id = uuid::Uuid::new_v4();
        let name = windows::pipe_name(id);
        let listener = windows::listen(&name, true)?;
        (listener, name, id)
    };

    let journal_path = dir.join("journal.jsonl");
    let journal_file = OpenOptions::new().create(true).write(true).truncate(true).open(&journal_path)?;
    let stderr_file = OpenOptions::new().create(true).append(true).open(dir.join("stderr.log"))?;
    let (close, mut close_requests) = mpsc::unbounded_channel::<()>();
    let shared = Arc::new(Shared {
        journal: Mutex::new(Journal { written: 0, file: journal_file }),
        journal_path,
        head: watch::channel(0).0,
        exit_code: watch::channel(None).0,
        generation: watch::channel(0).0,
        attached: watch::channel(0).0,
        close,
    });

    #[cfg(unix)]
    let mut agent = Command::new(program);
    #[cfg(windows)]
    let mut agent = {
        let mut command = Command::new(std::env::current_exe()?);
        command.args(["keeper-child", "--", program]);
        command.kill_on_drop(true);
        command
    };
    agent.env(SESSION_MARK, &dir);
    stop_with_keeper(&mut agent);
    let mut child = agent
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::from(stderr_file))
        .spawn()
        .with_context(|| format!("start {program}"))?;
    let mut child_stdin = child.stdin.take().context("agent stdin")?;
    #[cfg(windows)]
    let job = {
        let job = windows::Job::new(&child)?;
        child_stdin.write_all(&[1]).await?;
        std::fs::write(&socket_path, pipe_id.to_string())?;
        job
    };
    let child_stdout = child.stdout.take().context("agent stdout")?;

    let (frames_to_agent, mut frames_from_clients) = mpsc::unbounded_channel::<Value>();

    let writer_shared = shared.clone();
    tokio::spawn(async move {
        while let Some(frame) = frames_from_clients.recv().await {
            if writer_shared.append("out", frame.clone()).is_err() {
                break;
            }

            let line = format!("{frame}\n");
            if child_stdin.write_all(line.as_bytes()).await.is_err() || child_stdin.flush().await.is_err() {
                break;
            }
        }
    });

    let reader_shared = shared.clone();
    let reader = tokio::spawn(async move {
        let mut lines = BufReader::new(child_stdout).lines();
        while let Ok(Some(line)) = lines.next_line().await {
            let Ok(frame) = serde_json::from_str::<Value>(&line) else {
                tracing::warn!("agent wrote a line that is not JSON");
                continue;
            };

            if reader_shared.append("in", frame).is_err() {
                break;
            }
        }
    });

    let accept_shared = shared.clone();
    let accept = tokio::spawn(async move {
        loop {
            #[cfg(unix)]
            let Ok((stream, _)) = listener.accept().await else { break };
            #[cfg(windows)]
            let stream = {
                if listener.connect().await.is_err() {
                    break;
                }

                match windows::listen(&pipe_name, false) {
                    Ok(next) => std::mem::replace(&mut listener, next),
                    Err(error) => {
                        tracing::warn!("could not listen for keeper clients: {error}");
                        break;
                    }
                }
            };
            let client_shared = accept_shared.clone();
            let frames = frames_to_agent.clone();
            tokio::spawn(async move {
                client_shared.attached.send_modify(|count| *count += 1);
                let _ = serve_client(stream, &client_shared, frames).await;
                client_shared.attached.send_modify(|count| *count -= 1);
            });
        }
    });

    let status = tokio::select! {
        status = child.wait() => status?,
        _ = close_requests.recv() => {
            #[cfg(unix)]
            child.start_kill()?;
            #[cfg(windows)]
            job.terminate()?;
            child.wait().await?
        }
    };
    #[cfg(windows)]
    drop(job);
    stop_strays(&dir).await;
    let _ = tokio::time::timeout(EXIT_DRAIN_TIMEOUT, reader).await;
    let code = status.code().unwrap_or(-1);
    std::fs::write(dir.join("exit.json"), json!({"exit": code}).to_string())?;
    shared.exit_code.send_replace(Some(code));

    let mut attached = shared.attached.subscribe();
    let _ = tokio::time::timeout(EXIT_DRAIN_TIMEOUT, attached.wait_for(|count| *count == 0)).await;

    accept.abort();
    let _ = std::fs::remove_file(&socket_path);

    Ok(())
}

async fn serve_client(stream: impl tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin, shared: &Shared, frames_to_agent: mpsc::UnboundedSender<Value>) -> Result<()> {
    let (read_half, mut write_half) = tokio::io::split(stream);
    let mut lines = BufReader::new(read_half).lines();

    let first = lines.next_line().await?.context("client closed before attaching")?;
    let attach: Value = serde_json::from_str(&first)?;
    let mut cursor = attach["attach"]["after"].as_u64().context("first message must be an attach")? as usize;

    let mut generation = 0;
    shared.generation.send_modify(|current| {
        *current += 1;
        generation = *current;
    });

    let mut head = shared.head.subscribe();
    let mut replaced = shared.generation.subscribe();
    let mut exit_code = shared.exit_code.subscribe();
    let mut live_marker_sent = false;
    cursor = cursor.min(*head.borrow());
    let mut journal = BufReader::new(tokio::fs::File::open(&shared.journal_path).await?).lines();
    for _ in 0..cursor {
        journal.next_line().await?;
    }

    loop {
        let written = *head.borrow();
        while cursor < written {
            let Some(line) = journal.next_line().await? else {
                break;
            };

            write_half.write_all(line.as_bytes()).await?;
            write_half.write_all(b"\n").await?;
            cursor += 1;
        }

        if !live_marker_sent {
            write_half.write_all(b"{\"live\":true}\n").await?;
            live_marker_sent = true;
        }

        let exited = *exit_code.borrow();
        if let Some(code) = exited
            && cursor >= *head.borrow()
        {
            write_half.write_all(format!("{}\n", json!({"exit": code})).as_bytes()).await?;

            return Ok(());
        }

        tokio::select! {
            _ = head.changed() => {}
            _ = exit_code.changed() => {}
            _ = replaced.changed() => {
                if *replaced.borrow() != generation {
                    return Ok(());
                }
            }
            line = lines.next_line() => {
                let Some(line) = line? else {
                    return Ok(());
                };

                let message: Value = serde_json::from_str(&line)?;
                if let Some(frame) = message.get("send") {
                    let _ = frames_to_agent.send(frame.clone());
                }

                if message.get("close").is_some() {
                    let _ = shared.close.send(());
                }
            }
        }
    }
}

fn hold_session(dir: &Path) -> Result<std::fs::File> {
    let lock = OpenOptions::new().create(true).truncate(false).read(true).write(true).open(dir.join("keeper.lock"))?;
    match lock.try_lock() {
        Ok(()) => {}
        Err(std::fs::TryLockError::WouldBlock) => bail!("Another keeper already runs the agent for {}.", dir.display()),
        Err(std::fs::TryLockError::Error(error)) => return Err(error.into()),
    }

    Ok(lock)
}

#[cfg(target_os = "linux")]
async fn stop_strays(dir: &Path) {
    let mark = format!("{SESSION_MARK}={}", dir.display());
    let strays = marked_processes(&mark);

    for pid in &strays {
        unsafe { libc::kill(*pid, libc::SIGTERM) };
    }

    for _ in 0..STRAY_GRACE_STEPS {
        if strays.iter().all(|pid| !running(*pid)) {
            return;
        }

        tokio::time::sleep(STRAY_GRACE_STEP).await;
    }

    for pid in strays.iter().filter(|pid| running(**pid)) {
        unsafe { libc::kill(*pid, libc::SIGKILL) };
    }
}

#[cfg(target_os = "linux")]
fn marked_processes(mark: &str) -> Vec<libc::pid_t> {
    let me = std::process::id() as libc::pid_t;
    let Ok(entries) = std::fs::read_dir("/proc") else {
        return Vec::new();
    };

    entries
        .filter_map(|entry| entry.ok()?.file_name().to_str()?.parse::<libc::pid_t>().ok())
        .filter(|pid| *pid != me)
        .filter(|pid| std::fs::read(format!("/proc/{pid}/environ")).is_ok_and(|environ| environ.split(|byte| *byte == 0).any(|entry| entry == mark.as_bytes())))
        .collect()
}

#[cfg(target_os = "linux")]
fn running(pid: libc::pid_t) -> bool {
    std::fs::read_to_string(format!("/proc/{pid}/stat")).is_ok_and(|stat| stat.rsplit(')').next().is_some_and(|rest| !rest.trim_start().starts_with('Z')))
}

#[cfg(not(target_os = "linux"))]
async fn stop_strays(_dir: &Path) {}

#[cfg(target_os = "linux")]
fn stop_with_keeper(agent: &mut Command) {
    let keeper = std::process::id() as libc::pid_t;

    unsafe {
        agent.pre_exec(move || {
            if libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGTERM) != 0 {
                return Err(std::io::Error::last_os_error());
            }

            if libc::getppid() != keeper {
                libc::_exit(1);
            }

            Ok(())
        });
    }
}

#[cfg(not(target_os = "linux"))]
fn stop_with_keeper(_agent: &mut Command) {}

pub fn socket_path(dir: &Path) -> PathBuf {
    dir.join("keeper.sock")
}
