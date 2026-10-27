use chrono::{DateTime, Utc};
use mycelium_proto::conversation::{ResetCredits, UsageWindow};
use mycelium_proto::usage_history::OrchestratorReading;
use serde::Deserialize;
use crate::codex_app_server::read_reset_credits;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, Instant};

pub const USAGE_ENDPOINT: &str = "https://api.anthropic.com/api/oauth/usage";
pub const CODEX_USAGE_ENDPOINT: &str = "https://chatgpt.com/backend-api/wham/usage";
pub const USAGE_MIN_AGE: Duration = Duration::from_secs(300);
const USAGE_TIMEOUT: Duration = Duration::from_secs(5);
const LEADING_WINDOWS: [&str; 2] = ["five_hour", "seven_day"];

#[derive(Clone, Debug, PartialEq)]
pub struct Reading {
    pub windows: Vec<UsageWindow>,
    pub read_at: DateTime<Utc>,
    pub reset_credits: Option<ResetCredits>,
}

#[derive(Deserialize)]
struct OrchestratorLimits {
    #[serde(default)]
    fine: Vec<OrchestratorReading>,
    #[serde(default)]
    coarse: Vec<OrchestratorReading>,
}

pub fn orchestrator_limits_path(home: &Path) -> PathBuf {
    home.join(".local/share/orchestrator/limits.json")
}

pub fn orchestrator_readings(path: &Path) -> Option<(Vec<OrchestratorReading>, Vec<OrchestratorReading>)> {
    let text = std::fs::read_to_string(path).ok()?;
    let limits: OrchestratorLimits = serde_json::from_str(&text).ok()?;

    (!limits.fine.is_empty() || !limits.coarse.is_empty()).then_some((limits.fine, limits.coarse))
}

pub struct UsageCache {
    endpoint: String,
    codex_endpoint: String,
    codex_command: String,
    min_age: Duration,
    http: reqwest::Client,
    entries: Mutex<HashMap<PathBuf, (Instant, Option<Reading>)>>,
}

impl UsageCache {
    pub fn new(endpoint: &str, codex_endpoint: &str, codex_command: &str, min_age: Duration) -> UsageCache {
        UsageCache { endpoint: endpoint.to_string(), codex_endpoint: codex_endpoint.to_string(), codex_command: codex_command.to_string(), min_age, http: reqwest::Client::new(), entries: Mutex::new(HashMap::new()) }
    }

    pub fn reading(&self, config_dir: &Path) -> Option<Reading> {
        self.entries.lock().ok()?.get(config_dir).and_then(|(_, reading)| reading.clone())
    }

    pub fn left(&self, config_dir: &Path) -> Option<u8> {
        usage_left(&self.reading(config_dir)?.windows)
    }

    pub fn invalidate(&self, config_dir: &Path) {
        if let Ok(mut entries) = self.entries.lock() {
            entries.remove(config_dir);
        }
    }

    pub async fn refresh(&self, config_dirs: &[PathBuf]) {
        for dir in config_dirs {
            let stale = self.entries.lock().is_ok_and(|entries| entries.get(dir).is_none_or(|(fetched, _)| fetched.elapsed() >= self.min_age));

            if !stale {
                continue;
            }

            let fetched = Instant::now();
            let fetched_reading = self.fetch(dir).await;
            let reading = fetched_reading.or_else(|| self.reading(dir));

            if let Ok(mut entries) = self.entries.lock() {
                entries.insert(dir.clone(), (fetched, reading));
            }
        }
    }

    async fn fetch(&self, config_dir: &Path) -> Option<Reading> {
        let (windows, reset_credits) = match read_json(&config_dir.join(".credentials.json")) {
            Some(credentials) => (self.fetch_claude(&credentials).await?, None),
            None => {
                let windows = self.fetch_codex(&read_json(&config_dir.join("auth.json"))?).await?;

                (windows, read_reset_credits(&self.codex_command, config_dir).await)
            }
        };

        if windows.is_empty() {
            return None;
        }

        Some(Reading { windows, read_at: Utc::now(), reset_credits })
    }

