use reqwest::dns::{Addrs, Name, Resolve, Resolving};
use std::fmt::Debug;
use std::future::Future;
use crate::config::PublicFetchRouting;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::pin::Pin;
use std::task::{Context, Poll};
use std::time::Duration;
use tower::{Layer, Service};
use url::{Host, Url};

pub const BODY_LIMIT: usize = 16 * 1024 * 1024;
pub const AUDIO_LIMIT: usize = 64 * 1024 * 1024;
pub const LINE_LIMIT: usize = 1024 * 1024;

const REDIRECT_HOPS: usize = 5;

type BoxError = Box<dyn std::error::Error + Send + Sync>;

#[derive(Debug)]
pub enum Oversize {
    Larger,
    Stopped,
}

pub fn refused_address(address: IpAddr) -> bool {
    match address {
        IpAddr::V4(address) => address.is_link_local() || address.is_unspecified() || address.is_multicast() || address.is_broadcast(),
        IpAddr::V6(address) => {
            let segments = address.segments();
            let link_local = segments[0] & 0xffc0 == 0xfe80;
            let aws_metadata = address.octets() == [0xfd, 0x00, 0x0e, 0xc2, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0x02, 0x54];

            match address.to_ipv4_mapped() {
                Some(mapped) => refused_address(IpAddr::V4(mapped)),
                None => link_local || aws_metadata || address.is_unspecified() || address.is_multicast(),
            }
        }
    }
}

pub fn refused_public_address(address: IpAddr) -> bool {
    match address {
        IpAddr::V4(address) => {
            let octets = address.octets();
            let carrier_grade_nat = octets[0] == 100 && octets[1] & 0xc0 == 64;
            let benchmarking = octets[0] == 198 && octets[1] & 0xfe == 18;

            refused_address(IpAddr::V4(address)) || address.is_loopback() || address.is_private() || carrier_grade_nat || benchmarking || octets[0] == 0
        }
        IpAddr::V6(address) => {
            let segments = address.segments();
            let unique_local = segments[0] & 0xfe00 == 0xfc00;
            let six_to_four = (segments[0] == 0x2002).then(|| Ipv4Addr::new((segments[1] >> 8) as u8, segments[1] as u8, (segments[2] >> 8) as u8, segments[2] as u8));
            let nat64 = (segments[0] == 0x0064 && segments[1] == 0xff9b && segments[2..6] == [0, 0, 0, 0]).then(|| Ipv4Addr::new((segments[6] >> 8) as u8, segments[6] as u8, (segments[7] >> 8) as u8, segments[7] as u8));

            match address.to_ipv4_mapped().or(six_to_four).or(nat64) {
                Some(embedded) => refused_public_address(IpAddr::V4(embedded)),
                None => refused_address(IpAddr::V6(address)) || address.is_loopback() || unique_local,
            }
        }
    }
}

#[derive(Debug)]
pub struct RefusedDestination(pub String);

