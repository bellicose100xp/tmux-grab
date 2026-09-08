//! End-to-end tests against a private tmux server with a real attached client.
//!
//! The client is attached on a pty opened by the test itself, so keys written
//! to the pty master travel through tmux key tables exactly like a user
//! typing. Tests are skipped when `tmux` is not installed.

use std::fs::File;
use std::io::Write;
use std::os::fd::FromRawFd;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

const BIN: &str = env!("CARGO_BIN_EXE_tmux-grab");
const COLS: u16 = 100;
const ROWS: u16 = 12;
const PATIENCE: Duration = Duration::from_secs(15);

/// A pty pair. Writing to `master` looks like typing on the slave terminal.
struct Pty {
    master: File,
    slave: File,
}

impl Pty {
    fn open() -> Pty {
        let mut master_fd: libc::c_int = -1;
        let mut slave_fd: libc::c_int = -1;
        let winsize = libc::winsize {
            ws_row: ROWS,
            ws_col: COLS,
            ws_xpixel: 0,
            ws_ypixel: 0,
        };
        let rc = unsafe {
            libc::openpty(
                &mut master_fd,
                &mut slave_fd,
                std::ptr::null_mut(),
                std::ptr::null(),
                &winsize,
            )
        };
        assert_eq!(rc, 0, "openpty failed: {}", std::io::Error::last_os_error());
        unsafe {
            Pty {
                master: File::from_raw_fd(master_fd),
                slave: File::from_raw_fd(slave_fd),
            }
        }
    }
}

struct Server {
    name: String,
    socket_path: String,
    client: Option<Child>,
    pty: Option<Pty>,
}

