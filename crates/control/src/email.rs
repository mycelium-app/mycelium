use crate::chat_tool_servers::StoredTool;
use crate::integrations::{CallOutput, Credentials};
use async_imap::Client;
use async_imap::imap_proto::{BodyStructure, MessageSection, SectionPath};
use futures_util::TryStreamExt;
use lettre::message::{Mailbox, MessageBuilder, header::ContentType};
use lettre::transport::smtp::authentication::Credentials as SmtpCredentials;
use lettre::transport::smtp::client::{Certificate, Tls, TlsParameters};
use lettre::{AsyncSmtpTransport, AsyncTransport, Message, Tokio1Executor};
use mail_parser::{HeaderValue, MessageParser};
use serde_json::{Value, json};
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};
use std::time::Duration;
use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};
use tokio::net::TcpStream;
use tokio_rustls::TlsConnector;
use tokio_rustls::rustls::pki_types::{CertificateDer, pem::PemObject};
use tokio_rustls::client::TlsStream;

const SEND_MESSAGE: &str = "send_message";
const DEFAULT_LIMIT: u64 = 20;
const MAX_LIMIT: u64 = 100;
const MAX_BODY_CHARACTERS: usize = 20_000;
const MAX_BODY_BYTES: usize = MAX_BODY_CHARACTERS * 12;
const INBOX: &str = "INBOX";
const HEADER_QUERY: &str = "(UID INTERNALDATE BODY.PEEK[HEADER])";
const EXTRA_CA_VARIABLE: &str = "MYCELIUM_MAIL_EXTRA_CA";
const TIMEOUT: Duration = Duration::from_secs(45);

#[derive(Clone, Copy, PartialEq)]
enum Encryption {
    Tls,
    StartTls,
}

impl Encryption {
    fn parse(value: &str, label: &str) -> Result<Encryption, String> {
        match value {
            "tls" => Ok(Encryption::Tls),
            "starttls" => Ok(Encryption::StartTls),
            "none" => Err(format!("Choose TLS or STARTTLS for {label}. Mycelium does not send your password unencrypted.")),
            _ => Err(format!("Choose how to secure the {label} connection: TLS or STARTTLS.")),
        }
    }

    fn name(self) -> &'static str {
        match self {
            Encryption::Tls => "tls",
            Encryption::StartTls => "starttls",
        }
    }
}

struct Settings {
    imap_host: String,
    imap_port: u16,
    imap_encryption: Encryption,
    smtp_host: String,
    smtp_port: u16,
    smtp_encryption: Encryption,
    from: Option<String>,
}

fn setting_text(settings: &Value, key: &str) -> Option<String> {
    settings[key].as_str().map(str::trim).filter(|value| !value.is_empty()).map(str::to_string)
}

fn setting_port(settings: &Value, key: &str, encryption: Encryption, defaults: (u16, u16)) -> Result<u16, String> {
    match &settings[key] {
        Value::Null => Ok(if encryption == Encryption::Tls { defaults.0 } else { defaults.1 }),
        value => value.as_u64().and_then(|port| u16::try_from(port).ok()).filter(|port| *port > 0).ok_or_else(|| format!("Enter {} as a number from 1 to 65535.", key.replace('_', " "))),
    }
}

fn parse_settings(settings: &Value) -> Result<Settings, String> {
    let encryption = |key: &str, label: &str| match settings[key].as_str() {
        None => Ok(Encryption::Tls),
        Some(value) => Encryption::parse(value, label),
    };
    let imap_encryption = encryption("imap_tls", "IMAP")?;
    let smtp_encryption = encryption("smtp_tls", "SMTP")?;
    let Some(imap_host) = setting_text(settings, "imap_host") else {
        return Err("Enter the IMAP server, for example imap.example.com.".to_string());
    };
    let Some(smtp_host) = setting_text(settings, "smtp_host") else {
        return Err("Enter the SMTP server, for example smtp.example.com.".to_string());
    };

    Ok(Settings {
        imap_port: setting_port(settings, "imap_port", imap_encryption, (993, 143))?,
        smtp_port: setting_port(settings, "smtp_port", smtp_encryption, (465, 587))?,
        imap_host,
        imap_encryption,
        smtp_host,
        smtp_encryption,
        from: setting_text(settings, "from"),
    })
}

