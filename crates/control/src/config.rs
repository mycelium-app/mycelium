use anyhow::{Result, bail};
use std::net::SocketAddr;
use std::path::PathBuf;

#[derive(Debug, Clone)]
pub struct OidcConfig {
    pub issuer: String,
    pub client_id: String,
    pub client_secret: Option<String>,
}

#[derive(Debug, Clone)]
pub struct Config {
    pub database_url: String,
    pub listen: SocketAddr,
    pub public_url: String,
    pub data_dir: PathBuf,
    pub oidc: Option<OidcConfig>,
    pub dev_login: Option<String>,
    pub web_dir: PathBuf,
    pub skip_migrations: bool,
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
            web_dir: optional(lookup, "MYCELIUM_WEB_DIR").unwrap_or_else(|| "/usr/share/mycelium/web".into()).into(),
            skip_migrations: optional(lookup, "MYCELIUM_SKIP_MIGRATIONS").is_some_and(|value| value == "true" || value == "1"),
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
    fn a_hostname_that_only_starts_with_localhost_is_not_localhost() {
        assert!(!is_localhost("http://localhost.evil.example"));
        assert!(is_localhost("http://localhost:3000"));
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
        assert!(!config.skip_migrations);
    }
}
