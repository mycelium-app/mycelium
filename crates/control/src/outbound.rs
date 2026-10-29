use reqwest::dns::{Addrs, Name, Resolve, Resolving};
use std::fmt::Debug;
use std::future::Future;
use std::net::{IpAddr, SocketAddr};
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

struct Resolver;

impl Resolve for Resolver {
    fn resolve(&self, name: Name) -> Resolving {
        let host = name.as_str().to_string();

        Box::pin(async move {
            let found = tokio::net::lookup_host((host.as_str(), 0)).await?;
            let allowed: Vec<SocketAddr> = found.filter(|address| !refused_address(address.ip())).collect();

            if allowed.is_empty() {
                return Err(BoxError::from(format!("{host} resolves only to link-local or metadata addresses, which Mycelium does not connect to")));
            }

            let addresses: Addrs = Box::new(allowed.into_iter());

            Ok(addresses)
        })
    }
}

#[derive(Clone)]
struct LiteralGuard;

#[derive(Clone)]
struct LiteralGuarded<S> {
    inner: S,
}

impl<S> Layer<S> for LiteralGuard {
    type Service = LiteralGuarded<S>;

    fn layer(&self, inner: S) -> Self::Service {
        LiteralGuarded { inner }
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

        if destination.is_none() || literal.is_some_and(refused_address) {
            return Box::pin(async { Err(BoxError::from("that address is link-local or a metadata address, which Mycelium does not connect to")) });
        }

        Box::pin(self.inner.call(request))
    }
}

pub fn client() -> reqwest::Result<reqwest::Client> {
    reqwest::Client::builder().timeout(Duration::from_secs(10)).redirect(reqwest::redirect::Policy::none()).dns_resolver(Resolver).connector_layer(LiteralGuard).build()
}

pub async fn get_following_redirects(http: &reqwest::Client, address: &str) -> reqwest::Result<reqwest::Response> {
    let mut target = address.to_string();
    let mut hops = 0;

    loop {
        let response = http.get(&target).send().await?;
        let next = response.headers().get(reqwest::header::LOCATION).and_then(|value| value.to_str().ok()).and_then(|location| response.url().join(location).ok());

        match next {
            Some(next) if response.status().is_redirection() && hops < REDIRECT_HOPS => {
                target = next.to_string();
                hops += 1;
            }
            _ => return Ok(response),
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
            stream.read(&mut buffer).await.unwrap();
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
}
