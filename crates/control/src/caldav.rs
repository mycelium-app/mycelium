use crate::auth::AuthUser;
use crate::chat_tool_servers::{SERVER_COLUMNS, ServerRow, StoredTool};
use crate::error::{ApiError, ApiResult};
use crate::integrations::{CallOutput, Credentials};
use crate::state::AppState;
use axum::Json;
use axum::extract::{Path, State};
use chrono::{DateTime, Duration as ChronoDuration, NaiveDate, NaiveDateTime, TimeZone, Utc};
use serde_json::{Value, json};
use std::time::Duration;
use url::Url;
use uuid::Uuid;

const TIMEOUT: Duration = Duration::from_secs(30);
const WRITES: [&str; 3] = ["create_event", "update_event", "delete_event"];
const CONFLICT: &str = "The event changed on the server since you read it. List it again and retry.";
const READ_ONLY: &str = "That calendar is read only.";
const FOLD_AT: usize = 73;

const PRINCIPAL_QUERY: &str = r#"<d:propfind xmlns:d="DAV:"><d:prop><d:current-user-principal/></d:prop></d:propfind>"#;
const HOME_QUERY: &str = r#"<d:propfind xmlns:d="DAV:" xmlns:c="urn:ietf:params:xml:ns:caldav"><d:prop><c:calendar-home-set/></d:prop></d:propfind>"#;
const COLLECTIONS_QUERY: &str = r#"<d:propfind xmlns:d="DAV:" xmlns:a="http://apple.com/ns/ical/"><d:prop><d:resourcetype/><d:displayname/><a:calendar-color/><d:current-user-privilege-set/></d:prop></d:propfind>"#;

pub enum Failure {
    Rejected,
    Unreachable(String),
    NotCalDav,
    NoCalendars,
}

impl Failure {
    pub fn message(&self, host: &str, username: &str) -> String {
        match self {
            Failure::Rejected => format!("{host} did not accept the username or app password. Check them, or create a new app password."),
            Failure::Unreachable(reason) => format!("Could not reach {host}: {reason}. Check the address and that the server is running."),
            Failure::NotCalDav => format!("{host} does not look like a CalDAV server. Enter the address your calendar provider gives for CalDAV."),
            Failure::NoCalendars => format!("Signed in to {host}, but found no calendars for {username}. Check the address points at your calendar server."),
        }
    }

    pub fn step(&self) -> &'static str {
        match self {
            Failure::Rejected => "credential",
            _ => "address",
        }
    }
}

#[derive(Clone)]
pub struct Calendar {
    href: Url,
    name: String,
    colour: Option<String>,
    read_only: bool,
}

pub struct Discovery {
    principal: Url,
    home: Url,
    calendars: Vec<Calendar>,
}

struct Reply {
    status: u16,
    url: Url,
    etag: Option<String>,
    body: String,
}

#[derive(Default)]
struct Entry {
    href: String,
    principal: Option<String>,
    home: Option<String>,
    is_calendar: bool,
    name: Option<String>,
    colour: Option<String>,
    privileges: Option<Vec<String>>,
    etag: Option<String>,
    data: Option<String>,
}

#[derive(Debug, PartialEq)]
enum When {
    Date(NaiveDate),
    Instant(DateTime<Utc>),
}

pub fn normalise_settings(_settings: &Value) -> Result<Value, String> {
    Ok(Value::Null)
}

