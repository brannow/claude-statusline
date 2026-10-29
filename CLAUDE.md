# claude-statusline

A Rust binary that Claude Code invokes on each prompt to render a status bar. Claude Code pipes a JSON blob to stdin; the binary parses it, optionally queries git, and writes 2–3 ANSI-colored lines to stdout.

## Build & Install

```bash
make install          # dev build: cargo build --release + cp to /usr/local/bin
make test             # cargo test + installer tests (tests/install_test.sh)
./target/release/claude-statusline          # demo mode (run from TTY)
./target/release/claude-statusline --debug  # dump raw JSON to /tmp/
```

## Release & one-line install

Pushing a tag `vX.Y.Z` (must equal `Cargo.toml`'s version — CI fails otherwise) runs `.github/workflows/release.yml`, which builds `claude-statusline-{darwin,linux}-{arm64,x86_64}` plus `SHA256SUMS` and publishes a GitHub Release. `install.sh` (`curl … | sh`) downloads from `releases/latest`, verifies the checksum, installs to `~/.local/bin`, then runs `claude-statusline --configure`.

- **Why not a Claude Code plugin:** plugin `settings.json` only honors `agent`/`subagentStatusLine`, and `${CLAUDE_PLUGIN_ROOT}` isn't substituted in `statusLine.command` — a plugin can ship the binary but can't activate it, so it adds nothing over this script.
- **Why `--configure` lives in the binary:** safe JSON editing from shell needs `jq`/`python3`, which aren't guaranteed. It writes through symlinks (settings.json is often a dotfiles link), refuses to touch unparseable JSON, keeps other `statusLine` fields (`padding`, `refreshInterval`), backs up first, and honors `CLAUDE_CONFIG_DIR`. serde_json's `preserve_order` keeps the user's key order.
- **Why `--configure`/`--version` run before `catch_unwind`:** the statusline path swallows all errors by design; installer errors must surface as a non-zero exit.
- **Why musl on Linux:** static binary, no glibc version coupling — runs on any distro.
- **Why the up-to-date check compares SHA-256, not versions:** re-running the one-liner is the repair path. A hash match against the release's `SHA256SUMS` proves the binary is current *and* intact (not truncated, damaged, or the wrong arch); a version string can't. `statusLine` is re-checked in every config dir on every run (`CLAUDE_STATUSLINE_CONFIG_DIRS`, colon-separated), and one broken `settings.json` doesn't stop the others.
- **Installer tests:** `tests/install_test.sh` runs `install.sh` against a fake `file://` release in a temp `HOME` (fresh install, every partial-install repair, multi-dir, checksum mismatch). Runs in `make test` and in CI on each native build. `SH=dash` checks Ubuntu's `/bin/sh` locally.
- **Why `~/.local/bin`:** no sudo; `statusLine` gets an absolute path, so it doesn't need to be on `PATH`.

## Architecture

```
src/
  main.rs      Entry point: TTY detection, stdin JSON parse, orchestration
  input.rs     Serde structs matching Claude Code's statusLine JSON schema
  render.rs    Build the 3 output lines from parsed input
  color.rs     Rgb type, 12-color palette, threshold colors
  git.rs       Git branch/status fetch with 5s cache and 500ms timeout
  usage.rs     format_time_until() helper for reset countdowns
  setup.rs     --configure: write statusLine into settings.json (used by install.sh)
  icons.rs     Unicode constants (edges, bar chars) — no Nerd Fonts needed
  platform.rs  Pure-Rust SHA-256 (used for cache keys and palette hashing)
```

### Why stdin instead of API polling for rate limits

Claude Code v2.1.80 added `rate_limits` to the statusLine JSON payload, delivering 5h/7d usage data directly without any HTTP requests. The previous approach used OAuth token lookups + the Anthropic API, which required keychain access, an HTTP client (`ureq`), and a 3-tier cache to avoid hammering the API. All of that is gone — the stdin approach is simpler, faster, and works without any credentials.

### Why no `libc` crate

`ioctl(TIOCGWINSZ)` and `isatty()` are called directly via minimal `extern "C"` bindings. Adding `libc` just for two syscalls adds a compile-time dependency and slows CI. The bindings are stable and the same on macOS/Linux.

### Why standard Unicode only

Nerd Font / Powerline glyphs are invisible on machines without the font installed (e.g. SSH sessions, CI terminals). The bar and edge characters (▐ ▌ ▰ ▱) render correctly everywhere.

### Git cache design

Git is cached per-cwd in a single flat file at `/tmp/claude-statusline-cache` (key = first 12 hex chars of SHA-256(cwd), TTL = 5s, eviction after 1 hour). A separate thread runs the git commands with a 500ms timeout — if git hangs (network mount, huge repo), the statusline returns empty git info rather than blocking.

### Responsive layout

Each line builds a list of segments, then `render_segments` fills them left-to-right until one won't fit, then stops. Segments at the end of the list are therefore dropped first on narrow terminals.

### Why the layout is split across three lines (not two)

The original layout packed everything onto two lines: Line 1 had model + git + **cost**, Line 2 had duration + context bar + **5h/7d usage bars**. On narrow terminals (< ~120 cols) the usage bars got dropped too early because they were competing with the context bar for space on the same line. Cost on Line 1 had the same problem — git status already takes variable width, so cost would vanish unpredictably.

The fix was to separate by information type rather than trying to cram everything in:
- Line 1 = identity (what session / repo / mode am I looking at?)
- Line 2 = session stats (how long / how much context / how much did it cost?)
- Line 3 = quota pressure (how close to rate limits?)

Each line now has a stable, narrow minimum width, so all three lines survive at typical split-pane terminal widths (~80 cols).

## Input schema (relevant fields)

```json
{
  "cwd": "/path/to/project",
  "session_id": "...",
  "model": { "id": "claude-opus-4-6", "display_name": "Opus" },
  "cost": {
    "total_cost_usd": 1.37,
    "total_duration_ms": 2340000,
    "total_lines_added": 246,
    "total_lines_removed": 58
  },
  "context_window": {
    "context_window_size": 200000,
    "used_percentage": 42.0,
    "remaining_percentage": 58.0
  },
  "agent": { "name": "..." },
  "mode": "auto",
  "rate_limits": {
    "five_hour":  { "used_percentage": 42.0, "resets_at": 1234567890 },
    "seven_day":  { "used_percentage": 73.0, "resets_at": 1234567890 }
  },
  "effort": { "level": "xhigh" }
}
```

`rate_limits` requires Claude Code ≥ v2.1.80. Absent fields are handled gracefully — the corresponding UI element is omitted.

### Reasoning effort on Line 1

`effort.level` (`low`/`medium`/`high`/`xhigh`/`max`) is rendered next to the model name, e.g. `Sonnet 5 (xhigh)`, and lives in the same segment so it can't be dropped independently on narrow terminals. Claude Code only populates this field when the current model supports the effort parameter — DeepSeek doesn't, so the field is always absent for it. Since DeepSeek has no real effort level to report, `render.rs` falls back to the `CLAUDE_CODE_EFFORT_LEVEL` env var (the same override var Claude Code itself supports) when the model is DeepSeek, letting a DeepSeek launch wrapper fake a level (e.g. `max`) directly.
