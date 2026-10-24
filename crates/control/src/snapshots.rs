use crate::artifacts;
use crate::attachments;
use crate::state::AppState;
use anyhow::{Context, Result};
use chromiumoxide::browser::Browser;
use chromiumoxide::cdp::browser_protocol::emulation::SetDeviceMetricsOverrideParams;
use chromiumoxide::cdp::browser_protocol::fetch::{ContinueRequestParams, EnableParams, EventRequestPaused, FailRequestParams};
use chromiumoxide::cdp::browser_protocol::network::ErrorReason;
use chromiumoxide::cdp::browser_protocol::target::{CreateBrowserContextParams, CreateTargetParams};
use chromiumoxide::page::ScreenshotParams;
use futures_util::StreamExt;
use mycelium_proto::api::{Artifact, ArtifactKind};
use serde_json::Value;
use std::net::IpAddr;
use std::time::Duration;
use tokio::sync::Semaphore;
use url::Url;
use uuid::Uuid;

const PAGE_WIDTH: i64 = 1280;
const PAGE_HEIGHT: i64 = 800;
const LARGEST_WIDTH: i64 = 1600;
const LARGEST_HEIGHT: i64 = 1200;
const DEFAULT_ARTBOARD_WIDTH: f64 = 1440.0;
const DEFAULT_ARTBOARD_HEIGHT: f64 = 900.0;
const DEVICE_SCALE_FACTOR: f64 = 0.5;
const SETTLE: Duration = Duration::from_secs(2);
const MOST_TIME: Duration = Duration::from_secs(15);
const CLEANUP: Duration = Duration::from_secs(2);
const MOST_AT_ONCE: usize = 2;

static SLOTS: Semaphore = Semaphore::const_new(MOST_AT_ONCE);

pub fn spawn(state: &AppState, artifact: &Artifact) {
    let Some(browser_url) = state.config.browser_url.clone() else {
        return;
    };

    if !matches!(artifact.kind, ArtifactKind::Page | ArtifactKind::Design) {
        return;
    }

    let state = state.clone();
    let id = artifact.id;
    let owner = artifact.owner;
    let kind = artifact.kind;
    let version = artifact.latest_version;

    tokio::spawn(async move {
        let result = snapshot(&state, &browser_url, id, owner, kind, version).await;

        if let Err(error) = result {
            tracing::warn!(artifact = %id, version, "The snapshot failed: {error:#}");
        }
    });
}

async fn snapshot(state: &AppState, browser_url: &str, id: Uuid, owner: Uuid, kind: ArtifactKind, version: i32) -> Result<()> {
    let _slot = SLOTS.acquire().await?;
    let sha256 = sqlx::query_scalar::<_, String>("select sha256 from artifact_versions where artifact_id = $1 and version = $2").bind(id).bind(version).fetch_one(&state.pool).await?;
    let bytes = tokio::fs::read(attachments::path_of(state, &sha256)).await.context("The version's content is not stored")?;
    let (html, width, height) = match kind {
        ArtifactKind::Design => {
            let design: Value = serde_json::from_slice(&bytes).context("The design is not JSON")?;
            let artboard = design["pages"][0]["items"].as_array().and_then(|items| items.iter().find(|item| item["type"] == "artboard"));
            let Some(artboard) = artboard else {
                return Ok(());
            };
            let width = (artboard["w"].as_f64().unwrap_or(DEFAULT_ARTBOARD_WIDTH).round() as i64).clamp(1, LARGEST_WIDTH);
            let height = (artboard["h"].as_f64().unwrap_or(DEFAULT_ARTBOARD_HEIGHT).round() as i64).clamp(1, LARGEST_HEIGHT);

            (artboard["html"].as_str().unwrap_or_default().to_string(), width, height)
        }
        _ => (String::from_utf8_lossy(&bytes).into_owned(), PAGE_WIDTH, PAGE_HEIGHT),
    };
    let png = tokio::time::timeout(MOST_TIME, render(browser_url, &html, width, height)).await.context("The snapshot took longer than 15 seconds")??;
    let (thumbnail, _, _) = attachments::store(state, owner, &png, "image/png").await.map_err(|error| anyhow::anyhow!("{}", error.message()))?;

    sqlx::query("update artifact_versions set thumbnail_sha256 = $3 where artifact_id = $1 and version = $2").bind(id).bind(version).bind(&thumbnail).execute(&state.pool).await?;

    artifacts::republish(state, id).await.map_err(|error| anyhow::anyhow!("{}", error.message()))
}

async fn reachable(browser_url: &str) -> Result<String> {
    let mut url = Url::parse(browser_url).context("MYCELIUM_BROWSER_URL is not a URL")?;

    if !matches!(url.scheme(), "http" | "https") {
        return Ok(browser_url.to_string());
    }

    let host = url.host_str().context("MYCELIUM_BROWSER_URL has no host")?.to_string();

    if host.parse::<IpAddr>().is_err() {
        let port = url.port_or_known_default().unwrap_or(80);
        let address = tokio::net::lookup_host((host.as_str(), port)).await?.next().with_context(|| format!("{host} does not resolve"))?;

        url.set_ip_host(address.ip()).map_err(|_| anyhow::anyhow!("MYCELIUM_BROWSER_URL cannot take an address"))?;
    }

    Ok(url.to_string())
}

async fn render(browser_url: &str, html: &str, width: i64, height: i64) -> Result<Vec<u8>> {
    let (browser, mut handler) = Browser::connect(reachable(browser_url).await?).await?;
    let events = tokio::spawn(async move { while handler.next().await.is_some() {} });
    let context = browser.create_browser_context(CreateBrowserContextParams::default()).await;
    let drawn = match &context {
        Ok(context) => draw(&browser, context.clone(), html, width, height).await,
        Err(error) => Err(anyhow::anyhow!("{error}")),
    };

    if let Ok(context) = context {
        tokio::time::timeout(CLEANUP, browser.dispose_browser_context(context)).await.ok();
    }

    events.abort();

    drawn
}

async fn draw(browser: &Browser, context: chromiumoxide::cdp::browser_protocol::browser::BrowserContextId, html: &str, width: i64, height: i64) -> Result<Vec<u8>> {
    let mut target = CreateTargetParams::new("about:blank");

    target.browser_context_id = Some(context);

    let page = browser.new_page(target).await?;
    let result = async {
        let mut paused = page.event_listener::<EventRequestPaused>().await?;

        page.execute(SetDeviceMetricsOverrideParams::new(width, height, DEVICE_SCALE_FACTOR, false)).await?;
        page.execute(EnableParams::default()).await?;

        let guard = page.clone();
        let blocker = tokio::spawn(async move {
            while let Some(event) = paused.next().await {
                let url = event.request.url.as_str();

                if url.starts_with("data:") || url == "about:blank" {
                    guard.execute(ContinueRequestParams::new(event.request_id.clone())).await.ok();
                } else {
                    guard.execute(FailRequestParams::new(event.request_id.clone(), ErrorReason::BlockedByClient)).await.ok();
                }
            }
        });
        let drawn = async {
            page.set_content(html).await?;
            tokio::time::timeout(SETTLE, page.evaluate("document.fonts.ready.then(() => true)")).await.ok();

            Ok::<_, anyhow::Error>(page.screenshot(ScreenshotParams::builder().build()).await?)
        }
        .await;

        blocker.abort();

        drawn
    }
    .await;

    tokio::time::timeout(CLEANUP, page.close()).await.ok();

    result
}
