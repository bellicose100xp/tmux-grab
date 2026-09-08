//! `tmux-grab start <pane>`: show hints over a pane and act on the pick.

use std::collections::HashSet;
use std::fs::File;
use std::io::Write;
use std::path::PathBuf;

use anyhow::{Context, Result, bail};

use crate::action::Action;
use crate::config::Config;
use crate::input::{self, InputServer};
use crate::matcher::{Matcher, Target, assign_hints, expand_tabs};
use crate::render::{Frame, Styles};
use crate::tmux::{HiddenWindow, PaneInfo, Tmux};

pub struct StartOpts {
    pub pane: String,
    pub patterns: Option<String>,
    pub main_action: Option<String>,
    pub ctrl_action: Option<String>,
    pub shift_action: Option<String>,
    pub alt_action: Option<String>,
}

#[derive(Default)]
struct State {
    typed: String,
    modifier: String,
    multi: bool,
    selected_hints: HashSet<String>,
    picked: Vec<String>,
    result: Option<(String, String)>,
    exiting: bool,
}

pub fn run(opts: StartOpts) -> Result<()> {
    let tmux = Tmux::new();
    if let Err(e) = run_inner(&tmux, opts) {
        let _ = tmux.display(&format!("[tmux-grab] {e}"), 4000);
        return Err(e);
    }
    Ok(())
}

fn run_inner(tmux: &Tmux, opts: StartOpts) -> Result<()> {
    let mut cfg = match Config::load(tmux) {
        Ok(c) => c,
        Err(errors) => bail!("{}", errors.join("; ")),
    };
    if let Some(a) = opts.main_action {
        cfg.main_action = a;
    }
    if let Some(a) = opts.ctrl_action {
        cfg.ctrl_action = a;
    }
    if let Some(a) = opts.shift_action {
        cfg.shift_action = a;
    }
    if let Some(a) = opts.alt_action {
        cfg.alt_action = a;
    }

    let pane = tmux.pane_info(&opts.pane)?;
    let regexes = cfg.pattern_regexes(opts.patterns.as_deref())?;
    let matcher = Matcher::new(&regexes)?;

    let raw_lines = tmux.capture_pane(&pane)?;
    let lines: Vec<String> = raw_lines.iter().map(|l| expand_tabs(l)).collect();
    let targets = assign_hints(matcher.find(&lines), &cfg.alphabet);
    if targets.is_empty() {
        tmux.display("[tmux-grab] nothing to grab on this screen", 1200)?;
        return Ok(());
    }

    let socket = input::socket_path_for(&pane.socket_path);
    let server = InputServer::bind(&socket)?;

    let mut session = Session::open(tmux, &pane, &lines, targets, &cfg.styles)?;
    let outcome = session.interact(&server, &cfg.alphabet);
    let restored = session.close();
    let state = outcome?;
    restored?;

    if let Some((text, modifier)) = state.result {
        Action {
            tmux,
            cfg: &cfg,
            pane: &pane,
            text: &text,
            hint: &state.typed,
            modifier: &modifier,
        }
        .run()?;
        if cfg.show_copied_notification {
            let shown: String = text.chars().take(60).collect();
            let _ = tmux.display(&format!("Copied: {shown}"), 1000);
        }
    }
    Ok(())
}

/// The hidden window swapped in over the target pane, plus everything needed
/// to put things back. `close` is idempotent so error paths can call it too.
struct Session<'a> {
    tmux: &'a Tmux,
    pane: &'a PaneInfo,
    window: HiddenWindow,
    tty: File,
    lines: &'a [String],
    targets: Vec<Target>,
    styles: &'a Styles,
    previous_key_table: String,
    open: bool,
}

impl<'a> Session<'a> {
    fn open(
        tmux: &'a Tmux,
        pane: &'a PaneInfo,
        lines: &'a [String],
        targets: Vec<Target>,
        styles: &'a Styles,
    ) -> Result<Self> {
        let window = tmux.create_hidden_window(pane, "[grab]")?;
        let tty = File::options()
            .write(true)
            .open(&window.tty)
            .with_context(|| format!("cannot open {}", window.tty))?;
        let previous_key_table = tmux.window_key_table(&pane.window_id)?;
        let mut s = Self {
            tmux,
            pane,
            window,
            tty,
            lines,
            targets,
            styles,
            previous_key_table,
            open: true,
        };
        let state = State::default();
        // Draw before swapping so the user never sees an empty pane. A zoomed
        // pane is the exception: drawing after the swap avoids a size mismatch.
        if pane.zoomed {
            s.swap_in()?;
            s.draw(&state)?;
        } else {
            s.draw(&state)?;
            s.swap_in()?;
        }
        tmux.enter_grab_mode(pane)?;
        Ok(s)
    }

