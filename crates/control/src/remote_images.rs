use crate::attachments::{path_of, store};
use crate::auth::AuthUser;
use crate::error::ApiError;
use crate::outbound::{Stopped, Followed, Oversize, get_following_redirects, read_limited, refused_public_address, was_refused};
use crate::state::AppState;
use crate::store::find_owned;
use axum::Json;
use axum::extract::{Path, Query, State};
use axum::http::{StatusCode, header};
use axum::response::{IntoResponse, Response};
use serde_json::json;
use std::collections::HashSet;
use std::net::IpAddr;
use url::{Host, Url};
use uuid::Uuid;

pub const IMAGE_LIMIT: usize = 10 * 1024 * 1024;
const PIXEL_LIMIT: u64 = 40_000_000;
const ADDRESS_LIMIT: usize = 2048;
const REFUSAL_MINUTES: i64 = 10;
const FROM_SEARCH: &str = "search_images";
const FROM_CLICK: &str = "click";

pub struct Refusal {
    status: StatusCode,
    code: &'static str,
    message: String,
    host: String,
}

pub enum ImageError {
    Refused(Refusal),
    Api(ApiError),
}

impl From<ApiError> for ImageError {
    fn from(error: ApiError) -> ImageError {
        ImageError::Api(error)
    }
}

impl From<sqlx::Error> for ImageError {
    fn from(error: sqlx::Error) -> ImageError {
        ImageError::Api(error.into())
    }
}

impl From<Refusal> for ImageError {
    fn from(refusal: Refusal) -> ImageError {
        ImageError::Refused(refusal)
    }
}

impl IntoResponse for ImageError {
    fn into_response(self) -> Response {
        match self {
            ImageError::Api(error) => error.into_response(),
            ImageError::Refused(refusal) => (refusal.status, Json(json!({"error": refusal.code, "message": refusal.message, "host": refusal.host}))).into_response(),
        }
    }
}

impl Refusal {
    fn new(status: StatusCode, code: &'static str, host: &str, message: impl Into<String>) -> Refusal {
        Refusal { status, code, message: message.into(), host: host.to_string() }
    }

    fn needs_click(host: &str) -> Refusal {
        Refusal::new(StatusCode::FORBIDDEN, "image_needs_click", host, format!("This image is not from an image search. Press Load image to fetch it from {host}."))
    }

    fn private_address(host: &str) -> Refusal {
        Refusal::new(StatusCode::UNPROCESSABLE_ENTITY, "image_private_address", host, "This address is on a private network, which Mycelium does not fetch for agents.")
    }

    fn address_refused(host: &str) -> Refusal {
        Refusal::new(StatusCode::UNPROCESSABLE_ENTITY, "image_address_refused", host, "Mycelium does not fetch images from addresses like this one. Use an address that starts with https:// and has a host name.")
    }

    fn not_https(host: &str) -> Refusal {
        Refusal::new(StatusCode::UNPROCESSABLE_ENTITY, "image_not_https", host, "Only https images are shown.")
    }

    fn too_large(host: &str, detail: String) -> Refusal {
        Refusal::new(StatusCode::UNPROCESSABLE_ENTITY, "image_too_large", host, detail)
    }

    fn not_image(host: &str) -> Refusal {
        Refusal::new(StatusCode::UNPROCESSABLE_ENTITY, "image_not_image", host, format!("{host} did not send an image Mycelium can show."))
    }

    fn unavailable(host: &str) -> Refusal {
        Refusal::new(StatusCode::UNPROCESSABLE_ENTITY, "image_unavailable", host, format!("{host} did not answer."))
    }

    fn stored(&self) -> String {
        json!({"error": self.code, "message": self.message, "host": self.host}).to_string()
    }

    fn from_stored(stored: &str) -> Option<Refusal> {
        let value: serde_json::Value = serde_json::from_str(stored).ok()?;
        let code = ["image_private_address", "image_address_refused", "image_not_https", "image_too_large", "image_not_image", "image_unavailable"].into_iter().find(|code| value["error"] == *code)?;

        Some(Refusal::new(StatusCode::UNPROCESSABLE_ENTITY, code, value["host"].as_str()?, value["message"].as_str()?))
    }
}

fn host_name(address: &str) -> String {
    Url::parse(address).ok().and_then(|parsed| parsed.host_str().map(str::to_string)).unwrap_or_default()
}