pub fn normalise_settings(settings: &Value) -> Result<Value, String> {
    let parsed = parse_settings(settings)?;
    let mut normalised = json!({"imap_host": parsed.imap_host, "imap_port": parsed.imap_port, "imap_tls": parsed.imap_encryption.name(), "smtp_host": parsed.smtp_host, "smtp_port": parsed.smtp_port, "smtp_tls": parsed.smtp_encryption.name()});

    if let Some(from) = &parsed.from {
        if from.parse::<Mailbox>().is_err() {
            return Err("Enter the address to send from as an email address, for example ada@example.com.".to_string());
        }

        normalised["from"] = json!(from);
    }

    Ok(normalised)
}

pub fn tools() -> Vec<StoredTool> {
    let folder = json!({"type": "string", "description": format!("The folder's name. Defaults to {INBOX}.")});
    let limit = json!({"type": "integer", "description": format!("How many messages to return, newest first. Defaults to {DEFAULT_LIMIT}, at most {MAX_LIMIT}.")});
    let addresses = |description: &str| json!({"type": "array", "items": {"type": "string"}, "description": description});

    vec![
        StoredTool { name: "list_folders".to_string(), description: "List the mailbox's folders.".to_string(), input_schema: json!({"type": "object", "properties": {}}) },
        StoredTool {
            name: "list_messages".to_string(),
            description: "List the most recent messages in a folder with sender, subject, date and id.".to_string(),
            input_schema: json!({"type": "object", "properties": {"folder": folder, "limit": limit}}),
        },
        StoredTool {
            name: "read_message".to_string(),
            description: "Read one message: its plain text body and the names and encoded sizes of its attachments. Attachments are never fetched.".to_string(),
            input_schema: json!({"type": "object", "properties": {"folder": folder, "id": {"type": "string", "description": "The message id from list_messages or search_messages."}}, "required": ["id"]}),
        },
        StoredTool {
            name: "search_messages".to_string(),
            description: "Search a folder. Give at least one of from, subject, text or since. Returns the newest matches.".to_string(),
            input_schema: json!({"type": "object", "properties": {"folder": folder, "from": {"type": "string", "description": "Part of the sender's name or address."}, "subject": {"type": "string", "description": "Words in the subject."}, "text": {"type": "string", "description": "Words anywhere in the message."}, "since": {"type": "string", "description": "Only messages on or after this date, as YYYY-MM-DD."}, "limit": limit}}),
        },
        StoredTool {
            name: SEND_MESSAGE.to_string(),
            description: "Send an email, or a reply to a message. Always needs the person's approval in chats, even when Always allow is on. Give to, or reply_to_id to answer the sender of that message.".to_string(),
            input_schema: json!({"type": "object", "properties": {"to": addresses("Recipients' email addresses."), "cc": addresses("Copied recipients' email addresses."), "subject": {"type": "string", "description": "The subject. For a reply it defaults to the original subject with Re: in front."}, "body": {"type": "string", "description": "The message, in plain text."}, "reply_to_id": {"type": "string", "description": "The id of the message being answered. Sets In-Reply-To and References."}, "reply_to_folder": folder}, "required": ["body"]}),
        },
    ]
}

pub fn always_asks(tool: &str) -> bool {
    tool == SEND_MESSAGE
}

#[derive(Debug)]
enum Transport {
    Plain(TcpStream),
    Tls(Box<TlsStream<TcpStream>>),
}

impl AsyncRead for Transport {
    fn poll_read(self: Pin<&mut Self>, context: &mut Context<'_>, buffer: &mut ReadBuf<'_>) -> Poll<std::io::Result<()>> {
        match self.get_mut() {
            Transport::Plain(stream) => Pin::new(stream).poll_read(context, buffer),
            Transport::Tls(stream) => Pin::new(stream.as_mut()).poll_read(context, buffer),
        }
    }
}

