# tmux-grab

Press a key and every path, URL, git SHA, IP address, UUID, number and hex value on the visible pane gets a one or two letter hint. Type the hint and the text is in your tmux buffer and system clipboard. Hold Ctrl to open it, Shift to paste it into the pane, Alt to run your own command on it. tmux-grab is inspired by [tmux-fingers](https://github.com/Morantron/tmux-fingers) and rewritten from scratch in Rust as a maintained alternative with the same workflow.

## Requirements

- tmux 3.2 or newer
- One of:
  - nothing else, if you let the install wizard download a prebuilt binary (Linux x86_64 and aarch64, macOS x86_64 and arm64)
  - `cargo`, if you prefer to build from source

## Installation

### With TPM

Add the plugin to your tmux config:

```tmux
set -g @plugin 'bellicose100xp/tmux-grab'
```

Press `prefix + I` to install. On first run a menu appears asking whether to download the prebuilt binary or build from source. Pick one and wait for the window to finish. The wizard then reloads your config. The same menu appears after a plugin update when the binary version no longer matches.

### Manual

```sh
git clone https://github.com/bellicose100xp/tmux-grab ~/.tmux/plugins/tmux-grab
```

Then add this line to your tmux config and reload it:

```tmux
run-shell ~/.tmux/plugins/tmux-grab/tmux-grab.tmux
```

If a `tmux-grab` binary is on your `PATH`, the plugin uses it and skips the wizard. To build one yourself:

```sh
cargo install --path ~/.tmux/plugins/tmux-grab
```

## Usage

Press `prefix + F` to enter grab mode. Hints appear on every match.

| Key | Effect |
|---|---|
| hint letters | copy the match (`@grab-main-action`) |
| Ctrl + hint | open the match with `xdg-open` or `open` (`@grab-ctrl-action`) |
| Shift + hint | paste the match into the pane (`@grab-shift-action`) |
| Alt + hint | run a custom command (`@grab-alt-action`) |
| Tab | toggle multi-select. Pick several hints, then press Tab again to act on all of them joined by spaces |
| q, Esc, Ctrl-c | leave grab mode |

Two letter hints wait for the second key. Type the hint in lowercase for the main action. Shift + hint means typing the hint in uppercase.

## Configuration

Set options with `set -g` in your tmux config and reload it afterwards.

| Option | Default | Meaning |
|---|---|---|
| `@grab-key` | `F` | `prefix + key` enters grab mode |
| `@grab-keyboard-layout` | `qwerty` | Which letters are used for hints. See layouts below |
| `@grab-main-action` | `:copy:` | Action for a plain hint |
| `@grab-ctrl-action` | `:open:` | Action for Ctrl + hint |
| `@grab-shift-action` | `:paste:` | Action for Shift + hint |
| `@grab-alt-action` | (empty) | Action for Alt + hint |
| `@grab-hint-style` | `bg=colour220,fg=colour16,bold` | Style of the hint letters |
| `@grab-highlight-style` | `bg=colour25,fg=colour231` | Style of the matched text |
| `@grab-selected-hint-style` | `bg=colour28,fg=colour231,bold` | Hint style for items already picked in multi-select |
| `@grab-selected-highlight-style` | `bg=colour22,fg=colour231` | Highlight style for items already picked in multi-select |
| `@grab-backdrop-style` | (empty) | Style applied to everything that is not a match |
| `@grab-hint-position` | `left` | `left` or `right`: which end of the match the hint covers |
| `@grab-use-system-clipboard` | `1` | Also copy to the system clipboard |
| `@grab-show-copied-notification` | `0` | Flash `Copied: ...` in the status line after copying |
| `@grab-enabled-builtin-patterns` | `all` | `all` or a comma separated list of built-in pattern names |
| `@grab-pattern-<name>` | | Add a custom regex. See Patterns |
| `@grab-enable-bindings` | `1` | Set to `0` to skip the root `@grab-key` binding and bind manually |
| `@grab-skip-wizard` | `0` | Set to `1` to never show the install menu |

Examples:

```tmux
set -g @grab-key 'Space'
set -g @grab-keyboard-layout 'qwerty-homerow'
set -g @grab-hint-style 'fg=#ff79c6,bold'
set -g @grab-highlight-style 'fg=colour214,underscore'
set -g @grab-backdrop-style 'fg=colour240'
set -g @grab-hint-position 'right'
set -g @grab-show-copied-notification 1
set -g @grab-enabled-builtin-patterns 'url,path,sha'
```

### Keyboard layouts

`@grab-keyboard-layout` picks the hint alphabet. Homerow variants only use the home row, so hints are longer but easier to reach. Left-hand and right-hand variants leave one hand free.

`qwerty`, `qwerty-homerow`, `qwerty-left-hand`, `qwerty-right-hand`,
`azerty`, `azerty-homerow`, `azerty-left-hand`, `azerty-right-hand`,
`qwertz`, `qwertz-homerow`, `qwertz-left-hand`, `qwertz-right-hand`,
`dvorak`, `dvorak-homerow`, `dvorak-left-hand`, `dvorak-right-hand`,
`colemak`, `colemak-homerow`, `colemak-left-hand`, `colemak-right-hand`

### Styles

Styles use tmux syntax: comma separated `fg=` and `bg=` colours plus attributes.

The defaults set both foreground and background from the fixed 256-colour cube, so they look the same on light and dark terminals. If you override them, keep both `fg=` and `bg=` set: a style with only one of them inherits the other from the terminal and can disappear against a theme's background.

- Colours: `black`, `red`, `green`, `yellow`, `blue`, `magenta`, `cyan`, `white`, their `bright*` variants, `colour0` to `colour255`, `#rrggbb`, and `default`.
- Attributes: `bold`, `dim`, `underscore`, `italics`, `reverse`, and the `no` prefixed forms such as `nobold`.

### System clipboard

With `@grab-use-system-clipboard 1`, tmux-grab loads the buffer with `tmux load-buffer -w` so tmux forwards it through OSC 52 when your terminal supports that. It also pipes the text into the first of `pbcopy`, `wl-copy`, `xclip`, `xsel`, or `clip.exe` that is present.

## Actions

The four `@grab-*-action` options accept:

| Value | Effect |
|---|---|
| `:copy:` | Copy to the tmux buffer, and to the system clipboard when enabled |
| `:open:` | Open with `xdg-open` on Linux or `open` on macOS |
| `:paste:` | Run `paste-buffer` into the pane the hints were shown on |
| any other string | Run it with `sh -c` in the pane's working directory |

Every action copies the match to the tmux buffer first, so a custom command can also rely on `tmux save-buffer -`.

A custom command gets the match on stdin and two environment variables: `MODIFIER` (`main`, `ctrl`, `shift`, or `alt`) and `HINT` (the letters that were typed).

```tmux
set -g @grab-alt-action 'xargs -I {} tmux split-window -h "nvim {}"'
set -g @grab-ctrl-action 'xargs -I {} tmux new-window "gh browse {}"'
```

## Patterns

### Built-in

| Name | Matches |
|---|---|
| `ip` | IPv4 addresses |
| `uuid` | UUIDs |
| `sha` | Lowercase hex strings of 7 or more characters, such as git SHAs |
| `digit` | Numbers of 4 or more digits |
| `url` | `http`, `https`, `ftp`, `file`, `git`, `ssh` URLs and `git@host:path` remotes |
| `path` | Absolute, relative and `~` paths with at least one `/` |
| `hex` | `0x` prefixed hex values |
| `kubernetes` | Kubernetes resource names such as `pod/name` and `deployment.apps/name` |
| `git-status` | File names after `modified:`, `deleted:` and `new file:` in `git status` output |
| `git-status-branch` | Branch names in the `git status` header |
| `diff` | File names in unified diff headers (`--- a/file`, `+++ b/file`) |

Enable only some of them:

```tmux
set -g @grab-enabled-builtin-patterns 'url,path,sha'
```

### Custom

Add any number of `@grab-pattern-<name>` options. The value is a regex in Rust [`regex`](https://docs.rs/regex) syntax. Note that `regex` has no lookaround or backreferences.

```tmux
set -g @grab-pattern-jira '[A-Z]{2,}-[0-9]+'
```

By default the whole match is copied. To highlight a wider context but copy only part of it, name the part `match`:

```tmux
set -g @grab-pattern-docker-image 'image: (?P<match>[a-z0-9./-]+:[a-z0-9._-]+)'
```

tmux-grab reports invalid regexes when the config loads and refuses to start until you fix them.

## Recipes

Bind one key to URLs only, next to the normal binding:

```tmux
bind-key U run-shell -b "tmux-grab start --patterns url '#{pane_id}'"
```

Take over all bindings yourself:

```tmux
set -g @grab-enable-bindings 0
bind-key F run-shell -b "tmux-grab start '#{pane_id}'"
bind-key U run-shell -b "tmux-grab start --patterns url '#{pane_id}'"
```

Grab from the pane to the right of the active one, for example when a log tails there:

```tmux
bind-key F run-shell -b "tmux-grab start '{right-of}'"
```

Any tmux pane target works in place of `#{pane_id}`, so `{last}`, `{up-of}`, or an explicit `%3` are also valid.

If `tmux-grab` is not on your `PATH`, use the full path to `bin/tmux-grab` inside the plugin directory in these bindings.

## Development

```sh
cargo build
cargo test
```

Integration tests start a tmux server on a private socket, so `tmux` has to be installed. CI runs `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`, and `cargo test` on Linux and macOS.

To try a local build inside your running tmux, copy `target/debug/tmux-grab` to `bin/tmux-grab` in your plugin checkout and reload the config, or set `@grab-skip-wizard 1` and put the binary on your `PATH`.

### Releasing

1. Bump `version` in `Cargo.toml` and add a section to `CHANGELOG.md`.
2. Commit, then tag with the same version: `git tag v0.2.0`.
3. `git push --tags`.

The release workflow checks that the tag matches `Cargo.toml`, builds static binaries for Linux (x86_64, aarch64) and macOS (x86_64, arm64), and attaches them to a GitHub release. The install wizard downloads the tarball whose version matches the `Cargo.toml` of the checked out plugin, so a tag without matching binaries breaks installs for that version.

## Acknowledgements

- [tmux-fingers](https://github.com/Morantron/tmux-fingers) by Jorge Morante, which defined this workflow. The option names, the install wizard, and the built-in pattern set follow its lead.
- [tmux-thumbs](https://github.com/fcsonline/tmux-thumbs) by Ferran Basora, another take on the same idea in Rust.

## License

MIT. See [LICENSE](LICENSE).
