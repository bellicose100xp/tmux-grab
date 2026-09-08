#!/usr/bin/env bash
# TPM entry point for tmux-grab. Finds the binary (or launches the install
# wizard), checks that its version matches this checkout, then asks the
# binary to read the @grab-* options and install the key bindings.

CURRENT_DIR="$( cd "$( dirname "${BASH_SOURCE[0]}" )" && pwd )"

if command -v tmux-grab &>/dev/null; then
  GRAB_BINARY="tmux-grab"
elif [[ -x "$CURRENT_DIR/bin/tmux-grab" ]]; then
  GRAB_BINARY="$CURRENT_DIR/bin/tmux-grab"
fi

if [[ -z "$GRAB_BINARY" ]]; then
  tmux run-shell -b "bash $CURRENT_DIR/scripts/install.sh"
  exit 0
fi

CARGO_VERSION=$(grep -m1 '^version = ' "$CURRENT_DIR/Cargo.toml" | sed -E 's/^version = "([^"]+)".*/\1/')
BINARY_VERSION=$("$GRAB_BINARY" version 2>/dev/null)

SKIP_WIZARD=$(tmux show-option -gqv @grab-skip-wizard)
SKIP_WIZARD=${SKIP_WIZARD:-0}

if [[ "$SKIP_WIZARD" != "1" && "$BINARY_VERSION" != "$CARGO_VERSION" ]]; then
  tmux run-shell -b "GRAB_UPDATE=1 bash $CURRENT_DIR/scripts/install.sh"
  exit 0
fi

# Under systemd (and tmux 3.6a) TERM may be "dumb", which breaks colours.
# Fall back to the server's default-terminal in that case.
if [[ "$TERM" == "dumb" ]]; then
  GRAB_TERM=$(tmux show-option -gqv default-terminal)
else
  GRAB_TERM="$TERM"
fi

tmux run "TERM=$GRAB_TERM $GRAB_BINARY load-config"
exit $?