pub fn tools() -> Vec<StoredTool> {
    let range = |what: &str| json!({"type": "string", "description": format!("{what}, as RFC 3339 such as 2026-03-02T09:00:00Z, or a date such as 2026-03-02.")});
    let calendar = json!({"type": "string", "description": "The calendar's id or name from list_calendars. Defaults to every calendar."});
    let event_fields = json!({
        "summary": {"type": "string", "description": "The event's title."},
        "start": {"type": "string", "description": "When it starts, as RFC 3339, a local date and time such as 2026-03-02T09:00:00 (read in timezone), or a date for an all day event."},
        "end": {"type": "string", "description": "When it ends, in the same form as start. For an all day event the end date is the day after the last day."},
        "all_day": {"type": "boolean", "description": "Whether the event lasts whole days."},
        "location": {"type": "string"},
        "description": {"type": "string"},
        "timezone": {"type": "string", "description": "An IANA time zone such as Europe/London, used to read a start or end that has no offset. Defaults to UTC. Events are stored in UTC."}
    });
    let tool = |name: &str, description: &str, input_schema: Value| StoredTool { name: name.to_string(), description: description.to_string(), input_schema, read_only: None };
    let mut create = json!({"type": "object", "properties": event_fields.clone(), "required": ["calendar", "summary", "start", "end"]});
    let mut update = json!({"type": "object", "properties": event_fields, "required": ["href", "etag"]});

    create["properties"]["calendar"] = json!({"type": "string", "description": "The calendar's id or name from list_calendars."});
    update["properties"]["href"] = json!({"type": "string", "description": "The event's href from list_events."});
    update["properties"]["etag"] = json!({"type": "string", "description": "The event's etag from list_events."});

    vec![
        tool("list_calendars", "List the calendars of the account, with whether each is read only.", json!({"type": "object", "properties": {}})),
        tool("list_events", "List the events in a date range, with recurring events expanded into their occurrences.", json!({"type": "object", "properties": {"calendar": calendar.clone(), "start": range("The start of the range"), "end": range("The end of the range")}, "required": ["start", "end"]})),
        tool(
            "search_events",
            "Find events in a date range whose title, location or description contains the text.",
            json!({"type": "object", "properties": {"query": {"type": "string"}, "calendar": calendar, "start": range("The start of the range"), "end": range("The end of the range")}, "required": ["query", "start", "end"]}),
        ),
        tool("create_event", "Create an event. Always needs the person's approval under the default preset.", create),
        tool("update_event", "Change some properties of an event and leave the rest alone. Needs the href and etag from list_events. Always needs the person's approval under the default preset.", update),
        tool("delete_event", "Delete an event. Needs the href and etag from list_events. Always needs the person's approval under the default preset.", json!({"type": "object", "properties": {"href": {"type": "string"}, "etag": {"type": "string"}}, "required": ["href", "etag"]})),
    ]
}

pub fn always_asks(tool: &str) -> bool {
    WRITES.contains(&tool)
}

pub fn host_of(address: &str) -> String {
    Url::parse(address).ok().and_then(|parsed| parsed.host_str().map(str::to_string)).unwrap_or_else(|| address.to_string())
}

async fn send_once(http: &reqwest::Client, credentials: &Credentials, method: &reqwest::Method, url: &Url, headers: &[(&str, &str)], body: &Option<String>) -> Result<reqwest::Response, Failure> {
    let mut builder = http.request(method.clone(), url.clone()).basic_auth(&credentials.account, Some(&credentials.secret)).timeout(TIMEOUT);

    for (name, value) in headers {
        builder = builder.header(*name, *value);
    }

    if let Some(body) = body {
        builder = builder.body(body.clone());
    }

    builder.send().await.map_err(|error| Failure::Unreachable(error.without_url().to_string()))
}

async fn request(http: &reqwest::Client, credentials: &Credentials, method: &str, url: &Url, headers: &[(&str, &str)], body: Option<String>) -> Result<Reply, Failure> {
    let method = reqwest::Method::from_bytes(method.as_bytes()).map_err(|error| Failure::Unreachable(error.to_string()))?;
    let mut response = send_once(http, credentials, &method, url, headers, &body).await?;

    if response.status().as_u16() == 401 && response.url() != url {
        let redirected = response.url().clone();

        response = send_once(http, credentials, &method, &redirected, headers, &body).await?;
    }

    let status = response.status().as_u16();

    if status == 401 {
        return Err(Failure::Rejected);
    }

    let url = response.url().clone();
    let etag = response.headers().get("etag").and_then(|value| value.to_str().ok()).map(str::to_string);
    let body = response.text().await.map_err(|error| Failure::Unreachable(error.without_url().to_string()))?;

    Ok(Reply { status, url, etag, body })
}

async fn propfind(http: &reqwest::Client, credentials: &Credentials, url: &Url, depth: &str, body: &str) -> Result<Reply, Failure> {
    request(http, credentials, "PROPFIND", url, &[("depth", depth), ("content-type", "application/xml; charset=utf-8")], Some(body.to_string())).await
}

