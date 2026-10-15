//! Fuzzy matching for the popup: every space-separated word of the query must appear in the text, in
//! order (not necessarily adjacent); contiguous matches and matches at word starts score higher.

/// Scores `needle` (already lowercased) against `hay` (already lowercased). `None` = no match.
pub fn fuzzy_score(needle: &str, hay: &str) -> Option<i32> {
    let mut total = 0;
    let mut any = false;
    for word in needle.split_whitespace() {
        any = true;
        total += score_word(word, hay)?;
    }
    any.then_some(total)
}

fn score_word(word: &str, hay: &str) -> Option<i32> {
    if let Some(i) = hay.find(word) {
        let at_boundary = i == 0 || !hay[..i].chars().next_back().is_some_and(char::is_alphanumeric);
        // an exact substring always beats a scattered match; earlier is better
        return Some(1000 + if at_boundary { 200 } else { 0 } - (i.min(500) as i32) / 4);
    }
    let w: Vec<char> = word.chars().collect();
    let mut wi = 0;
    let mut score = 0i32;
    let mut prev_match: Option<usize> = None;
    let mut prev_char: Option<char> = None;
    for (ci, c) in hay.chars().enumerate() {
        if wi < w.len() && c == w[wi] {
            let consecutive = prev_match.is_some_and(|p| p + 1 == ci);
            let boundary = prev_char.is_none_or(|p| !p.is_alphanumeric());
            score += 10 + if consecutive { 15 } else { 0 } + if boundary { 12 } else { 0 };
            if let Some(p) = prev_match {
                score -= ((ci - p - 1).min(20)) as i32;
            }
            prev_match = Some(ci);
            wi += 1;
        }
        prev_char = Some(c);
        if ci > 4000 {
            break;
        }
    }
    (wi == w.len()).then_some(score)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matches_and_ranks() {
        assert!(fuzzy_score("hlo", "hello world").is_some());
        assert!(fuzzy_score("xyz", "hello world").is_none());
        assert!(
            fuzzy_score("wor hel", "hello world").is_some(),
            "words may come in any order"
        );
        assert!(fuzzy_score("", "anything").is_none());
        let exact = fuzzy_score("hello", "say hello there").unwrap();
        let loose = fuzzy_score("hello", "h e l l o x").unwrap();
        assert!(exact > loose);
        let start = fuzzy_score("net", "network").unwrap();
        let mid = fuzzy_score("net", "cabinet").unwrap();
        assert!(start > mid);
    }

    #[test]
    fn cyrillic_works() {
        assert!(fuzzy_score("прив", "привет, мир").is_some());
        assert!(fuzzy_score("пмр", "привет, мир").is_some());
    }
}
