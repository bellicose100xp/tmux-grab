#!/usr/bin/env bash
# Install wizard for tmux-grab.
#
#   install.sh                    show the tmux menu
#   install.sh download-binary    fetch the prebuilt binary from GitHub Releases
#   install.sh build-from-source  cargo build --release and copy into bin/
#
# GRAB_UPDATE=1 changes the menu text to say the binary is out of date.

SCRIPT_DIR="$( cd "$( dirname "${BASH_SOURCE[0]}" )" && pwd )"
CURRENT_DIR="$( cd "$SCRIPT_DIR/.." && pwd )"
REPO="bellicose100xp/tmux-grab"
action="$1"

function finish {
  exit_code=$?

  # Only intercept the exit code when an action was given. Sourcing the tmux
  # config from the menu path would re-run tmux-grab.tmux and loop forever.
  if [[ -z "$action" ]]; then
    exit $exit_code
  fi

  if [[ $exit_code -eq 0 ]]; then
    echo "Reloading tmux config"
    if [[ -f "$HOME/.config/tmux/tmux.conf" ]]; then
      tmux source-file "$HOME/.config/tmux/tmux.conf"
    else
      tmux source-file "$HOME/.tmux.conf"
    fi
    exit 0
  else
    echo ""
    echo "Something went wrong. Press any key to close"
    read -n 1
    exit 1
  fi
}

trap finish EXIT

function cargo_version() {
  grep -m1 '^version = ' "$CURRENT_DIR/Cargo.toml" | sed -E 's/^version = "([^"]+)".*/\1/'
}

function detect_target() {
  local os arch
  os=$(uname -s)
  arch=$(uname -m)
  case "$os/$arch" in
    Linux/x86_64) echo "x86_64-unknown-linux-musl" ;;
    Linux/aarch64|Linux/arm64) echo "aarch64-unknown-linux-musl" ;;
    Darwin/x86_64) echo "x86_64-apple-darwin" ;;
    Darwin/arm64|Darwin/aarch64) echo "aarch64-apple-darwin" ;;
    *) return 1 ;;
  esac
}

function verify_binary() {
  local installed
  installed=$("$CURRENT_DIR/bin/tmux-grab" version 2>/dev/null)
  if [[ -z "$installed" ]]; then
    echo "bin/tmux-grab did not run. The binary may be for the wrong platform."
    exit 1
  fi
  echo "Installed tmux-grab $installed"
}

function download_binary() {
  local target version url tmpdir
  if ! target=$(detect_target); then
    echo "No prebuilt binary for $(uname -s)/$(uname -m)."
    echo "Pick \"Build from source\" instead, or install cargo from https://rustup.rs and run: bash $SCRIPT_DIR/install.sh build-from-source"
    exit 1
  fi

  version=$(cargo_version)
  url="https://github.com/$REPO/releases/download/v${version}/tmux-grab-${target}.tar.gz"

  echo "Downloading tmux-grab $version for $target"
  echo "  $url"

  tmpdir=$(mktemp -d)
  if ! curl -fsSL "$url" -o "$tmpdir/tmux-grab.tar.gz"; then
    echo "Download failed. Check your network, or that release v${version} exists at https://github.com/$REPO/releases"
    rm -rf "$tmpdir"
    exit 1
  fi

  mkdir -p "$CURRENT_DIR/bin"
  if ! tar -xzf "$tmpdir/tmux-grab.tar.gz" -C "$CURRENT_DIR/bin" tmux-grab; then
    echo "Could not extract tmux-grab from the archive."
    rm -rf "$tmpdir"
    exit 1
  fi
  rm -rf "$tmpdir"
  chmod +x "$CURRENT_DIR/bin/tmux-grab"

  verify_binary
  echo "Download complete"
  exit 0
}

function build_from_source() {
  if ! command -v cargo >/dev/null 2>&1; then
    echo "cargo is not installed. Install Rust first:"
    echo ""
    echo "  https://rustup.rs"
    echo ""
    exit 1
  fi

  echo "Building tmux-grab from source (this can take a minute)"
  if ! (cd "$CURRENT_DIR" && cargo build --release); then
    echo "cargo build failed."
    exit 1
  fi

  mkdir -p "$CURRENT_DIR/bin"
  cp "$CURRENT_DIR/target/release/tmux-grab" "$CURRENT_DIR/bin/tmux-grab"
  chmod +x "$CURRENT_DIR/bin/tmux-grab"

  verify_binary
  echo "Build complete"
  exit 0
}

case "$action" in
  download-binary) download_binary ;;
  build-from-source) build_from_source ;;
  "") ;;
  *)
    echo "Unknown action: $action"
    echo "Usage: install.sh [download-binary|build-from-source]"
    exit 1
    ;;
esac

function get_message() {
  if [[ "$GRAB_UPDATE" == "1" ]]; then
    echo "tmux-grab was updated. The binary needs updating to match."
  else
    echo "First run. tmux-grab needs its binary before it can work."
  fi
}

tmux display-menu -T "tmux-grab" "" "- " "" "" "-  #[nodim,bold]Welcome to tmux-grab " "" "" "- " "" "" "-  $(get_message) " "" "" "- " "" "" "" "Download prebuilt binary" d "new-window \"bash $SCRIPT_DIR/install.sh download-binary\"" "Build from source (cargo required)" s "new-window \"bash $SCRIPT_DIR/install.sh build-from-source\"" "" "Exit" q ""
