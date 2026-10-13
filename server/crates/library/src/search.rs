//! Turning what a user typed into FTS5 queries. User text never reaches FTS5 syntax unquoted.

/// Queries for the two search indexes. Either may be absent when the input cannot use it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FtsQueries {
    /// A substring match on the trigram index. Needs at least three characters.
    pub trigram: Option<String>,
    /// Every word as a prefix, on the word index.
    pub words: Option<String>,
}

fn quote(text: &str) -> String {
    format!("\"{}\"", text.replace('"', "\"\""))
}

pub fn fts_queries(input: &str) -> FtsQueries {
    let text = input.split_whitespace().collect::<Vec<_>>().join(" ");
    let trigram = (text.chars().count() >= 3).then(|| quote(&text));
    let words: Vec<String> = text
        .split(|c: char| !c.is_alphanumeric())
        .filter(|word| !word.is_empty())
        .map(|word| format!("{}*", quote(word)))
        .collect();
    FtsQueries {
        trigram,
        words: (!words.is_empty()).then(|| words.join(" ")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn queries(input: &str) -> (Option<&'static str>, Option<&'static str>) {
        let q = fts_queries(input);
        (q.trigram.map(|s| &*s.leak()), q.words.map(|s| &*s.leak()))
    }

    #[test]
    fn builds_both_queries() {
        assert_eq!(queries("gamma"), (Some(r#""gamma""#), Some(r#""gamma"*"#)));
        assert_eq!(
            queries("  first   album "),
            (Some(r#""first album""#), Some(r#""first"* "album"*"#))
        );
    }

    #[test]
    fn short_input_uses_words_only() {
        assert_eq!(queries("ga"), (None, Some(r#""ga"*"#)));
    }

    #[test]
    fn syntax_and_punctuation_are_inert() {
        assert_eq!(
            queries(r#"AC/DC "live" OR x*"#),
            (
                Some(r#""AC/DC ""live"" OR x*""#),
                Some(r#""AC"* "DC"* "live"* "OR"* "x"*"#)
            )
        );
        assert_eq!(queries("   "), (None, None));
        assert_eq!(queries("!!!"), (Some(r#""!!!""#), None));
    }

    #[test]
    fn keeps_non_latin_words() {
        assert_eq!(queries("Björk"), (Some(r#""Björk""#), Some(r#""Björk"*"#)));
    }
}
