# claude-statusline

A Rust binary that Claude Code invokes on each prompt to render a status bar. Claude Code pipes a JSON blob to stdin; the binary parses it, optionally queries git, and writes 2–3 ANSI-colored lines to stdout.

## Build & Install

```bash
make install          # cargo build --release + cp to /usr/local/bin
cargo test            # run unit tests
./target/release/claude-statusline          # demo mode (run from TTY)
./target/release/claude-statusline --debug  # dump raw JSON to /tmp/
```

## Architecture

```
src/
  main.rs      Entry point: TTY detection, stdin JSON parse, orchestration
  input.rs     Serde structs matching Claude Code's statusLine JSON schema
  render.rs    Build the 3 output lines from parsed input
  color.rs     Rgb type, 12-color palette, threshold colors
  git.rs       Git branch/status fetch with 5s cache and 500ms timeout
  usage.rs     format_time_until() helper for reset countdowns
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
  }
}
```

`rate_limits` requires Claude Code ≥ v2.1.80. Absent fields are handled gracefully — the corresponding UI element is omitted.
