use axum::Json;
use axum::Router;
use axum::extract::State;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use crate::common::{Server, serve};
use mycelium_proto::stream::StreamMessage;
use serde_json::{Value, json};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use uuid::Uuid;

#[derive(Default)]
struct FakeTimes {
    failing: AtomicBool,
    requests: AtomicUsize,
}

async fn wordle_file(State(times): State<Arc<FakeTimes>>) -> Response {
    times.requests.fetch_add(1, Ordering::SeqCst);

    if times.failing.load(Ordering::SeqCst) {
        return StatusCode::SERVICE_UNAVAILABLE.into_response();
    }

    Json(json!({"id": 1, "solution": "worse", "print_date": "2026-10-27", "days_since_launch": 1226, "editor": "Tracy Bennett"})).into_response()
}

async fn connections_file(State(times): State<Arc<FakeTimes>>) -> Response {
    times.requests.fetch_add(1, Ordering::SeqCst);

    if times.failing.load(Ordering::SeqCst) {
        return StatusCode::SERVICE_UNAVAILABLE.into_response();
    }

    let group = |title: &str, words: [(&str, i64); 4]| json!({"title": title, "cards": words.iter().map(|(content, position)| json!({"content": content, "position": position})).collect::<Vec<_>>()});

    Json(json!({
        "status": "OK",
        "print_date": "2026-10-27",
        "categories": [
            group("THINGS WITH KEYS", [("PIANO", 5), ("LOCK", 9), ("MAP", 14), ("TYPEWRITER", 2)]),
            group("FISH", [("BASS", 0), ("PIKE", 11), ("SOLE", 13), ("PERCH", 7)]),
            group("SEATS", [("BENCH", 1), ("STOOL", 4), ("THRONE", 8), ("SOFA", 10)]),
            group("DOUBLE ___ AGENT", [("MOLE", 3), ("SPY", 6), ("PLANT", 12), ("AGENT", 15)]),
        ],
    }))
    .into_response()
}

async fn nyt() -> (Arc<FakeTimes>, Server) {
    let times = Arc::new(FakeTimes::default());
    let router = Router::new().route("/svc/wordle/v2/{file}", get(wordle_file)).route("/svc/connections/v2/{file}", get(connections_file)).with_state(times.clone());
    let base = serve(router).await;
    let server = Server::start_with(move |config| config.nyt_games_url = base.clone()).await;

    (times, server)
}

async fn guess_wordle(server: &Server, word: &str) -> (u16, Value) {
    server.user.json(server.user.post("/api/v1/wellness/puzzles/wordle/guesses").json(&json!({"word": word}))).await
}

async fn guess_words(server: &Server, words: &[&str]) -> (u16, Value) {
    server.user.json(server.user.post("/api/v1/wellness/puzzles/connections/guesses").json(&json!({"words": words}))).await
}

async fn summary(server: &Server, puzzle: &str) -> Value {
    let (_, all) = server.user.json(server.user.get("/api/v1/wellness/puzzles?days=30")).await;

    all.as_array().unwrap().iter().find(|summary| summary["puzzle"] == puzzle).unwrap().clone()
}

async fn owner(server: &Server) -> Uuid {
    sqlx::query_scalar("select id from users limit 1").fetch_one(&server.pool).await.unwrap()
}

#[tokio::test]
async fn a_wordle_guess_is_marked_without_revealing_the_answer() {
    let (_, server) = nyt().await;

    let (status, game) = guess_wordle(&server, "crane").await;

    assert_eq!(status, 200, "{game}");
    assert_eq!(game["status"], "playing");
    assert_eq!(game["guesses"][0]["marks"], json!(["absent", "present", "absent", "absent", "correct"]));
    assert!(!game.to_string().contains("worse"));
}

#[tokio::test]
async fn a_wordle_repeated_letter_is_marked_present_only_as_often_as_the_answer_holds_it() {
    let (_, server) = nyt().await;

    let (_, game) = guess_wordle(&server, "sheer").await;

    assert_eq!(game["guesses"][0]["marks"], json!(["present", "absent", "present", "absent", "present"]));
}

#[tokio::test]
async fn a_word_that_is_not_in_the_list_or_is_too_short_is_refused_and_costs_no_guess() {
    let (_, server) = nyt().await;

    let (status, body) = guess_wordle(&server, "zzzzz").await;
    assert_eq!((status, body["error"].clone(), body["message"].clone()), (400, json!("not_in_word_list"), json!("Not in the word list")));

    let (status, body) = guess_wordle(&server, "wor").await;
    assert_eq!((status, body["error"].clone()), (400, json!("word_too_short")));

    let (_, game) = server.user.json(server.user.get("/api/v1/wellness/puzzles/wordle/game")).await;
    assert_eq!(game["guesses"], json!([]));
}

