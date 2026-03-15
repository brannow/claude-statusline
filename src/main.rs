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
    // Read JSON from stdin
    let mut buf = String::new();
    if std::io::stdin().read_to_string(&mut buf).is_err() || buf.is_empty() {
        return;
    }

    let input: input::Input = match serde_json::from_str(&buf) {
        Ok(v) => v,
        Err(_) => return,
    };

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

    // Usage limits (fetched async with cache — only for subscription installs)
    let usage_limits = input
        .transcript_path
        .as_deref()
        .and_then(usage::get_usage_limits);

    // Terminal width
    let cols = terminal_width();

    // Render and print
    let (line1, line2) = render::render(&input, &git, &palette, cols, usage_limits.as_ref());
    println!("{}", line1);
    print!("{}", line2);
}

fn terminal_width() -> u16 {
    #[cfg(unix)]
    {
        unsafe {
            let mut ws: libc_winsize = std::mem::zeroed();
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
struct libc_winsize {
    ws_row: u16,
    ws_col: u16,
    ws_xpixel: u16,
    ws_ypixel: u16,
}

#[cfg(unix)]
extern "C" {
    fn ioctl(fd: i32, request: u64, ...) -> i32;
}

#[cfg(target_os = "macos")]
const TIOCGWINSZ: u64 = 0x40087468;

#[cfg(target_os = "linux")]
const TIOCGWINSZ: u64 = 0x5413;
