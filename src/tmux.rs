//! Thin wrapper over the `tmux` binary. Every call spawns `tmux`; the `TMUX`
//! environment variable set by tmux for run-shell commands makes it target
//! the right server.

use std::collections::BTreeMap;
use std::io::Write;
use std::process::{Command, Stdio};

use anyhow::{Context, Result, bail};

/// Field delimiter for `display-message -F` output.
///
/// It has to be printable: some tmux versions render non-printable characters
/// in printed command output as octal escapes, so a control character comes
/// back as the four characters `\037` and the output cannot be split on it.
/// Free-form values such as paths are fetched with their own call instead of
/// being packed into a delimited line.
const SEP: &str = "@@tmux-grab@@";

#[derive(Debug, Clone)]
pub struct Tmux {
    bin: String,
}

#[derive(Debug, Clone)]
pub struct PaneInfo {
    pub pane_id: String,
    pub window_id: String,
    pub session_id: String,
    pub width: usize,
    pub height: usize,
    pub current_path: String,
    pub in_mode: bool,
    pub scroll_position: Option<usize>,
    pub zoomed: bool,
    pub active: bool,
    /// Client that issued the command, if tmux could tell.
    pub client: String,
    pub prefix: String,
    pub prefix2: String,
    pub socket_path: String,
}

#[derive(Debug, Clone)]
pub struct HiddenWindow {
    pub window_id: String,
    pub pane_id: String,
    pub tty: String,
}

impl Default for Tmux {
    fn default() -> Self {
        Self::new()
    }
}

impl Tmux {
    pub fn new() -> Self {
        Self {
            bin: std::env::var("TMUX_GRAB_TMUX_BIN").unwrap_or_else(|_| "tmux".into()),
        }
    }

    pub fn run<I, S>(&self, args: I) -> Result<String>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<std::ffi::OsStr>,
    {
        let args: Vec<std::ffi::OsString> =
            args.into_iter().map(|a| a.as_ref().to_owned()).collect();
        let out = Command::new(&self.bin)
            .args(&args)
            .stdin(Stdio::null())
            .output()
            .with_context(|| format!("failed to spawn {}", self.bin))?;
        if !out.status.success() {
            let shown: String = args
                .iter()
                .map(|a| a.to_string_lossy().into_owned())
                .collect::<Vec<_>>()
                .join(" ")
                .chars()
                .take(160)
                .collect();
            bail!(
                "tmux {shown} failed: {}",
                String::from_utf8_lossy(&out.stderr).trim()
            );
        }
        Ok(String::from_utf8_lossy(&out.stdout).into_owned())
    }

    /// Run several tmux commands with as few processes as possible, separated
    /// by `;`. tmux caps a single command line, so batches are chunked.
    pub fn run_batch(&self, cmds: &[Vec<String>]) -> Result<()> {
        const MAX_BYTES: usize = 6000;
        let mut args: Vec<String> = Vec::new();
        let mut bytes = 0usize;
        for c in cmds {
            let size: usize = c.iter().map(|a| a.len() + 1).sum();
            if !args.is_empty() && bytes + size > MAX_BYTES {
                self.run(&args)?;
                args.clear();
                bytes = 0;
            }
            if !args.is_empty() {
                args.push(";".into());
            }
            args.extend(c.iter().cloned());
            bytes += size;
        }
        if !args.is_empty() {
            self.run(&args)?;
        }
        Ok(())
    }

    pub fn version(&self) -> Result<String> {
        let v = self.run(["-V"])?;
        Ok(v.trim().trim_start_matches("tmux ").to_string())
    }

