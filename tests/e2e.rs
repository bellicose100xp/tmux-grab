//! End-to-end tests against a private tmux server with a real attached client.
//!
//! The client is attached through `script`, which allocates a pty, so keys we
//! write to its stdin go through tmux key tables exactly like a user typing.
//! Tests are skipped when `tmux` or `script` is missing.

use std::io::Write;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

const BIN: &str = env!("CARGO_BIN_EXE_tmux-grab");

struct Server {
    name: String,
    socket_path: String,
    client: Option<Child>,
}

impl Server {
    fn start(test: &str) -> Option<Self> {
        if !have("tmux") || !have("script") {
            eprintln!("skipping: tmux or script not installed");
            return None;
        }
        let name = format!("tmux-grab-e2e-{}-{}", test, std::process::id());
        let _ = Command::new("tmux")
            .args(["-L", &name, "kill-server"])
            .output();
        let ok = Command::new("tmux")
            .args([
                "-L",
                &name,
                "-f",
                "/dev/null",
                "new-session",
                "-d",
                "-s",
                "t",
                "-x",
                "100",
                "-y",
                "12",
                "sh",
            ])
            .status()
            .expect("tmux new-session")
            .success();
        assert!(ok, "could not start tmux server");
        let socket_path = {
            let out = Command::new("tmux")
                .args(["-L", &name, "display-message", "-p", "#{socket_path}"])
                .output()
                .unwrap();
            String::from_utf8_lossy(&out.stdout).trim().to_string()
        };
        let mut s = Self {
            name,
            socket_path,
            client: None,
        };
        s.tmux(&["set-option", "-g", "@grab-key", "f"]);
        s.tmux(&["set-option", "-g", "escape-time", "0"]);
        s.grab(&["load-config"])
            .unwrap_or_else(|e| panic!("load-config failed: {e}"));
        s.attach_client();
        Some(s)
    }

    fn tmux(&self, args: &[&str]) -> String {
        let out = Command::new("tmux")
            .arg("-L")
            .arg(&self.name)
            .args(args)
            .output()
            .expect("run tmux");
        assert!(
            out.status.success(),
            "tmux {:?} failed: {}",
            args,
            String::from_utf8_lossy(&out.stderr)
        );
        String::from_utf8_lossy(&out.stdout)
            .trim_end_matches('\n')
            .to_string()
    }

    /// Run the binary against this server, like tmux's run-shell would.
    fn grab(&self, args: &[&str]) -> Result<String, String> {
        let out = Command::new(BIN)
            .args(args)
            .env("TMUX", format!("{},0,0", self.socket_path))
            .output()
            .expect("run tmux-grab");
        if out.status.success() {
            Ok(String::from_utf8_lossy(&out.stdout).to_string())
        } else {
            Err(String::from_utf8_lossy(&out.stderr).to_string())
        }
    }

    fn attach_client(&mut self) {
        let attach = format!("tmux -L {} attach-session -t t", self.name);
        let mut cmd = Command::new("script");
        if cfg!(target_os = "macos") {
            cmd.args(["-q", "/dev/null", "sh", "-c", &attach]);
        } else {
            cmd.args(["-qfc", &attach, "/dev/null"]);
        }
        let child = cmd
            .env("TERM", "xterm-256color")
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("spawn script");
        self.client = Some(child);
        self.wait_for(|s| !s.tmux(&["list-clients"]).is_empty(), "client attach");
    }

    fn type_keys(&mut self, keys: &str) {
        let stdin = self.client.as_mut().unwrap().stdin.as_mut().unwrap();
        stdin.write_all(keys.as_bytes()).unwrap();
        stdin.flush().unwrap();
    }

    fn shell(&self, line: &str) {
        self.tmux(&["send-keys", "-t", "t:0.0", line, "Enter"]);
    }

    fn screen(&self) -> String {
        self.tmux(&["capture-pane", "-p", "-t", "t:0.0"])
    }

    fn windows(&self) -> Vec<String> {
        self.tmux(&["list-windows", "-t", "t", "-F", "#{window_name}"])
            .lines()
            .map(str::to_string)
            .collect()
    }

    fn buffer(&self) -> String {
        let out = Command::new("tmux")
            .args(["-L", &self.name, "show-buffer"])
            .output()
            .unwrap();
        String::from_utf8_lossy(&out.stdout).to_string()
    }

