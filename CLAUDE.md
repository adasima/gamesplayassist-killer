# CLAUDE.md

Resident Windows tool that terminates DMM GAME PLAYER's `GamesPlayAssist.exe`, whose global
hotkeys swallow key input before it reaches the running game.

Background, measurements, and the reasoning behind every design choice live in
`docs/investigation.md`. Read it before changing behaviour — most of what looks like an
arbitrary decision is backed by a measurement recorded there.

## Invariants

Breaking any of these defeats the purpose of the tool.

- **No elevation.** The target runs at Medium integrity under the same user, so
  `PROCESS_TERMINATE` succeeds unelevated. Never introduce anything that needs a UAC prompt.
- **No hardcoded game executable.** The game's path is read from the target's own
  `--game-exe-file-path` argument, which keeps the tool working for every DMM title.
- **Kill only once the game is already running** (unless `--force`). The target may take part
  in the launch sequence; killing it while the game is up is the only case verified as safe.
- **x64 only.** `proc::command_line` walks the PEB using hardcoded x64 offsets. The
  `compile_error!` in `main.rs` guards this.
- **Single purpose.** No tray icon, no config file, no logging. Feature requests that add
  surface area to a permanently resident process should be declined.

## Layout

- `src/main.rs` — CLI dispatch, the watch loop, command-line parsing, message boxes
- `src/proc.rs` — process snapshot, PEB command-line read, terminate, single-instance mutex
- `src/autostart.rs` — `HKCU\...\Run` registration for `--install` / `--uninstall`

`#![cfg_attr(not(test), windows_subsystem = "windows")]` keeps the release build console-free
while leaving test output visible. Keep the `not(test)` guard.

## Verifying

```
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test
```

Unit tests cover argument parsing against a real captured command line, plus PEB reading and
process enumeration against the current process. CI runs the same three commands.

The end-to-end behaviour cannot be unit tested — it needs DMM GAME PLAYER and a game running.
The manual check is: start the release binary, then confirm `GamesPlayAssist.exe` is gone and
the game process is still alive.

## Publishing

Pushing a `v*` tag builds on `windows-latest` and attaches the exe to a GitHub release.

Never commit values captured from a live session — the target's real command line contains a
`--cid` account identifier. The sample in the tests and in `docs/investigation.md` has it
stripped; keep it that way.