    /// Everything about the target pane plus the calling client.
    pub fn pane_info(&self, target: &str) -> Result<PaneInfo> {
        let fields = [
            "#{pane_id}",
            "#{window_id}",
            "#{session_id}",
            "#{pane_width}",
            "#{pane_height}",
            "#{pane_in_mode}",
            "#{scroll_position}",
            "#{window_zoomed_flag}",
            "#{pane_active}",
            "#{client_name}",
            "#{prefix}",
            "#{prefix2}",
        ];
        let fmt = fields.join(SEP);
        let out = self.run(["display-message", "-p", "-t", target, "-F", &fmt])?;
        let out = out.trim_end_matches('\n');
        let parts: Vec<&str> = out.split(SEP).collect();
        if parts.len() != fields.len() {
            bail!(
                "display-message for pane {target} gave {} fields, expected {}: {out:?}",
                parts.len(),
                fields.len()
            );
        }
        Ok(PaneInfo {
            pane_id: parts[0].to_string(),
            window_id: parts[1].to_string(),
            session_id: parts[2].to_string(),
            width: parts[3].parse().context("pane_width")?,
            height: parts[4].parse().context("pane_height")?,
            in_mode: parts[5] == "1",
            scroll_position: parts[6].parse().ok(),
            zoomed: parts[7] == "1",
            active: parts[8] == "1",
            client: parts[9].to_string(),
            prefix: parts[10].to_string(),
            prefix2: parts[11].to_string(),
            current_path: self.pane_field(target, "#{pane_current_path}")?,
            socket_path: self.socket_path()?,
        })
    }

    /// One format field on its own, for values that can contain anything.
    fn pane_field(&self, target: &str, format: &str) -> Result<String> {
        Ok(self
            .run(["display-message", "-p", "-t", target, "-F", format])?
            .trim_end_matches('\n')
            .to_string())
    }

    pub fn socket_path(&self) -> Result<String> {
        Ok(self
            .run(["display-message", "-p", "#{socket_path}"])?
            .trim()
            .to_string())
    }

    /// Visible text of the pane, wrapped lines joined back together.
    pub fn capture_pane(&self, pane: &PaneInfo) -> Result<Vec<String>> {
        let mut args = vec![
            "capture-pane".to_string(),
            "-p".into(),
            "-J".into(),
            "-t".into(),
            pane.pane_id.clone(),
        ];
        if pane.in_mode
            && let Some(sp) = pane.scroll_position
        {
            let start = -(sp as i64);
            let end = pane.height as i64 - sp as i64 - 1;
            args.extend(["-S".into(), start.to_string(), "-E".into(), end.to_string()]);
        }
        let out = self.run(&args)?;
        let mut lines: Vec<String> = out.split('\n').map(str::to_string).collect();
        if lines.last().is_some_and(|l| l.is_empty()) {
            lines.pop();
        }
        Ok(lines)
    }

    /// Create a detached window running `cat` to draw into, sized like the pane.
    pub fn create_hidden_window(&self, pane: &PaneInfo, name: &str) -> Result<HiddenWindow> {
        let fmt = format!("#{{window_id}}{SEP}#{{pane_id}}");
        let target = format!("{}:", pane.session_id);
        let out = self.run([
            "new-window",
            "-d",
            "-P",
            "-t",
            &target,
            "-n",
            name,
            "-F",
            &fmt,
            "cat",
        ])?;
        let out = out.trim();
        let parts: Vec<&str> = out.split(SEP).collect();
        if parts.len() != 2 {
            bail!("unexpected new-window output: {out:?}");
        }
        let window_id = parts[0].to_string();
        let pane_id = parts[1].to_string();
        let tty = self.pane_field(&pane_id, "#{pane_tty}")?;
        let win = HiddenWindow {
            window_id,
            pane_id,
            tty,
        };
        self.run([
            "resize-window",
            "-t",
            &win.window_id,
            "-x",
            &pane.width.to_string(),
            "-y",
            &pane.height.to_string(),
        ])?;
        Ok(win)
    }

    pub fn swap_panes(&self, src: &str, dst: &str) -> Result<()> {
        self.run(["swap-pane", "-d", "-Z", "-s", src, "-t", dst])
            .map(|_| ())
    }

    pub fn kill_pane(&self, id: &str) -> Result<()> {
        self.run(["kill-pane", "-t", id]).map(|_| ())
    }

    pub fn select_pane(&self, id: &str) -> Result<()> {
        self.run(["select-pane", "-Z", "-t", id]).map(|_| ())
    }

    pub fn window_key_table(&self, window_id: &str) -> Result<String> {
        Ok(self
            .run(["show-options", "-wqv", "-t", window_id, "key-table"])?
            .trim()
            .to_string())
    }

