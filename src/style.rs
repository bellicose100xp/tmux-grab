//! Translate a tmux style string ("bg=yellow,fg=black,bold") into ANSI SGR
//! escape sequences we can write straight into the hidden pane's tty.

use anyhow::{Result, bail};

const RESET: &str = "\x1b[0m";

/// Parse a tmux-style string into an ANSI escape sequence.
///
/// Accepts `fg=`/`bg=` with named colours, `brightX`, `colourN`/`colorN`,
/// `#rrggbb`, `default`, and the attributes bold/bright, dim, underscore,
/// italics, reverse (optionally prefixed with `no` to switch off).
/// An empty string yields an empty sequence.
pub fn to_ansi(style: &str) -> Result<String> {
    let mut out = String::new();
    for token in style
        .split([' ', ','])
        .map(str::trim)
        .filter(|t| !t.is_empty())
    {
        if let Some(rest) = token.strip_prefix("fg=") {
            out.push_str(&colour_seq(rest, 30, token)?);
        } else if let Some(rest) = token.strip_prefix("bg=") {
            out.push_str(&colour_seq(rest, 40, token)?);
        } else {
            out.push_str(attr_seq(token)?);
        }
    }
    Ok(out)
}

fn colour_seq(colour: &str, base: u8, token: &str) -> Result<String> {
    let named = |idx: u8, bright: bool| -> String {
        let code = if bright { base + 60 + idx } else { base + idx };
        format!("\x1b[{code}m")
    };

    if colour == "default" {
        return Ok(format!("\x1b[{}m", base + 9));
    }
    if colour == "terminal" {
        return Ok(String::new());
    }
    if let Some(n) = colour
        .strip_prefix("colour")
        .or_else(|| colour.strip_prefix("color"))
    {
        let Ok(n) = n.parse::<u8>() else {
            bail!("invalid colour definition: {token}");
        };
        return Ok(format!("\x1b[{};5;{}m", base + 8, n));
    }
    if let Some(hex) = colour.strip_prefix('#') {
        if hex.len() != 6 || !hex.chars().all(|c| c.is_ascii_hexdigit()) {
            bail!("invalid colour definition: {token}");
        }
        let r = u8::from_str_radix(&hex[0..2], 16)?;
        let g = u8::from_str_radix(&hex[2..4], 16)?;
        let b = u8::from_str_radix(&hex[4..6], 16)?;
        return Ok(format!("\x1b[{};2;{};{};{}m", base + 8, r, g, b));
    }
    let (name, bright) = match colour.strip_prefix("bright") {
        Some(rest) => (rest, true),
        None => (colour, false),
    };
    let idx = match name {
        "black" => 0,
        "red" => 1,
        "green" => 2,
        "yellow" => 3,
        "blue" => 4,
        "magenta" => 5,
        "cyan" => 6,
        "white" => 7,
        _ => bail!("invalid colour definition: {token}"),
    };
    Ok(named(idx, bright))
}

fn attr_seq(token: &str) -> Result<&'static str> {
    Ok(match token {
        "none" | "default" => RESET,
        "bold" | "bright" => "\x1b[1m",
        "dim" => "\x1b[2m",
        "italics" => "\x1b[3m",
        "underscore" => "\x1b[4m",
        "reverse" => "\x1b[7m",
        "nobold" | "nobright" => "\x1b[22m",
        "nodim" => "\x1b[22m",
        "noitalics" => "\x1b[23m",
        "nounderscore" => "\x1b[24m",
        "noreverse" => "\x1b[27m",
        _ => bail!("invalid style definition: {token}"),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn named_colours_and_attributes() {
        assert_eq!(
            to_ansi("bg=yellow,fg=black,bold").unwrap(),
            "\x1b[43m\x1b[30m\x1b[1m"
        );
    }

    #[test]
    fn extended_colours() {
        assert_eq!(to_ansi("fg=colour123").unwrap(), "\x1b[38;5;123m");
        assert_eq!(to_ansi("bg=color7").unwrap(), "\x1b[48;5;7m");
        assert_eq!(to_ansi("fg=#ff8800").unwrap(), "\x1b[38;2;255;136;0m");
        assert_eq!(to_ansi("fg=brightred").unwrap(), "\x1b[91m");
        assert_eq!(to_ansi("bg=default").unwrap(), "\x1b[49m");
    }

    #[test]
    fn space_separated_and_empty() {
        assert_eq!(to_ansi("dim underscore").unwrap(), "\x1b[2m\x1b[4m");
        assert_eq!(to_ansi("").unwrap(), "");
    }

    #[test]
    fn rejects_garbage() {
        assert!(to_ansi("fg=purple").is_err());
        assert!(to_ansi("blink").is_err());
        assert!(to_ansi("fg=#12").is_err());
    }
}
