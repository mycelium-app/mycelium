use crate::common::Server;
use serde_json::Value;
use uuid::Uuid;

struct Seeded {
    owner: Uuid,
    node: Uuid,
}

async fn seeded(server: &Server) -> Seeded {
    let link = server.online_node("atlas").await;
    let owner = sqlx::query_scalar::<_, Uuid>("select id from users where email = 'dev@example.com'").fetch_one(&server.pool).await.unwrap();

    Seeded { owner, node: link.id }
}

async fn insert(server: &Server, seeded: &Seeded, title: &str, minutes_ago: i32, extra: &str) -> Uuid {
    let id = sqlx::query_scalar::<_, Uuid>(
        "insert into conversations (owner, node_id, agent, cwd, title, status, updated_at, created_at) values ($1, $2, 'claude', '/work/app', $3, 'review', now() - make_interval(mins => $4), now() - make_interval(mins => $4)) returning id",
    )
    .bind(seeded.owner)
    .bind(seeded.node)
    .bind(title)
    .bind(minutes_ago)
    .fetch_one(&server.pool)
    .await
    .unwrap();

    if let Some((columns, values)) = extra.split_once(" = ") {
        let assignment = if columns.contains(',') { format!("({columns}) = ({values})") } else { format!("{columns} = {values}") };
        sqlx::query(sqlx::AssertSqlSafe(format!("update conversations set {assignment}, updated_at = now() - make_interval(mins => $2) where id = $1"))).bind(id).bind(minutes_ago).execute(&server.pool).await.unwrap();
    }

    id
}

async fn page(server: &Server, query: &str) -> Value {
    let (status, body) = server.user.json(server.user.get(&format!("/api/v1/conversations?view=sidebar{query}"))).await;

    assert_eq!(status, 200, "{body}");

    body
}

fn titles(body: &Value) -> Vec<String> {
    body["conversations"].as_array().unwrap().iter().map(|conversation| conversation["title"].as_str().unwrap().to_string()).collect()
}

#[tokio::test]
async fn the_first_page_holds_the_fifty_most_recent_sidebar_conversations_and_a_cursor() {
    let server = Server::start().await;
    let seeded = seeded(&server).await;

    for index in 0..60 {
        insert(&server, &seeded, &format!("Chat {index:02}"), index, "").await;
    }

    let parent = insert(&server, &seeded, "Chat 00 parent", 0, "").await;
    insert(&server, &seeded, "A subagent", 1, &format!("parent_id, side_kind = '{parent}', 'subagent'")).await;
    let manager = insert(&server, &seeded, "Manager", 200, "").await;
    insert(&server, &seeded, "Managed elsewhere", 1, &format!("managed_by = '{manager}'")).await;

    let first = page(&server, "&limit=50").await;
    let titles = titles(&first);

    assert_eq!(titles.iter().filter(|title| title.starts_with("Chat ")).count(), 50, "{titles:?}");
    assert!(titles.contains(&"Chat 00".to_string()) && !titles.contains(&"Chat 55".to_string()), "{titles:?}");
    assert!(!titles.contains(&"A subagent".to_string()), "{titles:?}");
    assert!(!titles.contains(&"Managed elsewhere".to_string()), "{titles:?}");
    assert!(first["next"].is_string(), "{first}");
}

#[tokio::test]
async fn following_the_cursor_continues_without_repeats_or_gaps_while_new_conversations_arrive() {
    let server = Server::start().await;
    let seeded = seeded(&server).await;

    for index in 0..30 {
        insert(&server, &seeded, &format!("Chat {index:02}"), index + 1, "").await;
    }

    let first = page(&server, "&limit=10").await;
    insert(&server, &seeded, "Arrived later", 0, "").await;
    let mut seen: Vec<String> = titles(&first);
    let mut next = first["next"].as_str().map(str::to_string);

    while let Some(cursor) = next {
        let following = page(&server, &format!("&limit=10&cursor={}", urlencoding(&cursor))).await;
        seen.extend(titles(&following));
        next = following["next"].as_str().map(str::to_string);
    }

    let mut expected: Vec<String> = (0..30).map(|index| format!("Chat {index:02}")).collect();
    let mut chats: Vec<String> = seen.iter().filter(|title| title.starts_with("Chat ")).cloned().collect();
    expected.sort();
    chats.sort();

    assert_eq!(chats, expected);
    assert!(!seen.contains(&"Arrived later".to_string()), "a newer conversation leaked into a later page: {seen:?}");
}

