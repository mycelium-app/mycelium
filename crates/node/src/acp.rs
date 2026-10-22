use mycelium_proto::link::NodeToControl;
use serde_json::{Value, json};
use std::collections::{BTreeMap, HashMap, VecDeque};
use uuid::Uuid;

const METHOD_NOT_FOUND: i64 = -32601;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ResumeKind {
    Load,
    Resume,
    Unsupported,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Pending {
    Initialize,
    NewSession,
    ResumeSession,
    Prompt,
    Other,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Direction {
    In,
    Out,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Action {
    Emit(i64, NodeToControl),
    Started(NodeToControl),
    Send(Value),
    CloseKeeper,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Command {
    Prompt { text: String },
    Cancel,
    PermissionResponse { request_id: String, outcome: Value },
    SetConfig { mode: Option<String>, model: Option<String>, config: Option<BTreeMap<String, Value>> },
    Close,
}

#[derive(Debug, Clone)]
pub struct StartPlan {
    pub cwd: String,
    pub resume: Option<String>,
}

pub struct SessionState {
    conversation_id: Uuid,
    plan: Option<StartPlan>,
    acked: i64,
    head: i64,
    live: bool,
    closing: bool,
    next_id: i64,
    pending: HashMap<String, Pending>,
    initialize_sent: bool,
    initialized: bool,
    resume_kind: ResumeKind,
    session_requested: bool,
    session_id: Option<String>,
    modes_advertised: bool,
    models_advertised: bool,
    config_advertised: bool,
    history_loading: bool,
    queued_prompts: VecDeque<String>,
    unanswered_requests: HashMap<String, Value>,
    permissions: HashMap<String, Value>,
}

fn request_frame(id: i64, method: &str, params: Value) -> Value {
    json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params})
}

fn notification_frame(method: &str, params: Value) -> Value {
    json!({"jsonrpc": "2.0", "method": method, "params": params})
}

fn result_frame(id: &Value, result: Value) -> Value {
    json!({"jsonrpc": "2.0", "id": id, "result": result})
}

fn error_frame(id: &Value, code: i64, message: &str) -> Value {
    json!({"jsonrpc": "2.0", "id": id, "error": {"code": code, "message": message}})
}

fn permission_request_id(id: &Value) -> String {
    match id {
        Value::String(text) => text.clone(),
        other => other.to_string(),
    }
}

fn non_empty(value: &Value) -> Option<Value> {
    match value {
        Value::Null => None,
        Value::Array(items) if items.is_empty() => None,
        other => Some(other.clone()),
    }
}

impl SessionState {
    pub fn new(conversation_id: Uuid, plan: Option<StartPlan>, acked: i64) -> SessionState {
        SessionState {
            conversation_id,
            plan,
            acked,
            head: 0,
            live: false,
            closing: false,
            next_id: 1,
            pending: HashMap::new(),
            initialize_sent: false,
            initialized: false,
            resume_kind: ResumeKind::Unsupported,
            session_requested: false,
            session_id: None,
            modes_advertised: false,
            models_advertised: false,
            config_advertised: false,
            history_loading: false,
            queued_prompts: VecDeque::new(),
            unanswered_requests: HashMap::new(),
            permissions: HashMap::new(),
        }
    }

    pub fn acp_session_id(&self) -> Option<&str> {
        self.session_id.as_deref()
    }

    pub fn set_acked(&mut self, acked: i64) {
        self.acked = acked;
    }

    pub fn on_entry(&mut self, seq: i64, direction: Direction, frame: Value) -> Vec<Action> {
        self.head = self.head.max(seq);

        match direction {
            Direction::Out => {
                if !self.live {
                    self.note_outgoing(&frame);
                }

                Vec::new()
            }
            Direction::In => {
                let mut actions = self.handle_incoming(seq, frame);
                actions.extend(self.advance());

                actions
            }
        }
    }

    pub fn on_live(&mut self) -> Vec<Action> {
        self.live = true;
        let mut actions = Vec::new();

        let unknown: Vec<Value> = self
            .unanswered_requests
            .iter()
            .filter(|(key, _)| !self.permissions.contains_key(*key))
            .map(|(_, id)| id.clone())
            .collect();
        for id in unknown {
            actions.push(self.outgoing(error_frame(&id, METHOD_NOT_FOUND, "Method not found")));
        }

        actions.extend(self.advance());

        actions
    }

    pub fn on_exit(&mut self, code: i64) -> Vec<Action> {
        if self.closing {
            return Vec::new();
        }

        let message = format!("The agent exited with code {code}.");

        vec![Action::Emit(self.head + 1, NodeToControl::SessionFailed { conversation_id: self.conversation_id, seq: self.head + 1, message })]
    }

    pub fn on_command(&mut self, command: Command) -> Vec<Action> {
        match command {
            Command::Prompt { text } => {
                self.queued_prompts.push_back(text);

                self.advance()
            }
            Command::Cancel => self.cancel(),
            Command::PermissionResponse { request_id, outcome } => {
                let Some(id) = self.permissions.get(&request_id).cloned() else {
                    return Vec::new();
                };

                vec![self.outgoing(result_frame(&id, json!({"outcome": outcome})))]
            }
            Command::SetConfig { mode, model, config } => self.set_config(mode, model, config),
            Command::Close => {
                self.closing = true;

                vec![Action::CloseKeeper]
            }
        }
    }

    fn cancel(&mut self) -> Vec<Action> {
        let Some(session_id) = self.session_id.clone() else {
            return Vec::new();
        };

        let mut actions = Vec::new();
        let waiting: Vec<Value> = self.permissions.values().cloned().collect();
        for id in waiting {
            actions.push(self.outgoing(result_frame(&id, json!({"outcome": {"outcome": "cancelled"}}))));
        }

        actions.push(self.outgoing(notification_frame("session/cancel", json!({"sessionId": session_id}))));

        actions
    }

    fn set_config(&mut self, mode: Option<String>, model: Option<String>, config: Option<BTreeMap<String, Value>>) -> Vec<Action> {
        let Some(session_id) = self.session_id.clone() else {
            return Vec::new();
        };

        let mut actions = Vec::new();

        if let Some(mode) = mode
            && self.modes_advertised
        {
            actions.push(self.outgoing_request("session/set_mode", json!({"sessionId": session_id, "modeId": mode})));
        }

        if let Some(model) = model
            && self.models_advertised
        {
            actions.push(self.outgoing_request("session/set_model", json!({"sessionId": session_id, "modelId": model})));
        }

        if let Some(options) = config
            && self.config_advertised
        {
            for (config_id, value) in options {
                actions.push(self.outgoing_request("session/set_config_option", json!({"sessionId": session_id, "configId": config_id, "value": value})));
            }
        }

        actions
    }

    fn advance(&mut self) -> Vec<Action> {
        if !self.live {
            return Vec::new();
        }

        let mut actions = Vec::new();

        if !self.initialize_sent {
            let params = json!({
                "protocolVersion": 1,
                "clientCapabilities": {"fs": {"readTextFile": false, "writeTextFile": false}, "terminal": false},
            });
            actions.push(self.outgoing_request("initialize", params));
        } else if self.initialized
            && !self.session_requested
            && let Some(plan) = self.plan.clone()
        {
            actions.push(self.start_session_request(&plan));
        }

        if let Some(session_id) = self.session_id.clone() {
            while let Some(text) = self.queued_prompts.pop_front() {
                let params = json!({"sessionId": session_id, "prompt": [{"type": "text", "text": text}]});
                actions.push(self.outgoing_request("session/prompt", params));
            }
        }

        actions
    }

    fn start_session_request(&mut self, plan: &StartPlan) -> Action {
        let Some(resume) = &plan.resume else {
            return self.outgoing_request("session/new", json!({"cwd": plan.cwd, "mcpServers": []}));
        };

        let params = json!({"sessionId": resume, "cwd": plan.cwd, "mcpServers": []});
        match self.resume_kind {
            ResumeKind::Load => self.outgoing_request("session/load", params),
            ResumeKind::Resume => self.outgoing_request("session/resume", params),
            ResumeKind::Unsupported => self.outgoing_request("session/new", json!({"cwd": plan.cwd, "mcpServers": []})),
        }
    }

    fn outgoing_request(&mut self, method: &str, params: Value) -> Action {
        let id = self.next_id;
        self.next_id += 1;

        self.outgoing(request_frame(id, method, params))
    }

    fn outgoing(&mut self, frame: Value) -> Action {
        self.note_outgoing(&frame);

        Action::Send(frame)
    }

    fn note_outgoing(&mut self, frame: &Value) {
        let id = frame.get("id");
        let method = frame.get("method").and_then(Value::as_str);

        if let (Some(id), Some(method)) = (id, method) {
            if let Some(number) = id.as_i64() {
                self.next_id = self.next_id.max(number + 1);
            }

            let pending = match method {
                "initialize" => {
                    self.initialize_sent = true;
                    Pending::Initialize
                }
                "session/new" => {
                    self.session_requested = true;
                    Pending::NewSession
                }
                "session/load" | "session/resume" => {
                    self.session_requested = true;
                    self.history_loading = method == "session/load";
                    Pending::ResumeSession
                }
                "session/prompt" => Pending::Prompt,
                _ => Pending::Other,
            };
            self.pending.insert(id.to_string(), pending);
        }

        if let (Some(id), None) = (id, method) {
            let key = id.to_string();
            self.unanswered_requests.remove(&key);
            self.permissions.retain(|_, permission_id| permission_id != id);
        }
    }

    fn emit(&self, seq: i64, message: NodeToControl) -> Vec<Action> {
        if seq <= self.acked {
            return Vec::new();
        }

        vec![Action::Emit(seq, message)]
    }

    fn fail(&self, seq: i64, message: String) -> Vec<Action> {
        self.emit(seq, NodeToControl::SessionFailed { conversation_id: self.conversation_id, seq, message })
    }

    fn handle_incoming(&mut self, seq: i64, frame: Value) -> Vec<Action> {
        let method = frame.get("method").and_then(Value::as_str).map(str::to_string);
        let id = frame.get("id").cloned();

        match (method, id) {
            (Some(method), Some(id)) => self.handle_agent_request(seq, &method, id, frame.get("params").cloned().unwrap_or(Value::Null)),
            (Some(method), None) => {
                if method == "session/update" && !self.history_loading {
                    let update = frame.get("params").cloned().unwrap_or(Value::Null);

                    return self.emit(seq, NodeToControl::AcpUpdate { conversation_id: self.conversation_id, seq, update });
                }

                Vec::new()
            }
            (None, Some(id)) => self.handle_response(seq, id, &frame),
            (None, None) => Vec::new(),
        }
    }

    fn handle_agent_request(&mut self, seq: i64, method: &str, id: Value, params: Value) -> Vec<Action> {
        let key = id.to_string();
        self.unanswered_requests.insert(key, id.clone());

        if method == "session/request_permission" {
            let request_id = permission_request_id(&id);
            self.permissions.insert(request_id.clone(), id);

            return self.emit(seq, NodeToControl::PermissionRequest { conversation_id: self.conversation_id, seq, request_id, request: params });
        }

        if self.live {
            return vec![self.outgoing(error_frame(&id, METHOD_NOT_FOUND, "Method not found"))];
        }

        Vec::new()
    }

    fn handle_response(&mut self, seq: i64, id: Value, frame: &Value) -> Vec<Action> {
        let Some(pending) = self.pending.remove(&id.to_string()) else {
            return Vec::new();
        };
        let result = frame.get("result").cloned().unwrap_or(Value::Null);
        let error_message = frame.get("error").map(|error| error.get("message").and_then(Value::as_str).unwrap_or("The agent returned an error.").to_string());

        match pending {
            Pending::Initialize => {
                if let Some(message) = error_message {
                    return self.fail(seq, format!("The agent could not start: {message}"));
                }

                self.initialized = true;
                self.resume_kind = resume_kind_of(&result);

                Vec::new()
            }
            Pending::NewSession => {
                if let Some(message) = error_message {
                    return self.fail(seq, format!("The agent could not start a session: {message}"));
                }

                let session_id = result.get("sessionId").and_then(Value::as_str).unwrap_or_default().to_string();
                self.session_ready(session_id, &result)
            }
            Pending::ResumeSession => {
                self.history_loading = false;
                if let Some(message) = error_message {
                    return self.fail(seq, format!("The agent could not resume the session: {message}"));
                }

                let session_id = self.plan.as_ref().and_then(|plan| plan.resume.clone()).unwrap_or_default();
                self.session_ready(session_id, &result)
            }
            Pending::Prompt => {
                if let Some(message) = error_message {
                    return self.fail(seq, message);
                }

                let stop_reason = result.get("stopReason").and_then(Value::as_str).unwrap_or("end_turn").to_string();

                self.emit(seq, NodeToControl::TurnEnd { conversation_id: self.conversation_id, seq, stop_reason })
            }
            Pending::Other => Vec::new(),
        }
    }

    fn session_ready(&mut self, session_id: String, result: &Value) -> Vec<Action> {
        let modes = result.get("modes").and_then(non_empty);
        let models = result.get("models").and_then(non_empty);
        let config_options = result.get("configOptions").and_then(non_empty);

        self.modes_advertised = modes.is_some();
        self.models_advertised = models.is_some();
        self.config_advertised = config_options.is_some();
        self.session_id = Some(session_id.clone());

        vec![Action::Started(NodeToControl::SessionStarted { conversation_id: self.conversation_id, acp_session_id: session_id, modes, models, config_options })]
    }
}

fn resume_kind_of(initialize_result: &Value) -> ResumeKind {
    let capabilities = &initialize_result["agentCapabilities"];

    if capabilities["loadSession"].as_bool() == Some(true) {
        return ResumeKind::Load;
    }

    if capabilities["sessionCapabilities"]["resume"].is_object() {
        return ResumeKind::Resume;
    }

    ResumeKind::Unsupported
}

#[cfg(test)]
mod tests {
    use super::*;

    fn conversation() -> Uuid {
        Uuid::from_u128(7)
    }

    fn plan(resume: Option<&str>) -> Option<StartPlan> {
        Some(StartPlan { cwd: "/work".into(), resume: resume.map(str::to_string) })
    }

    fn sent_frames(actions: &[Action]) -> Vec<Value> {
        actions.iter().filter_map(|action| if let Action::Send(frame) = action { Some(frame.clone()) } else { None }).collect()
    }

    fn started(state: &mut SessionState, result: Value) {
        state.on_live();
        state.on_entry(1, Direction::Out, json!({}));
        state.on_entry(2, Direction::In, json!({"jsonrpc": "2.0", "id": 1, "result": {"agentCapabilities": {"loadSession": true}}}));
        state.on_entry(3, Direction::In, json!({"jsonrpc": "2.0", "id": 2, "result": result}));
    }

    #[test]
    fn going_live_initializes_with_the_stage_one_client_capabilities() {
        let mut state = SessionState::new(conversation(), plan(None), 0);

        let frames = sent_frames(&state.on_live());

        assert_eq!(frames[0]["method"], "initialize");
        assert_eq!(frames[0]["params"]["protocolVersion"], 1);
        assert_eq!(frames[0]["params"]["clientCapabilities"], json!({"fs": {"readTextFile": false, "writeTextFile": false}, "terminal": false}));
    }

    #[test]
    fn a_fresh_session_follows_initialize_with_session_new() {
        let mut state = SessionState::new(conversation(), plan(None), 0);
        state.on_live();

        let actions = state.on_entry(1, Direction::In, json!({"jsonrpc": "2.0", "id": 1, "result": {}}));

        let frames = sent_frames(&actions);
        assert_eq!(frames[0]["method"], "session/new");
        assert_eq!(frames[0]["params"], json!({"cwd": "/work", "mcpServers": []}));
    }

    #[test]
    fn resuming_uses_session_load_when_the_agent_advertises_it() {
        let mut state = SessionState::new(conversation(), plan(Some("abc")), 0);
        state.on_live();

        let actions = state.on_entry(1, Direction::In, json!({"jsonrpc": "2.0", "id": 1, "result": {"agentCapabilities": {"loadSession": true}}}));

        let frames = sent_frames(&actions);
        assert_eq!(frames[0]["method"], "session/load");
        assert_eq!(frames[0]["params"]["sessionId"], "abc");
    }

    #[test]
    fn resuming_uses_session_resume_when_only_that_is_advertised() {
        let mut state = SessionState::new(conversation(), plan(Some("abc")), 0);
        state.on_live();

        let result = json!({"agentCapabilities": {"sessionCapabilities": {"resume": {}}}});
        let actions = state.on_entry(1, Direction::In, json!({"jsonrpc": "2.0", "id": 1, "result": result}));

        assert_eq!(sent_frames(&actions)[0]["method"], "session/resume");
    }

    #[test]
    fn history_replayed_by_session_load_is_not_forwarded() {
        let mut state = SessionState::new(conversation(), plan(Some("abc")), 0);
        state.on_live();
        state.on_entry(1, Direction::In, json!({"jsonrpc": "2.0", "id": 1, "result": {"agentCapabilities": {"loadSession": true}}}));

        let history = state.on_entry(2, Direction::In, json!({"jsonrpc": "2.0", "method": "session/update", "params": {"n": 1}}));
        state.on_entry(3, Direction::In, json!({"jsonrpc": "2.0", "id": 2, "result": {}}));
        let fresh = state.on_entry(4, Direction::In, json!({"jsonrpc": "2.0", "method": "session/update", "params": {"n": 2}}));

        assert!(history.is_empty());
        assert_eq!(fresh.len(), 1);
    }

    #[test]
    fn prompts_wait_for_the_session_then_go_out_in_order() {
        let mut state = SessionState::new(conversation(), plan(None), 0);
        state.on_live();
        assert!(sent_frames(&state.on_command(Command::Prompt { text: "one".into() })).is_empty());
        state.on_command(Command::Prompt { text: "two".into() });

        state.on_entry(1, Direction::In, json!({"jsonrpc": "2.0", "id": 1, "result": {}}));
        let actions = state.on_entry(2, Direction::In, json!({"jsonrpc": "2.0", "id": 2, "result": {"sessionId": "s1"}}));

        let prompts: Vec<String> = sent_frames(&actions).iter().map(|frame| frame["params"]["prompt"][0]["text"].as_str().unwrap().to_string()).collect();
        assert_eq!(prompts, ["one", "two"]);
    }

    #[test]
    fn unknown_agent_requests_get_method_not_found_once_live() {
        let mut state = SessionState::new(conversation(), plan(None), 0);
        state.on_live();

        let actions = state.on_entry(5, Direction::In, json!({"jsonrpc": "2.0", "id": "fs-1", "method": "fs/read_text_file", "params": {}}));

        let frames = sent_frames(&actions);
        assert_eq!(frames[0]["id"], "fs-1");
        assert_eq!(frames[0]["error"]["code"], -32601);
    }

    #[test]
    fn replayed_requests_the_journal_already_answered_are_not_answered_again() {
        let mut state = SessionState::new(conversation(), plan(None), 0);
        state.on_entry(1, Direction::In, json!({"jsonrpc": "2.0", "id": 4, "method": "fs/read_text_file"}));
        state.on_entry(2, Direction::Out, json!({"jsonrpc": "2.0", "id": 4, "error": {"code": -32601, "message": "Method not found"}}));
        state.on_entry(3, Direction::In, json!({"jsonrpc": "2.0", "id": 5, "method": "terminal/create"}));

        let frames = sent_frames(&state.on_live());

        let answered: Vec<&Value> = frames.iter().filter(|frame| frame.get("error").is_some()).map(|frame| &frame["id"]).collect();
        assert_eq!(answered, [&json!(5)]);
    }

    #[test]
    fn a_permission_request_is_forwarded_and_answered_with_the_outcome() {
        let mut state = SessionState::new(conversation(), plan(None), 0);
        started(&mut state, json!({"sessionId": "s1"}));

        let actions = state.on_entry(4, Direction::In, json!({"jsonrpc": "2.0", "id": 9001, "method": "session/request_permission", "params": {"options": []}}));
        assert_eq!(actions.len(), 1);
        let Action::Emit(4, NodeToControl::PermissionRequest { request_id, .. }) = &actions[0] else {
            panic!("expected a permission request, got {actions:?}");
        };
        assert_eq!(request_id, "9001");

        let outcome = json!({"outcome": "selected", "optionId": "allow"});
        let answer = state.on_command(Command::PermissionResponse { request_id: "9001".into(), outcome: outcome.clone() });

        assert_eq!(sent_frames(&answer), [json!({"jsonrpc": "2.0", "id": 9001, "result": {"outcome": outcome}})]);
    }

    #[test]
    fn cancel_declines_waiting_permissions_then_sends_session_cancel() {
        let mut state = SessionState::new(conversation(), plan(None), 0);
        started(&mut state, json!({"sessionId": "s1"}));
        state.on_entry(4, Direction::In, json!({"jsonrpc": "2.0", "id": 9001, "method": "session/request_permission", "params": {}}));

        let frames = sent_frames(&state.on_command(Command::Cancel));

        assert_eq!(frames[0]["result"]["outcome"], json!({"outcome": "cancelled"}));
        assert_eq!(frames[1], json!({"jsonrpc": "2.0", "method": "session/cancel", "params": {"sessionId": "s1"}}));
    }

    #[test]
    fn set_config_only_sends_what_the_agent_advertised() {
        let mut state = SessionState::new(conversation(), plan(None), 0);
        started(&mut state, json!({"sessionId": "s1", "modes": {"currentModeId": "default", "availableModes": []}}));

        let config = Some(BTreeMap::from([("effort".to_string(), json!("high"))]));
        let actions = state.on_command(Command::SetConfig { mode: Some("plan".into()), model: Some("opus".into()), config });

        let frames = sent_frames(&actions);
        assert_eq!(frames.len(), 1);
        assert_eq!(frames[0]["method"], "session/set_mode");
        assert_eq!(frames[0]["params"], json!({"sessionId": "s1", "modeId": "plan"}));
    }

    #[test]
    fn set_config_sends_model_and_options_when_advertised() {
        let mut state = SessionState::new(conversation(), plan(None), 0);
        started(&mut state, json!({"sessionId": "s1", "models": {"availableModels": []}, "configOptions": [{"id": "effort"}]}));

        let config = Some(BTreeMap::from([("effort".to_string(), json!("high"))]));
        let frames = sent_frames(&state.on_command(Command::SetConfig { mode: None, model: Some("opus".into()), config }));

        assert_eq!(frames[0]["method"], "session/set_model");
        assert_eq!(frames[0]["params"]["modelId"], "opus");
        assert_eq!(frames[1]["method"], "session/set_config_option");
        assert_eq!(frames[1]["params"], json!({"sessionId": "s1", "configId": "effort", "value": "high"}));
    }

    #[test]
    fn entries_at_or_below_the_acked_seq_update_state_but_are_not_emitted() {
        let mut state = SessionState::new(conversation(), plan(None), 3);

        let old = state.on_entry(3, Direction::In, json!({"jsonrpc": "2.0", "method": "session/update", "params": {}}));
        let new = state.on_entry(4, Direction::In, json!({"jsonrpc": "2.0", "method": "session/update", "params": {}}));

        assert!(old.is_empty());
        assert_eq!(new.len(), 1);
    }

    #[test]
    fn a_prompt_response_becomes_a_turn_end_with_its_stop_reason() {
        let mut state = SessionState::new(conversation(), plan(None), 0);
        started(&mut state, json!({"sessionId": "s1"}));
        let prompt = state.on_command(Command::Prompt { text: "hi".into() });
        let id = sent_frames(&prompt)[0]["id"].clone();

        let actions = state.on_entry(6, Direction::In, json!({"jsonrpc": "2.0", "id": id, "result": {"stopReason": "cancelled"}}));

        assert_eq!(actions, [Action::Emit(6, NodeToControl::TurnEnd { conversation_id: conversation(), seq: 6, stop_reason: "cancelled".into() })]);
    }

    #[test]
    fn an_unexpected_exit_fails_the_session_after_the_last_journal_entry() {
        let mut state = SessionState::new(conversation(), plan(None), 0);
        state.on_entry(8, Direction::In, json!({"jsonrpc": "2.0", "method": "session/update", "params": {}}));

        let actions = state.on_exit(2);

        assert_eq!(actions.len(), 1);
        let Action::Emit(9, NodeToControl::SessionFailed { message, .. }) = &actions[0] else {
            panic!("expected a failure, got {actions:?}");
        };
        assert!(message.contains("code 2"));
    }

    #[test]
    fn an_exit_after_close_is_not_a_failure() {
        let mut state = SessionState::new(conversation(), plan(None), 0);
        state.on_command(Command::Close);

        assert!(state.on_exit(-1).is_empty());
    }
}
