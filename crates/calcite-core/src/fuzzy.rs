//! Small fuzzy matcher for the command palette: every query word must
//! appear in the text, as a prefix of a word or in order as letters.
//! Higher scores are better matches.

/// How well `query` matches `text`, or None when it doesn't.
pub fn score(query: &str, text: &str) -> Option<i32> {
    let text_lower = text.to_lowercase();
    let words: Vec<&str> = text_lower
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty())
        .collect();
    let mut total = 0;
    for part in query.to_lowercase().split_whitespace() {
        total += part_score(part, &text_lower, &words)?;
    }
    // Shorter labels win ties: "Play X" before "Open X's folder".
    Some(total * 4 - text.len() as i32 / 8)
}

fn part_score(part: &str, text: &str, words: &[&str]) -> Option<i32> {
    if words.first().is_some_and(|w| w.starts_with(part)) {
        return Some(30);
    }
    if words.iter().any(|w| w.starts_with(part)) {
        return Some(20);
    }
    if text.contains(part) {
        return Some(10);
    }
    // Letters in order, like "fbo" in "Fabulously Optimized".
    let mut chars = text.chars();
    part.chars()
        .all(|wanted| chars.any(|c| c == wanted))
        .then_some(2)
}

#[cfg(test)]
mod tests {
    use super::score;

    #[test]
    fn words_and_prefixes() {
        assert!(score("fab op", "Play Fabulously Optimized").is_some());
        assert!(score("play fab", "Play Fabulously Optimized").is_some());
        assert!(score("xyz", "Play Fabulously Optimized").is_none());
        assert!(score("fbo", "Fabulously Optimized").is_some());
    }

    #[test]
    fn better_matches_rank_higher() {
        let starts = score("disc", "Discover").unwrap();
        let inside = score("disc", "Open the discord settings").unwrap();
        assert!(starts > inside);
        let short = score("meteor", "Play Meteor Client").unwrap();
        let long = score("meteor", "Open Meteor Client's folder").unwrap();
        assert!(short >= long);
    }

    #[test]
    fn empty_query_matches_everything() {
        assert!(score("", "Anything").is_some());
        assert!(score("   ", "Anything").is_some());
    }
}
