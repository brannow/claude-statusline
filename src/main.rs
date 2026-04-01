#![allow(dead_code)]

mod color;
mod git;
mod icons;
mod input;
mod platform;
mod render;
mod usage;

use std::io::Read;
use std::time::Duration;

fn main() {
    // Statusline must never crash — catch panics and exit silently
    let _ = std::panic::catch_unwind(run);
}

fn run() {
    let debug = std::env::args().any(|a| a == "--debug");
    let is_tty = unsafe { isatty(0) } != 0;

    let (raw_json, input): (Option<String>, input::Input) = if is_tty {
        // Interactive terminal — show preview with test data
        (None, demo_input())
    } else {
        // Piped from Claude Code — read JSON from stdin
        let mut buf = String::new();
        if std::io::stdin().read_to_string(&mut buf).is_err() || buf.is_empty() {
            return;
        }
        let raw = if debug { Some(buf.clone()) } else { None };
        match serde_json::from_str(&buf) {
            Ok(v) => (raw, v),
            Err(_) => return,
        }
    };

    if debug {
        dump_debug(&raw_json, &input);
    }

    // Determine palette key (session_id > cwd > fallback)
    let palette_key = input
        .session_id
        .as_deref()
        .or(input.cwd.as_deref())
        .unwrap_or("default");
    let palette = color::palette_for(palette_key);

    // Git info with timeout
    let cwd = input.cwd.as_deref().unwrap_or(".");
    let git = git::get_info_with_timeout(cwd, Duration::from_millis(500));

    // Terminal width
    let cols = terminal_width();

    // Render and print
    let lines = render::render(&input, &git, &palette, cols, input.rate_limits.as_ref());
    for (i, line) in lines.iter().enumerate() {
        let is_last = i == lines.len() - 1;
        if is_last && !is_tty {
            print!("{}", line);
        } else {
            println!("{}", line);
        }
    }
}

fn dump_debug(raw_json: &Option<String>, input: &input::Input) {
    use std::fs;
    use std::io::Write;

    let session_hash = input
        .session_id
        .as_deref()
        .map(|s| platform::sha256_hex(s)[..8].to_string())
        .unwrap_or_else(|| "unknown".to_string());

    let debug_dir = format!("/tmp/claude-statusline-debug-{}", session_hash);
    let _ = fs::create_dir_all(&debug_dir);

    // Append raw JSON (one line per invocation)
    if let Some(json) = raw_json {
        if let Ok(mut f) = fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(format!("{}/raw.jsonl", debug_dir))
        {
            // Compact to single line
            let compact = json.lines()
                .map(|l| l.trim())
                .collect::<Vec<_>>()
                .join("");
            let _ = writeln!(f, "{}", compact);
        }
    }

    // Write latest pretty-printed for quick inspection
    if let Ok(pretty) = serde_json::to_string_pretty(&serde_json::json!({
        "cwd": input.cwd,
        "session_id": input.session_id,
        "model": input.model.as_ref().map(|m| serde_json::json!({
            "id": m.id,
            "display_name": m.display_name,
        })),
        "cost": input.cost.as_ref().map(|c| serde_json::json!({
            "total_cost_usd": c.total_cost_usd,
            "total_duration_ms": c.total_duration_ms,
            "total_lines_added": c.total_lines_added,
            "total_lines_removed": c.total_lines_removed,
        })),
        "context_window": input.context_window.as_ref().map(|cw| serde_json::json!({
            "context_window_size": cw.context_window_size,
            "used_percentage": cw.used_percentage,
            "remaining_percentage": cw.remaining_percentage,
        })),
        "transcript_path": input.transcript_path,
    })) {
        let _ = fs::write(format!("{}/latest.json", debug_dir), pretty);
    }
}

fn demo_input() -> input::Input {
    let cwd = std::env::current_dir()
        .map(|p| p.to_string_lossy().into_owned())
        .ok();

    input::Input {
        cwd,
        session_id: Some("demo-preview".into()),
        model: Some(input::Model {
            id: Some("claude-opus-4-6".into()),
            display_name: Some("Opus".into()),
        }),
        cost: Some(input::Cost {
            total_cost_usd: Some(1.37),
            total_duration_ms: Some(2_340_000), // 39 minutes
            total_lines_added: Some(246),
            total_lines_removed: Some(58),
        }),
        context_window: Some(input::ContextWindow {
            context_window_size: Some(200_000),
            used_percentage: Some(42.0),
            remaining_percentage: Some(58.0),
            current_usage: None,
        }),
        rate_limits: Some(input::RateLimits {
            five_hour: Some(input::RateLimitWindow {
                used_percentage: Some(42.0),
                resets_at: Some(
                    std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .map(|d| d.as_secs() + 7200) // 2h from now
                        .unwrap_or(0),
                ),
            }),
            seven_day: Some(input::RateLimitWindow {
                used_percentage: Some(73.0),
                resets_at: Some(
                    std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .map(|d| d.as_secs() + 172800) // 2d from now
                        .unwrap_or(0),
                ),
            }),
        }),
        ..Default::default()
    }
}

fn terminal_width() -> u16 {
    #[cfg(unix)]
    {
        unsafe {
            let mut ws: LibcWinsize = std::mem::zeroed();
            if ioctl(1, TIOCGWINSZ, &mut ws) == 0 && ws.ws_col > 0 {
                return ws.ws_col;
            }
        }
    }
    120
}

// Minimal libc bindings — avoids adding a libc crate dependency
#[cfg(unix)]
#[repr(C)]
struct LibcWinsize {
    ws_row: u16,
    ws_col: u16,
    ws_xpixel: u16,
    ws_ypixel: u16,
}

#[cfg(unix)]
extern "C" {
    fn ioctl(fd: i32, request: u64, ...) -> i32;
    fn isatty(fd: i32) -> i32;
}

#[cfg(target_os = "macos")]
const TIOCGWINSZ: u64 = 0x40087468;

#[cfg(target_os = "linux")]
const TIOCGWINSZ: u64 = 0x5413;
