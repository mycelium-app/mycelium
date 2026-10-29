use crate::model_providers::ProviderRow;
use crate::outbound::{BODY_LIMIT, read_text_limited};
use crate::state::AppState;
use mycelium_proto::api::ModelProviderKind;
use serde_json::{Map, Value, json};
use std::time::Duration;

const REQUEST_TIMEOUT: Duration = Duration::from_secs(60);
const TOP_LOGPROBS: u32 = 20;
const ANSWER_TOKENS: u32 = 8;
const LETTERS: &str = "ABCDEFGHIJKLMNOPQRSTUVWXYZ";
const MOST_LEVELS: usize = 9;

#[derive(Clone, Copy, PartialEq)]
enum Kind {
    Noul,
    Choice,
    Score,
}

struct Candidate {
    label: String,
    name: String,
    meaning: String,
}

struct Prompt {
    kind: Kind,
    options: Vec<Candidate>,
    text: String,
}

fn rendered(subject: &Value) -> String {
    match subject {
        Value::String(text) => text.clone(),
        other => serde_json::to_string_pretty(other).unwrap_or_default(),
    }
}

fn meaning_of(value: &Value) -> String {
    value.as_str().map(str::to_string).unwrap_or_else(|| value.to_string())
}

fn prompt_for(name: &str, question: &Value, subject: &Value) -> Result<Prompt, String> {
    let kind = match question["type"].as_str().unwrap_or_default() {
        "noul" => Kind::Noul,
        "choice" => Kind::Choice,
        "score" => Kind::Score,
        "box" => return Err(format!("Question {name} is a box question, which needs a Jev server. A chat provider answers noul, choice and score questions.")),
        other => return Err(format!("Question {name} has type {other:?}. Use noul, choice or score.")),
    };
    let instructions = question["instructions"].as_str().unwrap_or_default().trim();
    let criteria = &question["criteria"];
    let options: Vec<Candidate> = match kind {
        Kind::Noul => vec![
            Candidate { label: "Yes".to_string(), name: "yes".to_string(), meaning: criteria["true"].as_str().unwrap_or_default().to_string() },
            Candidate { label: "No".to_string(), name: "no".to_string(), meaning: criteria["false"].as_str().unwrap_or_default().to_string() },
        ],
        Kind::Choice => {
            let Some(listed) = criteria.as_object().filter(|listed| !listed.is_empty()) else {
                return Err(format!("Question {name} is a choice, so it needs criteria mapping each option to what it means."));
            };

            if listed.len() > LETTERS.len() {
                return Err(format!("Question {name} has more than {} options. Give fewer.", LETTERS.len()));
            }

            listed.iter().zip(LETTERS.chars()).map(|((option, meaning), letter)| Candidate { label: letter.to_string(), name: option.clone(), meaning: meaning_of(meaning) }).collect()
        }
        Kind::Score => {
            let Some(levels) = criteria.as_array().filter(|levels| !levels.is_empty()) else {
                return Err(format!("Question {name} is a score, so it needs criteria listing what each level means, lowest first."));
            };

            if levels.len() > MOST_LEVELS {
                return Err(format!("Question {name} has more than {MOST_LEVELS} levels. Give fewer."));
            }

            levels.iter().enumerate().map(|(index, meaning)| Candidate { label: (index + 1).to_string(), name: (index + 1).to_string(), meaning: meaning_of(meaning) }).collect()
        }
    };
    let listing: Vec<String> = options.iter().map(|option| if option.meaning.is_empty() { option.label.clone() } else { format!("{}: {}", option.label, option.meaning) }).collect();
    let labels: Vec<&str> = options.iter().map(|option| option.label.as_str()).collect();
    let text = format!("State:\n{}\n\nQuestion: {instructions}\n\nOptions:\n{}\n\nReply with only the label of one option ({}), and nothing else.", rendered(subject), listing.join("\n"), labels.join(", "));

    Ok(Prompt { kind, options, text })
}

fn probabilities_over(top: &[(String, f64)], options: &[Candidate]) -> Option<Vec<f64>> {
    let mut mass = vec![0.0; options.len()];

    for (token, logprob) in top {
        let spoken = token.trim().to_lowercase();
        let found = options.iter().position(|option| option.label.to_lowercase() == spoken || (option.label == "Yes" && spoken == "y") || (option.label == "No" && spoken == "n"));

        if let Some(index) = found {
            mass[index] += logprob.exp();
        }
    }

    let total: f64 = mass.iter().sum();

    (total > 0.0).then(|| mass.into_iter().map(|share| share / total).collect())
}

