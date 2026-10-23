use mycelium_proto::api::*;
use mycelium_proto::conversation::*;
use mycelium_proto::link::*;
use mycelium_proto::stream::StreamMessage;
use serde::{Serialize, de::DeserializeOwned};
use serde_json::{Value, json};
use std::collections::BTreeMap;
use uuid::Uuid;

fn round_trip<T: Serialize + DeserializeOwned + PartialEq + std::fmt::Debug>(value: &T) -> Value {
    let json = serde_json::to_value(value).expect("serialise");
    let back: T = serde_json::from_value(json.clone()).expect("deserialise");
    assert_eq!(&back, value);
    json
}

fn id() -> Uuid {
    Uuid::from_u128(7)
}

fn now() -> chrono::DateTime<chrono::Utc> {
    chrono::DateTime::from_timestamp(1_700_000_000, 0).unwrap()
}

fn sample() -> ResourceSample {
    ResourceSample {
        at_ms: 1,
        available: true,
        cpu_percent: 12.5,
        cores: 4,
        load: [0.5, 0.4, 0.3],
        memory_total_bytes: 8,
        memory_available_bytes: 4,
        swap_total_bytes: 2,
        swap_free_bytes: 1,
        disks: vec![DiskUsage { mount: "/".into(), total_bytes: 10, free_bytes: 5 }],
        processes: vec![ProcessUsage { pid: 7, name: "claude".into(), command: "claude".into(), cpu_percent: 1.0, memory_bytes: 3, agent: true, stoppable: true, conversation_id: Some(id()), cwd: Some("/work".into()) }],
        ports: vec![ListeningPort { port: 3000, address: "0.0.0.0".into(), pid: Some(7), process: Some("node".into()), conversation_id: Some(id()), cwd: Some("/work".into()) }],
        docker: Some(DockerUsage { running: 1, containers: vec![DockerContainer { name: "db".into(), state: "running".into() }], reclaimable_bytes: Some(21_800_000_000) }),
    }
}

fn conversation() -> Conversation {
    Conversation {
        id: id(),
        owner: id(),
        node_id: id(),
        agent: ConversationAgent::Claude,
        cwd: "/work".into(),
        title: "t".into(),
        title_user_set: false,
        status: ConversationStatus::Input,
        acp_session_id: Some("s".into()),
        mode: None,
        model: Some("m".into()),
        account: None,
        options: None,
        parent_id: None,
        side_kind: None,
        provider_id: None,
        project_id: None,
        group_id: None,
        created_at: now(),
        updated_at: now(),
        archived_at: None,
        pinned_at: None,
        resume_at: None,
    }
}

#[test]
fn node_hello_is_tagged_snake_case_with_exact_fields() {
    let json = round_trip(&NodeToControl::Hello {
        version: "0.1.0".into(),
        hostname: "h".into(),
        os: "linux".into(),
        arch: "x86_64".into(),
        agents: vec![AgentInfo { agent: AgentId::Codex, version: None, available: false, update_available: None }],
        sessions: vec![SessionPosition { conversation_id: id(), last_seq: 4, live: true }],
    });

    assert_eq!(json["type"], "hello");
    assert_eq!(json["agents"][0]["agent"], "codex");
    assert_eq!(json["sessions"][0]["last_seq"], 4);
}

