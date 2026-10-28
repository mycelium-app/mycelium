use crate::chat_tool_servers::{ServerRow, StoredTool};
use crate::integration_policy::{self, Effective};
use mycelium_proto::api::{ApprovalPreset, IntegrationStatus, ToolKind};
use crate::{actual, caldav, coolify, discord, email, freeagent, lastfm, paperless, stoat, tandoor, zulip};
use crate::state::AppState;
use uuid::Uuid;
use serde_json::Value;

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Integration {
    Zulip,
    Email,
    Stoat,
    LastFm,
    CalDav,
    Discord,
    FreeAgent,
    Actual,
    Paperless,
    Coolify,
    Tandoor,
}

pub struct Credentials {
    pub address: String,
    pub account: String,
    pub secret: String,
    pub settings: Value,
    pub session: Option<String>,
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

    pub fn default_preset(self) -> ApprovalPreset {
        match self {
            Integration::Zulip => ApprovalPreset::AskAllEdit,
            _ => ApprovalPreset::AskWrites,
        }
    }

    pub fn parse(kind: &str) -> Option<Integration> {
        match kind {
            "zulip" => Some(Integration::Zulip),
            "email" => Some(Integration::Email),
            "stoat" => Some(Integration::Stoat),
            "lastfm" => Some(Integration::LastFm),
            "caldav" => Some(Integration::CalDav),
            "discord" => Some(Integration::Discord),
            "freeagent" => Some(Integration::FreeAgent),
            "actual" => Some(Integration::Actual),
            "paperless" => Some(Integration::Paperless),
            "coolify" => Some(Integration::Coolify),
            "tandoor" => Some(Integration::Tandoor),
            _ => None,
        }
    }

    pub fn needs_address(self) -> bool {
        self != Integration::Email
    }