#[tokio::test]
async fn winning_wordle_records_the_guesses_and_the_time_as_a_native_result() {
    let (_, server) = nyt().await;

    guess_wordle(&server, "crane").await;
    let (_, game) = guess_wordle(&server, "worse").await;

    assert_eq!((game["status"].clone(), game["answer"].clone()), (json!("won"), json!("worse")));
    assert!(game["seconds"].is_number());

    let wordle = summary(&server, "wordle").await;
    assert_eq!((wordle["today"]["solved"].clone(), wordle["today"]["score"].clone(), wordle["today"]["source"].clone()), (json!(true), json!(2), json!("native")));
    assert_eq!(wordle["today"]["seconds"], game["seconds"]);
    assert_eq!(wordle["stats"]["streak"], json!(1));
}

#[tokio::test]
async fn losing_wordle_reveals_the_answer_and_records_no_score() {
    let (_, server) = nyt().await;

    for word in ["crane", "spore", "horse", "abide", "fjord", "light"] {
        guess_wordle(&server, word).await;
    }

    let (_, game) = server.user.json(server.user.get("/api/v1/wellness/puzzles/wordle/game")).await;
    assert_eq!((game["status"].clone(), game["answer"].clone()), (json!("lost"), json!("worse")));

    let wordle = summary(&server, "wordle").await;
    assert_eq!((wordle["today"]["solved"].clone(), wordle["today"]["score"].clone()), (json!(false), Value::Null));
    assert_eq!(wordle["stats"]["streak"], json!(0));
}

#[tokio::test]
async fn a_finished_wordle_takes_no_more_guesses() {
    let (_, server) = nyt().await;

    guess_wordle(&server, "worse").await;
    let (status, body) = guess_wordle(&server, "crane").await;

    assert_eq!((status, body["error"].clone()), (409, json!("puzzle_finished")));
}

#[tokio::test]
async fn the_days_puzzle_is_fetched_once_and_a_failed_fetch_says_so() {
    let (times, server) = nyt().await;

    server.user.json(server.user.get("/api/v1/wellness/puzzles/wordle/game")).await;
    guess_wordle(&server, "crane").await;
    assert_eq!(times.requests.load(Ordering::SeqCst), 1);

    times.failing.store(true, Ordering::SeqCst);

    let (status, body) = server.user.json(server.user.get("/api/v1/wellness/puzzles/connections/game")).await;
    assert_eq!((status, body["error"].clone()), (502, json!("puzzle_unavailable")));
}

#[tokio::test]
async fn a_guess_is_streamed_so_other_screens_refresh() {
    let (_, server) = nyt().await;
    let mut stream = server.user.stream().await;

    guess_wordle(&server, "crane").await;

    stream.until(|message| matches!(message, StreamMessage::WellnessChanged).then_some(())).await;
}

#[tokio::test]
async fn connections_shows_sixteen_words_and_no_group_titles_until_a_group_is_solved() {
    let (_, server) = nyt().await;

    let (status, game) = server.user.json(server.user.get("/api/v1/wellness/puzzles/connections/game")).await;

    assert_eq!(status, 200, "{game}");
    assert_eq!(game["words"].as_array().unwrap().len(), 16);
    assert_eq!(game["words"][0], "Bass");
    assert_eq!((game["mistakes"].clone(), game["mistakes_allowed"].clone()), (json!(0), json!(4)));
    assert!(!game.to_string().to_lowercase().contains("things with keys"));
}

#[tokio::test]
async fn a_correct_connections_group_joins_the_solved_rows_and_leaves_the_board() {
    let (_, server) = nyt().await;

    let (status, answer) = guess_words(&server, &["Piano", "Lock", "Map", "Typewriter"]).await;

    assert_eq!(status, 200, "{answer}");
    assert_eq!(answer["outcome"], "correct");
    assert_eq!(answer["game"]["solved"][0]["title"], "Things with keys");
    assert_eq!(answer["game"]["words"].as_array().unwrap().len(), 12);
    assert_eq!(answer["game"]["mistakes"], json!(0));
}

#[tokio::test]
async fn three_right_is_one_away_and_costs_a_mistake_and_a_repeat_costs_nothing() {
    let (_, server) = nyt().await;

    let (_, answer) = guess_words(&server, &["Bass", "Pike", "Sole", "Bench"]).await;
    assert_eq!((answer["outcome"].clone(), answer["game"]["mistakes"].clone()), (json!("one_away"), json!(1)));

    let (_, answer) = guess_words(&server, &["Bench", "Sole", "Pike", "Bass"]).await;
    assert_eq!((answer["outcome"].clone(), answer["game"]["mistakes"].clone()), (json!("already_guessed"), json!(1)));

    let (_, answer) = guess_words(&server, &["Bass", "Mole", "Sofa", "Lock"]).await;
    assert_eq!((answer["outcome"].clone(), answer["game"]["mistakes"].clone()), (json!("incorrect"), json!(2)));
}

