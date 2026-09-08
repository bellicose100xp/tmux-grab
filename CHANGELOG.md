# Changelog

All notable changes to this project are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.1.1] - 2026-09-08

### Fixed

- Pane lookups failed on tmux 3.4 and 3.5 with an unexpected-output error, so grab mode never started. Those versions render non-printable characters in printed command output as octal escapes, which broke the field delimiter tmux-grab used to read pane state.
- A hint key could overtake the Tab before it, which turned multi-select off again and copied a single match instead. Key events are now delivered in order.

## [0.1.0] - 2026-09-08

### Added

- Grab mode: `prefix + F` overlays a one or two letter hint on every match in the visible pane. Typing the hint copies the match to the tmux buffer and the system clipboard.
- Modifier actions: Ctrl+hint opens the match, Shift+hint pastes it into the pane, Alt+hint runs a user command. Each action is configurable, and any action can be a shell command that receives the match on stdin.
- Multi-select: Tab toggles a mode where several hints can be picked before copying them together.
- Built-in patterns for IP addresses, UUIDs, git SHAs, numbers, URLs, file paths, hex values, kubernetes resource names, `git status` output, and diff hunks. Patterns can be enabled individually with `@grab-enabled-builtin-patterns`.
- Custom patterns through `@grab-pattern-<name>` options, with an optional `match` named group to narrow what gets copied.
- Configurable hint alphabets for qwerty, azerty, qwertz, dvorak, and colemak layouts, including homerow and single-hand variants.
- Styles for hints, highlights, selected items, and the backdrop, using tmux style syntax.
- Install wizard shown on first run and after upgrades. It downloads a prebuilt binary or builds from source with cargo.
- Prebuilt binaries for Linux (x86_64, aarch64, static musl) and macOS (x86_64, arm64) attached to each GitHub release.

[Unreleased]: https://github.com/bellicose100xp/tmux-grab/compare/v0.1.1...HEAD
[0.1.1]: https://github.com/bellicose100xp/tmux-grab/releases/tag/v0.1.1
[0.1.0]: https://github.com/bellicose100xp/tmux-grab/releases/tag/v0.1.0
