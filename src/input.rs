//! Key events travel from tmux key bindings to the running `start` process
//! over a Unix socket: each binding runs `tmux-grab send-input <sock> <msg>`.

use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

pub struct InputServer {
    listener: UnixListener,
    path: PathBuf,
}

impl InputServer {
    pub fn bind(path: &Path) -> Result<Self> {
        let _ = std::fs::remove_file(path);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let listener = UnixListener::bind(path)
            .with_context(|| format!("cannot listen on {}", path.display()))?;
        Ok(Self {
            listener,
            path: path.to_path_buf(),
        })
    }

    /// Block until one message arrives.
    pub fn recv(&self) -> Result<String> {
        let (stream, _) = self.listener.accept()?;
        let mut line = String::new();
        BufReader::new(stream).read_line(&mut line)?;
        Ok(line.trim_end_matches(['\n', '\r']).to_string())
    }
}

impl Drop for InputServer {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

pub fn send(path: &Path, msg: &str) -> Result<()> {
    let mut stream = UnixStream::connect(path)
        .with_context(|| format!("no grab session listening on {}", path.display()))?;
    stream.write_all(msg.as_bytes())?;
    stream.write_all(b"\n")?;
    Ok(())
}

/// Where the socket for a given tmux server lives. Derived from the tmux
/// server socket path so bindings installed at load-config time and the
/// `start` process agree without extra tmux calls.
pub fn socket_path_for(tmux_socket: &str) -> PathBuf {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    tmux_socket.hash(&mut h);
    let name = format!("tmux-grab-{:016x}.sock", h.finish());
    crate::config::runtime_dir().join(name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip() {
        let dir = std::env::temp_dir().join(format!("tmux-grab-test-{}", std::process::id()));
        let path = dir.join("in.sock");
        let server = InputServer::bind(&path).unwrap();
        let p2 = path.clone();
        let t = std::thread::spawn(move || send(&p2, "hint:a:main").unwrap());
        assert_eq!(server.recv().unwrap(), "hint:a:main");
        t.join().unwrap();
        drop(server);
        assert!(!path.exists());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn socket_path_is_stable() {
        assert_eq!(
            socket_path_for("/tmp/tmux-1000/default"),
            socket_path_for("/tmp/tmux-1000/default")
        );
        assert_ne!(
            socket_path_for("/tmp/tmux-1000/default"),
            socket_path_for("/tmp/tmux-1000/other")
        );
    }
}
