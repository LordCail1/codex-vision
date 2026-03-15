# codex-vision

Local Codex CLI session visualizer with a live graph UI and terminal inspector.

## What It Does

- Reads local Codex session metadata from `~/.codex`
- Reconstructs fork lineage from rollout metadata
- Detects live sessions from active Codex processes
- Annotates active sessions with tmux pane labels when tmux is available
- Serves a browser graph UI and a terminal inspector from the same Rust core

## Commands

```bash
cargo run -- web
cargo run -- web --all
cargo run -- tui
cargo run -- snapshot --json
```

The web UI binds to `127.0.0.1` and opens a browser unless you pass `--no-open`.

## Notes

- The current workspace is selected automatically from the directory you launch from.
- If you launch from a Git worktree, codex-vision also treats sibling worktrees and the main checkout from the same repo as related when it can infer that relationship.
- v1 is read-only by design. Session archive/delete flows stay in `codex-cleaner`.

## Build Requirements

Rust plus a normal system C toolchain are required for local builds because SQLite is compiled as part of the dependency graph.