fn top_logprobs_of(response: &Value) -> Vec<(String, f64)> {
    let first = &response["choices"][0]["logprobs"]["content"][0];

    first["top_logprobs"].as_array().into_iter().flatten().filter_map(|entry| Some((entry["token"].as_str()?.to_string(), entry["logprob"].as_f64()?))).collect()
}

fn spoken_option(response: &Value, options: &[Candidate]) -> Option<usize> {
    let text = response["choices"][0]["message"]["content"].as_str()?.trim().to_lowercase();
    let first_word = text.split(|character: char| !character.is_alphanumeric()).find(|word| !word.is_empty())?;

    options.iter().position(|option| option.label.to_lowercase() == first_word)
}

fn answer_from(prompt: &Prompt, response: &Value) -> Result<Value, String> {
    let reported = probabilities_over(&top_logprobs_of(response), &prompt.options);
    let (shares, probabilities_reported) = match reported {
        Some(shares) => (shares, true),
        None => {
            let Some(spoken) = spoken_option(response, &prompt.options) else {
                return Err("The model did not answer with one of the options, so nothing was decided.".to_string());
            };

            (prompt.options.iter().enumerate().map(|(index, _)| if index == spoken { 1.0 } else { 0.0 }).collect(), false)
        }
    };
    let best = shares.iter().enumerate().max_by(|left, right| left.1.total_cmp(right.1)).map(|(index, _)| index).unwrap_or(0);
    let confidence = shares[best];
    let mut answer = Map::new();

    match prompt.kind {
        Kind::Noul => {
            answer.insert("noul".to_string(), json!(shares[0]));
        }
        Kind::Choice | Kind::Score => {
            let named: Map<String, Value> = prompt.options.iter().zip(&shares).map(|(option, share)| (option.name.clone(), json!(share))).collect();
            let picked = if prompt.kind == Kind::Score { json!(best + 1) } else { json!(prompt.options[best].name) };

            answer.insert(if prompt.kind == Kind::Score { "score" } else { "choice" }.to_string(), picked);
            answer.insert("probabilities".to_string(), Value::Object(named));
            answer.insert("confidence".to_string(), json!(confidence));
        }
    }

    if !probabilities_reported {
        answer.insert("probabilities_reported".to_string(), json!(false));
    }

    Ok(Value::Object(answer))
}

fn request_for(model: &str, prompt: &Prompt, with_logprobs: bool) -> Value {
    let mut body = json!({
        "model": model,
        "messages": [
            {"role": "system", "content": "You answer one multiple choice question about some state. Reply with the label of the option that fits best and nothing else."},
            {"role": "user", "content": prompt.text},
        ],
        "temperature": 0,
        "max_tokens": ANSWER_TOKENS,
    });

    if with_logprobs {
        body["logprobs"] = json!(true);
        body["top_logprobs"] = json!(TOP_LOGPROBS);
    }

    body
}

async fn complete(state: &AppState, provider: &ProviderRow, body: &Value) -> Result<Result<Value, u16>, String> {
    let address = provider.address().map_err(|_| "The chat provider has no address Mycelium can ask.".to_string())?;
    let request = provider.authorised(state.http.post(format!("{address}/chat/completions")).timeout(REQUEST_TIMEOUT).json(body)).map_err(|_| "The chat provider could not be asked.".to_string())?;
    let Ok(response) = request.send().await else {
        return Err("The chat provider could not be reached. Ask the person to check it is running.".to_string());
    };
    let status = response.status();
    let text = read_text_limited(response, BODY_LIMIT).await.unwrap_or_default();

    if status.as_u16() == 401 || status.as_u16() == 403 {
        return Err("The chat provider refused Mycelium's key. Ask the person to check the key in Settings, Providers.".to_string());
    }

    if !status.is_success() {
        return Ok(Err(status.as_u16()));
    }

    serde_json::from_str(&text).map(Ok).map_err(|_| "The chat provider answered in a shape Mycelium does not know.".to_string())
}

