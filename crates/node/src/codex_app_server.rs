use anyhow::{Result, anyhow, bail};
use chrono::{DateTime, Utc};
use mycelium_proto::conversation::{ResetCredit, ResetCredits};
use serde_json::{Value, json};
use std::path::Path;
use std::process::Stdio;
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::Command;

const APP_SERVER_TIMEOUT: Duration = Duration::from_secs(15);

pub async fn read_reset_credits(codex: &str, account_dir: &Path) -> Option<ResetCredits> {
    let result = request(codex, account_dir, "account/rateLimits/read", Value::Null).await.ok()?;

    parse_reset_credits(&result)
}

pub async fn use_reset(codex: &str, account_dir: &Path, idempotency_key: &str) -> Result<String> {
    let result = request(codex, account_dir, "account/rateLimitResetCredit/consume", json!({"idempotencyKey": idempotency_key})).await?;

    result["outcome"].as_str().map(str::to_string).ok_or_else(|| anyhow!("Codex gave no outcome."))
}

fn parse_reset_credits(result: &Value) -> Option<ResetCredits> {
    let body = result["rateLimitResetCredits"].as_object()?;
    let available_count = u32::try_from(body.get("availableCount")?.as_u64()?).ok()?;
    let credits = body.get("credits").and_then(Value::as_array).map(|rows| {
        rows.iter()
            .filter(|row| row["status"].as_str() == Some("available"))
            .filter_map(|row| {
                let expires_at = row["expiresAt"].as_str().and_then(|text| text.parse::<DateTime<Utc>>().ok());

                Some(ResetCredit { id: row["id"].as_str()?.to_string(), expires_at, title: row["title"].as_str().map(str::to_string) })
            })
            .collect()
    });

    Some(ResetCredits { available_count, credits })
}

async fn request(codex: &str, account_dir: &Path, method: &str, params: Value) -> Result<Value> {
    let exchange = exchange(codex, account_dir, method, params);

    tokio::time::timeout(APP_SERVER_TIMEOUT, exchange).await.map_err(|_| anyhow!("Codex did not answer."))?
}

async fn exchange(codex: &str, account_dir: &Path, method: &str, params: Value) -> Result<Value> {
    let mut child = Command::new(codex)
        .arg("app-server")
        .env("CODEX_HOME", account_dir)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .kill_on_drop(true)
        .spawn()
        .map_err(|error| anyhow!("Codex could not start: {error}"))?;
    let mut stdin = child.stdin.take().ok_or_else(|| anyhow!("Codex has no input."))?;
    let mut lines = BufReader::new(child.stdout.take().ok_or_else(|| anyhow!("Codex has no output."))?).lines();
    let initialize = json!({"id": 1, "method": "initialize", "params": {"clientInfo": {"name": "mycelium", "title": "Mycelium", "version": env!("CARGO_PKG_VERSION")}}});
    let initialized = json!({"method": "initialized"});
    let call = json!({"id": 2, "method": method, "params": params});

    send(&mut stdin, &initialize).await?;
    answer(&mut lines, 1).await?;
    send(&mut stdin, &initialized).await?;
    send(&mut stdin, &call).await?;

    answer(&mut lines, 2).await
}

async fn send(stdin: &mut tokio::process::ChildStdin, message: &Value) -> Result<()> {
    let line = format!("{message}\n");

    stdin.write_all(line.as_bytes()).await.map_err(|error| anyhow!("Codex closed early: {error}"))
}

async fn answer(lines: &mut tokio::io::Lines<BufReader<tokio::process::ChildStdout>>, id: u64) -> Result<Value> {
    while let Some(line) = lines.next_line().await? {
        let Ok(message) = serde_json::from_str::<Value>(&line) else {
            continue;
        };

        if message["id"].as_u64() != Some(id) {
            continue;
        }

        if let Some(error) = message["error"]["message"].as_str() {
            bail!("{error}");
        }

        return Ok(message["result"].clone());
    }

    Err(anyhow!("Codex closed before answering."))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    fn fake_codex(dir: &Path, rate_limits: &str, consume: &str) -> String {
        let script = dir.join("codex");
        let body = format!(
            r#"#!/bin/sh
while IFS= read -r line; do
  printf '%s\n' "$line" >> "$CODEX_HOME/seen"
  case "$line" in
    *'"method":"initialize"'*) echo '{{"id":1,"result":{{}}}}';;
    *'"method":"account/rateLimits/read"'*) echo '{rate_limits}';;
    *'"method":"account/rateLimitResetCredit/consume"'*) echo '{consume}';;
  esac
done
"#
        );
        std::fs::write(&script, body).unwrap();
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();

        script.display().to_string()
    }

    #[tokio::test]
    async fn only_available_credits_are_kept_with_their_expiry() {
        let account = tempfile::tempdir().unwrap();
        let rate_limits = r#"{"id":2,"result":{"rateLimitResetCredits":{"availableCount":2,"credits":[{"id":"a","resetType":"usage","status":"available","grantedAt":"2026-10-01T00:00:00Z","expiresAt":"2026-11-14T00:00:00Z","title":"Bonus reset"},{"id":"b","resetType":"usage","status":"redeemed","grantedAt":"2026-09-01T00:00:00Z","expiresAt":null,"title":null}]}}}"#;
        let codex = fake_codex(account.path(), rate_limits, "{}");

        let credits = read_reset_credits(&codex, account.path()).await.unwrap();

        assert_eq!(credits.available_count, 2);
        let rows = credits.credits.unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].id, "a");
        assert_eq!(rows[0].expires_at, "2026-11-14T00:00:00Z".parse().ok());
        assert_eq!(rows[0].title.as_deref(), Some("Bonus reset"));
    }

    #[tokio::test]
    async fn only_the_count_is_kept_when_the_app_server_gives_no_credit_rows() {
        let account = tempfile::tempdir().unwrap();
        let codex = fake_codex(account.path(), r#"{"id":2,"result":{"rateLimitResetCredits":{"availableCount":3,"credits":null}}}"#, "{}");

        let credits = read_reset_credits(&codex, account.path()).await.unwrap();

        assert_eq!((credits.available_count, credits.credits), (3, None));
    }

    #[tokio::test]
    async fn no_banked_resets_are_reported_when_the_service_gives_nothing_or_codex_is_missing() {
        let account = tempfile::tempdir().unwrap();
        let codex = fake_codex(account.path(), r#"{"id":2,"result":{"rateLimitResetCredits":null}}"#, "{}");

        assert_eq!(read_reset_credits(&codex, account.path()).await, None);
        assert_eq!(read_reset_credits("/nonexistent/codex", account.path()).await, None);
    }

    #[tokio::test]
    async fn using_a_reset_sends_the_idempotency_key_after_initialising_and_reports_the_outcome() {
        let account = tempfile::tempdir().unwrap();
        let codex = fake_codex(account.path(), "{}", r#"{"id":2,"result":{"outcome":"reset"}}"#);

        let outcome = use_reset(&codex, account.path(), "key-1").await;

        assert_eq!(outcome.unwrap(), "reset");
        let seen = std::fs::read_to_string(account.path().join("seen")).unwrap();
        let methods: Vec<_> = seen.lines().filter_map(|line| serde_json::from_str::<Value>(line).ok()).map(|message| message["method"].as_str().unwrap_or_default().to_string()).collect();
        assert_eq!(methods, vec!["initialize", "initialized", "account/rateLimitResetCredit/consume"]);
        assert!(seen.contains(r#""idempotencyKey":"key-1""#));
    }
}
