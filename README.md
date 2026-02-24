# mmtui

[![Built With Ratatui](https://ratatui.rs/built-with-ratatui/badge.svg)](https://ratatui.rs/)

A terminal user interface for the NCAA March Madness tournament.

`mmtui` shows a live bracket-oriented experience with:
- Intro banner screen
- Bracket rounds (`64`, `32`, `16`, `8`, `Final Four`, `Championship`)
- Region navigation
- Game detail view

## Install

### Homebrew

Current (until `homebrew-core` PR is merged):

```bash
brew tap holynakamoto/mmtui git@github.com:holynakamoto/mmtui.git
brew install holynakamoto/mmtui/mmtui
```

After `homebrew-core` merge:

```bash
brew install mmtui
```

Verify install:

```bash
mmtui --version
```

Upgrade:

```bash
brew update
brew upgrade mmtui
```

Uninstall:

```bash
brew uninstall mmtui
```

### Cargo

```bash
cargo install --path .
```

### Build Local Binary

```bash
cargo build --release
./target/release/mmtui
```

### Docker

```bash
docker build -t mmtui .
docker run -it --rm --name mmtui mmtui:latest
```

## Usage

Run:

```bash
mmtui
```

If using a local snapshot file:

```bash
MMTUI_BRACKET_JSON=2025_bracket.json mmtui
```

### Global Chat Relay

Run a relay server (deploy anywhere reachable by clients):

```bash
cargo run --bin chat-relay
```

Point clients at that relay:

```bash
MMTUI_CHAT_WS=ws://YOUR_SERVER:8787 mmtui
```

Optional room override:

```bash
MMTUI_CHAT_ROOM=your-room MMTUI_CHAT_WS=ws://YOUR_SERVER:8787 mmtui
```

## Navigation

- `Enter`: continue from intro / open selected game detail
- `h` / `l` or `←` / `→`: previous / next round
- `j` / `k` or `↓` / `↑`: move selection
- `r`: cycle region
- `1` / `2` / `3`: Bracket / Scoreboard / Game Detail tabs
- `4`: Chat tab
- Chat controls: `i` or `Enter` to compose, `Enter` to send, `Esc` to cancel
- `?`: Help
- `Esc`: back from Help or Game Detail
- `f`: toggle fullscreen
- `q`: quit

## Release Executable

Build optimized executable:

```bash
cargo build --release
```

Artifact path:

```text
target/release/mmtui
```

## License

[MIT](LICENSE)