fn rules(address: &Url) -> Result<(), Refusal> {
    let host = address.host_str().unwrap_or_default();

    if let Some(Host::Ipv4(_) | Host::Ipv6(_)) = address.host() {
        let literal: Option<IpAddr> = host.trim_matches(['[', ']']).parse().ok();

        return Err(match literal {
            Some(literal) if refused_public_address(literal) => Refusal::private_address(host),
            _ => Refusal::address_refused(host),
        });
    }

    if address.scheme() != "https" {
        return Err(Refusal::not_https(host));
    }

    if address.host().is_none() || !address.username().is_empty() || address.password().is_some() || address.as_str().len() > ADDRESS_LIMIT {
        return Err(Refusal::address_refused(host));
    }

    Ok(())
}

fn requested(raw: &str) -> Result<Url, Refusal> {
    let Ok(mut parsed) = Url::parse(raw) else {
        return Err(Refusal::address_refused(""));
    };

    if raw.len() > ADDRESS_LIMIT {
        return Err(Refusal::address_refused(parsed.host_str().unwrap_or_default()));
    }

    rules(&parsed)?;
    parsed.set_fragment(None);

    Ok(parsed)
}

enum Trust {
    Untrusted,
    Recorded { host: String, recorded: HashSet<String>, previous: Vec<String> },
}

impl Trust {
    fn allows(&self, position: usize, hop: &Url) -> Result<(), Refusal> {
        let Trust::Recorded { host, recorded, previous } = self else {
            return Ok(());
        };
        let allowed = match previous.get(position) {
            Some(pinned) => pinned == hop.as_str(),
            None if !previous.is_empty() => false,
            None => hop.host_str() == Some(host.as_str()) || recorded.contains(hop.as_str()),
        };

        match allowed {
            true => Ok(()),
            false => Err(Refusal::needs_click(hop.host_str().unwrap_or_default())),
        }
    }
}

struct Fetched {
    bytes: Vec<u8>,
    media_type: &'static str,
    hops: Vec<String>,
}

fn size_text(bytes: u64) -> String {
    format!("{} MB", bytes.div_ceil(1024 * 1024))
}

async fn fetch(state: &AppState, first: &Url, trust: &Trust) -> Result<Fetched, Refusal> {
    let host = first.host_str().unwrap_or_default().to_string();
    let mut position = 0;
    let followed = get_following_redirects(&state.http_public, first.as_str(), |hop: &Url| {
        rules(hop)?;
        trust.allows(position, hop)?;
        position += 1;

        Ok(())
    })
    .await;
    let Followed { response, chain } = match followed {
        Ok(followed) => followed,
        Err(Stopped::Refused(refusal)) => return Err(refusal),
        Err(Stopped::Request(error)) if was_refused(&error) => return Err(Refusal::private_address(&host)),
        Err(Stopped::Request(_)) => return Err(Refusal::unavailable(&host)),
    };

    if !response.status().is_success() {
        return Err(Refusal::unavailable(&host));
    }

    let announced = response.content_length();
    let bytes = match read_limited(response, IMAGE_LIMIT).await {
        Ok(bytes) => bytes,
        Err(Oversize::Larger) => {
            let detail = match announced {
                Some(length) => format!("The image is {}, over the {} limit.", size_text(length), size_text(IMAGE_LIMIT as u64)),
                None => format!("The image is over the {} limit.", size_text(IMAGE_LIMIT as u64)),
            };

            return Err(Refusal::too_large(&host, detail));
        }
        Err(Oversize::Stopped) => return Err(Refusal::unavailable(&host)),
    };
    let Some((media_type, width, height)) = sniff(&bytes) else {
        return Err(Refusal::not_image(&host));
    };

    if width * height > PIXEL_LIMIT {
        return Err(Refusal::too_large(&host, format!("The image is {width} by {height} pixels, over the 40 megapixel limit.")));
    }

    Ok(Fetched { bytes, media_type, hops: chain.iter().map(Url::to_string).collect() })
}

fn be32(bytes: &[u8], at: usize) -> Option<u64> {
    bytes.get(at..at + 4).map(|part| u32::from_be_bytes([part[0], part[1], part[2], part[3]]) as u64)
}