    pub fn default_address(self) -> Option<&'static str> {
        match self {
            Integration::LastFm => Some(lastfm::DEFAULT_ADDRESS),
            Integration::Discord => Some(discord::DEFAULT_ADDRESS),
            Integration::FreeAgent => Some(freeagent::PRODUCTION_ADDRESS),
            _ => None,
        }
    }

    pub fn account_prompt(self) -> Option<&'static str> {
        match self {
            Integration::Zulip => Some("Enter the email address of your Zulip account."),
            Integration::Email => Some("Enter your email username, usually your full email address."),
            Integration::Stoat => None,
            Integration::LastFm => Some("Enter your Last.fm username."),
            Integration::CalDav => Some("Enter your calendar username."),
            Integration::Discord => Some("Enter the name people know you by on Discord."),
            Integration::FreeAgent => Some("Enter your FreeAgent OAuth identifier."),
            Integration::Actual | Integration::Paperless | Integration::Coolify | Integration::Tandoor => None,
        }
    }

    pub fn secret_prompt(self) -> &'static str {
        match self {
            Integration::Zulip => "Enter your Zulip API key.",
            Integration::Email => "Enter your email password, or an app password if your provider needs one.",
            Integration::Stoat => "Enter your Stoat token.",
            Integration::LastFm => "Enter your Last.fm shared secret.",
            Integration::CalDav => "Enter an app password for your calendar account.",
            Integration::Discord => "Enter your Discord bot token.",
            Integration::FreeAgent => "Enter your FreeAgent OAuth secret.",
            Integration::Actual => "Enter the API key your Actual HTTP API is set up with.",
            Integration::Paperless => "Enter your Paperless-ngx API token.",
            Integration::Coolify => "Enter your Coolify API token.",
            Integration::Tandoor => "Enter your Tandoor API token.",
        }
    }

    pub fn normalise_settings(self, settings: &Value) -> Result<Value, String> {
        match self {
            Integration::Zulip => Ok(Value::Null),
            Integration::Email => email::normalise_settings(settings),
            Integration::Stoat => stoat::normalise_settings(settings),
            Integration::LastFm => lastfm::normalise_settings(settings),
            Integration::CalDav => caldav::normalise_settings(settings),
            Integration::Actual => actual::normalise_settings(settings),
            Integration::Coolify => coolify::normalise_settings(settings),
            Integration::Discord | Integration::FreeAgent | Integration::Paperless | Integration::Tandoor => Ok(Value::Null),
        }
    }

    pub fn tools(self) -> Vec<StoredTool> {
        match self {
            Integration::Zulip => zulip::tools(),
            Integration::Email => email::tools(),
            Integration::Stoat => stoat::tools(),
            Integration::LastFm => lastfm::tools(),
            Integration::CalDav => caldav::tools(),
            Integration::Discord => discord::tools(),
            Integration::FreeAgent => freeagent::tools(),
            Integration::Actual => actual::tools(),
            Integration::Paperless => paperless::tools(),
            Integration::Coolify => coolify::tools(),
            Integration::Tandoor => tandoor::tools(),
        }
    }

    pub fn always_asks(self, tool: &str) -> bool {
        match self {
            Integration::Zulip => zulip::always_asks(tool),
            Integration::Email => email::always_asks(tool),
            Integration::Stoat => stoat::always_asks(tool),
            Integration::LastFm => lastfm::always_asks(tool),
            Integration::CalDav => caldav::always_asks(tool),
            Integration::Discord => discord::always_asks(tool),
            Integration::FreeAgent => freeagent::always_asks(tool),
            Integration::Actual => actual::always_asks(tool),
            Integration::Paperless => paperless::always_asks(tool),
            Integration::Coolify => coolify::always_asks(tool),
            Integration::Tandoor => tandoor::always_asks(tool),
        }
    }

    pub async fn verify(self, http: &reqwest::Client, credentials: &Credentials) -> Result<Value, String> {
        match self {
            Integration::Zulip => zulip::verify(http, credentials).await.map(|_| Value::Null),
            Integration::Email => email::verify(credentials).await.map(|_| Value::Null),
            Integration::Stoat => stoat::verify(http, credentials).await.map(|_| Value::Null),
            Integration::LastFm => lastfm::verify(http, credentials).await.map(|_| Value::Null),
            Integration::CalDav => caldav::verify(http, credentials).await,
            Integration::Discord => discord::verify(http, credentials).await.map(|_| Value::Null),
            Integration::FreeAgent => freeagent::verify(http, credentials).await.map(|_| Value::Null),
            Integration::Actual => actual::verify(http, credentials).await.map(|_| Value::Null),
            Integration::Paperless => paperless::verify(http, credentials).await.map(|_| Value::Null),
            Integration::Coolify => coolify::verify(http, credentials).await,
            Integration::Tandoor => tandoor::verify(http, credentials).await.map(|_| Value::Null),
        }
    }

    pub async fn call(self, state: &AppState, owner: Uuid, server: &ServerRow, conversation: Option<Uuid>, tool: &str, arguments: &Value) -> CallOutput {
        let credentials = server.credentials();

        match self {
            Integration::Zulip => zulip::call(&state.http, &credentials, tool, arguments).await,
            Integration::Email => email::call(state, owner, server, &credentials, tool, arguments).await,
            Integration::Stoat => stoat::call(&state.http, &credentials, tool, arguments).await,
            Integration::CalDav => caldav::call(&state.http, &credentials, tool, arguments).await,
            Integration::LastFm => lastfm::call(state, owner, server, &credentials, tool, arguments).await,
            Integration::Discord => discord::call(&state.http, &credentials, tool, arguments).await,
            Integration::FreeAgent => freeagent::call(state, owner, server, &credentials, tool, arguments).await,
            Integration::Actual => actual::call(&state.http, &credentials, tool, arguments).await,
            Integration::Paperless => paperless::call(&state.http, &credentials, tool, arguments).await,
            Integration::Coolify => coolify::call(state, owner, server, &credentials, conversation, tool, arguments).await,
            Integration::Tandoor => tandoor::call(&state.http, &credentials, tool, arguments).await,
        }
    }
}

impl ServerRow {
    pub fn integration(&self) -> Option<Integration> {
        Integration::parse(&self.kind)
    }

    pub fn credentials(&self) -> Credentials {
        Credentials { address: self.url.clone(), account: self.account.clone().unwrap_or_default(), secret: self.bearer_token.clone().unwrap_or_default(), settings: self.settings.0.clone(), session: self.session_key.clone() }
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
        integration_policy::effective(&self.policy.0, tool, self.tool_kind(tool))
    }

    pub fn status(&self) -> IntegrationStatus {
        let now = chrono::Utc::now();

        if !self.enabled {
            return IntegrationStatus::Disabled;
        }

        if self.integration() == Some(Integration::FreeAgent) && self.session_key.is_none() {
            return IntegrationStatus::NeedsSignIn;
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