async fn ask_one(state: &AppState, provider: &ProviderRow, model: &str, prompt: &Prompt) -> Result<Value, String> {
    let with = complete(state, provider, &request_for(model, prompt, true)).await?;
    let response = match with {
        Ok(response) => response,
        Err(400 | 422) => match complete(state, provider, &request_for(model, prompt, false)).await? {
            Ok(response) => response,
            Err(code) => return Err(format!("The chat provider answered {code}. Ask the person to check it.")),
        },
        Err(429) => return Err("The chat provider is rate limiting requests. Wait a little and ask again.".to_string()),
        Err(code) => return Err(format!("The chat provider answered {code}. Ask the person to check it.")),
    };

    answer_from(prompt, &response)
}

pub async fn ask(state: &AppState, provider: &ProviderRow, model: &str, subject: &Value, questions: &Value) -> Result<Value, String> {
    if provider.parsed_kind().ok() != Some(ModelProviderKind::OpenaiCompatible) {
        return Err("A decisions provider chosen from your chat providers must be OpenAI compatible, because it reads token probabilities. Ask the person to choose another.".to_string());
    }

    let mut answers = Map::new();

    for (name, question) in questions.as_object().into_iter().flatten() {
        let prompt = prompt_for(name, question, subject)?;

        answers.insert(name.clone(), ask_one(state, provider, model, &prompt).await?);
    }

    Ok(Value::Object(answers))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn answer(question: Value, response: Value) -> Value {
        let prompt = prompt_for("q", &question, &json!("A customer asks where their refund is.")).unwrap();

        answer_from(&prompt, &response).unwrap()
    }

    fn with_top(entries: &[(&str, f64)]) -> Value {
        let top: Vec<Value> = entries.iter().map(|(token, probability)| json!({"token": token, "logprob": probability.ln()})).collect();

        json!({"choices": [{"message": {"content": entries[0].0}, "logprobs": {"content": [{"token": entries[0].0, "top_logprobs": top}]}}]})
    }

    #[test]
    fn a_yes_or_no_question_is_the_probability_of_yes_normalised_over_the_yes_and_no_tokens() {
        let reply = answer(json!({"type": "noul", "instructions": "Urgent?"}), with_top(&[("Yes", 0.6), (" yes", 0.1), ("No", 0.1), ("Maybe", 0.2)]));

        assert!((reply["noul"].as_f64().unwrap() - 0.875).abs() < 1e-9, "{reply}");
        assert!(reply.get("probabilities_reported").is_none());
    }

    #[test]
    fn a_choice_reports_each_option_by_name_with_the_best_as_the_choice() {
        let reply = answer(json!({"type": "choice", "instructions": "Which team?", "criteria": {"billing": "money", "support": "how to"}}), with_top(&[("B", 0.7), ("A", 0.3)]));

        assert_eq!(reply["choice"], "support");
        assert!((reply["probabilities"]["billing"].as_f64().unwrap() - 0.3).abs() < 1e-9);
        assert!((reply["confidence"].as_f64().unwrap() - 0.7).abs() < 1e-9);
    }

    #[test]
    fn a_score_counts_levels_from_one() {
        let reply = answer(json!({"type": "score", "instructions": "How angry?", "criteria": ["calm", "annoyed", "furious"]}), with_top(&[("3", 0.5), ("2", 0.5)]));

        assert!(reply["score"] == 3 || reply["score"] == 2);
        assert_eq!(reply["probabilities"]["1"], 0.0);
    }

    #[test]
    fn without_token_probabilities_the_spoken_option_is_certain_and_the_answer_says_so() {
        let reply = answer(json!({"type": "noul", "instructions": "Urgent?"}), json!({"choices": [{"message": {"content": "No."}}]}));

        assert_eq!(reply["noul"], 0.0);
        assert_eq!(reply["probabilities_reported"], false);
    }

    #[test]
    fn a_reply_that_names_no_option_decides_nothing() {
        let prompt = prompt_for("q", &json!({"type": "noul", "instructions": "Urgent?"}), &json!("x")).unwrap();

        assert!(answer_from(&prompt, &json!({"choices": [{"message": {"content": "It depends."}}]})).is_err());
    }

    #[test]
    fn a_box_question_and_a_choice_without_criteria_are_refused_in_words() {
        let subject = json!("x");

        assert!(prompt_for("where", &json!({"type": "box", "instructions": "the button"}), &subject).err().unwrap().contains("Jev server"));
        assert!(prompt_for("team", &json!({"type": "choice", "instructions": "Which?"}), &subject).err().unwrap().contains("needs criteria"));
    }
}
