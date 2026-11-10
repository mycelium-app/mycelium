use crate::common::Server;
use mycelium_proto::stream::StreamMessage;
use serde_json::{Value, json};
use uuid::Uuid;

async fn post(server: &Server, token: &str, conversation: Uuid, message: &str) -> Value {
    let reply = server.agent_mcp(token, conversation, json!({"jsonrpc": "2.0", "id": 1, "method": "tools/call", "params": {"name": "post_to_feed", "arguments": {"message": message}}})).await;

    reply["result"].clone()
}

async fn posts(server: &Server) -> Vec<Value> {
    let (status, feed) = server.user.json(server.user.get("/api/v1/inbox/feed")).await;
    assert_eq!(status, 200, "{feed}");

    feed["posts"].as_array().unwrap().clone()
}

#[tokio::test]
async fn a_post_appears_in_the_feed_with_the_conversations_author_project_and_link() {
    let server = Server::start().await;
    let link = server.online_node("laptop").await;
    let conversation = server.create_conversation(link.id, None).await;
    let id = conversation.id;
    let project: Uuid = sqlx::query_scalar("insert into projects (owner, node_id, name, path) select owner, node_id, 'harbour', '/work/harbour' from conversations where id = $1 returning id").bind(id).fetch_one(&server.pool).await.unwrap();
    sqlx::query("update conversations set project_id = $1 where id = $2").bind(project).bind(id).execute(&server.pool).await.unwrap();

    let reply = post(&server, &link.token, id, "  Search now finds conversations by branch name.  ").await;

    assert_eq!(reply["isError"], false, "{reply}");
    let listed = posts(&server).await;
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0]["message"], "Search now finds conversations by branch name.");
    assert_eq!(listed[0]["author"], "Claude Code");
    assert_eq!(listed[0]["project"], "harbour");
    assert_eq!(listed[0]["conversation_id"], json!(id));
    assert_eq!(listed[0]["conversation_title"], json!(conversation.title));
    assert!(listed[0]["created_at"].is_string());
}

#[tokio::test]
async fn a_post_from_an_assistants_conversation_is_authored_by_the_assistant() {
    let server = Server::start().await;
    let link = server.online_node("laptop").await;
    let conversation = server.create_conversation(link.id, None).await;
    let assistant: Uuid = sqlx::query_scalar("insert into assistants (owner, name) select owner, 'Archivist' from conversations where id = $1 returning id").bind(conversation.id).fetch_one(&server.pool).await.unwrap();
    sqlx::query("update conversations set assistant_id = $1 where id = $2").bind(assistant).bind(conversation.id).execute(&server.pool).await.unwrap();

    post(&server, &link.token, conversation.id, "The restore flow copy is live.").await;

    assert_eq!(posts(&server).await[0]["author"], "Archivist");
}

#[tokio::test]
async fn the_feed_lists_the_newest_post_first() {
    let server = Server::start().await;
    let link = server.online_node("laptop").await;
    let conversation = server.create_conversation(link.id, None).await;

    post(&server, &link.token, conversation.id, "The first release is out.").await;
    post(&server, &link.token, conversation.id, "The second release is out.").await;

    let listed = posts(&server).await;
    assert_eq!(listed[0]["message"], "The second release is out.");
    assert_eq!(listed[1]["message"], "The first release is out.");
}

#[tokio::test]
async fn a_post_over_the_limit_is_refused_and_stores_nothing() {
    let server = Server::start().await;
    let link = server.online_node("laptop").await;
    let conversation = server.create_conversation(link.id, None).await;

    let reply = post(&server, &link.token, conversation.id, &"x".repeat(281)).await;

    assert_eq!(reply["isError"], true, "{reply}");
    assert_eq!(reply["content"][0]["text"], "Message must be 280 characters or fewer.");
    assert!(posts(&server).await.is_empty());
}

#[tokio::test]
async fn a_post_at_the_limit_is_accepted() {
    let server = Server::start().await;
    let link = server.online_node("laptop").await;
    let conversation = server.create_conversation(link.id, None).await;

    let reply = post(&server, &link.token, conversation.id, &"é".repeat(280)).await;

    assert_eq!(reply["isError"], false, "{reply}");
    assert_eq!(posts(&server).await.len(), 1);
}

