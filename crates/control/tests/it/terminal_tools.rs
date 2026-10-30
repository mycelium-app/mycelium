use crate::common::{NodeLink, Server, eventually};
use base64::Engine;
use mycelium_proto::link::{ControlToNode, NodeToControl};
use serde_json::{Value, json};
use uuid::Uuid;

struct Fixture {
    server: Server,
    link: NodeLink,
    token: String,
    conversation_id: Uuid,
}

async fn fixture() -> Fixture {
    let server = Server::start().await;
    let (mut link, token) = server.online_node_with_token("laptop", env!("CARGO_PKG_VERSION")).await;
    let conversation = server.create_conversation(link.id, None).await;
    link.expect_start_session().await;

    Fixture { server, link, token, conversation_id: conversation.id }
}

impl Fixture {
    async fn open_terminal(&mut self) -> Uuid {
        let (status, started) = self.server.user.json(self.server.user.post(&format!("/api/v1/conversations/{}/terminal", self.conversation_id)).json(&json!({"cols": 80, "rows": 24}))).await;
        assert_eq!(status, 200, "{started}");

        let ControlToNode::StartTerminal { terminal_id, .. } = self.link.next().await else {
            panic!("expected the node to be told to start a terminal");
        };

        terminal_id
    }

    async fn tool(&self, name: &str, arguments: Value) -> (String, bool) {
        let reply = self.server.agent_mcp(&self.token, self.conversation_id, json!({"jsonrpc": "2.0", "id": 1, "method": "tools/call", "params": {"name": name, "arguments": arguments}})).await;

        (reply["result"]["content"][0]["text"].as_str().unwrap_or_default().to_string(), reply["result"]["isError"].as_bool().unwrap_or(false))
    }

    async fn output(&mut self, terminal_id: Uuid, bytes: &[u8]) {
        self.link.send(NodeToControl::TerminalOutput { terminal_id, data: base64::engine::general_purpose::STANDARD.encode(bytes) }).await;
    }
}

#[tokio::test]
async fn an_agent_lists_the_terminals_the_person_has_open() {
    let mut fixture = fixture().await;

    let (none, is_error) = fixture.tool("terminal_list", json!({})).await;
    assert!(is_error);
    assert!(none.contains("No terminal is open"));

    let terminal_id = fixture.open_terminal().await;
    let (listed, is_error) = fixture.tool("terminal_list", json!({})).await;

    assert!(!is_error);
    assert!(listed.contains(&terminal_id.to_string()));
}

#[tokio::test]
async fn an_agent_reads_what_a_terminal_printed_as_plain_text() {
    let mut fixture = fixture().await;
    let terminal_id = fixture.open_terminal().await;
    fixture.output(terminal_id, b"\x1b[32mbuild ok\x1b[0m\r\n").await;
    fixture.output(terminal_id, b"$ ").await;

    let read = eventually("the output to be readable", || async {
        let (text, _) = fixture.tool("terminal_read", json!({"terminal_id": terminal_id})).await;

        text.contains("$ ").then_some(text)
    })
    .await;

    assert_eq!(read, "build ok\n$ ");
}

#[tokio::test]
async fn an_agent_types_into_a_terminal_and_enter_runs_the_command() {
    let mut fixture = fixture().await;
    let terminal_id = fixture.open_terminal().await;

    let (reply, is_error) = fixture.tool("terminal_type", json!({"terminal_id": terminal_id, "text": "ls", "enter": true})).await;

    assert!(!is_error, "{reply}");
    assert_eq!(fixture.link.next().await, ControlToNode::TerminalInput { terminal_id, data: "bHMN".into() });
}

#[tokio::test]
async fn an_agent_cannot_reach_a_terminal_that_is_not_open_in_its_conversation() {
    let mut fixture = fixture().await;
    fixture.open_terminal().await;

    let (read, read_failed) = fixture.tool("terminal_read", json!({"terminal_id": Uuid::new_v4()})).await;
    let (typed, type_failed) = fixture.tool("terminal_type", json!({"terminal_id": Uuid::new_v4(), "text": "rm -rf /"})).await;

    assert!(read_failed && type_failed);
    assert!(read.contains("not open in this conversation") && typed.contains("not open in this conversation"));
}