#[tokio::test]
async fn anything_other_than_four_words_on_the_board_is_refused() {
    let (_, server) = nyt().await;

    let (status, body) = guess_words(&server, &["Bass", "Pike", "Sole"]).await;
    assert_eq!((status, body["error"].clone(), body["message"].clone()), (400, json!("select_four"), json!("Select four words")));

    let (status, body) = guess_words(&server, &["Bass", "Bass", "Pike", "Sole"]).await;
    assert_eq!((status, body["error"].clone()), (400, json!("select_four")));

    let (status, body) = guess_words(&server, &["Bass", "Pike", "Sole", "Sofia"]).await;
    assert_eq!((status, body["error"].clone()), (400, json!("select_four")));
}

#[tokio::test]
async fn a_fourth_connections_mistake_ends_the_game_and_reveals_the_groups() {
    let (_, server) = nyt().await;

    guess_words(&server, &["Bass", "Mole", "Sofa", "Lock"]).await;
    guess_words(&server, &["Bass", "Mole", "Sofa", "Map"]).await;
    guess_words(&server, &["Bass", "Mole", "Sofa", "Piano"]).await;
    let (_, answer) = guess_words(&server, &["Bass", "Mole", "Sofa", "Typewriter"]).await;

    assert_eq!((answer["game"]["status"].clone(), answer["game"]["words"].clone()), (json!("lost"), json!([])));
    assert_eq!(answer["game"]["unsolved"].as_array().unwrap().len(), 4);

    let connections = summary(&server, "connections").await;
    assert_eq!((connections["today"]["solved"].clone(), connections["today"]["score"].clone(), connections["today"]["source"].clone()), (json!(false), json!(4), json!("native")));
}

#[tokio::test]
async fn solving_all_four_connections_groups_records_the_mistakes() {
    let (_, server) = nyt().await;

    guess_words(&server, &["Bass", "Pike", "Sole", "Bench"]).await;
    guess_words(&server, &["Piano", "Lock", "Map", "Typewriter"]).await;
    guess_words(&server, &["Bass", "Pike", "Sole", "Perch"]).await;
    guess_words(&server, &["Bench", "Stool", "Throne", "Sofa"]).await;
    let (_, answer) = guess_words(&server, &["Mole", "Spy", "Plant", "Agent"]).await;

    assert_eq!(answer["game"]["status"], "won");

    let connections = summary(&server, "connections").await;
    assert_eq!((connections["today"]["solved"].clone(), connections["today"]["score"].clone()), (json!(true), json!(1)));
    assert_eq!(connections["stats"]["average"], json!(1.0));
}

#[tokio::test]
async fn a_streak_counts_consecutive_solved_days_and_a_miss_breaks_it() {
    let server = Server::start().await;
    let owner = owner(&server).await;
    let today = chrono::Utc::now().date_naive();

    for (offset, solved) in [(1, true), (2, true), (3, false), (4, true), (5, true), (6, true)] {
        sqlx::query("insert into puzzle_plays (owner, puzzle, day, solved, score, seconds, source) values ($1, 'wordle', $2, $3, 4, 200, 'manual')").bind(owner).bind(today - chrono::Duration::days(offset)).bind(solved).execute(&server.pool).await.unwrap();
    }

    let wordle = summary(&server, "wordle").await;

    assert_eq!((wordle["stats"]["streak"].clone(), wordle["stats"]["best_streak"].clone()), (json!(2), json!(3)));
    assert_eq!((wordle["stats"]["played"].clone(), wordle["stats"]["solved"].clone()), (json!(6), json!(5)));
    assert_eq!((wordle["stats"]["average"].clone(), wordle["stats"]["average_seconds"].clone()), (json!(4.0), json!(200.0)));
}

#[tokio::test]
async fn a_result_entered_by_hand_keeps_its_score_and_time_and_a_negative_one_is_refused() {
    let server = Server::start().await;

    let (status, _) = server.user.json(server.user.put("/api/v1/wellness/puzzles/enclose-horse/today").json(&json!({"solved": true, "score": 87, "seconds": 270}))).await;
    assert_eq!(status, 204);

    let horse = summary(&server, "enclose-horse").await;
    assert_eq!((horse["today"]["score"].clone(), horse["today"]["seconds"].clone(), horse["today"]["source"].clone()), (json!(87), json!(270), json!("manual")));

    let (status, body) = server.user.json(server.user.put("/api/v1/wellness/puzzles/daily-orbs/today").json(&json!({"score": -1}))).await;
    assert_eq!((status, body["error"].clone()), (400, json!("puzzle_result_invalid")));

    let (status, body) = server.user.json(server.user.put("/api/v1/wellness/puzzles/daily-orbs/today").json(&json!({"score": 5, "source": "native"}))).await;
    assert_eq!((status, body["error"].clone()), (400, json!("puzzle_source_invalid")));
}