impl AsyncWrite for Transport {
    fn poll_write(self: Pin<&mut Self>, context: &mut Context<'_>, data: &[u8]) -> Poll<std::io::Result<usize>> {
        match self.get_mut() {
            Transport::Plain(stream) => Pin::new(stream).poll_write(context, data),
            Transport::Tls(stream) => Pin::new(stream.as_mut()).poll_write(context, data),
        }
    }

    fn poll_flush(self: Pin<&mut Self>, context: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        match self.get_mut() {
            Transport::Plain(stream) => Pin::new(stream).poll_flush(context),
            Transport::Tls(stream) => Pin::new(stream.as_mut()).poll_flush(context),
        }
    }

    fn poll_shutdown(self: Pin<&mut Self>, context: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        match self.get_mut() {
            Transport::Plain(stream) => Pin::new(stream).poll_shutdown(context),
            Transport::Tls(stream) => Pin::new(stream.as_mut()).poll_shutdown(context),
        }
    }
}

type Session = async_imap::Session<Transport>;

fn unreachable_server(protocol: &str, host: &str) -> String {
    format!("Mycelium could not connect to the {protocol} server {host}. Check the server, port and security setting, then try again.")
}

fn extra_roots() -> Vec<CertificateDer<'static>> {
    let Ok(path) = std::env::var(EXTRA_CA_VARIABLE) else { return Vec::new() };

    CertificateDer::pem_file_iter(path).into_iter().flatten().flatten().collect()
}

async fn encrypt(stream: TcpStream, host: &str) -> Result<Transport, String> {
    let mut roots = tokio_rustls::rustls::RootCertStore { roots: webpki_roots::TLS_SERVER_ROOTS.to_vec() };

    for root in extra_roots() {
        let _ = roots.add(root);
    }

    let provider = Arc::new(tokio_rustls::rustls::crypto::aws_lc_rs::default_provider());
    let config = tokio_rustls::rustls::ClientConfig::builder_with_provider(provider).with_safe_default_protocol_versions().map_err(|error| error.to_string())?.with_root_certificates(roots).with_no_client_auth();
    let name = tokio_rustls::rustls::pki_types::ServerName::try_from(host.to_string()).map_err(|_| format!("Enter {host} as a server name, for example imap.example.com."))?;
    let secured = TlsConnector::from(Arc::new(config)).connect(name, stream).await.map_err(|_| format!("Mycelium could not set up a secure connection to {host}. Check the security setting and that the server's certificate is valid, then try again."))?;

    Ok(Transport::Tls(Box::new(secured)))
}

async fn connect(settings: &Settings, credentials: &Credentials) -> Result<Session, String> {
    let host = settings.imap_host.as_str();
    let tcp = TcpStream::connect((host, settings.imap_port)).await.map_err(|_| unreachable_server("IMAP", host))?;
    let transport = match settings.imap_encryption {
        Encryption::Tls => encrypt(tcp, host).await?,
        _ => Transport::Plain(tcp),
    };
    let mut client = Client::new(transport);
    let greeting = client.read_response().await.map_err(|_| unreachable_server("IMAP", host))?;

    if greeting.is_none() {
        return Err(unreachable_server("IMAP", host));
    }

    if settings.imap_encryption == Encryption::StartTls {
        client.run_command_and_check_ok("STARTTLS", None).await.map_err(|_| format!("The IMAP server {host} does not offer STARTTLS. Choose TLS or check the port, then try again."))?;

        let Transport::Plain(plain) = client.into_inner() else {
            return Err(unreachable_server("IMAP", host));
        };
        client = Client::new(encrypt(plain, host).await?);
    }

    client.login(&credentials.account, &credentials.secret).await.map_err(|_| "The IMAP server did not accept this username and password. Check them, and use an app password if your provider needs one, then try again.".to_string())
}