    async fn fetch_claude(&self, credentials: &serde_json::Value) -> Option<Vec<UsageWindow>> {
        let token = credentials["claudeAiOauth"]["accessToken"].as_str().filter(|token| !token.is_empty())?;
        let response = self.http.get(&self.endpoint).timeout(USAGE_TIMEOUT).bearer_auth(token).header("anthropic-beta", "oauth-2025-04-20").send().await.ok()?;

        if !response.status().is_success() {
            return None;
        }

        Some(usage_windows(&response.json().await.ok()?))
    }

    async fn fetch_codex(&self, auth: &serde_json::Value) -> Option<Vec<UsageWindow>> {
        let token = auth["tokens"]["access_token"].as_str().filter(|token| !token.is_empty())?;
        let mut request = self.http.get(&self.codex_endpoint).timeout(USAGE_TIMEOUT).bearer_auth(token).header("user-agent", "codex_cli_rs");

        if let Some(account_id) = auth["tokens"]["account_id"].as_str() {
            request = request.header("chatgpt-account-id", account_id);
        }

        let response = request.send().await.ok()?;

        if !response.status().is_success() {
            return None;
        }

        Some(codex_usage_windows(&response.json().await.ok()?))
    }
}

fn read_json(path: &Path) -> Option<serde_json::Value> {
    serde_json::from_str(&std::fs::read_to_string(path).ok()?).ok()
}

fn usage_windows(body: &serde_json::Value) -> Vec<UsageWindow> {
    let Some(object) = body.as_object() else {
        return Vec::new();
    };
    let mut windows: Vec<UsageWindow> = object
        .iter()
        .filter_map(|(name, window)| {
            let used_percent = window["utilization"].as_f64()?;
            let resets_at = window["resets_at"].as_str().and_then(|text| text.parse::<DateTime<Utc>>().ok());

            Some(UsageWindow { name: name.clone(), used_percent, resets_at, rate_per_hour: None, runs_out_at: None })
        })
        .collect();
    windows.sort_by_key(|window| (LEADING_WINDOWS.iter().position(|leading| *leading == window.name).unwrap_or(LEADING_WINDOWS.len()), window.name.clone()));

    windows
}

fn codex_usage_windows(body: &serde_json::Value) -> Vec<UsageWindow> {
    ["primary_window", "secondary_window"]
        .iter()
        .filter_map(|key| {
            let window = &body["rate_limit"][key];
            let used_percent = window["used_percent"].as_f64()?;
            let resets_at = window["reset_at"].as_i64().and_then(|seconds| DateTime::from_timestamp(seconds, 0));
            let name = match window["limit_window_seconds"].as_i64() {
                Some(18_000) => "five_hour",
                Some(604_800) => "seven_day",
                _ => key.trim_end_matches("_window"),
            };

            Some(UsageWindow { name: name.to_string(), used_percent, resets_at, rate_per_hour: None, runs_out_at: None })
        })
        .collect()
}