#[test]
fn every_node_message_round_trips_under_its_tag() {
    let cases = vec![
        (NodeToControl::SessionStarted { conversation_id: id(), acp_session_id: "s".into(), modes: Some(json!([])), models: None, config_options: None }, "session_started"),
        (NodeToControl::SessionClosed { conversation_id: id() }, "session_closed"),
        (NodeToControl::PromptReceived { conversation_id: id(), prompt_id: id() }, "prompt_received"),
        (NodeToControl::AcpUpdate { conversation_id: id(), seq: 1, update: json!({"a": 1}) }, "acp_update"),
        (NodeToControl::PermissionRequest { conversation_id: id(), seq: 2, request_id: "r".into(), request: json!({}) }, "permission_request"),
        (NodeToControl::TurnEnd { conversation_id: id(), seq: 3, stop_reason: "end_turn".into(), usage: Some(json!({"inputTokens": 3})) }, "turn_end"),
        (NodeToControl::SessionFailed { conversation_id: id(), seq: 4, message: "x".into() }, "session_failed"),
        (NodeToControl::Logins { logins: vec![LoginInfo { agent: AgentId::Claude, account: None, method: "token".into(), state: LoginState::SignedOut, usage_left: None, windows: vec![], usage_read_at: None }] }, "logins"),
        (NodeToControl::LoginOutput { login_id: id(), data: "hi".into() }, "login_output"),
        (NodeToControl::LoginFinished { login_id: id(), ok: true }, "login_finished"),
        (NodeToControl::TerminalOutput { terminal_id: id(), data: "aGk=".into() }, "terminal_output"),
        (NodeToControl::TerminalExited { terminal_id: id() }, "terminal_exited"),
        (NodeToControl::GitResult { request_id: id(), ok: false, output: Value::Null, error: Some("fatal".into()) }, "git_result"),
        (NodeToControl::Transcript { path: "p".into(), offset: 0, data: "AA==".into() }, "transcript"),
        (NodeToControl::Workflow { conversation_id: id(), run_id: "run".into(), line: json!({}) }, "workflow"),
        (NodeToControl::InstructionsHeld { revision: "afad776".into(), unmanaged: vec!["Claude Code: /h/.claude/CLAUDE.md".into()] }, "instructions_held"),
        (NodeToControl::Resources { sample: sample() }, "resources"),
        (NodeToControl::Pong, "pong"),
    ];

    for (message, tag) in cases {
        let json = round_trip(&message);
        assert_eq!(json["type"], tag);
    }
}

#[test]
fn every_control_message_round_trips_under_its_tag() {
    let mut config = BTreeMap::new();
    config.insert("effort".to_string(), json!("high"));
    let cases = vec![
        (ControlToNode::Ack { conversation_id: id(), seq: 9 }, "ack"),
        (ControlToNode::StartSession { conversation_id: id(), agent: AgentId::Claude, cwd: "/w".into(), resume: Some("s".into()), fork: false, account: None, instructions: None }, "start_session"),
        (ControlToNode::Instructions { revision: "afad776".into(), global: Some("# Rules".into()), take_over: false }, "instructions"),
        (ControlToNode::Prompt { conversation_id: id(), prompt_id: id(), text: "go".into(), attachments: vec![] }, "prompt"),
        (ControlToNode::Cancel { conversation_id: id() }, "cancel"),
        (ControlToNode::PermissionResponse { conversation_id: id(), request_id: "r".into(), outcome: json!({"outcome": "selected", "optionId": "allow"}) }, "permission_response"),
        (ControlToNode::SetConfig { conversation_id: id(), mode: Some("plan".into()), model: None, config: Some(config) }, "set_config"),
        (ControlToNode::CloseSession { conversation_id: id() }, "close_session"),
        (ControlToNode::StartLogin { login_id: id(), agent: AgentId::Codex, method: "device".into(), account: None }, "start_login"),
        (ControlToNode::LoginInput { login_id: id(), text: "x".into() }, "login_input"),
        (ControlToNode::StopLogin { login_id: id() }, "stop_login"),
        (ControlToNode::StartTerminal { terminal_id: id(), conversation_id: id(), cwd: "/work".into(), cols: 80, rows: 24 }, "start_terminal"),
        (ControlToNode::TerminalInput { terminal_id: id(), data: "bHM=".into() }, "terminal_input"),
        (ControlToNode::ResizeTerminal { terminal_id: id(), cols: 100, rows: 30 }, "resize_terminal"),
        (ControlToNode::StopTerminal { terminal_id: id() }, "stop_terminal"),
        (ControlToNode::Watch { seconds: 30 }, "watch"),
        (ControlToNode::Git { request_id: id(), cwd: "/work".into(), command: GitCommand::Diff { path: "a".into(), staged: true } }, "git"),
        (ControlToNode::TranscriptAck { path: "p".into(), bytes: 10 }, "transcript_ack"),
        (ControlToNode::Ping, "ping"),
        (ControlToNode::Revoked, "revoked"),
    ];

    for (message, tag) in cases {
        let json = round_trip(&message);
        assert_eq!(json["type"], tag);
    }
}

