use crate::chat_tool_servers::{ServerRow, StoredTool};
use crate::{email, stoat, zulip};
use serde_json::Value;

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Integration {
    Zulip,
    Email,
    Stoat,
}

pub struct Credentials {
    pub address: String,
    pub account: String,
    pub secret: String,
    pub settings: Value,
}

pub struct CallOutput {
    pub text: String,
    pub is_error: bool,
}

impl Integration {
    pub fn parse(kind: &str) -> Option<Integration> {
        match kind {
            "zulip" => Some(Integration::Zulip),
            "email" => Some(Integration::Email),
            "stoat" => Some(Integration::Stoat),
            _ => None,
        }
    }

    pub fn needs_address(self) -> bool {
        self != Integration::Email
    }

    pub fn account_prompt(self) -> Option<&'static str> {
        match self {
            Integration::Zulip => Some("Enter the email address of your Zulip account."),
            Integration::Email => Some("Enter your email username, usually your full email address."),
            Integration::Stoat => None,
        }
    }

    pub fn secret_prompt(self) -> &'static str {
        match self {
            Integration::Zulip => "Enter your Zulip API key.",
            Integration::Email => "Enter your email password, or an app password if your provider needs one.",
            Integration::Stoat => "Enter your Stoat token.",
        }
    }

    pub fn normalise_settings(self, settings: &Value) -> Result<Value, String> {
        match self {
            Integration::Zulip => Ok(Value::Null),
            Integration::Email => email::normalise_settings(settings),
            Integration::Stoat => stoat::normalise_settings(settings),
        }
    }

    pub fn tools(self) -> Vec<StoredTool> {
        match self {
            Integration::Zulip => zulip::tools(),
            Integration::Email => email::tools(),
            Integration::Stoat => stoat::tools(),
        }
    }

    pub fn always_asks(self, tool: &str) -> bool {
        match self {
            Integration::Zulip => zulip::always_asks(tool),
            Integration::Email => email::always_asks(tool),
            Integration::Stoat => stoat::always_asks(tool),
        }
    }

    pub async fn verify(self, http: &reqwest::Client, credentials: &Credentials) -> Result<(), String> {
        match self {
            Integration::Zulip => zulip::verify(http, credentials).await,
            Integration::Email => email::verify(credentials).await,
            Integration::Stoat => stoat::verify(http, credentials).await,
        }
    }

    pub async fn call(self, http: &reqwest::Client, credentials: &Credentials, tool: &str, arguments: &Value) -> CallOutput {
        match self {
            Integration::Zulip => zulip::call(http, credentials, tool, arguments).await,
            Integration::Email => email::call(credentials, tool, arguments).await,
            Integration::Stoat => stoat::call(http, credentials, tool, arguments).await,
        }
    }
}

impl ServerRow {
    pub fn integration(&self) -> Option<Integration> {
        Integration::parse(&self.kind)
    }

    pub fn credentials(&self) -> Credentials {
        Credentials { address: self.url.clone(), account: self.account.clone().unwrap_or_default(), secret: self.bearer_token.clone().unwrap_or_default(), settings: self.settings.0.clone() }
    }

    pub fn tool_list(&self) -> Vec<StoredTool> {
        match self.integration() {
            Some(integration) => integration.tools(),
            None => self.tools.0.clone(),
        }
    }
}