    /// Route all keys for the window to the `grab` table and disable the
    /// prefix so it cannot steal keystrokes.
    pub fn enter_grab_mode(&self, pane: &PaneInfo) -> Result<()> {
        let mut cmds = vec![
            vec![
                "set-option".to_string(),
                "-w".into(),
                "-t".into(),
                pane.window_id.clone(),
                "key-table".into(),
                "grab".into(),
            ],
            vec![
                "set-option".into(),
                "-g".into(),
                "prefix".into(),
                "None".into(),
            ],
            vec![
                "set-option".into(),
                "-g".into(),
                "prefix2".into(),
                "None".into(),
            ],
        ];
        if !pane.client.is_empty() {
            cmds.push(vec![
                "switch-client".into(),
                "-c".into(),
                pane.client.clone(),
                "-T".into(),
                "grab".into(),
            ]);
        }
        self.run_batch(&cmds)
    }

    pub fn leave_grab_mode(&self, pane: &PaneInfo, previous_key_table: &str) -> Result<()> {
        let key_table_cmd = if previous_key_table.is_empty() {
            vec![
                "set-option".to_string(),
                "-w".into(),
                "-u".into(),
                "-t".into(),
                pane.window_id.clone(),
                "key-table".into(),
            ]
        } else {
            vec![
                "set-option".to_string(),
                "-w".into(),
                "-t".into(),
                pane.window_id.clone(),
                "key-table".into(),
                previous_key_table.to_string(),
            ]
        };
        let mut cmds = vec![
            key_table_cmd,
            vec![
                "set-option".into(),
                "-g".into(),
                "prefix".into(),
                pane.prefix.clone(),
            ],
            vec![
                "set-option".into(),
                "-g".into(),
                "prefix2".into(),
                pane.prefix2.clone(),
            ],
        ];
        if !pane.client.is_empty() {
            cmds.push(vec![
                "switch-client".into(),
                "-c".into(),
                pane.client.clone(),
                "-T".into(),
                "root".into(),
            ]);
        }
        self.run_batch(&cmds)
    }

    /// Put text in the tmux paste buffer; `-w` also forwards it to the
    /// terminal clipboard (OSC 52) when `set-clipboard` allows.
    pub fn set_buffer(&self, text: &str, to_clipboard: bool) -> Result<()> {
        let mut cmd = Command::new(&self.bin);
        cmd.arg("load-buffer");
        if to_clipboard {
            cmd.arg("-w");
        }
        cmd.arg("-");
        let mut child = cmd
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .spawn()
            .context("spawn tmux load-buffer")?;
        child
            .stdin
            .take()
            .expect("piped stdin")
            .write_all(text.as_bytes())?;
        let out = child.wait_with_output()?;
        if !out.status.success() {
            bail!(
                "tmux load-buffer failed: {}",
                String::from_utf8_lossy(&out.stderr).trim()
            );
        }
        Ok(())
    }

    pub fn paste_buffer(&self, pane: &PaneInfo) -> Result<()> {
        if pane.in_mode {
            self.run_batch(&[
                vec![
                    "send-keys".into(),
                    "-t".into(),
                    pane.pane_id.clone(),
                    "-X".into(),
                    "cancel".into(),
                ],
                vec!["paste-buffer".into(), "-t".into(), pane.pane_id.clone()],
            ])
        } else {
            self.run(["paste-buffer", "-t", &pane.pane_id]).map(|_| ())
        }
    }

    pub fn display(&self, msg: &str, millis: u32) -> Result<()> {
        self.run(["display-message", "-d", &millis.to_string(), msg])
            .map(|_| ())
    }

    /// All `@grab-*` global options and their raw values.
    pub fn grab_options(&self) -> Result<BTreeMap<String, String>> {
        let listing = self.run(["show-options", "-g"])?;
        let mut out = BTreeMap::new();
        for line in listing.lines() {
            let name = line.split_whitespace().next().unwrap_or("");
            if !name.starts_with(crate::config::OPTION_PREFIX) {
                continue;
            }
            let value = self.run(["show-options", "-gqv", name])?;
            out.insert(name.to_string(), value.trim_end_matches('\n').to_string());
        }
        Ok(out)
    }
}

/// Quote a string for inclusion inside a double-quoted tmux command string.
pub fn shell_quote(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('\'');
    for ch in s.chars() {
        if ch == '\'' {
            out.push_str("'\\''");
        } else {
            out.push(ch);
        }
    }
    out.push('\'');
    out
}