fn entries(body: &str) -> Vec<Entry> {
    let Ok(document) = roxmltree::Document::parse(body) else {
        return Vec::new();
    };
    let named = |node: roxmltree::Node, name: &str| node.is_element() && node.tag_name().name() == name;
    let text_of = |node: roxmltree::Node| node.text().map(str::trim).filter(|text| !text.is_empty()).map(str::to_string);
    let href_under = |node: roxmltree::Node| node.descendants().find(|child| named(*child, "href")).and_then(text_of);

    document
        .descendants()
        .filter(|node| named(*node, "response"))
        .map(|response| {
            let mut entry = Entry { href: response.children().find(|child| named(*child, "href")).and_then(text_of).unwrap_or_default(), ..Default::default() };

            for propstat in response.children().filter(|child| named(*child, "propstat")) {
                let accepted = propstat.children().find(|child| named(*child, "status")).and_then(text_of).is_none_or(|status| status.contains(" 200"));

                if !accepted {
                    continue;
                }

                for node in propstat.descendants().filter(|node| node.is_element()) {
                    match node.tag_name().name() {
                        "current-user-principal" => entry.principal = href_under(node),
                        "calendar-home-set" => entry.home = href_under(node),
                        "resourcetype" => entry.is_calendar = node.children().any(|child| named(child, "calendar")),
                        "displayname" => entry.name = text_of(node),
                        "calendar-color" => entry.colour = text_of(node),
                        "current-user-privilege-set" => entry.privileges = Some(node.descendants().filter(|child| child.is_element() && child.parent().is_some_and(|parent| named(parent, "privilege"))).map(|child| child.tag_name().name().to_string()).collect()),
                        "getetag" => entry.etag = text_of(node),
                        "calendar-data" => entry.data = node.text().map(str::to_string),
                        _ => {}
                    }
                }
            }

            entry
        })
        .collect()
}

fn calendars_in(body: &str, base: &Url) -> Vec<Calendar> {
    entries(body)
        .into_iter()
        .filter(|entry| entry.is_calendar)
        .filter_map(|entry| {
            let href = base.join(&entry.href).ok()?;
            let writable = entry.privileges.as_ref().is_none_or(|privileges| privileges.iter().any(|privilege| matches!(privilege.as_str(), "write" | "write-content" | "bind" | "all")));
            let fallback = href.path_segments().and_then(|mut segments| segments.rfind(|segment| !segment.is_empty())).unwrap_or("calendar").to_string();

            Some(Calendar { href, name: entry.name.unwrap_or(fallback), colour: entry.colour, read_only: !writable })
        })
        .collect()
}

pub async fn discover(http: &reqwest::Client, credentials: &Credentials) -> Result<Discovery, Failure> {
    let base = Url::parse(&credentials.address).map_err(|_| Failure::NotCalDav)?;
    let well_known = base.join("/.well-known/caldav").map_err(|_| Failure::NotCalDav)?;
    let mut principal = None;

    for candidate in [well_known, base.clone()] {
        let reply = propfind(http, credentials, &candidate, "0", PRINCIPAL_QUERY).await?;

        if matches!(reply.status, 401 | 403) {
            return Err(Failure::Rejected);
        }

        let found = entries(&reply.body).into_iter().find_map(|entry| entry.principal);

        if let Some(found) = found.and_then(|href| reply.url.join(&href).ok()) {
            principal = Some(found);
            break;
        }
    }

    let principal = principal.ok_or(Failure::NotCalDav)?;
    let reply = propfind(http, credentials, &principal, "0", HOME_QUERY).await?;
    let home = entries(&reply.body).into_iter().find_map(|entry| entry.home).and_then(|href| reply.url.join(&href).ok()).ok_or(Failure::NotCalDav)?;
    let listing = propfind(http, credentials, &home, "1", COLLECTIONS_QUERY).await?;
    let calendars = calendars_in(&listing.body, &listing.url);

    if calendars.is_empty() {
        return Err(Failure::NoCalendars);
    }

    Ok(Discovery { principal, home, calendars })
}

pub async fn verify(http: &reqwest::Client, credentials: &Credentials) -> Result<Value, String> {
    let host = host_of(&credentials.address);

    match discover(http, credentials).await {
        Ok(discovery) => Ok(json!({"principal": discovery.principal.as_str(), "calendar_home": discovery.home.as_str()})),
        Err(failure) => Err(failure.message(&host, &credentials.account)),
    }
}

async fn calendars_of(http: &reqwest::Client, credentials: &Credentials) -> Result<Vec<Calendar>, Failure> {
    let stored = credentials.settings["calendar_home"].as_str().and_then(|home| Url::parse(home).ok());

    let Some(home) = stored else {
        return Ok(discover(http, credentials).await?.calendars);
    };
    let listing = propfind(http, credentials, &home, "1", COLLECTIONS_QUERY).await?;

    match calendars_in(&listing.body, &listing.url).is_empty() {
        true => Ok(discover(http, credentials).await?.calendars),
        false => Ok(calendars_in(&listing.body, &listing.url)),
    }
}

fn calendar_json(calendar: &Calendar) -> Value {
    json!({"id": calendar.href.path(), "name": calendar.name, "colour": calendar.colour, "read_only": calendar.read_only})
}

