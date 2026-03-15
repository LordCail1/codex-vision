# codex-vision

Local Codex CLI session visualizer with a live graph UI and terminal inspector.

## Install

The intended long-term path is a release binary, not `cargo run`.

Linux, macOS, WSL2:

```bash
curl -fsSL https://raw.githubusercontent.com/LordCail1/codex-vision/main/scripts/install.sh | bash
```

Windows PowerShell:

```powershell
irm https://raw.githubusercontent.com/LordCail1/codex-vision/main/scripts/install.ps1 | iex
```

If you are developing locally from source instead, you still need Rust plus a normal system C toolchain because SQLite is compiled as part of the dependency graph.

## What It Does

- Reads local Codex session metadata from `~/.codex`
- Reconstructs fork lineage from rollout metadata
- Detects live sessions from active Codex processes
- Annotates active sessions with tmux pane labels when tmux is available
- Serves a browser graph UI and a terminal inspector from the same Rust core

## Commands

```bash
codex-vision web
codex-vision web --all
codex-vision tui
codex-vision snapshot --json
codex-vision doctor
```

The web UI binds to `127.0.0.1` and opens a browser unless you pass `--no-open`.

For development from source:

```bash
cargo run -- web
cargo run -- tui
cargo run -- snapshot --json
cargo run -- doctor
```

## Notes

- The current workspace is selected automatically from the directory you launch from.
- If you launch from a Git worktree, codex-vision also treats sibling worktrees and the main checkout from the same repo as related when it can infer that relationship.
- v1 is read-only by design. Session archive/delete flows stay in `codex-cleaner`.
- `doctor` reports Codex storage detection, Git/tmux/compiler availability, and a graph summary from a real scan.

## Releases

Tagged releases are built by GitHub Actions into downloadable binaries for:

- Linux `x86_64-unknown-linux-gnu`
- Windows `x86_64-pc-windows-msvc`
- macOS `aarch64-apple-darwin`
- macOS `x86_64-apple-darwin`

To publish a release:

```bash
git tag v0.1.0
git push origin v0.1.0
```