pub fn usage_left(windows: &[UsageWindow]) -> Option<u8> {
    let busiest = windows.iter().map(|window| window.used_percent).reduce(f64::max)?;

    Some((100.0 - busiest).round().clamp(0.0, 100.0) as u8)
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::Router;
    use axum::http::HeaderMap;
    use axum::routing::get;
    use mycelium_proto::conversation::{AgentId, AgentInfo};
    use serde_json::json;
    use std::sync::Arc;

    type Seen = Arc<Mutex<Vec<(String, String)>>>;

    #[test]
    fn orchestrator_limits_become_readings_and_a_missing_or_empty_file_gives_none() {
        let dir = std::env::temp_dir().join(format!("orchestrator-limits-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("limits.json");

        assert!(orchestrator_readings(&path).is_none());

        std::fs::write(&path, r#"{"schema":2,"fine":[{"at":10,"five":27,"seven":45,"five_resets":20,"seven_resets":30,"units":1.0}],"coarse":[{"at":5,"five":1,"seven":2}]}"#).unwrap();
        let (fine, coarse) = orchestrator_readings(&path).unwrap();

        assert_eq!((fine.len(), coarse.len(), fine[0].five_resets, coarse[0].five_resets), (1, 1, Some(20), None));

        std::fs::write(&path, r#"{"schema":2,"fine":[],"coarse":[]}"#).unwrap();
        assert!(orchestrator_readings(&path).is_none());
    }

    async fn serve(status: u16, body: serde_json::Value, seen: Seen) -> String {
        let router = Router::new().route(
            "/api/oauth/usage",
            get(move |headers: HeaderMap| {
                let seen = seen.clone();
                let body = body.clone();

                async move {
                    let header = |name: &str| headers.get(name).and_then(|value| value.to_str().ok()).unwrap_or_default().to_string();
                    seen.lock().unwrap().push((header("authorization"), header("anthropic-beta")));

                    (axum::http::StatusCode::from_u16(status).unwrap(), axum::Json(body))
                }
            }),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });

        format!("http://{address}/api/oauth/usage")
    }

    fn account_with_token(token: &str) -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join(".credentials.json"), json!({"claudeAiOauth": {"accessToken": token}}).to_string()).unwrap();

        dir
    }

    #[tokio::test]
    async fn a_failed_refresh_keeps_the_last_reported_usage_and_its_original_timestamp() {
        let seen = Seen::default();
        let endpoint = serve(200, json!({"five_hour": {"utilization": 42.0}}), seen.clone()).await;
        let account = account_with_token("fake-access-token");
        let mut cache = UsageCache::new(&endpoint, CODEX_USAGE_ENDPOINT, "codex", Duration::ZERO);
        cache.refresh(&[account.path().to_path_buf()]).await;
        let before = cache.reading(account.path()).unwrap();
        cache.endpoint = serve(503, json!({}), seen).await;
        cache.refresh(&[account.path().to_path_buf()]).await;
        assert_eq!(cache.reading(account.path()), Some(before));
    }

    #[tokio::test]
    async fn the_busiest_window_decides_how_much_usage_is_left_and_the_account_token_is_sent_with_the_beta_header() {
        let seen = Seen::default();
        let endpoint = serve(200, json!({"five_hour": {"utilization": 82.0}, "seven_day": {"utilization": 31}}), seen.clone()).await;
        let account = account_with_token("fake-access-token");
        let cache = UsageCache::new(&endpoint, CODEX_USAGE_ENDPOINT, "codex", USAGE_MIN_AGE);

        cache.refresh(&[account.path().to_path_buf()]).await;

        assert_eq!(cache.left(account.path()), Some(18));
        assert_eq!(*seen.lock().unwrap(), vec![("Bearer fake-access-token".to_string(), "oauth-2025-04-20".to_string())]);
    }

    #[tokio::test]
    async fn a_codex_account_reports_its_five_hour_and_weekly_windows_read_with_its_chatgpt_token() {
        let seen = Seen::default();
        let five_hour = json!({"used_percent": 40, "limit_window_seconds": 18000, "reset_at": 1_792_900_000});
        let weekly = json!({"used_percent": 75, "limit_window_seconds": 604800, "reset_at": 1_793_400_000});
        let endpoint = serve(200, json!({"rate_limit": {"primary_window": five_hour, "secondary_window": weekly}}), seen.clone()).await;
        let account = tempfile::tempdir().unwrap();
        std::fs::write(account.path().join("auth.json"), json!({"tokens": {"access_token": "chatgpt-token", "account_id": "acct"}}).to_string()).unwrap();
        let cache = UsageCache::new(USAGE_ENDPOINT, &endpoint, "codex", USAGE_MIN_AGE);

        cache.refresh(&[account.path().to_path_buf()]).await;

        let windows = cache.reading(account.path()).unwrap().windows;
        assert_eq!(windows.iter().map(|window| (window.name.as_str(), window.used_percent)).collect::<Vec<_>>(), vec![("five_hour", 40.0), ("seven_day", 75.0)]);
        assert_eq!(windows[0].resets_at, DateTime::from_timestamp(1_792_900_000, 0));
        assert_eq!(cache.left(account.path()), Some(25));
        assert_eq!(seen.lock().unwrap()[0].0, "Bearer chatgpt-token");
    }

    async fn codex_account(codex: &str) -> (tempfile::TempDir, UsageCache) {
        let five_hour = json!({"used_percent": 40, "limit_window_seconds": 18000, "reset_at": 1_792_900_000});
        let endpoint = serve(200, json!({"rate_limit": {"primary_window": five_hour}}), Seen::default()).await;
        let account = tempfile::tempdir().unwrap();
        std::fs::write(account.path().join("auth.json"), json!({"tokens": {"access_token": "chatgpt-token"}}).to_string()).unwrap();
        let cache = UsageCache::new(USAGE_ENDPOINT, &endpoint, codex, USAGE_MIN_AGE);
        cache.refresh(&[account.path().to_path_buf()]).await;

        (account, cache)
    }

    #[tokio::test]
    async fn a_codex_account_reports_its_banked_resets_and_their_expiry_from_the_app_server() {
        use std::os::unix::fs::PermissionsExt;

        let scripts = tempfile::tempdir().unwrap();
        let script = scripts.path().join("codex");
        let credits = r#"{"id":2,"result":{"rateLimitResetCredits":{"availableCount":1,"credits":[{"id":"a","status":"available","expiresAt":"2026-11-14T00:00:00Z"}]}}}"#;
        std::fs::write(&script, format!("#!/bin/sh\nwhile IFS= read -r line; do\n  case \"$line\" in\n    *'\"method\":\"initialize\"'*) echo '{{\"id\":1,\"result\":{{}}}}';;\n    *'\"method\":\"account/rateLimits/read\"'*) echo '{credits}';;\n  esac\ndone\n")).unwrap();
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();

        let (account, cache) = codex_account(&script.display().to_string()).await;

        let reading = cache.reading(account.path()).unwrap();
        let credits = reading.reset_credits.unwrap();
        assert_eq!(credits.available_count, 1);
        assert_eq!(credits.credits.unwrap()[0].expires_at, "2026-11-14T00:00:00Z".parse().ok());
    }

    #[tokio::test]
    async fn banked_resets_are_unknown_when_codex_is_missing_and_the_windows_still_come_from_wham() {
        let (account, cache) = codex_account("/nonexistent/codex").await;

        let reading = cache.reading(account.path()).unwrap();
        assert_eq!(reading.reset_credits, None);
        assert_eq!(reading.windows[0].used_percent, 40.0);
    }

    #[tokio::test]
    async fn a_fresh_reading_is_not_fetched_again_until_it_is_old_enough() {
        let seen = Seen::default();
        let endpoint = serve(200, json!({"five_hour": {"utilization": 10}}), seen.clone()).await;
        let account = account_with_token("fake-access-token");
        let dirs = [account.path().to_path_buf()];
        let cache = UsageCache::new(&endpoint, CODEX_USAGE_ENDPOINT, "codex", USAGE_MIN_AGE);
        let eager = UsageCache::new(&endpoint, CODEX_USAGE_ENDPOINT, "codex", Duration::ZERO);

        cache.refresh(&dirs).await;
        cache.refresh(&dirs).await;
        eager.refresh(&dirs).await;
        eager.refresh(&dirs).await;

        assert_eq!(seen.lock().unwrap().len(), 3);
    }

    #[tokio::test]
    async fn usage_is_unknown_when_the_endpoint_refuses_or_the_credentials_are_missing() {
        let refused = serve(401, json!({"error": "no"}), Seen::default()).await;
        let empty = serve(200, json!({}), Seen::default()).await;
        let account = account_with_token("fake-access-token");
        let bare = tempfile::tempdir().unwrap();

        for endpoint in [&refused, &empty] {
            let cache = UsageCache::new(endpoint, CODEX_USAGE_ENDPOINT, "codex", USAGE_MIN_AGE);
            cache.refresh(&[account.path().to_path_buf()]).await;

            assert_eq!(cache.left(account.path()), None);
        }

        let cache = UsageCache::new(&empty, CODEX_USAGE_ENDPOINT, "codex", USAGE_MIN_AGE);
        cache.refresh(&[bare.path().to_path_buf()]).await;

        assert_eq!(cache.left(bare.path()), None);
    }

    #[tokio::test]
    async fn usage_is_unknown_when_the_endpoint_cannot_be_reached() {
        let account = account_with_token("fake-access-token");
        let cache = UsageCache::new("http://127.0.0.1:9/api/oauth/usage", CODEX_USAGE_ENDPOINT, "codex", USAGE_MIN_AGE);

        cache.refresh(&[account.path().to_path_buf()]).await;

        assert_eq!(cache.left(account.path()), None);
    }

    #[tokio::test]
    async fn a_signed_in_claude_account_reports_the_usage_left_read_from_its_own_folder() {
        let endpoint = serve(200, json!({"five_hour": {"utilization": 23}}), Seen::default()).await;
        let home = tempfile::tempdir().unwrap();
        let accounts = home.path().join("accounts");
        let work = accounts.join("claude").join("work");
        std::fs::create_dir_all(&work).unwrap();
        std::fs::write(work.join(".credentials.json"), json!({"claudeAiOauth": {"accessToken": "fake-access-token"}}).to_string()).unwrap();
        let cache = UsageCache::new(&endpoint, CODEX_USAGE_ENDPOINT, "codex", USAGE_MIN_AGE);
        let available = [AgentInfo { agent: AgentId::Claude, version: None, available: true, update_available: None }];

        cache.refresh(std::slice::from_ref(&work)).await;
        let logins = crate::agents::login_state(home.path(), &accounts, &available, &cache);

        assert_eq!(logins.iter().map(|login| (login.account.as_deref(), login.usage_left)).collect::<Vec<_>>(), vec![(None, None), (Some("work"), Some(77))]);
    }

    #[test]
    fn a_window_over_its_limit_leaves_nothing() {
        assert_eq!(usage_left(&usage_windows(&json!({"five_hour": {"utilization": 140.5}}))), Some(0));
    }

    #[tokio::test]
    async fn every_window_the_response_carries_is_kept_with_its_percent_and_reset_time_five_hour_and_seven_day_first() {
        let body = json!({
            "seven_day_opus": {"utilization": 5.5, "resets_at": "2026-10-30T09:00:00.000000+00:00"},
            "seven_day": {"utilization": 31, "resets_at": "2026-10-28T09:00:00+00:00"},
            "five_hour": {"utilization": 82.0, "resets_at": "2026-10-23T22:30:00Z"},
            "extra_usage": null,
            "iguana": {"utilization": null}
        });
        let endpoint = serve(200, body, Seen::default()).await;
        let account = account_with_token("fake-access-token");
        let cache = UsageCache::new(&endpoint, CODEX_USAGE_ENDPOINT, "codex", USAGE_MIN_AGE);

        cache.refresh(&[account.path().to_path_buf()]).await;
        let reading = cache.reading(account.path()).unwrap();

        let summary: Vec<_> = reading.windows.iter().map(|window| (window.name.as_str(), window.used_percent, window.resets_at.map(|at| at.to_rfc3339()))).collect();
        assert_eq!(
            summary,
            vec![
                ("five_hour", 82.0, Some("2026-10-23T22:30:00+00:00".to_string())),
                ("seven_day", 31.0, Some("2026-10-28T09:00:00+00:00".to_string())),
                ("seven_day_opus", 5.5, Some("2026-10-30T09:00:00+00:00".to_string())),
            ]
        );
        assert!((Utc::now() - reading.read_at).num_seconds() < 5);
    }

    #[tokio::test]
    async fn the_logins_report_carries_the_windows_and_when_they_were_read() {
        let endpoint = serve(200, json!({"five_hour": {"utilization": 23, "resets_at": "2026-10-23T22:30:00Z"}}), Seen::default()).await;
        let home = tempfile::tempdir().unwrap();
        let accounts = home.path().join("accounts");
        let work = accounts.join("claude").join("work");
        std::fs::create_dir_all(&work).unwrap();
        std::fs::write(work.join(".credentials.json"), json!({"claudeAiOauth": {"accessToken": "fake-access-token"}}).to_string()).unwrap();
        let cache = UsageCache::new(&endpoint, CODEX_USAGE_ENDPOINT, "codex", USAGE_MIN_AGE);
        let available = [AgentInfo { agent: AgentId::Claude, version: None, available: true, update_available: None }];

        cache.refresh(std::slice::from_ref(&work)).await;
        let logins = crate::agents::login_state(home.path(), &accounts, &available, &cache);

        let work_login = logins.iter().find(|login| login.account.as_deref() == Some("work")).unwrap();
        assert_eq!(work_login.windows.iter().map(|window| (window.name.as_str(), window.used_percent)).collect::<Vec<_>>(), vec![("five_hour", 23.0)]);
        assert!(work_login.usage_read_at.is_some());
        assert!(logins.iter().find(|login| login.account.is_none()).unwrap().windows.is_empty());
    }
}