#[tokio::test]
async fn conversations_that_need_the_person_are_on_the_first_page_however_old() {
    let server = Server::start().await;
    let seeded = seeded(&server).await;

    for index in 0..60 {
        insert(&server, &seeded, &format!("Chat {index:02}"), index, "").await;
    }

    insert(&server, &seeded, "Old and waiting", 5000, "status = 'input'").await;
    insert(&server, &seeded, "Old and flagged", 5000, "flagged_at = now()").await;
    insert(&server, &seeded, "Old and pinned", 5000, "pinned_at = now()").await;
    insert(&server, &seeded, "Old and still working", 5000, "status = 'working'").await;

    let titles = titles(&page(&server, "&limit=10").await);

    for title in ["Old and waiting", "Old and flagged", "Old and pinned", "Old and still working"] {
        assert!(titles.contains(&title.to_string()), "{title} missing from {titles:?}");
    }
}

#[tokio::test]
async fn a_shown_conversation_brings_its_side_chats_and_its_subagents_are_asked_for_by_parent() {
    let server = Server::start().await;
    let seeded = seeded(&server).await;
    let parent = insert(&server, &seeded, "Parent", 0, "").await;
    insert(&server, &seeded, "Side chat", 1, &format!("parent_id = '{parent}'")).await;
    insert(&server, &seeded, "Subagent", 1, &format!("parent_id, side_kind = '{parent}', 'subagent'")).await;

    let first = titles(&page(&server, "").await);
    let (_, children) = server.user.json(server.user.get(&format!("/api/v1/conversations?parent={parent}"))).await;
    let children = titles(&children);

    assert!(first.contains(&"Side chat".to_string()), "{first:?}");
    assert!(!first.contains(&"Subagent".to_string()), "{first:?}");
    assert!(children.contains(&"Subagent".to_string()) && children.contains(&"Side chat".to_string()), "{children:?}");
}

#[tokio::test]
async fn search_finds_an_old_conversation_that_is_not_on_the_first_page() {
    let server = Server::start().await;
    let seeded = seeded(&server).await;

    for index in 0..60 {
        insert(&server, &seeded, &format!("Chat {index:02}"), index, "").await;
    }

    insert(&server, &seeded, "Migrate the billing exports", 9000, "").await;

    let first = titles(&page(&server, "&limit=50").await);
    let (_, found) = server.user.json(server.user.get("/api/v1/conversations?q=billing")).await;

    assert!(!first.contains(&"Migrate the billing exports".to_string()));
    assert_eq!(titles(&found), ["Migrate the billing exports"]);
}

#[tokio::test]
async fn the_settled_count_covers_every_settled_conversation_not_only_the_loaded_ones() {
    let server = Server::start().await;
    let seeded = seeded(&server).await;

    for index in 0..70 {
        insert(&server, &seeded, &format!("Done {index:02}"), index, "status = 'done'").await;
    }

    let first = page(&server, "&limit=20").await;

    assert_eq!(first["counts"]["settled"], 70, "{}", first["counts"]);
}

#[tokio::test]
async fn the_archive_asks_for_done_conversations_only() {
    let server = Server::start().await;
    let seeded = seeded(&server).await;
    insert(&server, &seeded, "Finished", 9000, "status = 'done'").await;
    insert(&server, &seeded, "Still open", 1, "").await;

    let (_, done) = server.user.json(server.user.get("/api/v1/conversations?status=done&limit=200")).await;

    assert_eq!(titles(&done), ["Finished"]);
}

#[tokio::test]
async fn an_older_app_that_asks_without_a_view_still_gets_the_whole_list() {
    let server = Server::start().await;
    let seeded = seeded(&server).await;

    for index in 0..60 {
        insert(&server, &seeded, &format!("Chat {index:02}"), index, "").await;
    }

    let (_, all) = server.user.json(server.user.get("/api/v1/conversations?archived=false")).await;

    assert_eq!(all.as_array().unwrap().len(), 60);
}

fn urlencoding(text: &str) -> String {
    text.bytes().map(|byte| if byte.is_ascii_alphanumeric() || b"-_.~".contains(&byte) { (byte as char).to_string() } else { format!("%{byte:02X}") }).collect()
}
