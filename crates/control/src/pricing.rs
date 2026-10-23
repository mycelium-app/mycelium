#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Tokens {
    pub input: i64,
    pub output: i64,
    pub cache_read: i64,
    pub cache_write_5m: i64,
    pub cache_write_1h: i64,
}

impl Tokens {
    pub fn add(&mut self, other: &Tokens) {
        self.input += other.input;
        self.output += other.output;
        self.cache_read += other.cache_read;
        self.cache_write_5m += other.cache_write_5m;
        self.cache_write_1h += other.cache_write_1h;
    }

    pub fn cache_write(&self) -> i64 {
        self.cache_write_5m + self.cache_write_1h
    }
}

struct Price {
    prefix: &'static str,
    input: f64,
    cache_write_5m: f64,
    cache_write_1h: f64,
    cache_read: f64,
    output: f64,
}

const fn price(prefix: &'static str, input: f64, cache_write_5m: f64, cache_write_1h: f64, cache_read: f64, output: f64) -> Price {
    Price { prefix, input, cache_write_5m, cache_write_1h, cache_read, output }
}

const PER_MILLION: f64 = 1_000_000.0;

const PRICES: &[Price] = &[
    price("claude-fable-5-1", 10.0, 12.5, 20.0, 0.25, 50.0),
    price("claude-fable-5", 10.0, 12.5, 20.0, 1.0, 50.0),
    price("claude-mythos-5-1", 10.0, 12.5, 20.0, 0.25, 50.0),
    price("claude-mythos-5", 10.0, 12.5, 20.0, 1.0, 50.0),
    price("claude-opus-5-5", 4.0, 5.0, 8.0, 0.2, 20.0),
    price("claude-opus-5", 5.0, 6.25, 10.0, 0.5, 25.0),
    price("claude-opus-4-8", 5.0, 6.25, 10.0, 0.5, 25.0),
    price("claude-opus-4-7", 5.0, 6.25, 10.0, 0.5, 25.0),
    price("claude-opus-4-6", 5.0, 6.25, 10.0, 0.5, 25.0),
    price("claude-opus-4-5", 5.0, 6.25, 10.0, 0.5, 25.0),
    price("claude-opus-4-1", 15.0, 18.75, 30.0, 1.5, 75.0),
    price("claude-opus-4", 15.0, 18.75, 30.0, 1.5, 75.0),
    price("claude-sonnet-5-5", 2.0, 2.5, 4.0, 0.1, 10.0),
    price("claude-sonnet-5", 2.0, 2.5, 4.0, 0.2, 10.0),
    price("claude-sonnet-4", 3.0, 3.75, 6.0, 0.3, 15.0),
    price("claude-haiku-4-5", 1.0, 1.25, 2.0, 0.1, 5.0),
    price("claude-3-5-haiku", 0.8, 1.0, 1.6, 0.08, 4.0),
    price("gpt-5.6-sol", 4.0, 0.0, 0.0, 0.4, 20.0),
    price("gpt-5.6-terra", 2.0, 0.0, 0.0, 0.2, 12.0),
    price("gpt-5.6-luna", 0.2, 0.0, 0.0, 0.02, 1.2),
    price("gpt-5.5", 5.0, 0.0, 0.0, 0.5, 30.0),
    price("gpt-5.4-mini", 0.75, 0.0, 0.0, 0.075, 4.5),
    price("gpt-5.4-nano", 0.2, 0.0, 0.0, 0.02, 1.25),
    price("gpt-5.4", 2.5, 0.0, 0.0, 0.25, 15.0),
    price("gpt-5.3-codex", 1.75, 0.0, 0.0, 0.175, 14.0),
    price("gpt-5.2", 1.75, 0.0, 0.0, 0.175, 14.0),
    price("gpt-5.1", 1.25, 0.0, 0.0, 0.125, 10.0),
    price("gpt-5-mini", 0.25, 0.0, 0.0, 0.025, 2.0),
    price("gpt-5-nano", 0.05, 0.0, 0.0, 0.005, 0.4),
    price("gpt-5", 1.25, 0.0, 0.0, 0.125, 10.0),
];

pub fn estimated_cost(model: &str, tokens: &Tokens) -> Option<f64> {
    let found = PRICES.iter().filter(|price| model.starts_with(price.prefix)).max_by_key(|price| price.prefix.len())?;
    let total = tokens.input as f64 * found.input
        + tokens.output as f64 * found.output
        + tokens.cache_read as f64 * found.cache_read
        + tokens.cache_write_5m as f64 * found.cache_write_5m
        + tokens.cache_write_1h as f64 * found.cache_write_1h;

    Some(total / PER_MILLION)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_model_id_with_a_date_suffix_uses_the_longest_matching_price() {
        let tokens = Tokens { input: 1_000_000, output: 1_000_000, cache_read: 1_000_000, cache_write_5m: 1_000_000, cache_write_1h: 1_000_000 };

        assert_eq!(estimated_cost("claude-opus-5-5-20261001", &tokens), Some(4.0 + 20.0 + 0.2 + 5.0 + 8.0));
        assert_eq!(estimated_cost("claude-opus-5", &tokens), Some(5.0 + 25.0 + 0.5 + 6.25 + 10.0));
        assert_eq!(estimated_cost("gpt-5.4-mini", &Tokens { input: 1_000_000, ..Tokens::default() }), Some(0.75));
    }

    #[test]
    fn an_unknown_model_has_no_cost() {
        assert_eq!(estimated_cost("some-local-model", &Tokens::default()), None);
        assert_eq!(estimated_cost("", &Tokens::default()), None);
    }
}
