//! Find grab targets (paths, URLs, hashes...) in captured pane text.

use anyhow::{Context, Result};
use regex::Regex;

/// One highlighted region on screen.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Target {
    /// Zero-based line index in the captured pane.
    pub line: usize,
    /// Char offset of the whole match within the (tab-expanded) line.
    pub start: usize,
    /// Char length of the whole match.
    pub len: usize,
    /// Char offset of the captured text relative to `start`.
    pub cap_off: usize,
    /// The text that gets copied.
    pub text: String,
    /// Assigned hint label (filled in by `assign_hints`).
    pub hint: String,
}

impl Target {
    pub fn cap_len(&self) -> usize {
        self.text.chars().count()
    }
}

pub struct Matcher {
    patterns: Vec<(Regex, Option<usize>)>,
}

impl Matcher {
    /// Compile the given regexes. A named group `match` narrows the copied text.
    pub fn new(patterns: &[String]) -> Result<Self> {
        let mut compiled = Vec::with_capacity(patterns.len());
        for p in patterns {
            let re = Regex::new(p).with_context(|| format!("invalid pattern: {p}"))?;
            let group = re.capture_names().position(|name| name == Some("match"));
            compiled.push((re, group));
        }
        Ok(Self { patterns: compiled })
    }

    /// Find non-overlapping targets in every line. Earlier start wins; on a
    /// tie the longer match wins.
    pub fn find(&self, lines: &[String]) -> Vec<Target> {
        let mut out = Vec::new();
        for (line_idx, line) in lines.iter().enumerate() {
            let mut candidates: Vec<Target> = Vec::new();
            for (re, group) in &self.patterns {
                for caps in re.captures_iter(line) {
                    let whole = caps.get(0).expect("group 0");
                    if whole.is_empty() {
                        continue;
                    }
                    let cap = group.and_then(|g| caps.get(g)).unwrap_or(whole);
                    let start = char_index(line, whole.start());
                    let end = char_index(line, whole.end());
                    let cap_start = char_index(line, cap.start());
                    candidates.push(Target {
                        line: line_idx,
                        start,
                        len: end - start,
                        cap_off: cap_start - start,
                        text: cap.as_str().to_string(),
                        hint: String::new(),
                    });
                }
            }
            candidates.sort_by(|a, b| a.start.cmp(&b.start).then(b.len.cmp(&a.len)));
            let mut cursor = 0usize;
            for t in candidates {
                if t.start >= cursor && !t.text.is_empty() {
                    cursor = t.start + t.len;
                    out.push(t);
                }
            }
        }
        out
    }
}

fn char_index(s: &str, byte_idx: usize) -> usize {
    s[..byte_idx].chars().count()
}

/// Expand tabs to spaces at 8-column stops so on-screen columns line up.
pub fn expand_tabs(line: &str) -> String {
    if !line.contains('\t') {
        return line.to_string();
    }
    let mut out = String::with_capacity(line.len() + 8);
    let mut col = 0usize;
    for ch in line.chars() {
        if ch == '\t' {
            let spaces = 8 - (col % 8);
            out.extend(std::iter::repeat_n(' ', spaces));
            col += spaces;
        } else {
            out.push(ch);
            col += unicode_width::UnicodeWidthChar::width(ch).unwrap_or(0);
        }
    }
    out
}

/// Give every target a hint. Identical text shares one hint. Targets nearer
/// the bottom of the screen get the best hints, since recent output is what
/// you usually want. Targets whose text is narrower than the hint are dropped.
pub fn assign_hints(mut targets: Vec<Target>, alphabet: &[char]) -> Vec<Target> {
    use std::collections::HashMap;
    let unique: std::collections::HashSet<&str> = targets.iter().map(|t| t.text.as_str()).collect();
    let hints = crate::hints::generate(alphabet, unique.len());
    let mut next = 0usize;
    let mut by_text: HashMap<String, String> = HashMap::new();

    // Bottom-up, left-to-right.
    let mut order: Vec<usize> = (0..targets.len()).collect();
    order.sort_by(|&a, &b| {
        targets[b]
            .line
            .cmp(&targets[a].line)
            .then(targets[a].start.cmp(&targets[b].start))
    });
    let mut keep = vec![true; targets.len()];
    for i in order {
        let text = targets[i].text.clone();
        let hint = if let Some(h) = by_text.get(&text) {
            h.clone()
        } else {
            let Some(h) = hints.get(next) else {
                keep[i] = false;
                continue;
            };
            let h = h.clone();
            if display_width(&h) > display_width(&text) {
                keep[i] = false;
                continue;
            }
            next += 1;
            by_text.insert(text.clone(), h.clone());
            h
        };
        targets[i].hint = hint;
    }
    let mut kept = Vec::with_capacity(targets.len());
    for (i, t) in targets.into_iter().enumerate() {
        if keep[i] {
            kept.push(t);
        }
    }
    kept
}