fn smtp_transport(settings: &Settings, credentials: &Credentials) -> Result<AsyncSmtpTransport<Tokio1Executor>, String> {
    let host = settings.smtp_host.as_str();
    let mut parameters = TlsParameters::builder(host.to_string());

    for root in extra_roots() {
        if let Ok(certificate) = Certificate::from_der(root.to_vec()) {
            parameters = parameters.add_root_certificate(certificate);
        }
    }

    let parameters = parameters.build().map_err(|_| format!("Enter {host} as a server name, for example smtp.example.com."))?;
    let tls = match settings.smtp_encryption {
        Encryption::Tls => Tls::Wrapper(parameters),
        Encryption::StartTls => Tls::Required(parameters),
    };
    let builder = AsyncSmtpTransport::<Tokio1Executor>::builder_dangerous(host).tls(tls);

    Ok(builder.port(settings.smtp_port).credentials(SmtpCredentials::new(credentials.account.clone(), credentials.secret.clone())).timeout(Some(TIMEOUT)).build())
}

pub async fn verify(credentials: &Credentials) -> Result<(), String> {
    let settings = parse_settings(&credentials.settings)?;
    let mut session = connect(&settings, credentials).await?;
    let _ = session.logout().await;
    let transport = smtp_transport(&settings, credentials)?;
    let accepted = transport.test_connection().await.map_err(|_| format!("The SMTP server {} did not accept this username and password, or could not be reached. Check the server, port, security setting and password, then try again.", settings.smtp_host))?;

    if !accepted {
        return Err(unreachable_server("SMTP", &settings.smtp_host));
    }

    Ok(())
}

fn failure(text: impl Into<String>) -> CallOutput {
    CallOutput { text: text.into(), is_error: true }
}

fn text<'a>(arguments: &'a Value, key: &str) -> Option<&'a str> {
    arguments[key].as_str().map(str::trim).filter(|value| !value.is_empty())
}

fn addresses(arguments: &Value, key: &str) -> Vec<String> {
    arguments[key].as_array().into_iter().flatten().filter_map(Value::as_str).map(str::trim).filter(|address| !address.is_empty()).map(str::to_string).collect()
}

fn limit(arguments: &Value) -> usize {
    arguments["limit"].as_u64().unwrap_or(DEFAULT_LIMIT).clamp(1, MAX_LIMIT) as usize
}

fn folder(arguments: &Value, key: &str) -> String {
    text(arguments, key).unwrap_or(INBOX).to_string()
}

fn uid(arguments: &Value, key: &str) -> Result<u32, String> {
    text(arguments, key).and_then(|id| id.parse::<u32>().ok()).filter(|id| *id > 0).ok_or_else(|| format!("Give the message's id in {key}, as listed by list_messages or search_messages."))
}

fn quoted(value: &str) -> Result<String, String> {
    if value.contains(['\r', '\n']) {
        return Err("Search words cannot contain line breaks.".to_string());
    }

    Ok(format!("\"{}\"", value.replace('\\', "\\\\").replace('"', "\\\"")))
}

fn search_criteria(arguments: &Value) -> Result<String, String> {
    let mut criteria = Vec::new();

    for (key, keyword) in [("from", "FROM"), ("subject", "SUBJECT"), ("text", "TEXT")] {
        if let Some(value) = text(arguments, key) {
            criteria.push(format!("{keyword} {}", quoted(value)?));
        }
    }

    if let Some(since) = text(arguments, "since") {
        let date = chrono::NaiveDate::parse_from_str(since, "%Y-%m-%d").map_err(|_| "Enter since as a date like 2025-01-31.".to_string())?;

        criteria.push(format!("SINCE {}", date.format("%-d-%b-%Y")));
    }

    if criteria.is_empty() {
        return Err("Give at least one of from, subject, text or since to search for.".to_string());
    }

    let joined = criteria.join(" ");

    Ok(if joined.is_ascii() { joined } else { format!("CHARSET UTF-8 {joined}") })
}

fn describe_addresses(list: Option<&mail_parser::Address<'_>>) -> Vec<String> {
    list.into_iter()
        .flat_map(|list| list.iter())
        .map(|entry| match (entry.name.as_deref(), entry.address.as_deref()) {
            (Some(name), Some(address)) => format!("{name} <{address}>"),
            (None, Some(address)) => address.to_string(),
            (Some(name), None) => name.to_string(),
            (None, None) => String::new(),
        })
        .collect()
}

