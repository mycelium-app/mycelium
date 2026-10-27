use jaq_core::load::{Arena, Error as LoadError, File, Loader};
use jaq_core::{Compiler, Ctx, Vars, data, unwrap_valr};
use jaq_json::{Val, read};
use serde_json::Value;
use std::time::{Duration, Instant};

const TIME_LIMIT: Duration = Duration::from_secs(2);
const MAX_OUTPUTS: usize = 100_000;
const MAX_OUTPUT_BYTES: usize = 8 * 1024 * 1024;

pub type Path = Vec<Value>;

#[derive(Debug, Clone, PartialEq)]
pub enum Selection {
    Paths(Vec<Path>),
    Value(Value),
}

fn load_message(errors: Vec<(File<&str, ()>, LoadError<&str>)>) -> String {
    let first = errors.into_iter().next().map(|(_, error)| error);

    match first {
        Some(LoadError::Lex(errors)) => errors.first().map(|(expected, found)| format!("expected {} near {found:?}", expected.as_str())).unwrap_or_default(),
        Some(LoadError::Parse(errors)) => errors.first().map(|(expected, found)| format!("expected {} near {found:?}", expected.as_str())).unwrap_or_default(),
        Some(LoadError::Io(errors)) => errors.first().map(|(_, message)| message.clone()).unwrap_or_default(),
        None => String::new(),
    }
}

fn run(code: &str, input: &Value) -> Result<Vec<Value>, String> {
    let deadline = Instant::now() + TIME_LIMIT;
    let input = read::parse_single(input.to_string().as_bytes()).map_err(|error| error.to_string())?;
    let defs = jaq_core::defs().chain(jaq_std::defs()).chain(jaq_json::defs());
    let funs = jaq_core::funs().chain(jaq_std::funs()).chain(jaq_json::funs());
    let arena = Arena::default();
    let modules = Loader::new(defs).load(&arena, File { code, path: () }).map_err(load_message)?;
    let filter = Compiler::default().with_funs(funs).compile(modules).map_err(|errors| {
        let undefined = errors.into_iter().flat_map(|(_, names)| names).next();

        undefined.map(|(name, kind)| format!("{} {name} is not defined", kind.as_str())).unwrap_or_default()
    })?;
    let context = Ctx::<data::JustLut<Val>>::new(&filter.lut, Vars::new([]));
    let mut outputs = Vec::new();
    let mut bytes = 0;

    for output in filter.id.run((context, input)).map(unwrap_valr) {
        if Instant::now() > deadline {
            return Err("it took too long to run".to_string());
        }

        let text = output.map_err(|error| error.to_string())?.to_string();
        bytes += text.len();

        if outputs.len() >= MAX_OUTPUTS || bytes > MAX_OUTPUT_BYTES {
            return Err("it produced too much output".to_string());
        }

        outputs.push(serde_json::from_str(&text).map_err(|error| error.to_string())?);
    }

    Ok(outputs)
}

fn paths_of(filter: &str, input: &Value) -> Result<Vec<Path>, String> {
    let collected = run(&format!("[path({filter}\n)]"), input)?;
    let paths = collected.into_iter().next().unwrap_or(Value::Null);

    serde_json::from_value(paths).map_err(|error| error.to_string())
}

fn outputs_of(filter: &str, input: &Value) -> Result<Value, String> {
    let mut outputs = run(filter, input)?;

    match outputs.len() {
        1 => Ok(outputs.remove(0)),
        _ => Ok(Value::Array(outputs)),
    }
}

pub fn select(filter: &str, input: &Value) -> Result<Selection, String> {
    match paths_of(filter, input) {
        Ok(paths) => Ok(Selection::Paths(paths)),
        Err(_) => outputs_of(filter, input).map(Selection::Value),
    }
}

pub async fn select_limited(filter: String, input: Value) -> Result<Selection, String> {
    let work = tokio::task::spawn_blocking(move || select(&filter, &input));

    match tokio::time::timeout(TIME_LIMIT * 2, work).await {
        Ok(Ok(result)) => result,
        Ok(Err(_)) => Err("it could not be run".to_string()),
        Err(_) => Err("it took too long to run".to_string()),
    }
}

pub fn unreadable(message: &str) -> String {
    format!("The filter could not be read: {message}. Fix it or clear it.")
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn a_path_expression_selects_paths_without_changing_the_value() {
        let input = json!({"messages": [{"content": "lunch at one"}, {"content": "standup"}]});
        let selection = select(".messages[] | select(.content | test(\"lunch\"))", &input).unwrap();

        assert_eq!(selection, Selection::Paths(vec![vec![json!("messages"), json!(0)]]));
    }

    #[test]
    fn a_filter_matching_nothing_selects_no_paths() {
        let selection = select(".[] | select(. > 9)", &json!([1, 2])).unwrap();

        assert_eq!(selection, Selection::Paths(vec![]));
    }

    #[test]
    fn a_filter_that_builds_new_values_replaces_the_value() {
        let input = json!([{"name": "a", "extra": 1}, {"name": "b", "extra": 2}]);
        let selection = select("map(.name)", &input).unwrap();

        assert_eq!(selection, Selection::Value(json!(["a", "b"])));
    }

    #[test]
    fn several_outputs_become_a_list() {
        let selection = select(".[] | .n + 1", &json!([{"n": 1}, {"n": 2}])).unwrap();

        assert_eq!(selection, Selection::Value(json!([2, 3])));
    }

    #[test]
    fn a_filter_that_does_not_parse_is_an_error_with_a_message() {
        let error = select(".messages[", &json!({})).unwrap_err();

        assert!(!error.is_empty());
    }

    #[test]
    fn an_unknown_filter_name_is_an_error() {
        let error = select("nonsense", &json!({})).unwrap_err();

        assert!(error.contains("nonsense"), "{error}");
    }

    #[test]
    fn a_runtime_error_is_reported() {
        assert!(select(".a.b", &json!({"a": "text"})).is_err());
    }

    #[test]
    fn output_beyond_the_size_limit_is_refused() {
        let error = select("[range(0; 20000000)]", &json!(null)).unwrap_err();

        assert!(error.contains("too much") || error.contains("too long"), "{error}");
    }

    #[tokio::test]
    async fn the_limited_form_returns_the_same_selection() {
        let selection = select_limited(".[0]".to_string(), json!([5])).await.unwrap();

        assert_eq!(selection, Selection::Paths(vec![vec![json!(0)]]));
    }
}
