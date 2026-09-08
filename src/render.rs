//! Turn captured lines plus targets into one ANSI frame for the hidden pane.

use std::collections::HashSet;

use crate::matcher::{Target, display_width};

const RESET: &str = "\x1b[0m";
pub const CLEAR: &str = "\x1b[H\x1b[J";
pub const HIDE_CURSOR: &str = "\x1b[?25l";

#[derive(Debug, Clone, Default)]
pub struct Styles {
    pub hint: String,
    pub highlight: String,
    pub selected_hint: String,
    pub selected_highlight: String,
    pub backdrop: String,
    pub hint_on_right: bool,
}

pub struct Frame<'a> {
    pub lines: &'a [String],
    pub width: usize,
    pub targets: &'a [Target],
    pub typed: &'a str,
    pub selected: &'a HashSet<String>,
    pub styles: &'a Styles,
}

impl Frame<'_> {
    pub fn render(&self) -> String {
        let mut out = String::with_capacity(self.lines.len() * (self.width + 16));
        out.push_str(CLEAR);
        out.push_str(HIDE_CURSOR);
        let mut targets_by_line: Vec<Vec<&Target>> = vec![Vec::new(); self.lines.len()];
        for t in self.targets {
            if t.line < self.lines.len() {
                targets_by_line[t.line].push(t);
            }
        }
        for (i, line) in self.lines.iter().enumerate() {
            let mut ts = targets_by_line[i].clone();
            ts.sort_by_key(|t| t.start);
            out.push_str(&self.render_line(line, &ts));
            if i + 1 < self.lines.len() {
                out.push_str("\r\n");
            }
        }
        out
    }

    fn render_line(&self, line: &str, targets: &[&Target]) -> String {
        let chars: Vec<char> = line.chars().collect();
        let mut out = String::new();
        let mut visible = 0usize;
        out.push_str(RESET);
        out.push_str(&self.styles.backdrop);
        let mut pos = 0usize;
        for t in targets {
            if t.start < pos {
                continue;
            }
            let plain: String = chars[pos..t.start.min(chars.len())].iter().collect();
            visible += display_width(&plain);
            out.push_str(&plain);
            let end = (t.start + t.len).min(chars.len());
            let seg: Vec<char> = chars[t.start..end].to_vec();
            let (segment, w) = self.render_target(t, &seg);
            out.push_str(&segment);
            visible += w;
            pos = end;
        }
        if pos < chars.len() {
            let rest: String = chars[pos..].iter().collect();
            visible += display_width(&rest);
            out.push_str(&rest);
        }
        if self.width > 0 {
            let rem = visible % self.width;
            if rem != 0 || visible == 0 {
                out.extend(std::iter::repeat_n(' ', self.width - rem));
            }
        }
        out.push_str(RESET);
        out
    }

    /// Render one target's whole-match region. Returns (ansi, visible width).
    fn render_target(&self, t: &Target, seg: &[char]) -> (String, usize) {
        let cap_end = (t.cap_off + t.cap_len()).min(seg.len());
        let before: String = seg[..t.cap_off.min(seg.len())].iter().collect();
        let cap: String = seg[t.cap_off.min(seg.len())..cap_end].iter().collect();
        let after: String = seg[cap_end..].iter().collect();
        let plain_width = display_width(&before) + display_width(&cap) + display_width(&after);

        let active = self.typed.is_empty() || t.hint.starts_with(self.typed);
        if !active {
            let s: String = seg.iter().collect();
            return (s, plain_width);
        }
        let selected = self.selected.contains(&t.hint);
        let (hint_style, hl_style) = if selected {
            (&self.styles.selected_hint, &self.styles.selected_highlight)
        } else {
            (&self.styles.hint, &self.styles.highlight)
        };

        let hint_w = display_width(&t.hint);
        let (shown, pad) = overlay(&cap, hint_w, self.styles.hint_on_right);
        let mut s = String::new();
        s.push_str(&before);
        s.push_str(RESET);
        if self.styles.hint_on_right {
            s.push_str(hl_style);
            s.push_str(&shown);
            s.push_str(RESET);
            s.push_str(hint_style);
            s.push_str(&t.hint);
            s.extend(std::iter::repeat_n(' ', pad));
        } else {
            s.push_str(hint_style);
            s.push_str(&t.hint);
            s.extend(std::iter::repeat_n(' ', pad));
            s.push_str(RESET);
            s.push_str(hl_style);
            s.push_str(&shown);
        }
        s.push_str(RESET);
        s.push_str(&self.styles.backdrop);
        s.push_str(&after);
        (s, plain_width)
    }
}