impl Server {
    fn start(test: &str) -> Option<Self> {
        if !have("tmux") {
            eprintln!("skipping {test}: tmux is not installed");
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
                &COLS.to_string(),
                "-y",
                &ROWS.to_string(),
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
            pty: None,
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

    /// Run the binary against this server, the way tmux's run-shell would.
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
        let pty = Pty::open();
        let stdin = pty.slave.try_clone().unwrap();
        let stdout = pty.slave.try_clone().unwrap();
        let stderr = pty.slave.try_clone().unwrap();
        let child = Command::new("tmux")
            .args(["-L", &self.name, "attach-session", "-t", "t"])
            .env("TERM", "xterm-256color")
            .stdin(Stdio::from(stdin))
            .stdout(Stdio::from(stdout))
            .stderr(Stdio::from(stderr))
            .spawn()
            .expect("spawn tmux attach");
        self.client = Some(child);
        self.pty = Some(pty);
        self.wait_for(|s| !s.tmux(&["list-clients"]).is_empty(), "client attach");
        self.wait_client_ready();
    }

    /// A freshly attached client is not necessarily reading keys yet, and until
    /// it is, keystrokes are either dropped or land in the pane as text. Bind a
    /// throwaway key in the root table and press it until it fires, so every
    /// test starts from a client that is known to be live.
    fn wait_client_ready(&mut self) {
        self.tmux(&[
            "bind-key",
            "-T",
            "root",
            "C-y",
            "set-option",
            "-g",
            "@grab-probe",
            "ok",
        ]);
        let start = Instant::now();
        loop {
            self.type_keys("\x19");
            let deadline = Instant::now() + Duration::from_millis(300);
            while Instant::now() < deadline {
                if self.tmux(&["show-options", "-gqv", "@grab-probe"]) == "ok" {
                    self.tmux(&["unbind-key", "-T", "root", "C-y"]);
                    self.tmux(&["set-option", "-gu", "@grab-probe"]);
                    // The probe key typed text into the pane on the attempts
                    // that were too early; give the shell a clean line.
                    self.tmux(&["send-keys", "-t", "t:0.0", "C-u"]);
                    return;
                }
                std::thread::sleep(Duration::from_millis(25));
            }
            assert!(
                start.elapsed() < PATIENCE,
                "client never started reading keys\nscreen:\n{}\nclients:\n{}",
                self.screen(),
                self.tmux(&["list-clients"])
            );
        }
    }

    fn type_keys(&mut self, keys: &str) {
        let master = &mut self.pty.as_mut().unwrap().master;
        master.write_all(keys.as_bytes()).unwrap();
        master.flush().unwrap();
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

    fn key_table(&self) -> String {
        self.tmux(&["show-options", "-wqv", "-t", "t:0", "key-table"])
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
        while start.elapsed() < PATIENCE {
            if pred(self) {
                return;
            }
            std::thread::sleep(Duration::from_millis(25));
        }
        panic!("timed out waiting for {what}\nscreen:\n{}", self.screen());
    }

    /// Press the grab key and wait until the overlay is up. The binary switches
    /// the key table last, so that is the signal that hints are on screen and
    /// further keys will reach grab mode.
    ///
    /// The keypress is repeated while nothing at all has happened. That is safe
    /// only in that state: once the overlay window exists, another `f` would be
    /// read as a hint, so from then on we only wait.
    fn enter_grab_mode(&mut self) {
        let start = Instant::now();
        loop {
            self.type_keys("\x02f");
            let deadline = Instant::now() + Duration::from_secs(2);
            while Instant::now() < deadline {
                if self.key_table() == "grab" && self.windows().len() == 2 {
                    return;
                }
                if self.windows().len() == 2 {
                    // Overlay is coming up. Stop pressing keys and wait it out.
                    self.wait_for(|s| s.key_table() == "grab", "grab key table");
                    return;
                }
                std::thread::sleep(Duration::from_millis(25));
            }
            assert!(
                start.elapsed() < PATIENCE,
                "grab mode never started\nscreen:\n{}\nlog:\n{}",
                self.screen(),
                std::fs::read_to_string(dirs_cache().join("tmux-grab.log")).unwrap_or_default()
            );
        }
    }

    fn wait_restored(&self) {
        self.wait_for(
            |s| {
                s.windows().len() == 1
                    && s.key_table().is_empty()
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
        self.pty = None;
        let _ = Command::new("tmux")
            .args(["-L", &self.name, "kill-server"])
            .output();
    }
}

/// Same cache directory the binary logs into.
fn dirs_cache() -> std::path::PathBuf {
    std::env::var_os("XDG_CACHE_HOME")
        .map(std::path::PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| std::path::PathBuf::from(h).join(".cache")))
        .unwrap_or_else(std::env::temp_dir)
        .join("tmux-grab")
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
    // The bottom line gets the best hint, then left to right going up.
    s.wait_for(
        |s| s.screen().contains("second atmp/foo.txt"),
        "hint on the last line",
    );
    let screen = s.screen();
    assert!(
        screen.contains("see susr/local/bin/tmux and dttps://example.com/x"),
        "{screen}"
    );
    assert_eq!(s.tmux(&["show-options", "-gv", "prefix"]), "None");

    s.type_keys("d");
    s.wait_for(|s| s.buffer() == "https://example.com/x", "copied url");
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
    s.wait_for(|s| s.screen().contains("pasteme atmp/p.txt"), "hint");
    s.type_keys("A");
    s.wait_for(
        |s| s.screen().contains("$ /tmp/p.txt"),
        "pasted text at the prompt",
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
    s.wait_for(|s| s.screen().contains("two atmp/two.txt"), "hints");

    s.type_keys("\t");
    s.type_keys("a");
    // The picked item is restyled, which is how we know the pick registered.
    s.wait_for(
        |s| {
            s.tmux(&["capture-pane", "-p", "-e", "-t", "t:0.0"])
                .contains("\u{1b}[34m")
        },
        "first pick highlighted",
    );
    s.type_keys("s");
    s.wait_for(
        |s| {
            let painted = s.tmux(&["capture-pane", "-p", "-e", "-t", "t:0.0"]);
            painted.matches("\u{1b}[34m").count() >= 2
        },
        "second pick highlighted",
    );
    s.type_keys("\t");
    s.wait_for(
        |s| s.buffer() == "/tmp/two.txt /tmp/one.txt",
        "both picks in the buffer",
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
    assert_eq!(s.key_table(), "grab", "prefix must not leave grab mode");
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
    s.wait_for(
        |s| s.screen().contains("left aetc/hosts"),
        "hint in left pane",
    );
    assert!(
        s.tmux(&["capture-pane", "-p", "-t", "t:0.1"])
            .contains("right /var/log/x.log"),
        "the other pane must be untouched"
    );
    s.type_keys("a");
    s.wait_for(|s| s.buffer() == "/etc/hosts", "copied path");
    s.wait_restored();
    assert_eq!(
        s.tmux(&["display-message", "-p", "-t", "t:0.0", "#{pane_active}"]),
        "1"
    );

    s.tmux(&["resize-pane", "-Z", "-t", "t:0.0"]);
    s.enter_grab_mode();
    s.wait_for(
        |s| s.screen().contains("left aetc/hosts"),
        "hint in zoomed pane",
    );
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
        "1",
        "zoom must survive grab mode"
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
    s.wait_for(
        |s| s.screen().contains("jjjj end stmp/z"),
        "hint after the wrap",
    );
    let screen = s.screen();
    assert!(
        screen.lines().any(|l| l.starts_with("aaaaaaaaaaa/bbbb")),
        "the wrapped line must keep its columns:\n{screen}"
    );
    s.type_keys("a");
    s.wait_for(|s| s.buffer() == long, "copied the whole wrapped path");
    s.wait_restored();
}

#[test]
fn nothing_to_grab_exits_cleanly() {
    let Some(mut s) = Server::start("empty") else {
        return;
    };
    s.shell("clear");
    s.wait_for(|s| !s.screen().contains("clear"), "cleared screen");
    s.type_keys("\x02f");
    std::thread::sleep(Duration::from_millis(800));
    assert_eq!(
        s.windows().len(),
        1,
        "no overlay window when there are no matches"
    );
    assert!(s.key_table().is_empty(), "must not enter grab mode");
    assert!(s.buffer().is_empty());
}
