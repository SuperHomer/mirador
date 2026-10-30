//! What Claude models cost, and what a run of them came to: the rates per
//! million tokens from Anthropic's model table, and a rough count of the
//! tokens a text is.
//!
//! Only Claude models have rates here. Codex's and the other harnesses'
//! are not known, so they have none — a cost that is a guess would read as
//! a fact.

/// When the rates below were last checked against Anthropic's model table:
/// what a cost shown from them is as of.
pub const PRICES_CHECKED: &str = "2026-10-06";

/// A model's prices, in US dollars per million tokens.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rates {
    pub input: f64,
    pub output: f64,
    /// Writing the prompt cache, with the 5-minute lifetime Claude Code
    /// uses: 1.25× input.
    pub cache_write: f64,
    pub cache_read: f64,
}

/// Tokens a run used, by how each is priced.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Usage {
    pub input: u64,
    pub cache_write: u64,
    pub cache_read: u64,
    pub output: u64,
}

/// Model, input, output, and cache read where the table gives one; any
/// other model's cache read is 0.1× its input.
const TABLE: &[(&str, f64, f64, Option<f64>)] = &[
    ("claude-fable-5-1", 10.0, 50.0, Some(0.25)),
    ("claude-fable-5", 10.0, 50.0, None),
    ("claude-mythos-5-1", 10.0, 50.0, None),
    ("claude-opus-5-5", 4.0, 20.0, Some(0.20)),
    ("claude-opus-5", 5.0, 25.0, None),
    ("claude-opus-4-8", 5.0, 25.0, None),
    ("claude-opus-4-7", 5.0, 25.0, None),
    ("claude-opus-4-6", 5.0, 25.0, None),
    ("claude-sonnet-5-5", 2.0, 10.0, Some(0.20)),
    ("claude-sonnet-5", 2.0, 10.0, None),
    ("claude-sonnet-4-6", 3.0, 15.0, None),
    // Up to a 100K-token prompt. Above that it is 0.50 / 2.50, which is
    // not modelled: these rates undercount such a turn.
    ("claude-haiku-5-5", 0.10, 0.50, None),
    ("claude-haiku-4-5", 1.0, 5.0, None),
];

/// Claude Code's `--model` aliases, as a role's `model:` may say them.
const ALIASES: &[(&str, &str)] = &[
    ("opus", "claude-opus-5-5"),
    ("sonnet", "claude-sonnet-5-5"),
    ("haiku", "claude-haiku-5-5"),
    ("fable", "claude-fable-5-1"),
];

/// `model`'s rates: a full id (`claude-opus-5-5`), a dated one
/// (`claude-haiku-4-5-20251001`), an alias (`opus`), any of them with a
/// suffix such as `[1m]`. None for a model with no known price.
pub fn rates(model: &str) -> Option<Rates> {
    let mut id = model.trim().to_ascii_lowercase();
    if let Some(open) = id.find('[') {
        id.truncate(open);
    }
    if let Some((_, full)) = ALIASES.iter().find(|(alias, _)| *alias == id) {
        id = full.to_string();
    }
    if let Some((base, date)) = id.rsplit_once('-') {
        if date.len() == 8 && date.bytes().all(|b| b.is_ascii_digit()) {
            id = base.to_string();
        }
    }
    let &(_, input, output, cache_read) = TABLE.iter().find(|(name, ..)| *name == id)?;
    Some(Rates { input, output, cache_write: input * 1.25, cache_read: cache_read.unwrap_or(input * 0.1) })
}

/// About how many tokens `text` is: one per 4 characters, rounded up. An
/// estimate — the real count depends on the model's tokenizer and runs
/// higher for code and for text that is not English.
pub fn estimate_tokens(text: &str) -> u64 {
    (text.chars().count() as u64).div_ceil(4)
}