fn chosen<'a>(calendars: &'a [Calendar], given: &str) -> Result<&'a Calendar, String> {
    let given = given.trim();

    calendars
        .iter()
        .find(|calendar| calendar.href.path() == given || calendar.href.as_str() == given)
        .or_else(|| calendars.iter().find(|calendar| calendar.name.eq_ignore_ascii_case(given)))
        .ok_or_else(|| format!("There is no calendar called {given}. Use list_calendars to see the calendars."))
}

fn text(arguments: &Value, key: &str) -> Result<String, String> {
    match arguments[key].as_str().map(str::trim).filter(|value| !value.is_empty()) {
        Some(value) => Ok(value.to_string()),
        None => Err(format!("Give the {key}.")),
    }
}

fn parse_when(given: &str, timezone: Option<&str>) -> Result<When, String> {
    let given = given.trim();
    let invalid = || format!("{given} is not a date or time. Use RFC 3339, such as 2026-03-02T09:00:00Z, or a date such as 2026-03-02.");

    if let Ok(date) = NaiveDate::parse_from_str(given, "%Y-%m-%d") {
        return Ok(When::Date(date));
    }

    if let Ok(instant) = DateTime::parse_from_rfc3339(given) {
        return Ok(When::Instant(instant.with_timezone(&Utc)));
    }

    let local = NaiveDateTime::parse_from_str(given, "%Y-%m-%dT%H:%M:%S").or_else(|_| NaiveDateTime::parse_from_str(given, "%Y-%m-%dT%H:%M")).map_err(|_| invalid())?;

    match timezone.map(str::trim).filter(|zone| !zone.is_empty()) {
        None => Ok(When::Instant(Utc.from_utc_datetime(&local))),
        Some(zone) => {
            let zone: chrono_tz::Tz = zone.parse().map_err(|_| format!("{zone} is not a time zone name such as Europe/London."))?;
            let resolved = zone.from_local_datetime(&local).earliest().ok_or_else(|| format!("{given} does not exist in {zone}."))?;

            Ok(When::Instant(resolved.with_timezone(&Utc)))
        }
    }
}

fn instant_of(when: &When) -> DateTime<Utc> {
    match when {
        When::Date(date) => Utc.from_utc_datetime(&date.and_hms_opt(0, 0, 0).unwrap_or_default()),
        When::Instant(instant) => *instant,
    }
}

fn range_of(arguments: &Value) -> Result<(DateTime<Utc>, DateTime<Utc>), String> {
    let timezone = arguments["timezone"].as_str();
    let start = instant_of(&parse_when(&text(arguments, "start")?, timezone)?);
    let end = instant_of(&parse_when(&text(arguments, "end")?, timezone)?);

    match end > start {
        true => Ok((start, end)),
        false => Err("The end of the range must be after the start.".to_string()),
    }
}

fn unfold(ics: &str) -> Vec<String> {
    let mut lines: Vec<String> = Vec::new();

    for line in ics.replace("\r\n", "\n").split('\n') {
        match (line.strip_prefix(' ').or_else(|| line.strip_prefix('\t')), lines.last_mut()) {
            (Some(continuation), Some(previous)) => previous.push_str(continuation),
            _ => lines.push(line.to_string()),
        }
    }

    lines.retain(|line| !line.is_empty());

    lines
}

fn fold(line: &str) -> String {
    let characters: Vec<char> = line.chars().collect();

    characters.chunks(FOLD_AT).map(|chunk| chunk.iter().collect::<String>()).collect::<Vec<_>>().join("\r\n ")
}

fn unescape(value: &str) -> String {
    let mut out = String::new();
    let mut characters = value.chars();

    while let Some(character) = characters.next() {
        if character != '\\' {
            out.push(character);
            continue;
        }

        match characters.next() {
            Some('n') | Some('N') => out.push('\n'),
            Some(other) => out.push(other),
            None => {}
        }
    }

    out
}

fn escape(value: &str) -> String {
    value.replace('\\', "\\\\").replace(';', "\\;").replace(',', "\\,").replace("\r\n", "\\n").replace('\n', "\\n")
}

fn split_line(line: &str) -> (String, Vec<(String, String)>, String) {
    let (head, value) = line.split_once(':').unwrap_or((line, ""));
    let mut parts = head.split(';');
    let name = parts.next().unwrap_or_default().to_ascii_uppercase();
    let parameters = parts.filter_map(|part| part.split_once('=')).map(|(key, value)| (key.to_ascii_uppercase(), value.trim_matches('"').to_string())).collect();

    (name, parameters, value.to_string())
}