fn summary(fetch: &async_imap::types::Fetch) -> Option<Value> {
    let parsed = MessageParser::default().parse(fetch.header()?)?;
    let date = parsed.date().map(mail_parser::DateTime::to_rfc3339).or_else(|| fetch.internal_date().map(|date| date.to_rfc3339()));

    Some(json!({"id": fetch.uid?.to_string(), "from": describe_addresses(parsed.from()).join(", "), "subject": parsed.subject(), "date": date}))
}

fn newest_first(fetched: &[async_imap::types::Fetch]) -> Value {
    let mut listed: Vec<(u32, Value)> = fetched.iter().filter_map(|fetch| Some((fetch.uid?, summary(fetch)?))).collect();

    listed.sort_by(|left, right| right.0.cmp(&left.0));

    Value::Array(listed.into_iter().map(|(_, message)| message).collect())
}

async fn open(session: &mut Session, name: &str) -> Result<async_imap::types::Mailbox, String> {
    session.examine(name).await.map_err(|_| format!("There is no folder called {name}. Use list_folders to see the folders."))
}

async fn read_headers(session: &mut Session, name: &str, id: u32) -> Result<Vec<u8>, String> {
    open(session, name).await?;

    let fetched: Vec<_> = session.uid_fetch(id.to_string(), "BODY.PEEK[HEADER]").await.map_err(|error| error.to_string())?.try_collect().await.map_err(|error| error.to_string())?;

    fetched.iter().find_map(|fetch| fetch.header().map(<[u8]>::to_vec)).ok_or_else(|| format!("There is no message with id {id} in {name}."))
}

fn describe_parts(part: &BodyStructure<'_>, section: Vec<u32>, bodies: &mut Vec<(Vec<u32>, bool, u32)>, attachments: &mut Vec<Value>) {
    let (common, octets) = match part {
        BodyStructure::Basic { common, other, .. } | BodyStructure::Text { common, other, .. } | BodyStructure::Message { common, other, .. } => (common, Some(other.octets)),
        BodyStructure::Multipart { common, .. } => (common, None),
    };
    let filename = common.disposition.as_ref().and_then(|disposition| disposition.params.as_ref()).into_iter().flatten().find(|(name, _)| name.eq_ignore_ascii_case("filename")).map(|(_, value)| value.as_ref())
        .or_else(|| common.ty.params.as_ref().into_iter().flatten().find(|(name, _)| name.eq_ignore_ascii_case("name")).map(|(_, value)| value.as_ref()));
    let attached = filename.is_some() || common.disposition.as_ref().is_some_and(|disposition| disposition.ty.eq_ignore_ascii_case("attachment"));

    match part {
        BodyStructure::Multipart { bodies: children, .. } if !attached => {
            for (index, child) in children.iter().enumerate() {
                let mut child_section = section.clone();
                child_section.push(index as u32 + 1);
                describe_parts(child, child_section, bodies, attachments);
            }
        }
        BodyStructure::Text { .. } if !attached && (common.ty.subtype.eq_ignore_ascii_case("plain") || common.ty.subtype.eq_ignore_ascii_case("html")) => {
            bodies.push((if section.is_empty() { vec![1] } else { section }, common.ty.subtype.eq_ignore_ascii_case("html"), octets.unwrap_or(0)));
        }
        _ => attachments.push(json!({"name": filename.unwrap_or("unnamed"), "size": octets})),
    }
}