/// Remove `hint_w` display columns from the start (or end) of `cap`.
/// Returns the remaining text and how many pad spaces are needed if a wide
/// char straddled the cut.
fn overlay(cap: &str, hint_w: usize, from_right: bool) -> (String, usize) {
    let chars: Vec<char> = cap.chars().collect();
    let mut removed = 0usize;
    let mut n = 0usize;
    if from_right {
        while n < chars.len() && removed < hint_w {
            removed +=
                unicode_width::UnicodeWidthChar::width(chars[chars.len() - 1 - n]).unwrap_or(0);
            n += 1;
        }
        let kept: String = chars[..chars.len() - n].iter().collect();
        (kept, removed.saturating_sub(hint_w))
    } else {
        while n < chars.len() && removed < hint_w {
            removed += unicode_width::UnicodeWidthChar::width(chars[n]).unwrap_or(0);
            n += 1;
        }
        let kept: String = chars[n..].iter().collect();
        (kept, removed.saturating_sub(hint_w))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn strip_ansi(s: &str) -> String {
        let re = regex::Regex::new("\x1b\\[[0-9;?]*[A-Za-z]").unwrap();
        re.replace_all(s, "").to_string()
    }

    fn target(line: usize, start: usize, text: &str, hint: &str) -> Target {
        Target {
            line,
            start,
            len: text.chars().count(),
            cap_off: 0,
            text: text.to_string(),
            hint: hint.to_string(),
        }
    }

    #[test]
    fn hint_overlays_start_and_pads_to_width() {
        let lines = vec!["cd /tmp/x".to_string()];
        let targets = vec![target(0, 3, "/tmp/x", "a")];
        let styles = Styles::default();
        let sel = HashSet::new();
        let f = Frame {
            lines: &lines,
            width: 12,
            targets: &targets,
            typed: "",
            selected: &sel,
            styles: &styles,
        };
        let plain = strip_ansi(&f.render());
        assert_eq!(plain, "cd atmp/x   ");
    }

    #[test]
    fn typed_prefix_hides_other_hints() {
        let lines = vec!["/a/b /c/d".to_string()];
        let targets = vec![target(0, 0, "/a/b", "fa"), target(0, 5, "/c/d", "fs")];
        let styles = Styles::default();
        let sel = HashSet::new();
        let f = Frame {
            lines: &lines,
            width: 9,
            targets: &targets,
            typed: "f",
            selected: &sel,
            styles: &styles,
        };
        assert_eq!(strip_ansi(&f.render()), "fa/b fs/d");
        let f2 = Frame { typed: "x", ..f };
        assert_eq!(strip_ansi(&f2.render()), "/a/b /c/d");
    }

    #[test]
    fn right_position_and_wide_chars() {
        let lines = vec!["日本/x".to_string()];
        let targets = vec![target(0, 0, "日本/x", "a")];
        let styles = Styles {
            hint_on_right: true,
            ..Default::default()
        };
        let sel = HashSet::new();
        let f = Frame {
            lines: &lines,
            width: 6,
            targets: &targets,
            typed: "",
            selected: &sel,
            styles: &styles,
        };
        assert_eq!(strip_ansi(&f.render()), "日本/a");
        let styles = Styles::default();
        let f = Frame {
            styles: &styles,
            ..f
        };
        // Removing a 2-wide char for a 1-wide hint leaves one pad space.
        assert_eq!(strip_ansi(&f.render()), "a 本/x");
    }

    #[test]
    fn multiline_uses_crlf_and_no_trailing_newline() {
        let lines = vec!["a".to_string(), "b".to_string()];
        let styles = Styles::default();
        let sel = HashSet::new();
        let f = Frame {
            lines: &lines,
            width: 2,
            targets: &[],
            typed: "",
            selected: &sel,
            styles: &styles,
        };
        let out = strip_ansi(&f.render());
        assert_eq!(out, "a \r\nb ");
    }
}