fn time_json(value: &str, parameters: &[(String, String)]) -> (Value, bool) {
    let all_day = parameters.iter().any(|(key, value)| key == "VALUE" && value == "DATE") || value.len() == 8;

    if all_day {
        return match NaiveDate::parse_from_str(value, "%Y%m%d") {
            Ok(date) => (json!(date.format("%Y-%m-%d").to_string()), true),
            Err(_) => (json!(value), true),
        };
    }

    let Ok(local) = NaiveDateTime::parse_from_str(value.trim_end_matches('Z'), "%Y%m%dT%H%M%S") else {
        return (json!(value), false);
    };

    if value.ends_with('Z') {
        return (json!(Utc.from_utc_datetime(&local).to_rfc3339_opts(chrono::SecondsFormat::Secs, true)), false);
    }

    let zone = parameters.iter().find(|(key, _)| key == "TZID").and_then(|(_, zone)| zone.parse::<chrono_tz::Tz>().ok());

    match zone.and_then(|zone| zone.from_local_datetime(&local).earliest()) {
        Some(resolved) => (json!(resolved.to_rfc3339_opts(chrono::SecondsFormat::Secs, true)), false),
        None => (json!(local.format("%Y-%m-%dT%H:%M:%S").to_string()), false),
    }
}

fn events_in(ics: &str, calendar: &str, href: &str, etag: Option<&str>) -> Vec<Value> {
    let mut events = Vec::new();
    let mut current: Option<serde_json::Map<String, Value>> = None;
    let mut nested = 0;

    for line in unfold(ics) {
        let (name, parameters, value) = split_line(&line);

        match (name.as_str(), value.as_str(), current.as_mut()) {
            ("BEGIN", "VEVENT", None) => {
                let mut event = serde_json::Map::new();

                event.insert("href".into(), json!(href));
                event.insert("etag".into(), json!(etag));
                event.insert("calendar".into(), json!(calendar));
                event.insert("all_day".into(), json!(false));
                current = Some(event);
            }
            ("END", "VEVENT", Some(_)) => {
                if let Some(event) = current.take() {
                    events.push(Value::Object(event));
                }
            }
            ("BEGIN", _, Some(_)) => nested += 1,
            ("END", _, Some(_)) => nested -= 1,
            (_, _, Some(event)) if nested == 0 => match name.as_str() {
                "UID" => {
                    event.insert("uid".into(), json!(value));
                }
                "SUMMARY" => {
                    event.insert("summary".into(), json!(unescape(&value)));
                }
                "LOCATION" => {
                    event.insert("location".into(), json!(unescape(&value)));
                }
                "DESCRIPTION" => {
                    event.insert("description".into(), json!(unescape(&value)));
                }
                "RRULE" => {
                    event.insert("recurrence".into(), json!(value));
                }
                "DTSTART" => {
                    let (start, all_day) = time_json(&value, &parameters);

                    event.insert("start".into(), start);
                    event.insert("all_day".into(), json!(all_day));
                }
                "DTEND" => {
                    event.insert("end".into(), time_json(&value, &parameters).0);
                }
                _ => {}
            },
            _ => {}
        }
    }

    events
}

fn ics_stamp(instant: DateTime<Utc>) -> String {
    instant.format("%Y%m%dT%H%M%SZ").to_string()
}

fn when_line(name: &str, when: &When) -> String {
    match when {
        When::Date(date) => format!("{name};VALUE=DATE:{}", date.format("%Y%m%d")),
        When::Instant(instant) => format!("{name}:{}", ics_stamp(*instant)),
    }
}

fn span_of(arguments: &Value) -> Result<(When, When), String> {
    let timezone = arguments["timezone"].as_str();
    let all_day = arguments["all_day"].as_bool().unwrap_or(false);
    let start = parse_when(&text(arguments, "start")?, timezone)?;
    let end = parse_when(&text(arguments, "end")?, timezone)?;

    if all_day {
        let first = instant_of(&start).date_naive();
        let last = instant_of(&end).date_naive();
        let last = if last > first { last } else { first + ChronoDuration::days(1) };

        return Ok((When::Date(first), When::Date(last)));
    }

    match instant_of(&end) > instant_of(&start) {
        true => Ok((start, end)),
        false => Err("The event must end after it starts.".to_string()),
    }
}

