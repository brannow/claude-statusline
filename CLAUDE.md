# claude-statusline

A Rust binary that Claude Code invokes on each prompt to render a status bar. Claude Code pipes a JSON blob to stdin; the binary parses it, optionally queries git, and writes 2–3 ANSI-colored lines to stdout.

## Build & Install

```bash
make install          # dev build: cargo build --release + install to ~/.local/bin
make install INSTALL_DIR=/usr/local/bin   # …or anywhere else
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
- **Why `make install` uses the same dir (and `CLAUDE_STATUSLINE_INSTALL_DIR`):** it used to copy to `/usr/local/bin` while `statusLine` pointed at the installer's `~/.local/bin`, so a dev build silently never ran. Note the installer's SHA check treats a dev build as "damaged" and restores the release on its next run.

## Architecture

```
src/
  main.rs      Entry point: TTY detection, stdin JSON parse, orchestration
  input.rs     Serde structs matching Claude Code's statusLine JSON schema
  render.rs    Build the 3 output lines from parsed input
  color.rs     Rgb type, 12-color palette, threshold colors
  git.rs       Git branch/status fetch with 5s cache and 500ms timeout
  usage.rs     Extra limit bars from Claude Code's usage snapshot + reset countdowns
  pricing.rs   DeepSeek session cost: transcript token usage × DeepSeek rates (memo in /tmp)
  setup.rs     --configure: write statusLine into settings.json (used by install.sh)
  icons.rs     Unicode constants (edges, bar chars) — no Nerd Fonts needed
  platform.rs  Pure-Rust SHA-256 (used for cache keys and palette hashing)
```

### Why stdin instead of API polling for rate limits

Claude Code v2.1.80 added `rate_limits` to the statusLine JSON payload, delivering 5h/7d usage data directly without any HTTP requests. The previous approach used OAuth token lookups + the Anthropic API, which required keychain access, an HTTP client (`ureq`), and a 3-tier cache to avoid hammering the API. All of that is gone — the stdin approach is simpler, faster, and works without any credentials.

### Line 3 is an open list of limit windows, fed by two sources

Anthropic adds limit windows without notice (per-model weekly buckets like "Current week (Fable)", gateway spend limits), so nothing on Line 3 is hard-coded to 5h/7d. Every window becomes a `LimitBar`; **5h and 7d are always shown, every other window only once it rounds to ≥ 1%** — most accounts sit at 0% on most of them, and a permanently empty bar is noise.

**Source 1 — stdin `rate_limits` (live).** Parsed as an open map, not named fields, so a key Claude Code starts sending tomorrow renders without a release. Its label is derived from the key (`seven_day_opus` → `7d opus`). Entries are read leniently (anything without a numeric `used_percentage` is skipped) because a strict parse would let one unexpected value fail the whole payload and blank the statusline.

**Source 2 — the usage snapshot in `.claude.json` (not live).** Verified against Claude Code 2.1.329: the statusLine payload only ever carries `five_hour`, `seven_day` and, on gateways, `spend_limit`. The per-model windows that `/usage` shows never reach stdin. They exist in exactly one place we can read without credentials: `cachedUsageUtilization.utilization.limits[]` in `$CLAUDE_CONFIG_DIR/.claude.json` (fallback `~/.claude.json`), which is what the `/usage` dialog itself renders from. Reading it keeps the "no keychain, no HTTP" property above; calling the usage endpoint ourselves would bring all of that back.

That snapshot is only rewritten when Claude Code fetches the usage endpoint (e.g. opening `/usage`) — *not* on every response. Everything about how it is used follows from that:

- **stdin stays authoritative for 5h/7d.** The snapshot's `session` / `weekly_all` entries are ignored; they are older copies of the same windows.
- **`~5%` marks a snapshot older than 1 hour** — the age at which Claude Code stops trusting it itself. We keep showing it (a weekly window moves slowly, and a hidden bar tells you less than an approximate one); opening `/usage` refreshes it.
- **Dropped when `resets_at` has passed** — the number belongs to the previous window — **or when the snapshot's `accountUuid` differs from `oauthAccount`** (left over from another login).
- **Never opens Line 3 on its own.** Without live stdin data there would be a lone, possibly stale bar with no 5h/7d beside it.
- **Undocumented internal format.** Every field is read leniently and any surprise yields no extra bars — the feature degrades to plain 5h/7d, never to an error. Labels are server-supplied text printed to a terminal, so control characters are stripped and length is capped.

If Claude Code ever forwards these windows on stdin, Source 1 picks them up live with no code change; the same window may then appear twice (once per source) until Source 2 is retired.

### Why no `libc` crate

`ioctl(TIOCGWINSZ)` and `isatty()` are called directly via minimal `extern "C"` bindings. Adding `libc` just for two syscalls adds a compile-time dependency and slows CI. The bindings are stable and the same on macOS/Linux.

### Why standard Unicode only

Nerd Font / Powerline glyphs are invisible on machines without the font installed (e.g. SSH sessions, CI terminals). The bar and edge characters (▐ ▌ ▰ ▱) render correctly everywhere.

### Git cache design

Git is cached per-cwd in a single flat file at `/tmp/claude-statusline-cache` (key = first 12 hex chars of SHA-256(cwd), TTL = 5s, eviction after 1 hour). A separate thread runs the git commands with a 500ms timeout — if git hangs (network mount, huge repo), the statusline returns empty git info rather than blocking.

### Responsive layout

Each line builds a list of segments, then `render_segments` fills them left-to-right until one won't fit, then stops. Segments at the end of the list are therefore dropped first on narrow terminals.

Line 3 gets one extra step first, because its segment count is not fixed: all bars shrink together (15 → 10 → 6 cells → percentage only) until the line fits. A third or fourth window should cost bar resolution, not push a whole window off an 80-column terminal.

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
  "transcript_path": "/path/to/session.jsonl",
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

`rate_limits` requires Claude Code ≥ v2.1.80. Absent fields are handled gracefully — the corresponding UI element is omitted. `five_hour` / `seven_day` are just the keys known today: any other key holding `{ "used_percentage": … }` is rendered too (see "Line 3 is an open list of limit windows").

### Reasoning effort on Line 1

`effort.level` (`low`/`medium`/`high`/`xhigh`/`max`) is rendered next to the model name, e.g. `Sonnet 5 (xhigh)`, and lives in the same segment so it can't be dropped independently on narrow terminals. Claude Code only populates this field when the current model supports the effort parameter — DeepSeek doesn't, so the field is always absent for it. Since DeepSeek has no real effort level to report, `render.rs` falls back to the `CLAUDE_CODE_EFFORT_LEVEL` env var (the same override var Claude Code itself supports) when the model is DeepSeek, letting a DeepSeek launch wrapper fake a level (e.g. `max`) directly.
