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

pub struct Viewer {
    pub subject: Option<String>,
    pub issuer: Option<String>,
}

pub enum Denied {
    Missing,
    Forbidden,
}

pub type Reply<T> = Result<T, Denied>;

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

    fn url(&self, slug: &str, tail: &[&str], viewer: &Viewer) -> Result<Url> {
        let mut url = Url::parse(&self.base)?;

        match url.path_segments_mut() {
            Ok(mut segments) => {
                segments.pop_if_empty().extend(["internal", "artifacts", "by-slug", slug]).extend(tail);
            }
            Err(()) => bail!("MYCELIUM_CONTROL_URL is not a base URL."),
        }

        if let Some(subject) = &viewer.subject {
            url.query_pairs_mut().append_pair("subject", subject);
        }

        if let Some(issuer) = &viewer.issuer {
            url.query_pairs_mut().append_pair("issuer", issuer);
        }

        Ok(url)
    }

    fn answered(response: reqwest::Response) -> Result<Reply<reqwest::Response>> {
        match response.status() {
            StatusCode::NOT_FOUND => Ok(Err(Denied::Missing)),
            StatusCode::FORBIDDEN => Ok(Err(Denied::Forbidden)),
            _ => Ok(Ok(response.error_for_status()?)),
        }
    }

    pub async fn artifact(&self, slug: &str, viewer: &Viewer) -> Result<Reply<Artifact>> {
        let response = self.http.get(self.url(slug, &[], viewer)?).bearer_auth(&self.token).send().await?;

        match Self::answered(response)? {
            Err(denied) => Ok(Err(denied)),
            Ok(response) => {
                let body: Value = response.json().await?;
                let artifact = if body["artifact"].is_object() { &body["artifact"] } else { &body };
                let text = |name: &str| body[name].as_str().or(artifact[name].as_str()).map(str::to_string);
                let kind = text("kind").ok_or_else(|| anyhow::anyhow!("the control plane sent an artifact without a kind"))?;

                Ok(Ok(Artifact {
                    kind,
                    title: text("title").unwrap_or_default(),
                    public: artifact["public"].as_bool().unwrap_or(false),
                    media_type: text("media_type").or_else(|| body["latest"]["media_type"].as_str().map(str::to_string)).unwrap_or_default(),
                }))
            }
        }
    }

    pub async fn content(&self, slug: &str, viewer: &Viewer) -> Result<Reply<Content>> {
        let response = self.http.get(self.url(slug, &["content"], viewer)?).bearer_auth(&self.token).send().await?;

        match Self::answered(response)? {
            Err(denied) => Ok(Err(denied)),
            Ok(response) => {
                let media_type = response.headers().get(CONTENT_TYPE).and_then(|value| value.to_str().ok()).unwrap_or_default().to_string();
                let bytes = response.bytes().await?;

                Ok(Ok(Content { media_type, bytes }))
            }
        }
    }

    pub async fn save_version(&self, slug: &str, viewer: &Viewer, bytes: Bytes) -> Result<(StatusCode, Value)> {
        let response = self.http.post(self.url(slug, &["versions"], viewer)?).bearer_auth(&self.token).body(bytes).send().await?;
        let status = response.status();
        let body: Value = response.json().await.unwrap_or(Value::Null);

        Ok((status, body))
    }

    pub async fn data(&self, slug: &str, viewer: &Viewer) -> Result<Reply<Value>> {
        let response = self.http.get(self.url(slug, &["data"], viewer)?).bearer_auth(&self.token).send().await?;

        match Self::answered(response)? {
            Err(denied) => Ok(Err(denied)),
            Ok(response) => Ok(Ok(response.json().await?)),
        }
    }

    pub async fn put_data(&self, slug: &str, key: &str, value: &Value, viewer: &Viewer) -> Result<(StatusCode, Value)> {
        let response = self.http.put(self.url(slug, &["data", key], viewer)?).bearer_auth(&self.token).json(value).send().await?;
        let status = response.status();
        let text = response.text().await?;

        Ok((status, serde_json::from_str(&text).unwrap_or(Value::Null)))
    }
}