    fn swap_in(&self) -> Result<()> {
        self.tmux
            .swap_panes(&self.window.pane_id, &self.pane.pane_id)
    }

    fn draw(&mut self, state: &State) -> Result<()> {
        let frame = Frame {
            lines: self.lines,
            width: self.pane.width,
            targets: &self.targets,
            typed: &state.typed,
            selected: &state.selected_hints,
            styles: self.styles,
        }
        .render();
        self.tty.write_all(frame.as_bytes())?;
        self.tty.flush()?;
        Ok(())
    }

    fn interact(&mut self, server: &InputServer, alphabet: &[char]) -> Result<State> {
        let mut state = State::default();
        while !state.exiting {
            let msg = server.recv()?;
            let mut parts = msg.split(':');
            match parts.next().unwrap_or("") {
                "hint" => {
                    let ch = parts.next().unwrap_or("");
                    let modifier = parts.next().unwrap_or("main");
                    if ch.chars().count() != 1 || !alphabet.contains(&ch.chars().next().unwrap()) {
                        continue;
                    }
                    state.typed.push_str(ch);
                    state.modifier = modifier.to_string();
                    self.on_typed(&mut state)?;
                }
                "backspace" => {
                    state.typed.pop();
                    self.draw(&state)?;
                }
                "toggle-multi" => {
                    if state.multi {
                        state.multi = false;
                        if !state.picked.is_empty() {
                            let modifier = if state.modifier.is_empty() {
                                "main".to_string()
                            } else {
                                state.modifier.clone()
                            };
                            state.result = Some((state.picked.join(" "), modifier));
                        }
                        state.exiting = true;
                    } else {
                        state.multi = true;
                        state.typed.clear();
                        self.draw(&state)?;
                    }
                }
                "exit" => state.exiting = true,
                _ => {}
            }
        }
        Ok(state)
    }

    fn on_typed(&mut self, state: &mut State) -> Result<()> {
        let exact = self
            .targets
            .iter()
            .find(|t| t.hint == state.typed)
            .map(|t| t.text.clone());
        if let Some(text) = exact {
            if state.multi {
                if !state.selected_hints.contains(&state.typed) {
                    state.selected_hints.insert(state.typed.clone());
                    state.picked.push(text);
                }
                state.typed.clear();
                self.draw(state)?;
            } else {
                state.result = Some((text, state.modifier.clone()));
                state.exiting = true;
            }
            return Ok(());
        }
        let is_prefix = self
            .targets
            .iter()
            .any(|t| t.hint.starts_with(&state.typed));
        if !is_prefix {
            state.typed.clear();
        }
        self.draw(state)
    }

    /// Swap the real pane back, remove the scratch window, restore key tables
    /// and the prefix. Safe to call more than once.
    fn close(&mut self) -> Result<()> {
        if !self.open {
            return Ok(());
        }
        self.open = false;
        let mut first_err: Option<anyhow::Error> = None;
        let mut record = |r: Result<()>| {
            if let Err(e) = r
                && first_err.is_none()
            {
                first_err = Some(e);
            }
        };
        record(
            self.tmux
                .swap_panes(&self.window.pane_id, &self.pane.pane_id),
        );
        record(self.tmux.kill_pane(&self.window.pane_id));
        if self.pane.active {
            record(self.tmux.select_pane(&self.pane.pane_id));
        }
        record(
            self.tmux
                .leave_grab_mode(self.pane, &self.previous_key_table),
        );
        match first_err {
            Some(e) => Err(e),
            None => Ok(()),
        }
    }
}

impl Drop for Session<'_> {
    fn drop(&mut self) {
        let _ = self.close();
    }
}

pub fn send_input(socket: PathBuf, message: String) -> Result<()> {
    input::send(&socket, &message)
}
