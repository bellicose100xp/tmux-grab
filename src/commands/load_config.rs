//! `tmux-grab load-config`: validate `@grab-*` options and install bindings.

use anyhow::{Context, Result, bail};

use crate::config::{Config, RESERVED_KEYS};
use crate::input;
use crate::tmux::{Tmux, shell_quote};

const EXIT_KEYS: &[&str] = &["q", "Escape", "C-c"];

pub fn run() -> Result<()> {
    let tmux = Tmux::new();
    let version = tmux.version().context("is tmux running?")?;
    if !version_at_least(&version, 3, 2) {
        bail!("tmux-grab needs tmux >= 3.2, found {version}");
    }

    let cfg = match Config::load(&tmux) {
        Ok(c) => c,
        Err(errors) => {
            eprintln!("[tmux-grab] problems in tmux.conf:");
            for e in &errors {
                eprintln!("  - {e}");
            }
            let _ = tmux.display(
                &format!("[tmux-grab] {} config error(s), see log", errors.len()),
                5000,
            );
            std::process::exit(1);
        }
    };

    let cli = std::env::current_exe().context("cannot locate own executable")?;
    let cli = cli.to_string_lossy().into_owned();
    let socket = input::socket_path_for(&tmux.socket_path()?);
    let socket = socket.to_string_lossy().into_owned();
    let log = crate::config::log_path();
    if let Some(parent) = log.parent() {
        std::fs::create_dir_all(parent).ok();
    }
    let log = log.to_string_lossy().into_owned();

    let mut cmds: Vec<Vec<String>> = Vec::new();

    if cfg.enable_bindings {
        cmds.push(bind_cmd(
            Background::Yes,
            None,
            &cfg.key,
            &format!(
                "{} start '#{{pane_id}}' >>{} 2>&1",
                shell_quote(&cli),
                shell_quote(&log)
            ),
        ));
    }

    let send = |msg: &str| -> String {
        format!(
            "{} send-input {} {}",
            shell_quote(&cli),
            shell_quote(&socket),
            msg
        )
    };

    for c in 'a'..='z' {
        if RESERVED_KEYS.contains(&c) {
            continue;
        }
        let upper = c.to_ascii_uppercase();
        cmds.push(bind_cmd(
            Background::No,
            Some("grab"),
            &c.to_string(),
            &send(&format!("hint:{c}:main")),
        ));
        cmds.push(bind_cmd(
            Background::No,
            Some("grab"),
            &upper.to_string(),
            &send(&format!("hint:{c}:shift")),
        ));
        cmds.push(bind_cmd(
            Background::No,
            Some("grab"),
            &format!("C-{c}"),
            &send(&format!("hint:{c}:ctrl")),
        ));
        cmds.push(bind_cmd(
            Background::No,
            Some("grab"),
            &format!("M-{c}"),
            &send(&format!("hint:{c}:alt")),
        ));
    }
    for k in EXIT_KEYS {
        cmds.push(bind_cmd(Background::No, Some("grab"), k, &send("exit")));
    }
    cmds.push(bind_cmd(
        Background::No,
        Some("grab"),
        "Tab",
        &send("toggle-multi"),
    ));
    cmds.push(bind_cmd(
        Background::No,
        Some("grab"),
        "BSpace",
        &send("backspace"),
    ));
    cmds.push(bind_cmd(
        Background::No,
        Some("grab"),
        "Enter",
        &send("noop"),
    ));
    cmds.push(vec![
        "bind-key".into(),
        "-T".into(),
        "grab".into(),
        "Any".into(),
        "display-message".into(),
        "-d".into(),
        "1".into(),
        "".into(),
    ]);
    cmds.push(vec![
        "set-option".into(),
        "-g".into(),
        "@grab-cli".into(),
        cli.clone(),
    ]);

    tmux.run_batch(&cmds)
        .context("installing tmux-grab key bindings")?;
    Ok(())
}

/// Whether the bound command runs detached.
///
/// Grab mode itself is detached: it lives for as long as the overlay is up, and
/// blocking the tmux server on it would freeze tmux. Key delivery is the
/// opposite. A detached `run-shell` per keystroke means one key can overtake
/// the key before it, so Tab followed quickly by a hint could arrive in the
/// wrong order. Running delivery in the foreground makes the server finish one
/// keystroke before it reads the next, which costs nothing because the command
/// only writes a line to a socket.
#[derive(Clone, Copy)]
enum Background {
    Yes,
    No,
}

fn bind_cmd(background: Background, table: Option<&str>, key: &str, shell: &str) -> Vec<String> {
    let mut v = vec!["bind-key".to_string()];
    if let Some(t) = table {
        v.push("-T".into());
        v.push(t.into());
    }
    v.push(key.into());
    v.push("run-shell".into());
    if matches!(background, Background::Yes) {
        v.push("-b".into());
    }
    v.push(shell.into());
    v
}

pub fn version_at_least(version: &str, major: u32, minor: u32) -> bool {
    let v = version.trim().trim_start_matches("tmux ");
    if v.starts_with("next") || v.starts_with("master") {
        return true;
    }
    let mut it = v.split('.');
    let maj: u32 = it
        .next()
        .and_then(|s| s.trim_matches(|c: char| !c.is_ascii_digit()).parse().ok())
        .unwrap_or(0);
    let min: u32 = it
        .next()
        .map(|s| {
            s.chars()
                .take_while(|c| c.is_ascii_digit())
                .collect::<String>()
        })
        .and_then(|s| s.parse().ok())
        .unwrap_or(0);
    (maj, min) >= (major, minor)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_parsing() {
        assert!(version_at_least("3.6a", 3, 2));
        assert!(version_at_least("3.2", 3, 2));
        assert!(version_at_least("tmux 3.3a", 3, 2));
        assert!(version_at_least("next-3.7", 3, 2));
        assert!(!version_at_least("3.1c", 3, 2));
        assert!(!version_at_least("2.9a", 3, 2));
    }
}