fn le16(bytes: &[u8], at: usize) -> Option<u64> {
    bytes.get(at..at + 2).map(|part| u16::from_le_bytes([part[0], part[1]]) as u64)
}

fn le24(bytes: &[u8], at: usize) -> Option<u64> {
    bytes.get(at..at + 3).map(|part| u32::from_le_bytes([part[0], part[1], part[2], 0]) as u64)
}

fn jpeg_size(bytes: &[u8]) -> Option<(u64, u64)> {
    let mut at = 2;

    while at + 4 <= bytes.len() {
        if bytes[at] != 0xff {
            return None;
        }

        let marker = bytes[at + 1];

        if marker == 0xff {
            at += 1;

            continue;
        }

        if marker == 0x01 || (0xd0..=0xd9).contains(&marker) {
            at += 2;

            continue;
        }

        let length = u16::from_be_bytes([bytes[at + 2], bytes[at + 3]]) as usize;
        let starts_frame = (0xc0..=0xcf).contains(&marker) && !matches!(marker, 0xc4 | 0xc8 | 0xcc);

        if starts_frame {
            let height = u16::from_be_bytes([*bytes.get(at + 5)?, *bytes.get(at + 6)?]) as u64;
            let width = u16::from_be_bytes([*bytes.get(at + 7)?, *bytes.get(at + 8)?]) as u64;

            return Some((width, height));
        }

        at += 2 + length;
    }

    None
}

fn webp_size(bytes: &[u8]) -> Option<(u64, u64)> {
    match bytes.get(12..16)? {
        b"VP8 " => Some((le16(bytes, 26)? & 0x3fff, le16(bytes, 28)? & 0x3fff)),
        b"VP8L" => {
            let packed = u32::from_le_bytes(bytes.get(21..25)?.try_into().ok()?) as u64;

            Some(((packed & 0x3fff) + 1, ((packed >> 14) & 0x3fff) + 1))
        }
        b"VP8X" => Some((le24(bytes, 24)? + 1, le24(bytes, 27)? + 1)),
        _ => None,
    }
}

fn avif_size(bytes: &[u8]) -> Option<(u64, u64)> {
    bytes.windows(4).enumerate().filter(|(_, window)| *window == b"ispe").filter_map(|(at, _)| Some((be32(bytes, at + 8)?, be32(bytes, at + 12)?))).max_by_key(|(width, height)| width * height)
}

pub fn sniff(bytes: &[u8]) -> Option<(&'static str, u64, u64)> {
    let with = |media_type: &'static str, size: Option<(u64, u64)>| size.map(|(width, height)| (media_type, width, height));

    if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        return with("image/png", be32(bytes, 16).zip(be32(bytes, 20)));
    }

    if bytes.starts_with(&[0xff, 0xd8, 0xff]) {
        return with("image/jpeg", jpeg_size(bytes));
    }

    if bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a") {
        return with("image/gif", le16(bytes, 6).zip(le16(bytes, 8)));
    }

    if bytes.starts_with(b"RIFF") && bytes.get(8..12) == Some(b"WEBP") {
        return with("image/webp", webp_size(bytes));
    }

    if bytes.get(4..8) == Some(b"ftyp") && (bytes.get(8..12) == Some(b"avif") || bytes.get(8..12) == Some(b"avis")) {
        return with("image/avif", avif_size(bytes));
    }

    None
}

#[derive(serde::Deserialize)]
pub struct Requested {
    url: String,
}

struct Row {
    allowed_by: String,
    hops: Vec<String>,
    sha256: Option<String>,
    media_type: Option<String>,
    refusal: Option<String>,
    refusal_is_fresh: bool,
}

async fn cached(state: &AppState, owner: Uuid, conversation: Uuid, address: &str) -> Result<Option<Row>, ImageError> {
    let row = sqlx::query_as::<_, (String, Vec<String>, Option<String>, Option<String>, Option<String>, bool)>(
        "select allowed_by, hops, sha256, media_type, refusal, fetched_at > now() - make_interval(mins => $4) from remote_images where owner = $1 and conversation_id = $2 and url = $3",
    )
    .bind(owner)
    .bind(conversation)
    .bind(address)
    .bind(REFUSAL_MINUTES as i32)
    .fetch_optional(&state.pool)
    .await?;

    Ok(row.map(|(allowed_by, hops, sha256, media_type, refusal, refusal_is_fresh)| Row { allowed_by, hops, sha256, media_type, refusal, refusal_is_fresh }))
}

