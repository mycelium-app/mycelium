use mycelium_keeper::{run, socket_path};
use serde_json::{Value, json};
use std::path::Path;
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader, Lines};
use tokio::net::UnixStream;
use tokio::net::unix::{OwnedReadHalf, OwnedWriteHalf};

struct Client {
    lines: Lines<BufReader<OwnedReadHalf>>,
    writer: OwnedWriteHalf,
}

impl Client {
    async fn attach(dir: &Path, after: u64) -> Client {
        let mut stream = None;
        for _ in 0..100 {
            if let Ok(connected) = UnixStream::connect(socket_path(dir)).await {
                stream = Some(connected);
                break;
            }

            tokio::time::sleep(Duration::from_millis(20)).await;
        }

        let (read, mut writer) = stream.expect("keeper socket").into_split();
        writer.write_all(format!("{}\n", json!({"attach": {"after": after}})).as_bytes()).await.unwrap();

        Client { lines: BufReader::new(read).lines(), writer }
    }

    async fn next(&mut self) -> Option<Value> {
        let line = tokio::time::timeout(Duration::from_secs(5), self.lines.next_line()).await.expect("keeper went quiet").unwrap()?;

        Some(serde_json::from_str(&line).unwrap())
    }

    async fn send(&mut self, frame: Value) {
        self.writer.write_all(format!("{}\n", json!({"send": frame})).as_bytes()).await.unwrap();
    }
}

fn start(dir: &Path, script: &str) -> tokio::task::JoinHandle<anyhow::Result<()>> {
    tokio::spawn(run(dir.to_path_buf(), vec!["sh".into(), "-c".into(), script.into()]))
}

#[tokio::test]
async fn journals_both_directions_with_increasing_seq() {
    let dir = tempfile::tempdir().unwrap();
    let _keeper = start(dir.path(), "cat");

    let mut client = Client::attach(dir.path(), 0).await;
    assert_eq!(client.next().await.unwrap(), json!({"live": true}));

    client.send(json!({"id": 1})).await;

    assert_eq!(client.next().await.unwrap(), json!({"seq": 1, "dir": "out", "frame": {"id": 1}}));
    assert_eq!(client.next().await.unwrap(), json!({"seq": 2, "dir": "in", "frame": {"id": 1}}));

    let journal = std::fs::read_to_string(dir.path().join("journal.jsonl")).unwrap();
    assert_eq!(journal.lines().count(), 2);
}

#[tokio::test]
async fn reattach_replays_only_entries_after_the_given_seq_then_goes_live() {
    let dir = tempfile::tempdir().unwrap();
    let _keeper = start(dir.path(), "cat");

    let mut first = Client::attach(dir.path(), 0).await;
    first.next().await.unwrap();
    first.send(json!({"n": 1})).await;
    first.next().await.unwrap();
    first.next().await.unwrap();
    drop(first);

    let mut second = Client::attach(dir.path(), 1).await;
    assert_eq!(second.next().await.unwrap()["seq"], 2);
    assert_eq!(second.next().await.unwrap(), json!({"live": true}));

    second.send(json!({"n": 2})).await;
    assert_eq!(second.next().await.unwrap()["seq"], 3);
    assert_eq!(second.next().await.unwrap()["seq"], 4);
}

#[tokio::test]
async fn a_new_attach_replaces_the_old_client() {
    let dir = tempfile::tempdir().unwrap();
    let _keeper = start(dir.path(), "cat");

    let mut old = Client::attach(dir.path(), 0).await;
    assert_eq!(old.next().await.unwrap(), json!({"live": true}));

    let mut new = Client::attach(dir.path(), 0).await;
    assert_eq!(new.next().await.unwrap(), json!({"live": true}));

    assert!(old.next().await.is_none());
}

#[tokio::test]
async fn agent_exit_writes_exit_json_and_tells_the_client() {
    let dir = tempfile::tempdir().unwrap();
    let keeper = start(dir.path(), "read line; echo '{\"done\":true}'; exit 3");

    let mut client = Client::attach(dir.path(), 0).await;
    client.next().await.unwrap();
    client.send(json!({"go": true})).await;

    assert_eq!(client.next().await.unwrap()["dir"], "out");
    assert_eq!(client.next().await.unwrap(), json!({"seq": 2, "dir": "in", "frame": {"done": true}}));
    assert_eq!(client.next().await.unwrap(), json!({"exit": 3}));

    keeper.await.unwrap().unwrap();
    let exit = std::fs::read_to_string(dir.path().join("exit.json")).unwrap();
    assert_eq!(serde_json::from_str::<Value>(&exit).unwrap(), json!({"exit": 3}));
    assert!(!socket_path(dir.path()).exists());
}

#[tokio::test]
async fn lines_that_are_not_json_are_not_journaled() {
    let dir = tempfile::tempdir().unwrap();
    let _keeper = start(dir.path(), "read line; echo noise; echo '{\"ok\":1}'; cat");

    let mut client = Client::attach(dir.path(), 0).await;
    client.next().await.unwrap();
    client.send(json!({"go": true})).await;

    client.next().await.unwrap();
    assert_eq!(client.next().await.unwrap(), json!({"seq": 2, "dir": "in", "frame": {"ok": 1}}));
}