#[test]
fn git_commands_are_tagged_snake_case() {
    assert_eq!(round_trip(&GitCommand::StageAll), json!({"type": "stage_all"}));
    assert_eq!(round_trip(&GitCommand::Log { limit: 5 }), json!({"type": "log", "limit": 5}));
    assert_eq!(round_trip(&GitCommand::UndoLastCommit), json!({"type": "undo_last_commit"}));
    assert_eq!(round_trip(&GitCommand::Register), json!({"type": "register"}));
    assert_eq!(round_trip(&GitCommand::AddWorktree { branch: "b".into() }), json!({"type": "add_worktree", "branch": "b"}));
    assert_eq!(
        round_trip(&GitCommand::Clone { url: "https://x/y.git".into(), credentials: Some(mycelium_proto::link::GitCredentials { username: "u".into(), password: "p".into() }) }),
        json!({"type": "clone", "url": "https://x/y.git", "credentials": {"username": "u", "password": "p"}})
    );
}

#[test]
fn start_session_omits_resume_when_absent_and_accepts_it_missing() {
    let message = ControlToNode::StartSession { conversation_id: id(), agent: AgentId::Claude, cwd: "/w".into(), resume: None, fork: false, account: None, instructions: None };
    let json = round_trip(&message);

    assert!(json.get("resume").is_none());
}

#[test]
fn permission_response_keeps_the_acp_outcome_untouched() {
    let outcome = json!({"outcome": "selected", "optionId": "allow_once"});
    let json = round_trip(&ControlToNode::PermissionResponse { conversation_id: id(), request_id: "r".into(), outcome: outcome.clone() });

    assert_eq!(json["outcome"], outcome);
}

#[test]
fn unknown_node_message_type_is_rejected() {
    let parsed = serde_json::from_value::<NodeToControl>(json!({"type": "mystery"}));

    assert!(parsed.is_err());
}

#[test]
fn conversation_status_serialises_snake_case_and_ranks_for_the_sidebar() {
    assert_eq!(serde_json::to_value(ConversationStatus::Input).unwrap(), "input");
    let mut statuses = vec![
        ConversationStatus::Done,
        ConversationStatus::Working,
        ConversationStatus::Review,
        ConversationStatus::Failed,
        ConversationStatus::Input,
    ];
    statuses.sort_by_key(|status| status.sidebar_rank());

    assert_eq!(
        statuses,
        vec![
            ConversationStatus::Input,
            ConversationStatus::Failed,
            ConversationStatus::Review,
            ConversationStatus::Working,
            ConversationStatus::Done,
        ]
    );
}

#[test]
fn event_kinds_use_the_documented_names() {
    let names: Vec<Value> = [
        EventKind::UserMessage,
        EventKind::AcpUpdate,
        EventKind::PermissionRequest,
        EventKind::PermissionResolved,
        EventKind::TurnEnd,
        EventKind::Error,
        EventKind::Workflow,
        EventKind::ModelChanged,
    ]
    .iter()
    .map(|kind| serde_json::to_value(kind).unwrap())
    .collect();

    assert_eq!(
        names,
        vec!["user_message", "acp_update", "permission_request", "permission_resolved", "turn_end", "error", "workflow", "model_changed"]
    );
}

#[test]
fn stream_messages_are_tagged_and_flatten_the_conversation_row() {
    let json = round_trip(&StreamMessage::Conversation(conversation()));

    assert_eq!(json["type"], "conversation");
    assert_eq!(json["status"], "input");
    assert_eq!(json["agent"], "claude");

    let event = round_trip(&StreamMessage::Event(Event { conversation_id: id(), seq: 3, kind: EventKind::TurnEnd, payload: json!({"stop_reason": "end_turn"}) }));

    assert_eq!(event["type"], "event");
    assert_eq!(event["kind"], "turn_end");
    assert_eq!(event["payload"]["stop_reason"], "end_turn");
}