pub fn display_width(s: &str) -> usize {
    unicode_width::UnicodeWidthStr::width(s)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::builtin_patterns;

    fn all() -> Matcher {
        Matcher::new(&builtin_patterns(&["all".to_string()]).unwrap()).unwrap()
    }

    fn texts(lines: &[&str]) -> Vec<String> {
        let lines: Vec<String> = lines.iter().map(|s| s.to_string()).collect();
        all().find(&lines).into_iter().map(|t| t.text).collect()
    }

    #[test]
    fn url_beats_embedded_path() {
        assert_eq!(
            texts(&["see https://example.com/a/b.html now"]),
            vec!["https://example.com/a/b.html"]
        );
    }

    #[test]
    fn paths_and_hashes() {
        assert_eq!(
            texts(&["/usr/local/bin/tmux and ~/.config/tmux/tmux.conf and deadbeefcafe"]),
            vec![
                "/usr/local/bin/tmux",
                "~/.config/tmux/tmux.conf",
                "deadbeefcafe"
            ]
        );
    }

    #[test]
    fn git_status_captures_only_the_path() {
        let t = all().find(&["\tmodified:   src/main.rs".to_string()]);
        assert_eq!(t.len(), 1);
        assert_eq!(t[0].text, "src/main.rs");
        assert_eq!(t[0].start, 1);
        assert_eq!(t[0].cap_off, "modified:   ".len());
    }

    #[test]
    fn ip_uuid_hex_digits() {
        assert_eq!(
            texts(&["10.0.0.1 0x1f 12345 123e4567-e89b-12d3-a456-426614174000"]),
            vec![
                "10.0.0.1",
                "0x1f",
                "12345",
                "123e4567-e89b-12d3-a456-426614174000"
            ]
        );
    }

    #[test]
    fn tab_expansion() {
        assert_eq!(expand_tabs("a\tb"), "a       b");
        assert_eq!(expand_tabs("12345678\tb"), "12345678        b");
        assert_eq!(expand_tabs("plain"), "plain");
    }

    #[test]
    fn hints_reused_for_same_text_and_bottom_gets_best() {
        let lines = vec![
            "/tmp/one /tmp/two".to_string(),
            "/tmp/one /tmp/three".to_string(),
        ];
        let t = assign_hints(all().find(&lines), &"asdf".chars().collect::<Vec<_>>());
        let hint_for = |text: &str| -> Vec<String> {
            t.iter()
                .filter(|x| x.text == text)
                .map(|x| x.hint.clone())
                .collect()
        };
        assert_eq!(hint_for("/tmp/one"), vec!["a", "a"]);
        assert_eq!(hint_for("/tmp/three"), vec!["s"]);
        assert_eq!(hint_for("/tmp/two"), vec!["d"]);
    }

    #[test]
    fn drops_targets_narrower_than_hint() {
        let lines = vec!["1 2 3 4 5 6 7 8 9 10 11".to_string()];
        let m = Matcher::new(&["\\d+".to_string()]).unwrap();
        let t = assign_hints(m.find(&lines), &"ab".chars().collect::<Vec<_>>());
        // 11 unique numbers over a 2-letter alphabet forces 2-char hints; the
        // single-digit numbers cannot carry a 2-char hint.
        assert!(
            t.iter()
                .all(|x| x.hint.chars().count() <= x.text.chars().count())
        );
    }
}