    fn wait_for(&self, pred: impl Fn(&Self) -> bool, what: &str) {
        let start = Instant::now();
        while start.elapsed() < Duration::from_secs(10) {
            if pred(self) {
                return;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        panic!("timed out waiting for {what}\nscreen:\n{}", self.screen());
    }

    fn enter_grab_mode(&mut self) {
        self.type_keys("\x02f");
        self.wait_for(|s| s.windows().len() == 2, "grab window");
        // The frame is drawn before the swap, so once the window exists the
        // hints are visible.
    }

    fn wait_restored(&self) {
        self.wait_for(
            |s| {
                s.windows().len() == 1
                    && s.tmux(&["show-options", "-wqv", "-t", "t:0", "key-table"])
                        .is_empty()
                    && s.tmux(&["show-options", "-gv", "prefix"]) == "C-b"
            },
            "state restored",
        );
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        if let Some(mut c) = self.client.take() {
            let _ = c.kill();
            let _ = c.wait();
        }
        let _ = Command::new("tmux")
            .args(["-L", &self.name, "kill-server"])
            .output();
    }
}

fn have(bin: &str) -> bool {
    Command::new("sh")
        .args(["-c", &format!("command -v {bin}")])
        .stdout(Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

#[test]
fn load_config_installs_bindings() {
    let Some(s) = Server::start("bindings") else {
        return;
    };
    let prefix = s.tmux(&["list-keys", "-T", "prefix"]);
    assert!(
        prefix.contains(" f ") && prefix.contains("start"),
        "{prefix}"
    );
    let grab = s.tmux(&["list-keys", "-T", "grab"]);
    for key in ["a ", "A ", "C-a ", "M-a ", "Tab ", "Escape ", "q ", "Any "] {
        assert!(grab.contains(&format!(" {key}")), "missing binding {key:?}");
    }
    assert!(!grab.contains(" c "), "reserved key c must not be a hint");
    assert!(
        s.tmux(&["show-options", "-gv", "@grab-cli"])
            .ends_with("tmux-grab")
    );
}

#[test]
fn load_config_reports_bad_options() {
    let Some(s) = Server::start("badopts") else {
        return;
    };
    s.tmux(&["set-option", "-g", "@grab-hint-style", "fg=purple"]);
    s.tmux(&["set-option", "-g", "@grab-bogus", "1"]);
    let err = s.grab(&["load-config"]).unwrap_err();
    assert!(err.contains("fg=purple"), "{err}");
    assert!(err.contains("@grab-bogus"), "{err}");
}

#[test]
fn hint_copies_to_buffer_and_restores_state() {
    let Some(mut s) = Server::start("copy") else {
        return;
    };
    s.shell(
        "clear; echo see /usr/local/bin/tmux and https://example.com/x; echo second /tmp/foo.txt",
    );
    s.wait_for(
        |s| s.screen().contains("second /tmp/foo.txt"),
        "shell output",
    );

    s.enter_grab_mode();
    let screen = s.screen();
    // Bottom line gets the best hint; then left to right on the line above.
    assert!(screen.contains("second atmp/foo.txt"), "{screen}");
    assert!(
        screen.contains("see susr/local/bin/tmux and dttps://example.com/x"),
        "{screen}"
    );
    assert_eq!(
        s.tmux(&["show-options", "-wqv", "-t", "t:0", "key-table"]),
        "grab"
    );
    assert_eq!(s.tmux(&["show-options", "-gv", "prefix"]), "None");

    s.type_keys("d");
    s.wait_for(|s| s.buffer() == "https://example.com/x", "buffer");
    s.wait_restored();
    assert!(s.screen().contains("second /tmp/foo.txt"));
}

#[test]
fn shift_hint_pastes_into_pane() {
    let Some(mut s) = Server::start("paste") else {
        return;
    };
    s.shell("clear; echo pasteme /tmp/p.txt");
    s.wait_for(
        |s| s.screen().contains("pasteme /tmp/p.txt"),
        "shell output",
    );
    s.enter_grab_mode();
    s.type_keys("A");
    s.wait_for(
        |s| s.screen().contains("$ /tmp/p.txt"),
        "pasted text at prompt",
    );
    s.wait_restored();
}

#[test]
fn multi_select_joins_with_spaces() {
    let Some(mut s) = Server::start("multi") else {
        return;
    };
    s.shell("clear; echo one /tmp/one.txt; echo two /tmp/two.txt");
    s.wait_for(|s| s.screen().contains("two /tmp/two.txt"), "shell output");
    s.enter_grab_mode();
    s.type_keys("\t");
    s.type_keys("a");
    std::thread::sleep(Duration::from_millis(200));
    s.type_keys("s");
    std::thread::sleep(Duration::from_millis(200));
    s.type_keys("\t");
    s.wait_for(
        |s| s.buffer() == "/tmp/two.txt /tmp/one.txt",
        "joined buffer",
    );
    s.wait_restored();
}

#[test]
fn q_exits_and_prefix_is_inert_while_active() {
    let Some(mut s) = Server::start("exit") else {
        return;
    };
    s.shell("clear; echo a /tmp/x");
    s.wait_for(|s| s.screen().contains("a /tmp/x"), "shell output");
    s.enter_grab_mode();
    s.type_keys("\x02");
    std::thread::sleep(Duration::from_millis(300));
    assert_eq!(s.windows().len(), 2, "prefix must not leave grab mode");
    s.type_keys("q");
    s.wait_restored();
    assert!(s.buffer().is_empty(), "nothing should be copied on exit");
}

#[test]
fn split_and_zoomed_panes() {
    let Some(mut s) = Server::start("split") else {
        return;
    };
    s.tmux(&["split-window", "-h", "-t", "t", "sh"]);
    s.tmux(&[
        "send-keys",
        "-t",
        "t:0.1",
        "echo right /var/log/x.log",
        "Enter",
    ]);
    s.tmux(&["select-pane", "-t", "t:0.0"]);
    s.shell("clear; echo left /etc/hosts");
    s.wait_for(|s| s.screen().contains("left /etc/hosts"), "shell output");

    s.enter_grab_mode();
    assert!(s.screen().contains("left aetc/hosts"));
    assert!(
        s.tmux(&["capture-pane", "-p", "-t", "t:0.1"])
            .contains("right /var/log/x.log")
    );
    s.type_keys("a");
    s.wait_for(|s| s.buffer() == "/etc/hosts", "buffer");
    s.wait_restored();
    assert_eq!(
        s.tmux(&["display-message", "-p", "-t", "t:0.0", "#{pane_active}"]),
        "1"
    );

    s.tmux(&["resize-pane", "-Z", "-t", "t:0.0"]);
    s.enter_grab_mode();
    assert!(s.screen().contains("left aetc/hosts"));
    s.type_keys("a");
    s.wait_restored();
    assert_eq!(
        s.tmux(&[
            "display-message",
            "-p",
            "-t",
            "t:0",
            "#{window_zoomed_flag}"
        ]),
        "1"
    );
}

#[test]
fn wrapped_line_keeps_columns() {
    let Some(mut s) = Server::start("wrap") else {
        return;
    };
    let long = "/aaaaaaaaaa/bbbbbbbbbb/cccccccccc/dddddddddd/eeeeeeeeee/ffffffffff/gggggggggg/hhhhhhhhhh/iiiiiiiiii/jjjj";
    s.shell(&format!("clear; echo {long} end /tmp/z"));
    s.wait_for(|s| s.screen().contains("end /tmp/z"), "shell output");
    s.enter_grab_mode();
    let screen = s.screen();
    assert!(screen.contains("jjjj end stmp/z"), "{screen}");
    assert!(
        screen.lines().any(|l| l.starts_with("aaaaaaaaaaa/bbbb")),
        "{screen}"
    );
    s.type_keys("a");
    s.wait_for(|s| s.buffer() == long, "buffer");
    s.wait_restored();
}

#[test]
fn nothing_to_grab_exits_cleanly() {
    let Some(mut s) = Server::start("empty") else {
        return;
    };
    s.shell("clear");
    std::thread::sleep(Duration::from_millis(200));
    s.type_keys("\x02f");
    std::thread::sleep(Duration::from_millis(500));
    assert_eq!(s.windows().len(), 1);
    assert!(
        s.tmux(&["show-options", "-wqv", "-t", "t:0", "key-table"])
            .is_empty()
    );
}