fn create_ics(uid: &str, arguments: &Value) -> Result<String, String> {
    let (start, end) = span_of(arguments)?;
    let mut lines = vec!["BEGIN:VCALENDAR".to_string(), "VERSION:2.0".to_string(), "PRODID:-//Mycelium//Calendar//EN".to_string(), "BEGIN:VEVENT".to_string(), format!("UID:{uid}"), format!("DTSTAMP:{}", ics_stamp(Utc::now())), when_line("DTSTART", &start), when_line("DTEND", &end), format!("SUMMARY:{}", escape(&text(arguments, "summary")?))];

    for (key, name) in [("location", "LOCATION"), ("description", "DESCRIPTION")] {
        if let Some(value) = arguments[key].as_str().filter(|value| !value.is_empty()) {
            lines.push(format!("{name}:{}", escape(value)));
        }
    }

    lines.push("END:VEVENT".to_string());
    lines.push("END:VCALENDAR".to_string());

    Ok(lines.iter().map(|line| fold(line)).collect::<Vec<_>>().join("\r\n") + "\r\n")
}

fn updated_ics(ics: &str, arguments: &Value) -> Result<String, String> {
    let mut lines = unfold(ics);
    let begin = lines.iter().position(|line| line == "BEGIN:VEVENT").ok_or("The server returned no event at that href.")?;
    let mut end = lines.iter().enumerate().skip(begin).find(|(_, line)| *line == "END:VEVENT").map(|(index, _)| index).ok_or("The event on the server is malformed.")?;
    let mut set = |lines: &mut Vec<String>, names: &[&str], replacement: Option<String>| {
        let mut depth = 0;
        let mut index = begin + 1;

        while index < end {
            let (name, _, value) = split_line(&lines[index]);

            match (name.as_str(), depth) {
                ("BEGIN", _) => depth += 1,
                ("END", _) if value != "VEVENT" => depth -= 1,
                (name, 0) if names.contains(&name) => {
                    lines.remove(index);
                    end -= 1;
                    continue;
                }
                _ => {}
            }

            index += 1;
        }

        if let Some(replacement) = replacement {
            lines.insert(end, replacement);
            end += 1;
        }
    };

    if let Some(summary) = arguments["summary"].as_str() {
        set(&mut lines, &["SUMMARY"], Some(format!("SUMMARY:{}", escape(summary))));
    }

    for (key, name) in [("location", "LOCATION"), ("description", "DESCRIPTION")] {
        if let Some(value) = arguments[key].as_str() {
            set(&mut lines, &[name], (!value.is_empty()).then(|| format!("{name}:{}", escape(value))));
        }
    }

    let changes_time = arguments["start"].is_string() || arguments["end"].is_string() || arguments["all_day"].is_boolean();

    if changes_time {
        let (start, finish) = span_of(arguments).map_err(|reason| match reason.starts_with("Give the") {
            true => "Give both start and end to change when the event happens.".to_string(),
            false => reason,
        })?;

        set(&mut lines, &["DTSTART"], Some(when_line("DTSTART", &start)));
        set(&mut lines, &["DTEND", "DURATION"], Some(when_line("DTEND", &finish)));
    }

    Ok(lines.iter().map(|line| fold(line)).collect::<Vec<_>>().join("\r\n") + "\r\n")
}

fn matches_query(event: &Value, query: &str) -> bool {
    let needle = query.to_lowercase();

    ["summary", "location", "description"].iter().any(|key| event[key].as_str().is_some_and(|value| value.to_lowercase().contains(&needle)))
}

fn range_query(start: DateTime<Utc>, end: DateTime<Utc>) -> String {
    let (start, end) = (ics_stamp(start), ics_stamp(end));

    format!(
        r#"<c:calendar-query xmlns:d="DAV:" xmlns:c="urn:ietf:params:xml:ns:caldav"><d:prop><d:getetag/><c:calendar-data><c:expand start="{start}" end="{end}"/></c:calendar-data></d:prop><c:filter><c:comp-filter name="VCALENDAR"><c:comp-filter name="VEVENT"><c:time-range start="{start}" end="{end}"/></c:comp-filter></c:comp-filter></c:filter></c:calendar-query>"#
    )
}

async fn events_between(http: &reqwest::Client, credentials: &Credentials, calendar: &Calendar, start: DateTime<Utc>, end: DateTime<Utc>) -> Result<Vec<Value>, Failure> {
    let reply = request(http, credentials, "REPORT", &calendar.href, &[("depth", "1"), ("content-type", "application/xml; charset=utf-8")], Some(range_query(start, end))).await?;
    let mut events = Vec::new();

    for entry in entries(&reply.body) {
        let Some(data) = entry.data.as_deref() else {
            continue;
        };
        let href = reply.url.join(&entry.href).map(|href| href.path().to_string()).unwrap_or(entry.href.clone());

        events.extend(events_in(data, &calendar.name, &href, entry.etag.as_deref()));
    }

    Ok(events)
}

