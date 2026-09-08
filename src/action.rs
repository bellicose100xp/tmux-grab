//! What happens once a hint has been picked.

use std::io::Write;
use std::process::{Command, Stdio};

use anyhow::{Context, Result};

use crate::config::Config;
use crate::tmux::{PaneInfo, Tmux};

pub struct Action<'a> {
    pub tmux: &'a Tmux,
    pub cfg: &'a Config,
    pub pane: &'a PaneInfo,
    pub text: &'a str,
    pub hint: &'a str,
    pub modifier: &'a str,
}

impl Action<'_> {
    pub fn run(&self) -> Result<()> {
        self.tmux
            .set_buffer(self.text, self.cfg.use_system_clipboard)?;
        let action = self.cfg.action_for(self.modifier);
        match action {
            "" => Ok(()),
            ":copy:" => self.system_copy(),
            ":open:" => self.open(),
            ":paste:" => self.tmux.paste_buffer(self.pane),
            custom => self.shell(custom),
        }
    }

    fn system_copy(&self) -> Result<()> {
        if !self.cfg.use_system_clipboard {
            return Ok(());
        }
        let Some(cmd) = system_copy_command() else {
            return Ok(());
        };
        self.pipe(&cmd, self.text.as_bytes())
    }

    fn open(&self) -> Result<()> {
        let opener = if which("xdg-open") {
            "xdg-open"
        } else if which("open") {
            "open"
        } else if which("cygstart") {
            "cygstart"
        } else {
            return Ok(());
        };
        let target = expand_home(self.text);
        Command::new(opener)
            .arg(&target)
            .current_dir(self.cwd())
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(log_file())
            .spawn()
            .with_context(|| format!("failed to run {opener}"))?;
        Ok(())
    }

    fn shell(&self, script: &str) -> Result<()> {
        let mut child = Command::new("sh")
            .arg("-c")
            .arg(script)
            .current_dir(self.cwd())
            .env("MODIFIER", self.modifier)
            .env("HINT", self.hint)
            .env("GRAB_TEXT", self.text)
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(log_file())
            .spawn()
            .with_context(|| format!("failed to run action: {script}"))?;
        if let Some(mut stdin) = child.stdin.take() {
            let _ = stdin.write_all(self.text.as_bytes());
        }
        // Detach: the action may be long-running (an editor, a browser).
        std::thread::spawn(move || {
            let _ = child.wait();
        });
        Ok(())
    }

    fn pipe(&self, cmd: &str, input: &[u8]) -> Result<()> {
        let mut child = Command::new("sh")
            .arg("-c")
            .arg(cmd)
            .current_dir(self.cwd())
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(log_file())
            .spawn()
            .with_context(|| format!("failed to run {cmd}"))?;
        if let Some(mut stdin) = child.stdin.take() {
            stdin.write_all(input)?;
        }
        child.wait()?;
        Ok(())
    }

    fn cwd(&self) -> std::path::PathBuf {
        let p = std::path::PathBuf::from(&self.pane.current_path);
        if p.is_dir() { p } else { std::env::temp_dir() }
    }
}

fn expand_home(s: &str) -> String {
    if let Some(rest) = s.strip_prefix("~/")
        && let Ok(home) = std::env::var("HOME")
    {
        return format!("{home}/{rest}");
    }
    s.to_string()
}

fn which(bin: &str) -> bool {
    std::env::var_os("PATH")
        .map(|paths| {
            std::env::split_paths(&paths).any(|dir| {
                let p = dir.join(bin);
                p.is_file()
            })
        })
        .unwrap_or(false)
}

/// First available system clipboard writer.
pub fn system_copy_command() -> Option<String> {
    if which("pbcopy") {
        if which("reattach-to-user-namespace") {
            return Some("reattach-to-user-namespace pbcopy".into());
        }
        return Some("pbcopy".into());
    }
    if which("clip.exe") {
        return Some("clip.exe".into());
    }
    if which("wl-copy") && std::env::var_os("WAYLAND_DISPLAY").is_some() {
        return Some("wl-copy".into());
    }
    if which("xclip") && std::env::var_os("DISPLAY").is_some() {
        return Some("xclip -selection clipboard".into());
    }
    if which("xsel") && std::env::var_os("DISPLAY").is_some() {
        return Some("xsel -i --clipboard".into());
    }
    if which("putclip") {
        return Some("putclip".into());
    }
    None
}

fn log_file() -> Stdio {
    let path = crate::config::log_path();
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    match std::fs::OpenOptions::new()
        .append(true)
        .create(true)
        .open(&path)
    {
        Ok(f) => Stdio::from(f),
        Err(_) => Stdio::null(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn expands_tilde_only_at_start() {
        unsafe { std::env::set_var("HOME", "/home/me") };
        assert_eq!(expand_home("~/x/y"), "/home/me/x/y");
        assert_eq!(expand_home("/a/~/b"), "/a/~/b");
        assert_eq!(expand_home("~x"), "~x");
    }
}
