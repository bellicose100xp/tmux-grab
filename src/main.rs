//! tmux-grab: vimium-style hints for tmux. Press a key, grab a path, URL,
//! hash or number off the screen.

mod action;
mod commands;
mod config;
mod hints;
mod input;
mod matcher;
mod render;
mod style;
mod tmux;

use std::path::PathBuf;

use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(name = "tmux-grab", version, about, disable_version_flag = true)]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Print the version and exit.
    Version,
    /// Read `@grab-*` options, validate them and install key bindings.
    /// Called by tmux-grab.tmux when tmux starts.
    LoadConfig,
    /// Show hints over a pane and act on the chosen one.
    Start {
        /// Pane id (%N) or any tmux target-pane token, e.g. '{right-of}'.
        pane: String,
        /// Comma separated pattern names to use instead of all configured ones.
        #[arg(long)]
        patterns: Option<String>,
        /// Override @grab-main-action for this run.
        #[arg(long)]
        main_action: Option<String>,
        /// Override @grab-ctrl-action for this run.
        #[arg(long)]
        ctrl_action: Option<String>,
        /// Override @grab-shift-action for this run.
        #[arg(long)]
        shift_action: Option<String>,
        /// Override @grab-alt-action for this run.
        #[arg(long)]
        alt_action: Option<String>,
    },
    /// Internal: deliver a key event to the running grab session.
    #[command(hide = true)]
    SendInput { socket: PathBuf, message: String },
}

fn main() {
    let cli = Cli::parse();
    let result = match cli.cmd {
        Cmd::Version => {
            println!("{}", env!("CARGO_PKG_VERSION"));
            Ok(())
        }
        Cmd::LoadConfig => commands::load_config::run(),
        Cmd::Start {
            pane,
            patterns,
            main_action,
            ctrl_action,
            shift_action,
            alt_action,
        } => commands::start::run(commands::start::StartOpts {
            pane,
            patterns,
            main_action,
            ctrl_action,
            shift_action,
            alt_action,
        }),
        Cmd::SendInput { socket, message } => commands::start::send_input(socket, message),
    };
    if let Err(e) = result {
        eprintln!("[tmux-grab] {e:#}");
        std::process::exit(1);
    }
}