fn served(bytes: Vec<u8>, sha256: &str, media_type: &str) -> Response {
    (
        [
            (header::CONTENT_TYPE, media_type.to_string()),
            (header::CACHE_CONTROL, "private, max-age=86400".to_string()),
            (header::ETAG, format!("\"{sha256}\"")),
            (header::CONTENT_DISPOSITION, "inline".to_string()),
            (header::X_CONTENT_TYPE_OPTIONS, "nosniff".to_string()),
            (header::CONTENT_SECURITY_POLICY, "sandbox".to_string()),
        ],
        bytes,
    )
        .into_response()
}

async fn from_cache(state: &AppState, row: &Row) -> Option<Response> {
    let (sha256, media_type) = (row.sha256.as_deref()?, row.media_type.as_deref()?);
    let bytes = tokio::fs::read(path_of(state, sha256)).await.ok()?;

    Some(served(bytes, sha256, media_type))
}

async fn remember(state: &AppState, owner: Uuid, conversation: Uuid, address: &str, allowed_by: &str, outcome: Result<(&str, &Fetched), &Refusal>) -> Result<(), ImageError> {
    let (sha256, media_type, hops, refusal) = match outcome {
        Ok((sha256, fetched)) => (Some(sha256), Some(fetched.media_type), fetched.hops.clone(), None),
        Err(refusal) => (None, None, Vec::new(), Some(refusal.stored())),
    };
    let upgrade = allowed_by == FROM_CLICK;

    sqlx::query(
        "insert into remote_images (owner, conversation_id, url, allowed_by, hops, sha256, media_type, refusal) values ($1, $2, $3, $4, $5, $6, $7, $8)
         on conflict (owner, conversation_id, url) do update set
           allowed_by = case when $9 then $4 else remote_images.allowed_by end,
           hops = case when $6::text is null then remote_images.hops else $5::text[] end,
           sha256 = $6, media_type = $7, refusal = $8, fetched_at = now()",
    )
    .bind(owner)
    .bind(conversation)
    .bind(address)
    .bind(allowed_by)
    .bind(hops)
    .bind(sha256)
    .bind(media_type)
    .bind(refusal)
    .bind(upgrade)
    .execute(&state.pool)
    .await?;

    Ok(())
}

async fn load(state: &AppState, owner: Uuid, conversation: Uuid, address: &str, first: &Url, trust: &Trust, allowed_by: &str) -> Result<Response, ImageError> {
    let fetched = fetch(state, first, trust).await;

    match fetched {
        Ok(fetched) => {
            let (sha256, _, _) = store(state, owner, &fetched.bytes, fetched.media_type).await?;

            remember(state, owner, conversation, address, allowed_by, Ok((&sha256, &fetched))).await?;

            Ok(served(fetched.bytes, &sha256, fetched.media_type))
        }
        Err(refusal) if refusal.code == "image_needs_click" => Err(refusal.into()),
        Err(refusal) => {
            remember(state, owner, conversation, address, allowed_by, Err(&refusal)).await?;

            Err(refusal.into())
        }
    }
}

async fn recorded_addresses(state: &AppState, owner: Uuid, conversation: Uuid) -> Result<HashSet<String>, ImageError> {
    let recorded = sqlx::query_scalar::<_, String>("select i.url from integration_call_images i join integration_calls c on c.id = i.call_id where c.owner = $1 and c.conversation_id = $2").bind(owner).bind(conversation).fetch_all(&state.pool).await?;

    Ok(recorded.into_iter().flat_map(|address| [Url::parse(&address).map(|parsed| parsed.to_string()).unwrap_or_default(), address]).collect())
}

