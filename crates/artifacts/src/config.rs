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
    pub domain: String,
    pub control_url: String,
    pub host_token: String,
    pub oidc: Option<OidcConfig>,
    pub listen: SocketAddr,
    pub editor_dir: PathBuf,
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

impl Config {
    pub fn from_env() -> Result<Config> {
        Config::from_lookup(&|name| std::env::var(name).ok())
    }

    pub fn from_lookup(lookup: Lookup) -> Result<Config> {
        let domain = required(lookup, "MYCELIUM_ARTIFACT_DOMAIN")?.trim_matches('.').to_lowercase();
        let issuer = optional(lookup, "MYCELIUM_ARTIFACT_OIDC_ISSUER");
        let client_id = optional(lookup, "MYCELIUM_ARTIFACT_OIDC_CLIENT_ID");

        let oidc = match (issuer, client_id) {
            (Some(issuer), Some(client_id)) => Some(OidcConfig { issuer, client_id, client_secret: optional(lookup, "MYCELIUM_ARTIFACT_OIDC_CLIENT_SECRET") }),
            (None, None) => None,
            (None, Some(_)) => bail!("Set MYCELIUM_ARTIFACT_OIDC_ISSUER next to MYCELIUM_ARTIFACT_OIDC_CLIENT_ID."),
            (Some(_), None) => bail!("Set MYCELIUM_ARTIFACT_OIDC_CLIENT_ID next to MYCELIUM_ARTIFACT_OIDC_ISSUER."),
        };

        Ok(Config {
            domain,
            control_url: required(lookup, "MYCELIUM_CONTROL_URL")?.trim_end_matches('/').to_string(),
            host_token: required(lookup, "MYCELIUM_ARTIFACT_HOST_TOKEN")?,
            oidc,
            listen: optional(lookup, "MYCELIUM_ARTIFACT_LISTEN").unwrap_or_else(|| "0.0.0.0:8081".into()).parse()?,
            editor_dir: optional(lookup, "MYCELIUM_EDITOR_DIR").unwrap_or_else(|| "/usr/share/mycelium/editor".into()).into(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn config_from(values: &[(&str, &str)]) -> Result<Config> {
        let values: HashMap<String, String> = values.iter().map(|(name, value)| (name.to_string(), value.to_string())).collect();

        Config::from_lookup(&|name| values.get(name).cloned())
    }

    #[test]
    fn defaults_apply_and_the_domain_is_normalised() {
        let config = config_from(&[
            ("MYCELIUM_ARTIFACT_DOMAIN", ".Artifacts.Example.com."),
            ("MYCELIUM_CONTROL_URL", "http://control:8080/"),
            ("MYCELIUM_ARTIFACT_HOST_TOKEN", "token"),
        ])
        .unwrap();

        assert_eq!(config.domain, "artifacts.example.com");
        assert_eq!(config.control_url, "http://control:8080");
        assert_eq!(config.listen.to_string(), "0.0.0.0:8081");
        assert!(config.oidc.is_none());
    }

    #[test]
    fn a_client_id_without_an_issuer_is_refused_with_a_fix() {
        let result = config_from(&[
            ("MYCELIUM_ARTIFACT_DOMAIN", "artifacts.example.com"),
            ("MYCELIUM_CONTROL_URL", "http://control:8080"),
            ("MYCELIUM_ARTIFACT_HOST_TOKEN", "token"),
            ("MYCELIUM_ARTIFACT_OIDC_CLIENT_ID", "artifacts"),
        ]);

        assert!(result.unwrap_err().to_string().contains("MYCELIUM_ARTIFACT_OIDC_ISSUER"));
    }
}
