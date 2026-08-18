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
- **Register a path the user does not own.** `--install` registers the `%LOCALAPPDATA%` copy,
  never `current_exe()` as invoked. Registering the invocation path means a Run entry pointing
  into the Downloads folder, which breaks silently the moment the user tidies up.

## Layout

- `src/main.rs` — CLI dispatch, the watch loop, command-line parsing, message boxes
- `src/proc.rs` — process snapshot, PEB command-line read, terminate, single-instance mutex
- `dist/` — files bundled into the release zip alongside the exe. `install.cmd` and
  `uninstall.cmd` only pass `--install` / `--uninstall` through; keep them ASCII, since
  `cmd` reads them in the OEM code page
- `src/autostart.rs` — `--install` / `--uninstall`: copies the exe into
  `%LOCALAPPDATA%\gamesplayassist-killer\`, registers that copy under `HKCU\...\Run`,
  and starts it immediately

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

Pushing a `v*` tag builds on `windows-latest`, zips `dist/` together with the release exe,
and attaches `gamesplayassist-killer-<tag>.zip` to a GitHub release. The zip is the
distribution: this is an install tool set, not a portable exe. Everything in it is
disposable once `--install` has run.
The workflow refuses tags that disagree with `version` in `Cargo.toml`, so bump both together.

Verify `--install` on real hardware before tagging: it is the one path with no unit test
coverage that touches the registry, the filesystem, and process creation at once.

Never commit values captured from a live session — the target's real command line contains a
`--cid` account identifier. The sample in the tests and in `docs/investigation.md` has it
stripped; keep it that way.