pub async fn show(State(state): State<AppState>, user: AuthUser, Path(id): Path<Uuid>, Query(query): Query<Requested>) -> Result<Response, ImageError> {
    find_owned(&state.pool, id, user.id).await?;

    let first = requested(&query.url)?;
    let row = cached(&state, user.id, id, &query.url).await?;

    if let Some(row) = &row {
        if let Some(response) = from_cache(&state, row).await {
            return Ok(response);
        }

        if let (Some(stored), true) = (row.refusal.as_deref().and_then(Refusal::from_stored), row.refusal_is_fresh) {
            return Err(stored.into());
        }

        if row.allowed_by == FROM_CLICK {
            return Err(Refusal::needs_click(&host_name(&query.url)).into());
        }
    }

    let recorded = recorded_addresses(&state, user.id, id).await?;

    if !recorded.contains(&query.url) {
        return Err(Refusal::needs_click(&host_name(&query.url)).into());
    }

    let previous = row.map(|row| row.hops).unwrap_or_default();
    let trust = Trust::Recorded { host: first.host_str().unwrap_or_default().to_string(), recorded, previous };

    load(&state, user.id, id, &query.url, &first, &trust, FROM_SEARCH).await
}

pub async fn load_on_click(State(state): State<AppState>, user: AuthUser, Path(id): Path<Uuid>, Json(query): Json<Requested>) -> Result<Response, ImageError> {
    find_owned(&state.pool, id, user.id).await?;

    let first = requested(&query.url)?;
    let row = cached(&state, user.id, id, &query.url).await?;

    if let Some(response) = match &row {
        Some(row) => from_cache(&state, row).await,
        None => None,
    } {
        return Ok(response);
    }

    load(&state, user.id, id, &query.url, &first, &Trust::Untrusted, FROM_CLICK).await
}

#[cfg(test)]
mod tests {
    use super::*;

    fn png(width: u32, height: u32) -> Vec<u8> {
        let mut bytes = b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR".to_vec();

        bytes.extend(width.to_be_bytes());
        bytes.extend(height.to_be_bytes());
        bytes.extend([8, 6, 0, 0, 0]);

        bytes
    }

    #[test]
    fn png_gif_jpeg_webp_and_avif_are_recognised_from_their_bytes_with_their_sizes() {
        let gif = [b"GIF89a".as_slice(), &[10, 0, 20, 0]].concat();
        let jpeg = [0xff, 0xd8, 0xff, 0xe0, 0, 4, 0, 0, 0xff, 0xc0, 0, 11, 8, 0x01, 0x2c, 0x02, 0x58, 3];
        let lossless = [b"RIFF\0\0\0\0WEBPVP8L\0\0\0\0\x2f".as_slice(), &(99u32 | (49u32 << 14)).to_le_bytes()].concat();
        let extended = [b"RIFF\0\0\0\0WEBPVP8X\0\0\0\0\0\0\0\0".as_slice(), &[0x1f, 0x03, 0x00, 0xe7, 0x01, 0x00]].concat();
        let avif = [b"\0\0\0\x18ftypavif\0\0\0\0avifmif1".as_slice(), b"\0\0\0\x14ispe\0\0\0\0\0\0\x03\x20\0\0\x02\x58"].concat();

        assert_eq!(sniff(&png(640, 480)), Some(("image/png", 640, 480)));
        assert_eq!(sniff(&gif), Some(("image/gif", 10, 20)));
        assert_eq!(sniff(&jpeg), Some(("image/jpeg", 600, 300)));
        assert_eq!(sniff(&lossless), Some(("image/webp", 100, 50)));
        assert_eq!(sniff(&extended), Some(("image/webp", 800, 488)));
        assert_eq!(sniff(&avif), Some(("image/avif", 800, 600)));
    }

    #[test]
    fn svg_html_and_unreadable_headers_are_not_images() {
        assert_eq!(sniff(b"<svg xmlns=\"http://www.w3.org/2000/svg\"></svg>"), None);
        assert_eq!(sniff(b"<!doctype html><title>Not an image</title>"), None);
        assert_eq!(sniff(b"\x89PNG\r\n\x1a\n"), None);
    }

    #[test]
    fn a_literal_address_is_refused_even_when_public_and_http_is_refused_by_name() {
        let refusal = |address: &str| requested(address).err().map(|refusal| refusal.code);

        assert_eq!(refusal("https://93.184.215.14/a.png"), Some("image_address_refused"));
        assert_eq!(refusal("https://10.0.0.1/a.png"), Some("image_private_address"));
        assert_eq!(refusal("http://10.0.0.1/a.png"), Some("image_private_address"));
        assert_eq!(refusal("http://example.com/a.png"), Some("image_not_https"));
        assert_eq!(refusal("https://user:word@example.com/a.png"), Some("image_address_refused"));
        assert_eq!(refusal("https://example.com/a.png"), None);
    }
}