#[test]
fn stream_memory_messages_round_trip_under_their_tags() {
    let now = "2026-10-22T10:00:00Z".parse().unwrap();
    let memory = Memory { id: id(), text: "Staging listens on 5433".into(), scopes: vec!["global".into()], created_by: "laptop/Fix login".into(), conversation_id: None, created_at: now, updated_at: now };

    let changed = round_trip(&StreamMessage::Memory(memory));
    assert_eq!((changed["type"].as_str(), changed["text"].as_str(), changed["scopes"][0].as_str()), (Some("memory"), Some("Staging listens on 5433"), Some("global")));

    let removed = round_trip(&StreamMessage::MemoryRemoved { memory_id: id() });
    assert_eq!(removed["type"], "memory_removed");
    assert_eq!(removed["memory_id"], id().to_string());
}

#[test]
fn stream_import_job_messages_carry_progress_and_the_files_that_failed() {
    let job = ImportJob { id: id(), node_id: id(), agent: AgentId::Claude, state: ImportJobState::Running, total: 149, done: 40, imported: 38, skipped: 1, waiting: 0, failed: vec![ImportProblem { path: "p.jsonl".into(), reason: "There is no conversation in this file.".into() }] };

    let message = round_trip(&StreamMessage::ImportJob(job));

    assert_eq!((message["type"].as_str(), message["state"].as_str(), message["done"].as_i64()), (Some("import_job"), Some("running"), Some(40)));
    assert_eq!(message["failed"][0]["path"], "p.jsonl");
}

#[test]
fn stream_login_and_queue_messages_round_trip() {
    let output = round_trip(&StreamMessage::LoginOutput { login_id: id(), data: "x".into() });
    assert_eq!(output["type"], "login_output");

    let finished = round_trip(&StreamMessage::LoginFinished { login_id: id(), ok: false });
    assert_eq!(finished["type"], "login_finished");

    let terminal = round_trip(&StreamMessage::TerminalOutput { terminal_id: id(), data: "aGk=".into() });
    assert_eq!(terminal["type"], "terminal_output");

    let exited = round_trip(&StreamMessage::TerminalExited { terminal_id: id() });
    assert_eq!(exited["type"], "terminal_exited");

    let resources = round_trip(&StreamMessage::NodeResources { node_id: id(), sample: sample() });
    assert_eq!(resources["type"], "node_resources");

    let gone = round_trip(&StreamMessage::ConversationRemoved { conversation_id: id() });
    assert_eq!(gone["type"], "conversation_removed");

    let removed = round_trip(&StreamMessage::NodeRemoved { node_id: id() });
    assert_eq!(removed["type"], "node_removed");

    let queue = round_trip(&StreamMessage::Queue { conversation_id: id(), queue: vec![] });
    assert_eq!(queue["type"], "queue");
}

#[test]
fn device_flow_bodies_use_the_documented_field_names() {
    let request = round_trip(&DeviceRequest { kind: DeviceKind::Node, label: "l".into(), hostname: "h".into(), os: "linux".into(), arch: "arm64".into() });
    assert_eq!(request["kind"], "node");

    let response = round_trip(&DeviceResponse {
        device_code: "d".into(),
        user_code: "WDJB-MJHT".into(),
        verification_uri: "https://c/approve".into(),
        verification_uri_complete: "https://c/approve?code=WDJB-MJHT".into(),
        interval: 5,
        expires_in: 600,
    });
    for field in ["device_code", "user_code", "verification_uri", "verification_uri_complete", "interval", "expires_in"] {
        assert!(response.get(field).is_some(), "missing {field}");
    }

    let token = round_trip(&DeviceTokenResponse { kind: DeviceKind::Client, id: id(), token: "t".into() });
    assert_eq!(token["kind"], "client");
}

#[test]
fn error_body_has_error_and_message() {
    let json = round_trip(&ErrorBody { error: "authorization_pending".into(), message: "Wait for approval.".into() });

    assert_eq!(json, json!({"error": "authorization_pending", "message": "Wait for approval."}));
}

