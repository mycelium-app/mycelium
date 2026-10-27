use crate::auth::AuthUser;
use crate::error::{ApiError, ApiResult};
use crate::state::AppState;
use crate::wellness::{save_puzzle_result, today_of};
use axum::Json;
use axum::extract::State;
use axum::http::StatusCode;
use chrono::{DateTime, NaiveDate, Utc};
use mycelium_proto::api::{ConnectionsGame, ConnectionsGroup, ConnectionsGuessAnswer, ConnectionsGuessRequest, PuzzleResult, WordleGame, WordleGuess, WordleGuessRequest};
use mycelium_proto::stream::StreamMessage;
use serde::{Deserialize, Serialize};
use sqlx::types::Json as Jsonb;
use std::collections::HashSet;
use std::sync::OnceLock;
use uuid::Uuid;

const WORDLE: &str = "wordle";
const CONNECTIONS: &str = "connections";
const WORD_LIST: &str = include_str!("wordle_words.txt");
const WORD_LENGTH: usize = 5;
const WORDLE_GUESSES: usize = 6;
const GROUP_SIZE: usize = 4;
const GROUPS: usize = 4;
const MISTAKES_ALLOWED: i32 = 4;

#[derive(Debug, Clone, Serialize, Deserialize)]
struct WordleDay {
    solution: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ConnectionsDay {
    groups: Vec<DayGroup>,
    board: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct DayGroup {
    title: String,
    words: Vec<String>,
}

struct Played {
    guesses: Vec<serde_json::Value>,
    started_at: DateTime<Utc>,
    finished_at: Option<DateTime<Utc>>,
}

fn unavailable() -> ApiError {
    ApiError::new(StatusCode::BAD_GATEWAY, "puzzle_unavailable", "The New York Times did not send today's puzzle. Try again in a moment.")
}

fn finished() -> ApiError {
    ApiError::new(StatusCode::CONFLICT, "puzzle_finished", "Today's puzzle is finished. Come back tomorrow for the next one.")
}

fn valid_words() -> &'static HashSet<&'static str> {
    static WORDS: OnceLock<HashSet<&'static str>> = OnceLock::new();

    WORDS.get_or_init(|| WORD_LIST.lines().map(str::trim).filter(|word| word.len() == WORD_LENGTH).collect())
}

fn display_word(content: &str) -> String {
    let trimmed = content.trim();
    let shouting = trimmed.chars().any(char::is_alphabetic) && trimmed == trimmed.to_uppercase();

    if !shouting {
        return trimmed.to_string();
    }

    let mut letters = trimmed.chars();

    match letters.next() {
        Some(first) => first.to_uppercase().chain(letters.flat_map(char::to_lowercase)).collect(),
        None => String::new(),
    }
}

fn same_word(left: &str, right: &str) -> bool {
    left.trim().to_lowercase() == right.trim().to_lowercase()
}

fn normalise_wordle(document: &serde_json::Value) -> Option<WordleDay> {
    let solution = document["solution"].as_str()?.trim().to_lowercase();

    (solution.len() == WORD_LENGTH && solution.chars().all(|letter| letter.is_ascii_lowercase())).then_some(WordleDay { solution })
}

fn normalise_connections(document: &serde_json::Value) -> Option<ConnectionsDay> {
    let categories = document["categories"].as_array()?;
    let mut groups = Vec::new();
    let mut positioned: Vec<(i64, String)> = Vec::new();

    for category in categories {
        let cards = category["cards"].as_array()?;
        let words: Vec<String> = cards.iter().filter_map(|card| card["content"].as_str()).map(display_word).collect();

        if cards.len() != GROUP_SIZE || words.len() != GROUP_SIZE {
            return None;
        }

        for (index, card) in cards.iter().enumerate() {
            let position = card["position"].as_i64().unwrap_or((groups.len() * GROUP_SIZE + index) as i64);

            positioned.push((position, words[index].clone()));
        }

        groups.push(DayGroup { title: display_word(category["title"].as_str()?), words });
    }

    if groups.len() != GROUPS {
        return None;
    }

    positioned.sort_by_key(|(position, _)| *position);

    Some(ConnectionsDay { groups, board: positioned.into_iter().map(|(_, word)| word).collect() })
}

async fn day_payload(state: &AppState, puzzle: &str, day: NaiveDate) -> ApiResult<serde_json::Value> {
    let cached = sqlx::query_scalar::<_, Jsonb<serde_json::Value>>("select payload from puzzle_days where puzzle = $1 and day = $2").bind(puzzle).bind(day).fetch_optional(&state.pool).await?;

    if let Some(Jsonb(payload)) = cached {
        return Ok(payload);
    }

    let address = format!("{}/svc/{puzzle}/v2/{day}.json", state.config.nyt_games_url.trim_end_matches('/'));
    let response = state.http.get(&address).send().await.map_err(|cause| {
        tracing::warn!("could not fetch the {puzzle} puzzle for {day}: {cause}");

        unavailable()
    })?;

    if !response.status().is_success() {
        tracing::warn!("the {puzzle} puzzle for {day} answered {}", response.status());

        return Err(unavailable());
    }

    let document = response.json::<serde_json::Value>().await.map_err(|_| unavailable())?;
    let normalised = if puzzle == WORDLE {
        serde_json::to_value(normalise_wordle(&document).ok_or_else(unavailable)?)
    } else {
        serde_json::to_value(normalise_connections(&document).ok_or_else(unavailable)?)
    };
    let payload = normalised.map_err(ApiError::internal)?;

    sqlx::query("insert into puzzle_days (puzzle, day, payload) values ($1, $2, $3) on conflict do nothing").bind(puzzle).bind(day).bind(Jsonb(&payload)).execute(&state.pool).await?;

    Ok(payload)
}

async fn wordle_day(state: &AppState, day: NaiveDate) -> ApiResult<WordleDay> {
    serde_json::from_value(day_payload(state, WORDLE, day).await?).map_err(ApiError::internal)
}

async fn connections_day(state: &AppState, day: NaiveDate) -> ApiResult<ConnectionsDay> {
    serde_json::from_value(day_payload(state, CONNECTIONS, day).await?).map_err(ApiError::internal)
}

async fn load_played(state: &AppState, owner: Uuid, puzzle: &str, day: NaiveDate) -> ApiResult<Option<Played>> {
    let row = sqlx::query_as::<_, (Jsonb<Vec<serde_json::Value>>, DateTime<Utc>, Option<DateTime<Utc>>)>("select guesses, started_at, finished_at from puzzle_games where owner = $1 and puzzle = $2 and day = $3").bind(owner).bind(puzzle).bind(day).fetch_optional(&state.pool).await?;

    Ok(row.map(|(Jsonb(guesses), started_at, finished_at)| Played { guesses, started_at, finished_at }))
}

async fn store_guesses(state: &AppState, owner: Uuid, puzzle: &str, day: NaiveDate, guesses: &[serde_json::Value], done: bool) -> ApiResult<Played> {
    let (started_at, finished_at) = sqlx::query_as::<_, (DateTime<Utc>, Option<DateTime<Utc>>)>(
        "insert into puzzle_games (owner, puzzle, day, guesses, finished_at) values ($1, $2, $3, $4, case when $5 then now() end)
         on conflict (owner, puzzle, day) do update set guesses = excluded.guesses, finished_at = excluded.finished_at
         returning started_at, finished_at",
    )
    .bind(owner)
    .bind(puzzle)
    .bind(day)
    .bind(Jsonb(guesses))
    .bind(done)
    .fetch_one(&state.pool)
    .await?;

    Ok(Played { guesses: guesses.to_vec(), started_at, finished_at })
}

fn seconds_of(played: &Played) -> Option<i32> {
    played.finished_at.map(|finished| (finished - played.started_at).num_seconds().clamp(0, i64::from(i32::MAX)) as i32)
}

fn marks_of(guess: &str, solution: &str) -> Vec<String> {
    let guess: Vec<char> = guess.chars().collect();
    let solution: Vec<char> = solution.chars().collect();
    let mut marks = vec!["absent"; guess.len()];
    let mut unmatched: Vec<char> = Vec::new();

    for (index, letter) in guess.iter().enumerate() {
        if solution.get(index) == Some(letter) {
            marks[index] = "correct";
        } else if let Some(left) = solution.get(index) {
            unmatched.push(*left);
        }
    }

    for (index, letter) in guess.iter().enumerate() {
        if marks[index] == "correct" {
            continue;
        }

        if let Some(found) = unmatched.iter().position(|left| left == letter) {
            unmatched.remove(found);
            marks[index] = "present";
        }
    }

    marks.into_iter().map(str::to_string).collect()
}

fn wordle_game_of(day: NaiveDate, solution: &str, played: Option<&Played>) -> WordleGame {
    let words: Vec<String> = played.map(|played| played.guesses.iter().filter_map(|guess| guess.as_str().map(str::to_string)).collect()).unwrap_or_default();
    let won = words.last().is_some_and(|word| word == solution);
    let lost = !won && words.len() >= WORDLE_GUESSES;
    let status = if won { "won" } else if lost { "lost" } else { "playing" };

    WordleGame {
        day: day.to_string(),
        status: status.to_string(),
        guesses: words.iter().map(|word| WordleGuess { word: word.clone(), marks: marks_of(word, solution) }).collect(),
        answer: (won || lost).then(|| solution.to_string()),
        seconds: played.filter(|_| won || lost).and_then(seconds_of),
    }
}

pub async fn wordle_game(State(state): State<AppState>, user: AuthUser) -> ApiResult<Json<WordleGame>> {
    let day = today_of(&state, user.id).await?;
    let today = wordle_day(&state, day).await?;
    let played = load_played(&state, user.id, WORDLE, day).await?;

    Ok(Json(wordle_game_of(day, &today.solution, played.as_ref())))
}

pub async fn wordle_guess(State(state): State<AppState>, user: AuthUser, Json(request): Json<WordleGuessRequest>) -> ApiResult<Json<WordleGame>> {
    user.require_full_access()?;

    let word = request.word.trim().to_lowercase();

    if word.chars().count() != WORD_LENGTH || !word.chars().all(|letter| letter.is_ascii_lowercase()) {
        return Err(ApiError::bad_request("word_too_short", "Enter five letters"));
    }

    let day = today_of(&state, user.id).await?;
    let today = wordle_day(&state, day).await?;

    if word != today.solution && !valid_words().contains(word.as_str()) {
        return Err(ApiError::bad_request("not_in_word_list", "Not in the word list"));
    }

    let played = load_played(&state, user.id, WORDLE, day).await?;

    if wordle_game_of(day, &today.solution, played.as_ref()).answer.is_some() {
        return Err(finished());
    }

    let mut guesses = played.map(|played| played.guesses).unwrap_or_default();
    guesses.push(serde_json::Value::String(word));

    let after = Played { guesses: guesses.clone(), started_at: Utc::now(), finished_at: None };
    let done = wordle_game_of(day, &today.solution, Some(&after)).answer.is_some();
    let stored = store_guesses(&state, user.id, WORDLE, day, &guesses, done).await?;
    let game = wordle_game_of(day, &today.solution, Some(&stored));

    if done {
        let won = game.status == "won";
        let result = PuzzleResult { solved: Some(won), score: won.then_some(game.guesses.len() as i32), seconds: game.seconds, source: None };

        save_puzzle_result(&state, user.id, WORDLE, day, &result, "native").await?;
    }

    state.publish(Some(user.id), StreamMessage::WellnessChanged);

    Ok(Json(game))
}

fn words_of(guess: &serde_json::Value) -> Vec<String> {
    guess.as_array().map(|words| words.iter().filter_map(|word| word.as_str().map(str::to_string)).collect()).unwrap_or_default()
}

fn group_matching<'a>(day: &'a ConnectionsDay, guess: &[String]) -> Option<(usize, &'a DayGroup)> {
    day.groups.iter().enumerate().find(|(_, group)| group.words.iter().all(|word| guess.iter().any(|picked| same_word(picked, word))))
}

fn closest_overlap(day: &ConnectionsDay, guess: &[String]) -> usize {
    day.groups.iter().map(|group| group.words.iter().filter(|word| guess.iter().any(|picked| same_word(picked, word))).count()).max().unwrap_or(0)
}

fn connections_game_of(day_date: NaiveDate, day: &ConnectionsDay, played: Option<&Played>) -> ConnectionsGame {
    let guesses: Vec<Vec<String>> = played.map(|played| played.guesses.iter().map(words_of).collect()).unwrap_or_default();
    let mut solved_levels: Vec<usize> = Vec::new();
    let mut mistakes = 0;

    for guess in &guesses {
        match group_matching(day, guess) {
            Some((level, _)) if !solved_levels.contains(&level) => solved_levels.push(level),
            Some(_) => {}
            None => mistakes += 1,
        }
    }

    let won = solved_levels.len() == GROUPS;
    let lost = !won && mistakes >= MISTAKES_ALLOWED;
    let status = if won { "won" } else if lost { "lost" } else { "playing" };
    let group_of = |level: usize| ConnectionsGroup { title: day.groups[level].title.clone(), words: day.groups[level].words.clone(), level: level as i32 };
    let solved: Vec<ConnectionsGroup> = solved_levels.iter().map(|level| group_of(*level)).collect();
    let unsolved: Vec<ConnectionsGroup> = if lost { (0..GROUPS).filter(|level| !solved_levels.contains(level)).map(group_of).collect() } else { Vec::new() };
    let words: Vec<String> = if lost { Vec::new() } else { day.board.iter().filter(|word| !solved.iter().any(|group| group.words.iter().any(|solved_word| same_word(solved_word, word)))).cloned().collect() };

    ConnectionsGame {
        day: day_date.to_string(),
        status: status.to_string(),
        words,
        solved,
        unsolved,
        mistakes,
        mistakes_allowed: MISTAKES_ALLOWED,
        seconds: played.filter(|_| won || lost).and_then(seconds_of),
    }
}

pub async fn connections_game(State(state): State<AppState>, user: AuthUser) -> ApiResult<Json<ConnectionsGame>> {
    let date = today_of(&state, user.id).await?;
    let day = connections_day(&state, date).await?;
    let played = load_played(&state, user.id, CONNECTIONS, date).await?;

    Ok(Json(connections_game_of(date, &day, played.as_ref())))
}

pub async fn connections_guess(State(state): State<AppState>, user: AuthUser, Json(request): Json<ConnectionsGuessRequest>) -> ApiResult<Json<ConnectionsGuessAnswer>> {
    user.require_full_access()?;

    let date = today_of(&state, user.id).await?;
    let day = connections_day(&state, date).await?;
    let played = load_played(&state, user.id, CONNECTIONS, date).await?;
    let before = connections_game_of(date, &day, played.as_ref());

    if before.status != "playing" {
        return Err(finished());
    }

    let mut picked: Vec<String> = Vec::new();

    for word in &request.words {
        let on_board = before.words.iter().find(|candidate| same_word(candidate, word));

        match on_board {
            Some(found) if !picked.contains(found) => picked.push(found.clone()),
            _ => return Err(ApiError::bad_request("select_four", "Select four words")),
        }
    }

    if picked.len() != GROUP_SIZE {
        return Err(ApiError::bad_request("select_four", "Select four words"));
    }

    let mut sorted = picked.clone();
    sorted.sort();

    let previous: Vec<serde_json::Value> = played.map(|played| played.guesses).unwrap_or_default();
    let repeated = previous.iter().any(|guess| {
        let mut earlier = words_of(guess);
        earlier.sort();

        earlier == sorted
    });

    if repeated {
        return Ok(Json(ConnectionsGuessAnswer { outcome: "already_guessed".to_string(), game: before }));
    }

    let outcome = match group_matching(&day, &picked) {
        Some(_) => "correct",
        None if closest_overlap(&day, &picked) == GROUP_SIZE - 1 => "one_away",
        None => "incorrect",
    };
    let mut guesses = previous;
    guesses.push(serde_json::json!(sorted));

    let preview = Played { guesses: guesses.clone(), started_at: Utc::now(), finished_at: None };
    let done = connections_game_of(date, &day, Some(&preview)).status != "playing";
    let stored = store_guesses(&state, user.id, CONNECTIONS, date, &guesses, done).await?;
    let game = connections_game_of(date, &day, Some(&stored));

    if done {
        let result = PuzzleResult { solved: Some(game.status == "won"), score: Some(game.mistakes), seconds: game.seconds, source: None };

        save_puzzle_result(&state, user.id, CONNECTIONS, date, &result, "native").await?;
    }

    state.publish(Some(user.id), StreamMessage::WellnessChanged);

    Ok(Json(ConnectionsGuessAnswer { outcome: outcome.to_string(), game }))
}
