use anyhow::{Result, bail};
use axum::body::Bytes;
use reqwest::StatusCode;
use reqwest::header::CONTENT_TYPE;
use serde_json::Value;
use url::Url;

#[derive(Debug, Clone)]
pub struct Artifact {
    pub kind: String,
    pub title: String,
    pub public: bool,
    pub media_type: String,
}

pub struct Content {
    pub media_type: String,
    pub bytes: Bytes,
}

#[derive(Clone)]
pub struct ControlClient {
    http: reqwest::Client,
    base: String,
    token: String,
}

impl ControlClient {
    pub fn new(http: reqwest::Client, base: String, token: String) -> ControlClient {
        ControlClient { http, base, token }
    }

    fn url(&self, slug: &str, tail: &[&str]) -> Result<Url> {
        let mut url = Url::parse(&self.base)?;

        match url.path_segments_mut() {
            Ok(mut segments) => {
                segments.pop_if_empty().extend(["internal", "artifacts", "by-slug", slug]).extend(tail);
            }
            Err(()) => bail!("MYCELIUM_CONTROL_URL is not a base URL."),
        }

        Ok(url)
    }

    pub async fn artifact(&self, slug: &str) -> Result<Option<Artifact>> {
        let response = self.http.get(self.url(slug, &[])?).bearer_auth(&self.token).send().await?;

        if response.status() == StatusCode::NOT_FOUND {
            return Ok(None);
        }

        let body: Value = response.error_for_status()?.json().await?;
        let artifact = if body["artifact"].is_object() { &body["artifact"] } else { &body };
        let text = |name: &str| body[name].as_str().or(artifact[name].as_str()).map(str::to_string);
        let kind = text("kind").ok_or_else(|| anyhow::anyhow!("the control plane sent an artifact without a kind"))?;

        Ok(Some(Artifact {
            kind,
            title: text("title").unwrap_or_default(),
            public: artifact["public"].as_bool().unwrap_or(false),
            media_type: text("media_type").or_else(|| body["latest"]["media_type"].as_str().map(str::to_string)).unwrap_or_default(),
        }))
    }

    pub async fn content(&self, slug: &str) -> Result<Option<Content>> {
        let response = self.http.get(self.url(slug, &["content"])?).bearer_auth(&self.token).send().await?;

        if response.status() == StatusCode::NOT_FOUND {
            return Ok(None);
        }

        let response = response.error_for_status()?;
        let media_type = response.headers().get(CONTENT_TYPE).and_then(|value| value.to_str().ok()).unwrap_or_default().to_string();
        let bytes = response.bytes().await?;

        Ok(Some(Content { media_type, bytes }))
    }

    pub async fn save_version(&self, slug: &str, subject: &str, bytes: Bytes) -> Result<(StatusCode, Value)> {
        let mut url = self.url(slug, &["versions"])?;
        url.query_pairs_mut().append_pair("subject", subject);

        let response = self.http.post(url).bearer_auth(&self.token).body(bytes).send().await?;
        let status = response.status();
        let body: Value = response.json().await.unwrap_or(Value::Null);

        Ok((status, body))
    }

    pub async fn data(&self, slug: &str) -> Result<Option<Value>> {
        let response = self.http.get(self.url(slug, &["data"])?).bearer_auth(&self.token).send().await?;

        if response.status() == StatusCode::NOT_FOUND {
            return Ok(None);
        }

        Ok(Some(response.error_for_status()?.json().await?))
    }

    pub async fn put_data(&self, slug: &str, key: &str, value: &Value) -> Result<Option<Value>> {
        let response = self.http.put(self.url(slug, &["data", key])?).bearer_auth(&self.token).json(value).send().await?;

        if response.status() == StatusCode::NOT_FOUND {
            return Ok(None);
        }

        let response = response.error_for_status()?;
        let text = response.text().await?;

        Ok(Some(serde_json::from_str(&text).unwrap_or(Value::Null)))
    }
}
