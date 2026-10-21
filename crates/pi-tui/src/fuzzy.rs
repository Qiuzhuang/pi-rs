//! Fuzzy matching utilities. Port of `packages/tui/src/fuzzy.ts`.
//!
//! Matches if all query characters appear in order (not necessarily consecutive).
//! Lower score = better match.

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FuzzyMatch {
    pub matches: bool,
    pub score: f64,
}

fn is_boundary_char(c: char) -> bool {
    crate::utils::is_whitespace_ch(c) || matches!(c, '-' | '_' | '.' | '/' | ':')
}

fn match_query(query: &[char], text: &[char], query_str: &str, text_str: &str) -> FuzzyMatch {
    if query.is_empty() {
        return FuzzyMatch { matches: true, score: 0.0 };
    }
    let utf16 = |s: &[char]| s.iter().map(|c| c.len_utf16()).sum::<usize>();
    if utf16(query) > utf16(text) {
        return FuzzyMatch { matches: false, score: 0.0 };
    }
    let mut query_index = 0;
    let mut score = 0.0f64;
    let mut last_match_index: isize = -1;
    let mut consecutive = 0.0f64;
    while query_index < query.len() {
        let start = (last_match_index + 1) as usize;
        let Some(off) = text[start.min(text.len())..].iter().position(|c| *c == query[query_index]) else {
            break;
        };
        let i = start + off;
        let is_word_boundary = i == 0 || is_boundary_char(text[i - 1]);
        if last_match_index == i as isize - 1 {
            consecutive += 1.0;
            score -= consecutive * 5.0;
        } else {
            consecutive = 0.0;
            if last_match_index >= 0 {
                score += (i as f64 - last_match_index as f64 - 1.0) * 2.0;
            }
        }
        if is_word_boundary {
            score -= 10.0;
        }
        score += i as f64 * 0.1;
        last_match_index = i as isize;
        query_index += 1;
    }
    if query_index < query.len() {
        return FuzzyMatch { matches: false, score: 0.0 };
    }
    if query_str == text_str {
        score -= 100.0;
    }
    FuzzyMatch { matches: true, score }
}

/// Fuzzy-match `query` against `text`.
pub fn fuzzy_match(query: &str, text: &str) -> FuzzyMatch {
    let query_lower = query.to_lowercase();
    let text_lower = text.to_lowercase();
    let text_chars: Vec<char> = text_lower.chars().collect();
    let run = |q: &str| {
        let qc: Vec<char> = q.chars().collect();
        match_query(&qc, &text_chars, q, &text_lower)
    };
    let primary = run(&query_lower);
    if primary.matches {
        return primary;
    }
    let letters = query_lower.bytes().take_while(u8::is_ascii_lowercase).count();
    let digits_after = query_lower[letters..].bytes().take_while(u8::is_ascii_digit).count();
    let digits = query_lower.bytes().take_while(u8::is_ascii_digit).count();
    let letters_after = query_lower[digits..].bytes().take_while(u8::is_ascii_lowercase).count();
    let swapped = if letters > 0 && digits_after > 0 && letters + digits_after == query_lower.len() {
        format!("{}{}", &query_lower[letters..], &query_lower[..letters])
    } else if digits > 0 && letters_after > 0 && digits + letters_after == query_lower.len() {
        format!("{}{}", &query_lower[digits..], &query_lower[..digits])
    } else {
        String::new()
    };
    if swapped.is_empty() {
        return primary;
    }
    let swapped_match = run(&swapped);
    if !swapped_match.matches {
        return primary;
    }
    FuzzyMatch { matches: true, score: swapped_match.score + 5.0 }
}

/// Filter and sort items by fuzzy match quality (best matches first). Supports whitespace-
/// and slash-separated tokens: all tokens must match.
pub fn fuzzy_filter<T: Clone>(items: &[T], query: &str, get_text: impl Fn(&T) -> String) -> Vec<T> {
    let trimmed = crate::utils::js_trim(query);
    if trimmed.is_empty() {
        return items.to_vec();
    }
    let tokens: Vec<&str> =
        trimmed.split(|c: char| crate::utils::is_whitespace_ch(c) || c == '/').filter(|t| !t.is_empty()).collect();
    if tokens.is_empty() {
        return items.to_vec();
    }
    let mut results: Vec<(usize, f64)> = Vec::new();
    for (idx, item) in items.iter().enumerate() {
        let text = get_text(item);
        let mut total = 0.0;
        let mut all = true;
        for token in &tokens {
            let m = fuzzy_match(token, &text);
            if m.matches {
                total += m.score;
            } else {
                all = false;
                break;
            }
        }
        if all {
            results.push((idx, total));
        }
    }
    // Array.prototype.sort is stable.
    results.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal));
    results.into_iter().map(|(i, _)| items[i].clone()).collect()
}
