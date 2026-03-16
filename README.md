# claude-statusline

A custom statusline for [Claude Code](https://docs.anthropic.com/en/docs/claude-code) written in Rust. Shows model, cost, duration, git branch, context usage, and more — rendered as a two-line bar with ANSI colors.

![statusbar.png](statusbar.png)

Run it directly to see a demo preview with test data:

```
claude-statusline
```

## Build & Install

```
make install
```

Installs to `/usr/local/bin/claude-statusline`.

## Configure Claude Code

Add to `~/.claude/settings.json`:

```json
{
  "env": {
    "CLAUDE_CODE_STATUSLINE_COMMAND": "claude-statusline"
  }
}
```

## Debug

Pass `--debug` to dump raw JSON input to `/tmp/claude-statusline-debug-<session>/`.
