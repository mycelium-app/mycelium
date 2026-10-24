use anyhow::{Result, bail};
use std::net::{IpAddr, SocketAddr};
use std::path::PathBuf;
use std::time::Duration;

pub const DEVICE_CODE_LIFETIME: Duration = Duration::from_secs(600);
pub const VERSION_CHECK_INTERVAL: Duration = Duration::from_secs(3600);
pub const SCHEDULER_TICK: Duration = Duration::from_secs(15);

#[derive(Debug, Clone)]
pub struct OidcConfig {
    pub issuer: String,
    pub client_id: String,
    pub client_secret: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Cidr {
    network: IpAddr,
    prefix: u8,
}

impl Cidr {
    pub fn parse(text: &str) -> Result<Cidr> {
        let (address, prefix) = match text.trim().split_once('/') {
            Some((address, prefix)) => (address, Some(prefix)),
            None => (text.trim(), None),
        };
        let network: IpAddr = address.parse().map_err(|_| anyhow::anyhow!("{text:?} is not an address or a CIDR range."))?;
        let longest = if network.is_ipv4() { 32 } else { 128 };
        let prefix = match prefix {
            Some(prefix) => prefix.parse::<u8>().ok().filter(|prefix| *prefix <= longest).ok_or_else(|| anyhow::anyhow!("{text:?} has a prefix length outside 0 to {longest}."))?,
            None => longest,
        };

        Ok(Cidr { network, prefix })
    }

    pub fn contains(&self, address: IpAddr) -> bool {
        match (self.network, address) {
            (IpAddr::V4(network), IpAddr::V4(address)) => shared_prefix(u128::from(u32::from(network)), u128::from(u32::from(address)), 32, self.prefix),
            (IpAddr::V6(network), IpAddr::V6(address)) => shared_prefix(u128::from(network), u128::from(address), 128, self.prefix),
            _ => false,
        }
    }
}

fn shared_prefix(left: u128, right: u128, width: u8, prefix: u8) -> bool {
    prefix == 0 || (left ^ right) >> (width - prefix) == 0
}

#[derive(Debug, Clone)]
pub struct Config {
    pub database_url: String,
    pub listen: SocketAddr,
    pub public_url: String,
    pub data_dir: PathBuf,
    pub oidc: Option<OidcConfig>,
    pub dev_login: Option<String>,
    pub allowed_emails: Vec<String>,
    pub web_dir: PathBuf,
    pub editor_dir: PathBuf,
    pub artifact_domain: Option<String>,
    pub artifact_host_token: Option<String>,
    pub browser_url: Option<String>,
    pub skip_migrations: bool,
    pub trusted_proxies: Vec<Cidr>,
    pub device_code_lifetime: Duration,
    pub release_url: String,
    pub npm_registry_url: String,
    pub version_check_interval: Duration,
    pub scheduler_tick: Duration,
    pub forge_cache_ttl: Duration,
}

type Lookup<'a> = &'a dyn Fn(&str) -> Option<String>;

fn optional(lookup: Lookup, name: &str) -> Option<String> {
    lookup(name).filter(|value| !value.trim().is_empty())
}

fn required(lookup: Lookup, name: &str) -> Result<String> {
    match optional(lookup, name) {
        Some(value) => Ok(value),
        None => bail!("Set {name}."),
    }
}

pub fn is_localhost(public_url: &str) -> bool {
    ["http://localhost", "http://127.0.0.1"].iter().any(|prefix| {
        public_url.strip_prefix(prefix).is_some_and(|rest| rest.is_empty() || rest.starts_with(':') || rest.starts_with('/'))
    })
}

impl Config {
    pub fn from_env() -> Result<Config> {
        Config::from_lookup(&|name| std::env::var(name).ok())
    }