fn event_url(calendars: &[Calendar], href: &str) -> Result<(Url, Calendar), String> {
    let base = calendars.first().map(|calendar| calendar.href.clone()).ok_or("The account has no calendars.")?;
    let url = base.join(href.trim()).map_err(|_| format!("{href} is not an event href. Use the href from list_events."))?;
    let owner = calendars.iter().find(|calendar| url.path().starts_with(calendar.href.path())).ok_or_else(|| format!("{href} is not in one of the account's calendars."))?;

    Ok((url, owner.clone()))
}

fn failed(reply: &Reply, what: &str) -> String {
    match reply.status {
        412 => CONFLICT.to_string(),
        403 => READ_ONLY.to_string(),
        404 => "There is no event at that href. List the events again.".to_string(),
        status => format!("The calendar server refused to {what} the event (status {status})."),
    }
}

fn quoted(etag: &str) -> String {
    match etag.starts_with('"') || etag.starts_with("W/") {
        true => etag.to_string(),
        false => format!("\"{etag}\""),
    }
}

async fn run(http: &reqwest::Client, credentials: &Credentials, tool: &str, arguments: &Value) -> Result<Value, String> {
    let host = host_of(&credentials.address);
    let calendars = calendars_of(http, credentials).await.map_err(|failure| failure.message(&host, &credentials.account))?;
    let unreachable = |failure: Failure| failure.message(&host, &credentials.account);

    match tool {
        "list_calendars" => Ok(Value::Array(calendars.iter().map(calendar_json).collect())),
        "list_events" | "search_events" => {
            let (start, end) = range_of(arguments)?;
            let query = match tool {
                "search_events" => Some(text(arguments, "query")?),
                _ => None,
            };
            let selected: Vec<&Calendar> = match arguments["calendar"].as_str().map(str::trim).filter(|given| !given.is_empty()) {
                Some(given) => vec![chosen(&calendars, given)?],
                None => calendars.iter().collect(),
            };
            let mut events = Vec::new();

            for calendar in selected {
                events.extend(events_between(http, credentials, calendar, start, end).await.map_err(unreachable)?);
            }

            if let Some(query) = query {
                events.retain(|event| matches_query(event, &query));
            }

            Ok(Value::Array(events))
        }
        "create_event" => {
            let calendar = chosen(&calendars, &text(arguments, "calendar")?)?;

            if calendar.read_only {
                return Err(READ_ONLY.to_string());
            }

            let uid = Uuid::new_v4().to_string();
            let ics = create_ics(&uid, arguments)?;
            let url = calendar.href.join(&format!("{uid}.ics")).map_err(|error| error.to_string())?;
            let reply = request(http, credentials, "PUT", &url, &[("content-type", "text/calendar; charset=utf-8"), ("if-none-match", "*")], Some(ics)).await.map_err(unreachable)?;

            if !matches!(reply.status, 200 | 201 | 204) {
                return Err(failed(&reply, "create"));
            }

            let etag = match reply.etag {
                Some(etag) => Some(etag),
                None => request(http, credentials, "GET", &url, &[], None).await.ok().and_then(|fetched| fetched.etag),
            };

            Ok(json!({"uid": uid, "href": url.path(), "etag": etag, "calendar": calendar.name}))
        }
        "update_event" => {
            let (url, calendar) = event_url(&calendars, &text(arguments, "href")?)?;
            let etag = text(arguments, "etag")?;

            if calendar.read_only {
                return Err(READ_ONLY.to_string());
            }

            let current = request(http, credentials, "GET", &url, &[], None).await.map_err(unreachable)?;

            if current.status != 200 {
                return Err(failed(&current, "read"));
            }

            let ics = updated_ics(&current.body, arguments)?;
            let reply = request(http, credentials, "PUT", &url, &[("content-type", "text/calendar; charset=utf-8"), ("if-match", &quoted(&etag))], Some(ics)).await.map_err(unreachable)?;

            if !matches!(reply.status, 200 | 201 | 204) {
                return Err(failed(&reply, "change"));
            }

            Ok(json!({"href": url.path(), "etag": reply.etag, "calendar": calendar.name}))
        }
        "delete_event" => {
            let (url, calendar) = event_url(&calendars, &text(arguments, "href")?)?;
            let etag = text(arguments, "etag")?;

            if calendar.read_only {
                return Err(READ_ONLY.to_string());
            }

            let reply = request(http, credentials, "DELETE", &url, &[("if-match", &quoted(&etag))], None).await.map_err(unreachable)?;

            if !matches!(reply.status, 200 | 204) {
                return Err(failed(&reply, "delete"));
            }

            Ok(json!({"done": true}))
        }
        _ => Err(format!("Calendar has no tool called {tool}.")),
    }
}

