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

fn optional(name: &str) -> Option<String> {
    std::env::var(name).ok().filter(|value| !value.trim().is_empty())
}

fn required(name: &str) -> Result<String> {
    match optional(name) {
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
        let public_url = required("MYCELIUM_PUBLIC_URL")?.trim_end_matches('/').to_string();
        let dev_login = optional("MYCELIUM_DEV_LOGIN");

        if dev_login.is_some() && !is_localhost(&public_url) {
            bail!("MYCELIUM_DEV_LOGIN is refused unless MYCELIUM_PUBLIC_URL is http://localhost or http://127.0.0.1.");
        }

        let oidc = match dev_login {
            Some(_) => None,
            None => Some(OidcConfig {
                issuer: required("MYCELIUM_OIDC_ISSUER")?,
                client_id: required("MYCELIUM_OIDC_CLIENT_ID")?,
                client_secret: optional("MYCELIUM_OIDC_CLIENT_SECRET"),
            }),
        };

        Ok(Config {
            database_url: required("MYCELIUM_DATABASE_URL")?,
            listen: optional("MYCELIUM_LISTEN").unwrap_or_else(|| "0.0.0.0:8080".into()).parse()?,
            public_url,
            data_dir: optional("MYCELIUM_DATA_DIR").unwrap_or_else(|| "/var/lib/mycelium".into()).into(),
            oidc,
            dev_login,
            web_dir: optional("MYCELIUM_WEB_DIR").unwrap_or_else(|| "/usr/share/mycelium/web".into()).into(),
            skip_migrations: optional("MYCELIUM_SKIP_MIGRATIONS").is_some_and(|value| value == "true" || value == "1"),
        })
    }
}
