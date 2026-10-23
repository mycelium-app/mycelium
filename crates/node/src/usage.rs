use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, Instant};

pub const USAGE_ENDPOINT: &str = "https://api.anthropic.com/api/oauth/usage";
pub const USAGE_MIN_AGE: Duration = Duration::from_secs(300);
const USAGE_TIMEOUT: Duration = Duration::from_secs(5);
const WINDOWS: [&str; 2] = ["five_hour", "seven_day"];

pub struct UsageCache {
    endpoint: String,
    min_age: Duration,
    http: reqwest::Client,
    entries: Mutex<HashMap<PathBuf, (Instant, Option<u8>)>>,
}

impl UsageCache {
    pub fn new(endpoint: &str, min_age: Duration) -> UsageCache {
        UsageCache { endpoint: endpoint.to_string(), min_age, http: reqwest::Client::new(), entries: Mutex::new(HashMap::new()) }
    }

    pub fn left(&self, config_dir: &Path) -> Option<u8> {
        self.entries.lock().ok()?.get(config_dir).and_then(|(_, left)| *left)
    }

    pub async fn refresh(&self, config_dirs: &[PathBuf]) {
        for dir in config_dirs {
            let stale = self.entries.lock().is_ok_and(|entries| entries.get(dir).is_none_or(|(fetched, _)| fetched.elapsed() >= self.min_age));

            if !stale {
                continue;
            }

            let fetched = Instant::now();
            let left = self.fetch(dir).await;

            if let Ok(mut entries) = self.entries.lock() {
                entries.insert(dir.clone(), (fetched, left));
            }
        }
    }

    async fn fetch(&self, config_dir: &Path) -> Option<u8> {
        let credentials: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(config_dir.join(".credentials.json")).ok()?).ok()?;
        let token = credentials["claudeAiOauth"]["accessToken"].as_str().filter(|token| !token.is_empty())?;
        let response = self.http.get(&self.endpoint).timeout(USAGE_TIMEOUT).bearer_auth(token).header("anthropic-beta", "oauth-2025-04-20").send().await.ok()?;

        if !response.status().is_success() {
            return None;
        }

        usage_left(&response.json().await.ok()?)
    }
}

fn usage_left(body: &serde_json::Value) -> Option<u8> {
    let busiest = WINDOWS.iter().filter_map(|window| body[*window]["utilization"].as_f64()).fold(None, |busiest: Option<f64>, used| Some(busiest.map_or(used, |known| known.max(used))))?;

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
    async fn the_busiest_window_decides_how_much_usage_is_left_and_the_account_token_is_sent_with_the_beta_header() {
        let seen = Seen::default();
        let endpoint = serve(200, json!({"five_hour": {"utilization": 82.0}, "seven_day": {"utilization": 31}}), seen.clone()).await;
        let account = account_with_token("fake-access-token");
        let cache = UsageCache::new(&endpoint, USAGE_MIN_AGE);

        cache.refresh(&[account.path().to_path_buf()]).await;

        assert_eq!(cache.left(account.path()), Some(18));
        assert_eq!(*seen.lock().unwrap(), vec![("Bearer fake-access-token".to_string(), "oauth-2025-04-20".to_string())]);
    }

    #[tokio::test]
    async fn a_fresh_reading_is_not_fetched_again_until_it_is_old_enough() {
        let seen = Seen::default();
        let endpoint = serve(200, json!({"five_hour": {"utilization": 10}}), seen.clone()).await;
        let account = account_with_token("fake-access-token");
        let dirs = [account.path().to_path_buf()];
        let cache = UsageCache::new(&endpoint, USAGE_MIN_AGE);
        let eager = UsageCache::new(&endpoint, Duration::ZERO);

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
            let cache = UsageCache::new(endpoint, USAGE_MIN_AGE);
            cache.refresh(&[account.path().to_path_buf()]).await;

            assert_eq!(cache.left(account.path()), None);
        }

        let cache = UsageCache::new(&empty, USAGE_MIN_AGE);
        cache.refresh(&[bare.path().to_path_buf()]).await;

        assert_eq!(cache.left(bare.path()), None);
    }

    #[tokio::test]
    async fn usage_is_unknown_when_the_endpoint_cannot_be_reached() {
        let account = account_with_token("fake-access-token");
        let cache = UsageCache::new("http://127.0.0.1:9/api/oauth/usage", USAGE_MIN_AGE);

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
        let cache = UsageCache::new(&endpoint, USAGE_MIN_AGE);
        let available = [AgentInfo { agent: AgentId::Claude, version: None, available: true, update_available: None }];

        cache.refresh(std::slice::from_ref(&work)).await;
        let logins = crate::agents::login_state(home.path(), &accounts, &available, &cache);

        assert_eq!(logins.iter().map(|login| (login.account.as_deref(), login.usage_left)).collect::<Vec<_>>(), vec![(None, None), (Some("work"), Some(77))]);
    }

    #[test]
    fn a_window_over_its_limit_leaves_nothing() {
        assert_eq!(usage_left(&json!({"five_hour": {"utilization": 140.5}})), Some(0));
    }
}
