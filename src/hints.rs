//! Generate a prefix-free set of hint labels over a keyboard alphabet.
//!
//! With `n <= alphabet.len()` every hint is a single key. Beyond that, the
//! least convenient single keys (end of the alphabet) are expanded into
//! two-key hints, and so on, so the best keys stay one keystroke.

/// Produce at least `n` prefix-free hints, best (shortest, earliest key) first.
pub fn generate(alphabet: &[char], n: usize) -> Vec<String> {
    if alphabet.is_empty() || n == 0 {
        return Vec::new();
    }
    let mut hints: Vec<String> = alphabet.iter().map(|c| c.to_string()).collect();
    if n <= hints.len() {
        hints.truncate(n);
        return hints;
    }
    if alphabet.len() == 1 {
        // Degenerate alphabet: only "a", "aa", "aaa"... is prefix-free-ish if we
        // use lengths; fall back to numbered lengths.
        return (1..=n)
            .map(|len| alphabet[0].to_string().repeat(len))
            .collect();
    }
    while hints.len() < n {
        let min_len = hints.iter().map(String::len).min().unwrap_or(0);
        let idx = hints
            .iter()
            .rposition(|h| h.len() == min_len)
            .expect("non-empty");
        let prefix = hints.remove(idx);
        for c in alphabet {
            let mut h = prefix.clone();
            h.push(*c);
            hints.push(h);
        }
    }
    let rank = |h: &String| -> (usize, Vec<usize>) {
        (
            h.chars().count(),
            h.chars()
                .map(|c| alphabet.iter().position(|a| *a == c).unwrap_or(0))
                .collect(),
        )
    };
    hints.sort_by_key(rank);
    hints
}

#[cfg(test)]
mod tests {
    use super::*;

    fn abc() -> Vec<char> {
        "asdf".chars().collect()
    }

    fn is_prefix_free(hints: &[String]) -> bool {
        for (i, a) in hints.iter().enumerate() {
            for (j, b) in hints.iter().enumerate() {
                if i != j && b.starts_with(a.as_str()) {
                    return false;
                }
            }
        }
        true
    }

    #[test]
    fn small_n_is_single_keys() {
        assert_eq!(generate(&abc(), 3), vec!["a", "s", "d"]);
    }

    #[test]
    fn expands_worst_key_first() {
        let h = generate(&abc(), 5);
        assert!(h.len() >= 5);
        assert_eq!(&h[..3], &["a", "s", "d"]);
        assert!(h.contains(&"fa".to_string()));
        assert!(is_prefix_free(&h));
    }

    #[test]
    fn large_n_stays_prefix_free() {
        let alphabet: Vec<char> = "asdfqwerzxcvjklmiuopghtybn".chars().collect();
        for n in [27, 100, 500, 1200] {
            let h = generate(&alphabet, n);
            assert!(h.len() >= n, "n={n} got {}", h.len());
            assert!(is_prefix_free(&h), "n={n}");
        }
    }

    #[test]
    fn empty_inputs() {
        assert!(generate(&[], 5).is_empty());
        assert!(generate(&abc(), 0).is_empty());
    }
}