    pub fn from_lookup(lookup: Lookup) -> Result<Config> {
        let public_url = required(lookup, "MYCELIUM_PUBLIC_URL")?.trim_end_matches('/').to_string();
        let dev_login = optional(lookup, "MYCELIUM_DEV_LOGIN");

        if dev_login.is_some() && !is_localhost(&public_url) {
            bail!("MYCELIUM_DEV_LOGIN is refused unless MYCELIUM_PUBLIC_URL is http://localhost or http://127.0.0.1.");
        }

        let oidc = match dev_login {
            Some(_) => None,
            None => Some(OidcConfig {
                issuer: required(lookup, "MYCELIUM_OIDC_ISSUER")?,
                client_id: required(lookup, "MYCELIUM_OIDC_CLIENT_ID")?,
                client_secret: optional(lookup, "MYCELIUM_OIDC_CLIENT_SECRET"),
            }),
        };

        Ok(Config {
            database_url: required(lookup, "MYCELIUM_DATABASE_URL")?,
            listen: optional(lookup, "MYCELIUM_LISTEN").unwrap_or_else(|| "0.0.0.0:8080".into()).parse()?,
            public_url,
            data_dir: optional(lookup, "MYCELIUM_DATA_DIR").unwrap_or_else(|| "/var/lib/mycelium".into()).into(),
            oidc,
            dev_login,
            allowed_emails: optional(lookup, "MYCELIUM_ALLOWED_EMAILS").map(|emails| emails.split(',').map(|email| email.trim().to_lowercase()).filter(|email| !email.is_empty()).collect()).unwrap_or_default(),
            web_dir: optional(lookup, "MYCELIUM_WEB_DIR").unwrap_or_else(|| "/usr/share/mycelium/web".into()).into(),
            editor_dir: optional(lookup, "MYCELIUM_EDITOR_DIR").unwrap_or_else(|| "/usr/share/mycelium/editor".into()).into(),
            artifact_domain: optional(lookup, "MYCELIUM_ARTIFACT_DOMAIN").map(|domain| domain.trim().trim_matches('.').to_lowercase()).filter(|domain| !domain.is_empty()),
            artifact_host_token: optional(lookup, "MYCELIUM_ARTIFACT_HOST_TOKEN"),
            browser_url: optional(lookup, "MYCELIUM_BROWSER_URL"),
            skip_migrations: optional(lookup, "MYCELIUM_SKIP_MIGRATIONS").is_some_and(|value| value == "true" || value == "1"),
            trusted_proxies: optional(lookup, "MYCELIUM_TRUSTED_PROXIES")
                .map(|ranges| ranges.split(',').map(str::trim).filter(|range| !range.is_empty()).map(Cidr::parse).collect::<Result<Vec<_>>>())
                .transpose()?
                .unwrap_or_default(),
            device_code_lifetime: DEVICE_CODE_LIFETIME,
            release_url: optional(lookup, "MYCELIUM_RELEASE_URL").unwrap_or_else(|| "https://github.com/mycelium-app/mycelium/releases/latest/download/latest".into()),
            npm_registry_url: optional(lookup, "MYCELIUM_NPM_REGISTRY_URL").unwrap_or_else(|| "https://registry.npmjs.org".into()),
            version_check_interval: VERSION_CHECK_INTERVAL,
            scheduler_tick: SCHEDULER_TICK,
            forge_cache_ttl: crate::forge_cache::FORGE_CACHE_TTL,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn config_from(pairs: &[(&str, &str)]) -> Result<Config> {
        let values: HashMap<String, String> = pairs.iter().map(|(key, value)| (key.to_string(), value.to_string())).collect();

        Config::from_lookup(&|name| values.get(name).cloned())
    }

    #[test]
    fn dev_login_is_refused_on_a_public_address() {
        let result = config_from(&[
            ("MYCELIUM_DATABASE_URL", "postgres://x"),
            ("MYCELIUM_PUBLIC_URL", "https://mycelium.example.com"),
            ("MYCELIUM_DEV_LOGIN", "me@example.com"),
        ]);

        assert!(result.is_err());
    }

    #[test]
    fn dev_login_is_allowed_on_localhost_without_oidc() {
        let config = config_from(&[
            ("MYCELIUM_DATABASE_URL", "postgres://x"),
            ("MYCELIUM_PUBLIC_URL", "http://127.0.0.1:8080"),
            ("MYCELIUM_DEV_LOGIN", "me@example.com"),
        ])
        .unwrap();

        assert!(config.oidc.is_none());
        assert_eq!(config.dev_login.as_deref(), Some("me@example.com"));
    }

    #[test]
    fn allowed_emails_are_split_trimmed_and_lowercased() {
        let config = config_from(&[
            ("MYCELIUM_DATABASE_URL", "postgres://x"),
            ("MYCELIUM_PUBLIC_URL", "http://127.0.0.1:8080"),
            ("MYCELIUM_DEV_LOGIN", "me@example.com"),
            ("MYCELIUM_ALLOWED_EMAILS", " Me@Example.com, you@example.com ,"),
        ])
        .unwrap();

        assert_eq!(config.allowed_emails, ["me@example.com", "you@example.com"]);
    }

    #[test]
    fn trusted_proxies_default_to_none_and_parse_ranges_and_single_addresses() {
        let base = [("MYCELIUM_DATABASE_URL", "postgres://x"), ("MYCELIUM_PUBLIC_URL", "http://127.0.0.1:8080"), ("MYCELIUM_DEV_LOGIN", "me@example.com")];
        assert!(config_from(&base).unwrap().trusted_proxies.is_empty());

        let mut listed = base.to_vec();
        listed.push(("MYCELIUM_TRUSTED_PROXIES", "10.0.0.0/8, ::1, 192.168.1.7"));
        let proxies = config_from(&listed).unwrap().trusted_proxies;

        assert!(proxies[0].contains("10.200.0.1".parse().unwrap()));
        assert!(!proxies[0].contains("11.0.0.1".parse().unwrap()));
        assert!(proxies[1].contains("::1".parse().unwrap()));
        assert!(proxies[2].contains("192.168.1.7".parse().unwrap()));
        assert!(!proxies[2].contains("192.168.1.8".parse().unwrap()));
    }

    #[test]
    fn a_trusted_proxy_range_that_is_not_one_is_refused_with_a_fix() {
        let mut values = vec![("MYCELIUM_DATABASE_URL", "postgres://x"), ("MYCELIUM_PUBLIC_URL", "http://127.0.0.1:8080"), ("MYCELIUM_DEV_LOGIN", "me@example.com")];
        values.push(("MYCELIUM_TRUSTED_PROXIES", "10.0.0.0/33"));

        assert!(config_from(&values).unwrap_err().to_string().contains("prefix length"));
    }

    #[test]
    fn a_hostname_that_only_starts_with_localhost_is_not_localhost() {
        assert!(!is_localhost("http://localhost.evil.example"));
        assert!(is_localhost("http://localhost:3000"));
    }

    #[test]
    fn the_artifact_domain_is_lowercased_and_loses_surrounding_dots() {
        let config = config_from(&[
            ("MYCELIUM_DATABASE_URL", "postgres://x"),
            ("MYCELIUM_PUBLIC_URL", "http://127.0.0.1:8080"),
            ("MYCELIUM_DEV_LOGIN", "me@example.com"),
            ("MYCELIUM_ARTIFACT_DOMAIN", " .Artifacts.Example.com. "),
        ])
        .unwrap();

        assert_eq!(config.artifact_domain.as_deref(), Some("artifacts.example.com"));
    }

    #[test]
    fn oidc_settings_are_required_without_dev_login() {
        let result = config_from(&[("MYCELIUM_DATABASE_URL", "postgres://x"), ("MYCELIUM_PUBLIC_URL", "https://mycelium.example.com")]);

        assert!(result.unwrap_err().to_string().contains("MYCELIUM_OIDC_ISSUER"));
    }

    #[test]
    fn defaults_apply_and_the_public_url_loses_its_trailing_slash() {
        let config = config_from(&[
            ("MYCELIUM_DATABASE_URL", "postgres://x"),
            ("MYCELIUM_PUBLIC_URL", "https://mycelium.example.com/"),
            ("MYCELIUM_OIDC_ISSUER", "https://id.example.com"),
            ("MYCELIUM_OIDC_CLIENT_ID", "mycelium"),
        ])
        .unwrap();

        assert_eq!(config.public_url, "https://mycelium.example.com");
        assert_eq!(config.listen.to_string(), "0.0.0.0:8080");
        assert_eq!(config.web_dir, PathBuf::from("/usr/share/mycelium/web"));
        assert_eq!(config.editor_dir, PathBuf::from("/usr/share/mycelium/editor"));
        assert!(config.artifact_domain.is_none());
        assert!(config.artifact_host_token.is_none());
        assert!(!config.skip_migrations);
    }
}
