use crate::chat_tool_servers::{ServerRow, StoredTool};
use crate::integration_policy::{self, Effective};
use mycelium_proto::api::{IntegrationStatus, ToolKind};
use crate::{email, stoat, zulip};
use crate::state::AppState;
use uuid::Uuid;
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

const EXPIRES_SOON_DAYS: i64 = 14;

impl Integration {
    pub fn tool_kind(self, tool: &str) -> ToolKind {
        match self.always_asks(tool) {
            true => ToolKind::Write,
            false => ToolKind::Read,
        }
    }

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

    pub async fn call(self, state: &AppState, owner: Uuid, server: &ServerRow, tool: &str, arguments: &Value) -> CallOutput {
        let credentials = server.credentials();

        match self {
            Integration::Zulip => zulip::call(&state.http, &credentials, tool, arguments).await,
            Integration::Email => email::call(state, owner, server, &credentials, tool, arguments).await,
            Integration::Stoat => stoat::call(&state.http, &credentials, tool, arguments).await,
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

    pub fn tool_kind(&self, tool: &str) -> ToolKind {
        match self.integration() {
            Some(integration) => integration.tool_kind(tool),
            None => match self.tools.0.iter().find(|stored| stored.name == tool).and_then(|stored| stored.read_only) {
                Some(true) => ToolKind::Read,
                _ => ToolKind::Write,
            },
        }
    }

    pub fn in_group(&self, group: Option<uuid::Uuid>) -> bool {
        self.groups.is_empty() || group.is_some_and(|group| self.groups.contains(&group))
    }

    pub fn effective(&self, tool: &str) -> Effective {
        let integration = self.integration();

        if integration == Some(Integration::Zulip) {
            return integration_policy::locked();
        }

        let kind = self.tool_kind(tool);
        let mut effective = integration_policy::effective(&self.policy.0, tool, kind, !self.always_allow);

        if integration.is_some() && kind == ToolKind::Write {
            effective.ask = true;
            effective.ask_source = integration_policy::Source::Locked;
        }

        effective
    }

    pub fn status(&self) -> IntegrationStatus {
        let now = chrono::Utc::now();

        if !self.enabled {
            return IntegrationStatus::Disabled;
        }

        match self.token_expires_at {
            Some(expires_at) if expires_at <= now => IntegrationStatus::Expired,
            Some(expires_at) if expires_at - now < chrono::Duration::days(EXPIRES_SOON_DAYS) => IntegrationStatus::ExpiresSoon,
            _ => IntegrationStatus::Connected,
        }
    }

    pub fn tool_list(&self) -> Vec<StoredTool> {
        match self.integration() {
            Some(integration) => integration.tools(),
            None => self.tools.0.clone(),
        }
    }
}