#[test]
fn request_bodies_treat_optional_fields_as_optional() {
    let create: CreateConversationRequest = serde_json::from_value(json!({"node_id": id(), "agent": "codex", "cwd": "/w"})).unwrap();
    assert_eq!(create.title, None);
    assert_eq!(create.prompt, None);

    let patch: PatchConversationRequest = serde_json::from_value(json!({"archived": true})).unwrap();
    assert_eq!(patch.archived, Some(true));
    assert_eq!(patch.title, None);

    let config: ConfigRequest = serde_json::from_value(json!({"mode": "plan"})).unwrap();
    assert_eq!(config.mode.as_deref(), Some("plan"));

    let query: EventsQuery = serde_json::from_value(json!({})).unwrap();
    assert_eq!((query.after, query.limit), (0, 500));
}

#[test]
fn node_summary_carries_online_draining_agents_and_logins() {
    let json = round_trip(&NodeSummary {
        id: id(),
        name: "n".into(),
        icon: None,
        colour: None,
        hostname: "h".into(),
        os: "linux".into(),
        arch: "x86_64".into(),
        version: "0.1.0".into(),
        address: "100.64.0.2".into(),
        update_available: None,
        online: true,
        draining: false,
        last_seen_at: Some(now()),
        agents: vec![],
        logins: vec![],
        sessions: vec![id()],
        instructions_revision: None,
        instructions_unmanaged: vec![],
        sessions_on_older_instructions: 0,
    });

    assert_eq!(json["online"], true);
    assert_eq!(json["draining"], false);
    assert_eq!(json["sessions"], json!([id()]));
}

#[test]
fn permission_answer_and_login_bodies_round_trip() {
    let answer = round_trip(&PermissionAnswer { request_id: "r".into(), option_id: "allow".into() });
    assert_eq!(answer, json!({"request_id": "r", "option_id": "allow"}));

    let login = round_trip(&StartLoginRequest { agent: AgentId::Claude, method: "token".into(), account: None });
    assert_eq!(login["agent"], "claude");

    round_trip(&StartLoginResponse { login_id: id() });
    round_trip(&LoginInputRequest { text: "x".into() });
    round_trip(&DrainRequest { draining: true });
    round_trip(&PromptRequest { text: "go".into(), attachments: vec![] });
    round_trip(&PatchQueueRequest { text: "edit".into() });
    round_trip(&Me { id: id(), email: "a@b".into(), name: "n".into() });
}

#[test]
fn device_stream_message_carries_state_and_a_null_result_until_consumed() {
    let pending = round_trip(&StreamMessage::Device { user_code: "WDJB-MJHT".into(), kind: DeviceKind::Node, label: "l".into(), hostname: "h".into(), state: DeviceState::Pending, result_id: None });

    assert_eq!(pending["type"], "device");
    assert_eq!(pending["state"], "pending");
    assert_eq!(pending["result_id"], Value::Null);

    let consumed = round_trip(&StreamMessage::Device { user_code: "WDJB-MJHT".into(), kind: DeviceKind::Client, label: "l".into(), hostname: "h".into(), state: DeviceState::Consumed, result_id: Some(id()) });

    assert_eq!(consumed["kind"], "client");
    assert_eq!(consumed["state"], "consumed");
    assert_eq!(consumed["result_id"], json!(id()));
}

#[test]
fn a_fork_start_session_names_the_session_to_fork_and_a_plain_one_omits_the_flag() {
    let forked = round_trip(&ControlToNode::StartSession { conversation_id: id(), agent: AgentId::Claude, cwd: "/w".into(), resume: Some("s".into()), fork: true, account: None, instructions: None });
    let plain = round_trip(&ControlToNode::StartSession { conversation_id: id(), agent: AgentId::Claude, cwd: "/w".into(), resume: None, fork: false, account: None, instructions: None });

    assert_eq!((forked["fork"].clone(), forked["resume"].clone()), (serde_json::json!(true), serde_json::json!("s")));
    assert!(plain.get("fork").is_none());
}