#[tokio::test]
async fn badge_markup_is_stored_as_written_and_counts_only_its_label() {
    let server = Server::start().await;
    let link = server.online_node("laptop").await;
    let conversation = server.create_conversation(link.id, None).await;
    let badge = "[[custom:Nightly|moon]]";
    let message = format!("{badge}{}", "x".repeat(280 - "Nightly".len()));

    let reply = post(&server, &link.token, conversation.id, &message).await;

    assert_eq!(reply["isError"], false, "{reply}");
    assert_eq!(posts(&server).await[0]["message"], message);
}

#[tokio::test]
async fn text_that_is_not_badge_markup_counts_every_character() {
    let server = Server::start().await;
    let link = server.online_node("laptop").await;
    let conversation = server.create_conversation(link.id, None).await;
    let message = format!("[[nonsense:Nightly]]{}", "x".repeat(261));

    let reply = post(&server, &link.token, conversation.id, &message).await;

    assert_eq!(reply["isError"], true, "{reply}");
    assert!(posts(&server).await.is_empty());
}

#[tokio::test]
async fn post_to_feed_teaches_the_badge_markup() {
    let server = Server::start().await;
    let link = server.online_node("laptop").await;
    let conversation = server.create_conversation(link.id, None).await;

    let reply = server.agent_mcp(&link.token, conversation.id, json!({"jsonrpc": "2.0", "id": 1, "method": "tools/list"})).await;

    let tools = reply["result"]["tools"].as_array().unwrap();
    let tool = tools.iter().find(|tool| tool["name"] == "post_to_feed").unwrap();
    let description = tool["description"].as_str().unwrap();
    assert!(description.contains("[[project:Mycelium]]"), "{description}");
    assert!(description.contains("[[custom:Nightly|moon]]"), "{description}");
}

#[tokio::test]
async fn an_empty_post_is_refused_and_says_what_to_enter() {
    let server = Server::start().await;
    let link = server.online_node("laptop").await;
    let conversation = server.create_conversation(link.id, None).await;

    let reply = post(&server, &link.token, conversation.id, "   ").await;

    assert_eq!(reply["isError"], true, "{reply}");
    assert_eq!(reply["content"][0]["text"], "Enter a message to post to the feed.");
    assert!(posts(&server).await.is_empty());
}

#[tokio::test]
async fn one_person_never_sees_anothers_posts() {
    let server = Server::start().await;
    let link = server.online_node("laptop").await;
    let conversation = server.create_conversation(link.id, None).await;
    let other = server.second_user("sam@example.com").await;

    post(&server, &link.token, conversation.id, "The billing guide is published.").await;

    let (status, feed) = other.json(other.get("/api/v1/inbox/feed")).await;
    assert_eq!(status, 200, "{feed}");
    assert_eq!(feed["posts"], json!([]));
    assert_eq!(posts(&server).await.len(), 1);
}

#[tokio::test]
async fn a_post_is_streamed_to_its_owner_as_it_is_made() {
    let server = Server::start().await;
    let link = server.online_node("laptop").await;
    let conversation = server.create_conversation(link.id, None).await;
    let mut stream = server.user.stream().await;

    post(&server, &link.token, conversation.id, "The restore flow is live.").await;

    let streamed = stream.until(|message| match message {
        StreamMessage::FeedPost(post) => Some(post),
        _ => None,
    }).await;
    assert_eq!(streamed.message, "The restore flow is live.");
    assert_eq!(streamed.conversation_id, conversation.id);
}

#[tokio::test]
async fn a_post_goes_when_its_conversation_is_deleted() {
    let server = Server::start().await;
    let link = server.online_node("laptop").await;
    let conversation = server.create_conversation(link.id, None).await;
    post(&server, &link.token, conversation.id, "The nightly backup now runs on orion.").await;

    sqlx::query("delete from conversations where id = $1").bind(conversation.id).execute(&server.pool).await.unwrap();

    assert!(posts(&server).await.is_empty());
}