impl std::fmt::Display for RefusedDestination {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl std::error::Error for RefusedDestination {}

pub fn was_refused(error: &reqwest::Error) -> bool {
    let mut source: Option<&(dyn std::error::Error + 'static)> = Some(error);

    while let Some(current) = source {
        if current.is::<RefusedDestination>() {
            return true;
        }

        source = current.source();
    }

    format!("{error:?}").contains("RefusedDestination(")
}

#[derive(Clone)]
struct Policy {
    refuses: fn(IpAddr) -> bool,
    routing: PublicFetchRouting,
}

impl Policy {
    fn refuses(&self, address: IpAddr) -> bool {
        (self.refuses)(address) && !(self.routing.allow_loopback && address.is_loopback())
    }
}

struct Resolver {
    policy: Policy,
}

impl Resolve for Resolver {
    fn resolve(&self, name: Name) -> Resolving {
        let host = name.as_str().to_string();
        let policy = self.policy.clone();

        Box::pin(async move {
            let found: Vec<SocketAddr> = match policy.routing.hosts.iter().find(|(mapped, _)| *mapped == host) {
                Some((_, address)) => vec![*address],
                None => tokio::net::lookup_host((host.as_str(), 0)).await?.collect(),
            };
            let allowed: Vec<SocketAddr> = found.into_iter().filter(|address| !policy.refuses(address.ip())).collect();

            if allowed.is_empty() {
                return Err(BoxError::from(RefusedDestination(format!("{host} resolves only to addresses Mycelium does not connect to"))));
            }

            let addresses: Addrs = Box::new(allowed.into_iter());

            Ok(addresses)
        })
    }
}

#[derive(Clone)]
struct LiteralGuard {
    policy: Policy,
}

#[derive(Clone)]
struct LiteralGuarded<S> {
    inner: S,
    policy: Policy,
}

impl<S> Layer<S> for LiteralGuard {
    type Service = LiteralGuarded<S>;

    fn layer(&self, inner: S) -> Self::Service {
        LiteralGuarded { inner, policy: self.policy.clone() }
    }
}

fn destination_of(request: &impl Debug) -> Option<Url> {
    let shown = format!("{request:?}");
    let start = shown.find('(')? + 1;
    let end = shown.rfind(')')?;

    Url::parse(shown.get(start..end)?).ok()
}

impl<S, R> Service<R> for LiteralGuarded<S>
where
    S: Service<R, Error = BoxError>,
    S::Future: Send + 'static,
    S::Response: Send + 'static,
    R: Debug,
{
    type Response = S::Response;
    type Error = BoxError;
    type Future = Pin<Box<dyn Future<Output = Result<S::Response, BoxError>> + Send>>;

    fn poll_ready(&mut self, context: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(context)
    }

    fn call(&mut self, request: R) -> Self::Future {
        let destination = destination_of(&request);
        let literal = match destination.as_ref().and_then(Url::host) {
            Some(Host::Ipv4(address)) => Some(IpAddr::V4(address)),
            Some(Host::Ipv6(address)) => Some(IpAddr::V6(address)),
            _ => None,
        };

        if destination.is_none() || literal.is_some_and(|address| self.policy.refuses(address)) {
            return Box::pin(async { Err(BoxError::from(RefusedDestination("that address is link-local or a metadata address, which Mycelium does not connect to".to_string()))) });
        }

        Box::pin(self.inner.call(request))
    }
}

fn builder(policy: Policy) -> reqwest::ClientBuilder {
    reqwest::Client::builder().timeout(Duration::from_secs(10)).redirect(reqwest::redirect::Policy::none()).dns_resolver(Resolver { policy: policy.clone() }).connector_layer(LiteralGuard { policy })
}

pub fn client() -> reqwest::Result<reqwest::Client> {
    builder(Policy { refuses: refused_address, routing: PublicFetchRouting::default() }).build()
}

pub fn public_client(routing: &PublicFetchRouting) -> reqwest::Result<reqwest::Client> {
    let mut headers = reqwest::header::HeaderMap::new();

    headers.insert(reqwest::header::ACCEPT, reqwest::header::HeaderValue::from_static("image/avif,image/webp,image/png,image/jpeg,image/gif"));

    let mut builder = builder(Policy { refuses: refused_public_address, routing: routing.clone() }).https_only(true).user_agent("mycelium").default_headers(headers);

    if let Some(pem) = &routing.trusted_root_pem {
        builder = builder.add_root_certificate(reqwest::Certificate::from_pem(pem)?);
    }

    builder.build()
}

pub enum Stopped<Refusal> {
    Request(reqwest::Error),
    Refused(Refusal),
}

pub struct Followed {
    pub response: reqwest::Response,
    pub chain: Vec<Url>,
}

pub async fn get_following_redirects<Refusal>(http: &reqwest::Client, address: &str, mut check: impl FnMut(&Url) -> Result<(), Refusal>) -> Result<Followed, Stopped<Refusal>> {
    let mut target = address.to_string();
    let mut chain = Vec::new();

    loop {
        if let Ok(parsed) = Url::parse(&target) {
            check(&parsed).map_err(Stopped::Refused)?;
            chain.push(parsed);
        }

        let response = http.get(&target).send().await.map_err(Stopped::Request)?;
        let next = response.headers().get(reqwest::header::LOCATION).and_then(|value| value.to_str().ok()).and_then(|location| response.url().join(location).ok());

        match next {
            Some(next) if response.status().is_redirection() && chain.len() <= REDIRECT_HOPS => target = next.to_string(),
            _ => return Ok(Followed { response, chain }),
        }
    }
}

pub async fn read_limited(mut response: reqwest::Response, limit: usize) -> Result<Vec<u8>, Oversize> {
    if response.content_length().is_some_and(|length| length > limit as u64) {
        return Err(Oversize::Larger);
    }

    let mut body = Vec::new();

    while let Some(chunk) = response.chunk().await.map_err(|_| Oversize::Stopped)? {
        if body.len() + chunk.len() > limit {
            return Err(Oversize::Larger);
        }

        body.extend_from_slice(&chunk);
    }

    Ok(body)
}

pub async fn read_text_limited(response: reqwest::Response, limit: usize) -> Result<String, Oversize> {
    let body = read_limited(response, limit).await?;

    Ok(String::from_utf8_lossy(&body).into_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn the_client_refuses_a_metadata_address_written_as_an_ip() {
        let error = client().unwrap().get("http://169.254.169.254/latest/meta-data/").send().await.unwrap_err();

        assert!(format!("{error:?}").contains("metadata"), "{error:?}");
    }

    #[tokio::test]
    async fn the_client_refuses_a_link_local_ipv6_address() {
        let error = client().unwrap().get("http://[fe80::1]/").send().await.unwrap_err();

        assert!(format!("{error:?}").contains("metadata"), "{error:?}");
    }

    #[tokio::test]
    async fn the_client_still_reaches_loopback_and_does_not_follow_redirects() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();

        tokio::spawn(async move {
            use tokio::io::{AsyncReadExt, AsyncWriteExt};

            let (mut stream, _) = listener.accept().await.unwrap();
            let mut buffer = [0u8; 1024];
            let read = stream.read(&mut buffer).await.unwrap();
            assert!(read > 0);
            stream.write_all(b"HTTP/1.1 307 Temporary Redirect\r\nlocation: http://169.254.169.254/\r\ncontent-length: 0\r\n\r\n").await.unwrap();
        });

        let response = client().unwrap().get(format!("http://127.0.0.1:{port}/")).send().await.unwrap();

        assert_eq!(response.status().as_u16(), 307);
    }

    #[test]
    fn link_local_unspecified_and_mapped_metadata_addresses_are_refused_but_lan_addresses_are_not() {
        for refused in ["169.254.169.254", "0.0.0.0", "fe80::1", "fd00:ec2::254", "::ffff:169.254.169.254"] {
            assert!(refused_address(refused.parse().unwrap()), "{refused}");
        }

        for allowed in ["127.0.0.1", "192.168.1.20", "10.0.0.5", "100.64.0.1", "::1", "fd12::1", "93.184.216.34"] {
            assert!(!refused_address(allowed.parse().unwrap()), "{allowed}");
        }
    }

    #[test]
    fn the_public_guard_refuses_private_loopback_and_embedded_private_addresses_but_allows_public_ones() {
        let refused = ["127.0.0.1", "10.1.2.3", "172.16.0.1", "192.168.1.20", "100.64.0.1", "0.1.2.3", "198.18.0.1", "::1", "fd12::1", "::ffff:192.168.1.20", "169.254.169.254", "2002:c0a8:0114::1", "64:ff9b::c0a8:0114"];
        let allowed = ["93.184.215.14", "2001:db8::1"];

        for address in refused {
            assert!(refused_public_address(address.parse().unwrap()), "{address}");
        }

        for address in allowed {
            assert!(!refused_public_address(address.parse().unwrap()), "{address}");
        }
    }

    #[tokio::test]
    async fn the_public_client_refuses_loopback_by_literal_and_by_name() {
        let client = public_client(&PublicFetchRouting::default()).unwrap();

        for address in ["https://127.0.0.1/", "https://localhost/", "https://[::1]/"] {
            let error = client.get(address).send().await.unwrap_err();

            assert!(was_refused(&error), "{address}: {error:?}");
        }
    }

    #[tokio::test]
    async fn the_public_client_refuses_plain_http() {
        let error = public_client(&PublicFetchRouting::default()).unwrap().get("http://example.com/").send().await.unwrap_err();

        assert!(error.is_builder() || error.to_string().contains("https"), "{error:?}");
    }

    #[tokio::test]
    async fn following_redirects_stops_before_requesting_a_hop_the_check_rejects_and_returns_the_chain_it_followed() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let requests = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let counted = requests.clone();

        tokio::spawn(async move {
            loop {
                let (mut stream, _) = listener.accept().await.unwrap();
                let mut buffer = [0u8; 1024];
                let read = stream.read(&mut buffer).await.unwrap();
                let first = String::from_utf8_lossy(&buffer[..read]).lines().next().unwrap_or_default().to_string();

                counted.fetch_add(1, std::sync::atomic::Ordering::SeqCst);

                let answer = match () {
                    _ if first.contains("/start") => "HTTP/1.1 302 Found\r\nlocation: /middle\r\ncontent-length: 0\r\n\r\n".to_string(),
                    _ if first.contains("/middle") => format!("HTTP/1.1 302 Found\r\nlocation: http://127.0.0.1:{port}/final\r\ncontent-length: 0\r\n\r\n"),
                    _ => "HTTP/1.1 200 OK\r\ncontent-length: 0\r\n\r\n".to_string(),
                };

                stream.write_all(answer.as_bytes()).await.unwrap();
            }
        });

        let start = format!("http://127.0.0.1:{port}/start");
        let rejected = get_following_redirects(&client().unwrap(), &start, |hop| if hop.path() == "/final" { Err(hop.to_string()) } else { Ok(()) }).await;

        match rejected {
            Err(Stopped::Refused(hop)) => assert!(hop.ends_with("/final"), "{hop}"),
            _ => panic!("the rejected hop was followed"),
        }

        assert_eq!(requests.load(std::sync::atomic::Ordering::SeqCst), 2);

        let accepted = get_following_redirects(&client().unwrap(), &start, |_| Ok::<(), ()>(())).await.ok().unwrap();
        let paths: Vec<&str> = accepted.chain.iter().map(Url::path).collect();

        assert_eq!(paths, ["/start", "/middle", "/final"]);
        assert_eq!(accepted.response.status().as_u16(), 200);
    }
}