pub async fn call(http: &reqwest::Client, credentials: &Credentials, tool: &str, arguments: &Value) -> CallOutput {
    match run(http, credentials, tool, arguments).await {
        Ok(value) => CallOutput { text: value.to_string(), is_error: false },
        Err(reason) => CallOutput { text: reason, is_error: true },
    }
}

#[derive(serde::Deserialize)]
pub struct TestRequest {
    url: String,
    username: String,
    password: String,
}

pub async fn test(State(state): State<AppState>, _user: AuthUser, Json(request): Json<TestRequest>) -> ApiResult<Json<Value>> {
    let address = Url::parse(request.url.trim()).ok().filter(|parsed| matches!(parsed.scheme(), "http" | "https") && parsed.host_str().is_some());
    let Some(address) = address else {
        return Err(ApiError::bad_request("caldav_address", "Enter the server's address, starting with http:// or https://, for example https://cal.example.com."));
    };
    let username = request.username.trim().to_string();
    let credentials = Credentials { address: address.as_str().to_string(), account: username.clone(), secret: request.password, settings: Value::Null, session: None };

    match discover(&state.http, &credentials).await {
        Ok(discovery) => Ok(Json(json!({"username": username, "calendars": discovery.calendars.iter().map(|calendar| json!({"name": calendar.name, "read_only": calendar.read_only})).collect::<Vec<_>>()}))),
        Err(failure) => {
            let code = match failure.step() {
                "credential" => "caldav_credential",
                _ => "caldav_address",
            };

            Err(ApiError::bad_request(code, failure.message(&host_of(&credentials.address), &username)))
        }
    }
}

pub async fn calendars(State(state): State<AppState>, user: AuthUser, Path(server_id): Path<Uuid>) -> ApiResult<Json<Value>> {
    let row = sqlx::query_as::<_, ServerRow>(sqlx::AssertSqlSafe(format!("select {SERVER_COLUMNS} from chat_tool_servers where id = $1 and owner = $2 and kind = 'caldav'"))).bind(server_id).bind(user.id).fetch_optional(&state.pool).await?;
    let row = row.ok_or_else(|| ApiError::not_found("That calendar account"))?;
    let credentials = row.credentials();
    let listed = calendars_of(&state.http, &credentials).await;

    match listed {
        Ok(calendars) => Ok(Json(Value::Array(calendars.iter().map(|calendar| json!({"name": calendar.name, "read_only": calendar.read_only})).collect()))),
        Err(failure) => Err(ApiError::bad_request("caldav_unavailable", failure.message(&host_of(&credentials.address), &credentials.account))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_calendar_without_write_privileges_is_read_only() {
        let body = r#"<d:multistatus xmlns:d="DAV:" xmlns:c="urn:ietf:params:xml:ns:caldav"><d:response><d:href>/sam/family/</d:href><d:propstat><d:prop><d:resourcetype><d:collection/><c:calendar/></d:resourcetype><d:displayname>Family</d:displayname><d:current-user-privilege-set><d:privilege><d:read/></d:privilege></d:current-user-privilege-set></d:prop><d:status>HTTP/1.1 200 OK</d:status></d:propstat></d:response></d:multistatus>"#;
        let calendars = calendars_in(body, &Url::parse("http://cal.example.com/sam/").unwrap());

        assert_eq!(calendars.len(), 1);
        assert!(calendars[0].read_only);
        assert_eq!(calendars[0].name, "Family");
    }

    #[test]
    fn an_event_is_read_with_unfolded_escaped_text_and_all_day_dates() {
        let ics = "BEGIN:VCALENDAR\r\nBEGIN:VEVENT\r\nUID:a\r\nSUMMARY:Lunch\\, with\r\n  Sam\r\nDTSTART;VALUE=DATE:20260302\r\nDTEND;VALUE=DATE:20260303\r\nBEGIN:VALARM\r\nSUMMARY:ignored\r\nEND:VALARM\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n";
        let events = events_in(ics, "Home", "/a.ics", Some("\"1\""));

        assert_eq!(events[0]["summary"], "Lunch, with Sam");
        assert_eq!(events[0]["start"], "2026-03-02");
        assert_eq!(events[0]["all_day"], true);
    }

    #[test]
    fn a_local_time_is_read_in_the_given_time_zone() {
        let when = parse_when("2026-07-01T09:00:00", Some("Europe/London")).unwrap();

        assert_eq!(when, When::Instant(Utc.with_ymd_and_hms(2026, 7, 1, 8, 0, 0).unwrap()));
    }
}