/// What `usage` costs at `rates`, in US dollars.
pub fn cost(rates: Rates, usage: Usage) -> f64 {
    let per_million = |tokens: u64, rate: f64| tokens as f64 * rate / 1_000_000.0;
    per_million(usage.input, rates.input)
        + per_million(usage.cache_write, rates.cache_write)
        + per_million(usage.cache_read, rates.cache_read)
        + per_million(usage.output, rates.output)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: f64, b: f64) -> bool {
        (a - b).abs() < 1e-9
    }

    #[test]
    fn aliases_dated_ids_and_suffixes_name_the_same_model() {
        let opus = rates("claude-opus-5-5").unwrap();
        assert_eq!(rates("opus"), Some(opus));
        assert_eq!(rates(" Opus "), Some(opus));
        assert_eq!(rates("opus[1m]"), Some(opus));
        assert_eq!(rates("claude-opus-5-5[1m]"), Some(opus));
        assert_eq!(rates("claude-opus-5-5-20261001"), Some(opus));
        assert_eq!(rates("sonnet"), rates("claude-sonnet-5-5"));
        assert_eq!(rates("haiku"), rates("claude-haiku-5-5"));
        assert_eq!(rates("fable"), rates("claude-fable-5-1"));
        assert_eq!(rates("claude-haiku-4-5-20251001"), rates("claude-haiku-4-5"));
        // Only a date comes off: `-5` is part of the name.
        assert_ne!(rates("claude-opus-5"), Some(opus));
    }

    #[test]
    fn rates_follow_the_table() {
        let opus = rates("claude-opus-5-5").unwrap();
        assert_eq!((opus.input, opus.output, opus.cache_read), (4.0, 20.0, 0.20));
        assert!(close(opus.cache_write, 5.0));
        // No cache read given: 0.1× input.
        let haiku = rates("claude-haiku-4-5").unwrap();
        assert!(close(haiku.cache_read, 0.1));
        assert!(close(haiku.cache_write, 1.25));
        assert_eq!(rates("claude-fable-5-1").unwrap().cache_read, 0.25);
        assert!(close(rates("claude-fable-5").unwrap().cache_read, 1.0));
    }

    #[test]
    fn a_model_with_no_known_price_has_none() {
        for model in ["gpt-6-luna", "gpt-5", "o3", "", "claude-unknown-9", "opus-ish", "claude-opus-5-5-2026"] {
            assert_eq!(rates(model), None, "{model}");
        }
    }

    #[test]
    fn a_usage_costs_each_bucket_at_its_rate() {
        let opus = rates("opus").unwrap();
        let million = |f: fn(&mut Usage)| {
            let mut u = Usage::default();
            f(&mut u);
            cost(opus, u)
        };
        assert!(close(million(|u| u.input = 1_000_000), 4.0));
        assert!(close(million(|u| u.cache_write = 1_000_000), 5.0));
        assert!(close(million(|u| u.cache_read = 1_000_000), 0.20));
        assert!(close(million(|u| u.output = 1_000_000), 20.0));
        let all = Usage { input: 1_000_000, cache_write: 1_000_000, cache_read: 1_000_000, output: 1_000_000 };
        assert!(close(cost(opus, all), 29.2));
        assert_eq!(cost(opus, Usage::default()), 0.0);
        // A small run: 12K in, 3K out on Sonnet 5.5.
        let small = Usage { input: 12_000, output: 3_000, ..Default::default() };
        assert!(close(cost(rates("sonnet").unwrap(), small), 0.054));
    }

    #[test]
    fn tokens_are_estimated_from_characters_not_bytes() {
        assert_eq!(estimate_tokens(""), 0);
        assert_eq!(estimate_tokens("abc"), 1);
        assert_eq!(estimate_tokens("abcd"), 1);
        assert_eq!(estimate_tokens("abcde"), 2);
        assert_eq!(estimate_tokens(&"x".repeat(4000)), 1000);
        // 8 characters, 24 bytes in UTF-8: 2 tokens, not 6.
        assert_eq!(estimate_tokens("日本語のテキスト"), 2);
        assert_eq!(estimate_tokens("café"), 1);
    }
}