async fn read_tool(session: &mut Session, tool: &str, arguments: &Value) -> Result<Value, String> {
    match tool {
        "list_folders" => {
            let names: Vec<_> = session.list(Some(""), Some("*")).await.map_err(|error| error.to_string())?.try_collect().await.map_err(|error| error.to_string())?;

            Ok(Value::Array(names.iter().map(|name| json!(name.name())).collect()))
        }
        "list_messages" => {
            let name = folder(arguments, "folder");
            let mailbox = open(session, &name).await?;

            if mailbox.exists == 0 {
                return Ok(json!([]));
            }

            let first = mailbox.exists.saturating_sub(limit(arguments) as u32 - 1).max(1);
            let fetched: Vec<_> = session.fetch(format!("{first}:{}", mailbox.exists), HEADER_QUERY).await.map_err(|error| error.to_string())?.try_collect().await.map_err(|error| error.to_string())?;

            Ok(newest_first(&fetched))
        }
        "read_message" => {
            let name = folder(arguments, "folder");
            let id = uid(arguments, "id")?;
            open(session, &name).await?;

            let fetched: Vec<_> = session.uid_fetch(id.to_string(), "(UID BODYSTRUCTURE BODY.PEEK[HEADER])").await.map_err(|error| error.to_string())?.try_collect().await.map_err(|error| error.to_string())?;
            let message = fetched.iter().find(|fetch| fetch.uid == Some(id)).ok_or_else(|| format!("There is no message with id {id} in {name}."))?;
            let headers = message.header().ok_or_else(|| "That message could not be read.".to_string())?;
            let parsed = MessageParser::default().parse(headers).ok_or_else(|| "That message could not be read.".to_string())?;
            let structure = message.bodystructure().ok_or_else(|| "The mail server did not describe that message's body.".to_string())?;
            let mut bodies = Vec::new();
            let mut attachments = Vec::new();
            describe_parts(structure, Vec::new(), &mut bodies, &mut attachments);
            bodies.sort_by_key(|(_, html, _)| *html);
            let mut body = String::new();
            let mut truncated = false;

            if let Some((section, _, octets)) = bodies.first() {
                let path = section.iter().map(u32::to_string).collect::<Vec<_>>().join(".");
                let query = format!("(BODY.PEEK[{path}.MIME] BODY.PEEK[{path}]<0.{MAX_BODY_BYTES}>)");
                let parts: Vec<_> = session.uid_fetch(id.to_string(), query).await.map_err(|error| error.to_string())?.try_collect().await.map_err(|error| error.to_string())?;
                let part = parts.first().ok_or_else(|| "That message's text could not be read.".to_string())?;
                let mime = part.section(&SectionPath::Part(section.clone(), Some(MessageSection::Mime))).ok_or_else(|| "That message's text headers could not be read.".to_string())?;
                let text = part.section(&SectionPath::Part(section.clone(), None)).ok_or_else(|| "That message's text could not be read.".to_string())?;
                let mut raw = mime.to_vec();
                raw.extend_from_slice(text);
                let decoded = MessageParser::default().parse(&raw).ok_or_else(|| "That message's text could not be read.".to_string())?;
                let decoded = decoded.body_text(0).unwrap_or_default();
                truncated = *octets as usize > text.len() || decoded.chars().count() > MAX_BODY_CHARACTERS;
                body = decoded.chars().take(MAX_BODY_CHARACTERS).collect();
            }

            Ok(json!({
                "id": id.to_string(),
                "folder": name,
                "from": describe_addresses(parsed.from()).join(", "),
                "to": describe_addresses(parsed.to()),
                "cc": describe_addresses(parsed.cc()),
                "subject": parsed.subject(),
                "date": parsed.date().map(mail_parser::DateTime::to_rfc3339),
                "message_id": parsed.message_id(),
                "body": body,
                "body_truncated": truncated,
                "attachments": attachments,
            }))
        }
        "search_messages" => {
            let name = folder(arguments, "folder");
            let criteria = search_criteria(arguments)?;

            open(session, &name).await?;

            let found = session.uid_search(criteria).await.map_err(|error| error.to_string())?;
            let mut uids: Vec<u32> = found.into_iter().collect();

            uids.sort_unstable_by(|left, right| right.cmp(left));
            uids.truncate(limit(arguments));

            if uids.is_empty() {
                return Ok(json!([]));
            }

            let set = uids.iter().map(u32::to_string).collect::<Vec<_>>().join(",");
            let fetched: Vec<_> = session.uid_fetch(set, HEADER_QUERY).await.map_err(|error| error.to_string())?.try_collect().await.map_err(|error| error.to_string())?;

            Ok(newest_first(&fetched))
        }
        _ => Err(format!("Email has no tool called {tool}.")),
    }
}

fn mailbox(address: &str) -> Result<Mailbox, String> {
    address.parse::<Mailbox>().map_err(|_| format!("{address} is not an email address. Use the form ada@example.com."))
}

fn reply_subject(original: Option<&str>) -> String {
    let original = original.unwrap_or_default();

    if original.to_lowercase().starts_with("re:") { original.to_string() } else { format!("Re: {original}") }
}

async fn send_tool(settings: &Settings, credentials: &Credentials, arguments: &Value) -> Result<Value, String> {
    let Some(body) = text(arguments, "body") else {
        return Err("Give the message in body.".to_string());
    };
    let mut to = addresses(arguments, "to");
    let mut subject = text(arguments, "subject").map(str::to_string);
    let mut builder: MessageBuilder = Message::builder();

    if text(arguments, "reply_to_id").is_some() {
        let id = uid(arguments, "reply_to_id")?;
        let name = folder(arguments, "reply_to_folder");
        let mut session = connect(settings, credentials).await?;
        let raw = read_headers(&mut session, &name, id).await;
        let _ = session.logout().await;
        let raw = raw?;
        let original = MessageParser::default().parse(&raw).ok_or_else(|| "The message being answered could not be read.".to_string())?;

        if to.is_empty() {
            let senders = original.reply_to().or_else(|| original.from());

            to = senders.into_iter().flat_map(|list| list.iter()).filter_map(|sender| sender.address.as_deref().map(str::to_string)).collect();
        }

        subject.get_or_insert_with(|| reply_subject(original.subject()));

        if let Some(original_id) = original.message_id() {
            let mut references: Vec<String> = match original.references() {
                HeaderValue::TextList(list) => list.iter().map(|reference| format!("<{reference}>")).collect(),
                HeaderValue::Text(reference) => vec![format!("<{reference}>")],
                _ => Vec::new(),
            };

            references.push(format!("<{original_id}>"));
            builder = builder.in_reply_to(format!("<{original_id}>")).references(references.join(" "));
        }
    }

    if to.is_empty() {
        return Err("Give at least one recipient in to, or the id of the message to reply to in reply_to_id.".to_string());
    }

    let Some(subject) = subject else {
        return Err("Give the subject of the message.".to_string());
    };
    let from = settings.from.clone().unwrap_or_else(|| credentials.account.clone());
    let from = mailbox(&from).map_err(|_| "Enter the address to send from as an email address in the integration's settings, for example ada@example.com.".to_string())?;
    let domain = from.email.domain().to_string();
    let message_id = format!("<{}@{domain}>", uuid::Uuid::new_v4());

    builder = builder.from(from).subject(subject).message_id(Some(message_id.clone())).header(ContentType::TEXT_PLAIN);

    for recipient in &to {
        builder = builder.to(mailbox(recipient)?);
    }

    for copied in addresses(arguments, "cc") {
        builder = builder.cc(mailbox(&copied)?);
    }

    let message = builder.body(body.to_string()).map_err(|error| format!("The message could not be built: {error}"))?;

    smtp_transport(settings, credentials)?.send(message).await.map_err(|error| format!("The SMTP server did not accept the message: {error}"))?;

    Ok(json!({"sent": true, "message_id": message_id}))
}

pub async fn call(credentials: &Credentials, tool: &str, arguments: &Value) -> CallOutput {
    let settings = match parse_settings(&credentials.settings) {
        Ok(settings) => settings,
        Err(reason) => return failure(reason),
    };
    let work = async {
        if tool == SEND_MESSAGE {
            return send_tool(&settings, credentials, arguments).await;
        }

        let mut session = connect(&settings, credentials).await?;
        let result = read_tool(&mut session, tool, arguments).await;
        let _ = session.logout().await;

        result
    };

    match tokio::time::timeout(TIMEOUT, work).await {
        Ok(Ok(value)) => CallOutput { text: value.to_string(), is_error: false },
        Ok(Err(reason)) => failure(reason),
        Err(_) => failure("The mail server took too long to answer. Try again in a moment."),
    }
}
