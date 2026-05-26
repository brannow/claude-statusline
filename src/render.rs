use crate::color::{self, Palette, Rgb, L2_BG, L2_DIM, L2_TXT, RST};
use crate::git::GitInfo;
use crate::icons::*;
use crate::input::{Input, RateLimits};

/// Returns 2 or 3 lines depending on whether rate limits are available.
/// Line 1: Model, project dir, git branch+status (acts as spacer for Claude Code overlay)
/// Line 2: Duration, context bar, cost, lines changed
/// Line 3 (optional): 5h and 7d usage bars
pub fn render(input: &Input, git: &GitInfo, palette: &Palette, cols: u16, rate_limits: Option<&RateLimits>) -> Vec<String> {
    let cols = cols as usize;
    let inner_width = if cols > 2 { cols - 2 } else { cols };

    let line1 = build_line1(input, git, palette, inner_width);
    let line2 = build_line2(input, inner_width);
    let line3 = build_line3(inner_width, rate_limits);

    let mut lines = Vec::new();

    let fg = palette.base.fg();
    lines.push(format!(
        "{RST}{fg}{EDGE_LEFT}{line1}{RST}{fg}{EDGE_RIGHT}\x1b[?7h",
    ));

    let l2_fg = L2_BG.fg();
    lines.push(format!(
        "{RST}{l2_fg}{EDGE_LEFT}{line2}{RST}{l2_fg}{EDGE_RIGHT}\x1b[?7h",
    ));

    if let Some(l3) = line3 {
        lines.push(format!(
            "{RST}{l2_fg}{EDGE_LEFT}{l3}{RST}{l2_fg}{EDGE_RIGHT}\x1b[?7h",
        ));
    }

    lines
}

// ── Line 1: Identity ────────────────────────────────────────────────────────

fn build_line1(input: &Input, git: &GitInfo, p: &Palette, max_width: usize) -> String {
    let bg = p.base.bg();
    let txt = p.txt.fg();
    let txt_bold = p.txt.fg_bold();
    let sep_clr = p.sep.fg();

    let model_name = input.model.as_ref()
        .and_then(|m| m.display_name.as_deref()).unwrap_or("Claude");
    let dir = input.cwd.as_deref().unwrap_or("~")
        .rsplit('/').next().unwrap_or("~");

    let mut segments: Vec<String> = Vec::new();

    // Model
    segments.push(format!("{txt_bold}{model_name}{RST}{bg}"));

    // Project directory
    segments.push(format!("{txt}{dir}{RST}{bg}"));

    // Git branch + status
    if let Some(branch) = &git.branch {
        let mut git_str = format!("{txt}{branch}{RST}{bg}");
        if git.has_status() {
            let mut parts = Vec::new();
            if git.staged > 0 { parts.push(format!("+{}", git.staged)); }
            if git.modified > 0 { parts.push(format!("!{}", git.modified)); }
            if git.untracked > 0 { parts.push(format!("?{}", git.untracked)); }
            git_str.push_str(&format!(" {txt}{}{RST}{bg}", parts.join(" ")));
        }
        segments.push(git_str);
    }

    // Agent name
    if let Some(agent) = input.agent.as_ref().and_then(|a| a.name.as_deref()) {
        if !agent.is_empty() {
            segments.push(format!("{txt}{agent}{RST}{bg}"));
        }
    }

    // Mode
    if let Some(mode) = &input.mode {
        if !mode.is_empty() {
            let mode_clr = Rgb(150, 100, 0).fg_bold();
            segments.push(format!("{mode_clr}{mode}{RST}{bg}"));
        }
    }

    render_segments(&segments, max_width, &bg, &sep_clr)
}

// ── Line 2: Session stats ───────────────────────────────────────────────────

fn build_line2(input: &Input, max_width: usize) -> String {
    let bg = L2_BG.bg();
    let txt = L2_TXT.fg();
    let dim = L2_DIM.fg();

    let mut segments: Vec<String> = Vec::new();

    // Duration
    let duration_ms = input.cost.as_ref().and_then(|c| c.total_duration_ms).unwrap_or(0);
    let total_sec = duration_ms / 1000;
    let h = total_sec / 3600;
    let m = (total_sec % 3600) / 60;
    let secs = total_sec % 60;
    let time_str = if h > 0 { format!("{}h{}m", h, m) }
    else if m > 0 { format!("{}m{}s", m, secs) }
    else { format!("{}s", secs) };
    let time_clr = color::duration_color(h);
    let tc = time_clr.fg();
    segments.push(format!("{tc}{time_str}{RST}{bg}"));

    // Context bar
    const COMPACT_BUFFER: u64 = 33_000;
    let ctx_size_raw = input.context_window.as_ref()
        .and_then(|c| c.context_window_size).unwrap_or(200_000);
    let ctx_usable = ctx_size_raw.saturating_sub(COMPACT_BUFFER);
    let ctx_usable_k = ctx_usable / 1000;
    let tokens_used = input.context_window.as_ref()
        .and_then(|c| c.used_percentage).map(|p| (ctx_size_raw as f64 * p / 100.0) as u64).unwrap_or(0);
    let pct = if ctx_usable > 0 { (tokens_used * 100 / ctx_usable).min(100) } else { 0 };
    let ctx_clr = color::pct_color(pct);
    let cc = ctx_clr.fg();
    let bar = make_bar(pct, 20, &ctx_clr, &L2_DIM);
    segments.push(format!("{bar} {cc}{pct}%{RST}{bg} {txt}of {ctx_usable_k}k{RST}{bg}"));

    // Cost — calculate from tokens if DeepSeek model, otherwise use JSON-provided
    let model_id = input.model.as_ref().and_then(|m| m.id.as_deref());
    let token_cost = model_id.and_then(|id| {
        let usage = input.context_window.as_ref()?.current_usage.as_ref()?;
        let input_tok = usage.input_tokens?;
        let cache_read = usage.cache_read_input_tokens.unwrap_or(0);
        let output_tok = usage.output_tokens?;
        crate::pricing::calculate_cost(id, input_tok, cache_read, output_tok)
    });
    let cost = token_cost.unwrap_or(
        input.cost.as_ref().and_then(|c| c.total_cost_usd).unwrap_or(0.0),
    );
    if cost > 0.0001 {
        let cost_str = if cost < 0.01 {
            format!("${:.4}", cost)
        } else if cost < 10.0 {
            format!("${:.2}", cost)
        } else {
            format!("${:.1}", cost)
        };
        let cost_bold = L2_TXT.fg_bold();
        segments.push(format!("{cost_bold}{cost_str}{RST}{bg}"));
    }

    render_segments(&segments, max_width, &bg, &dim)
}

// ── Line 3: Usage limits (subscription only) ────────────────────────────────

fn build_line3(max_width: usize, rate_limits: Option<&RateLimits>) -> Option<String> {
    let limits = rate_limits?;

    // Need at least 5h data to show this line
    let five = limits.five_hour.as_ref()?;
    let five_pct = five.used_percentage?;

    let bg = L2_BG.bg();
    let txt = L2_TXT.fg();
    let dim = L2_DIM.fg();

    let mut segments: Vec<String> = Vec::new();

    // 5h usage
    let fp = five_pct.round() as u64;
    let fc = color::pct_color(fp);
    let fc_fg = fc.fg();
    let fr = crate::usage::format_time_until(five.resets_at);
    let bar5 = make_bar(fp, 15, &fc, &L2_DIM);
    segments.push(format!("{txt}5h{RST}{bg} {bar5} {fc_fg}{fp}%{RST}{bg} {dim}{fr}{RST}{bg}"));

    // 7d usage (omit if absent)
    if let Some(seven) = &limits.seven_day {
        if let Some(seven_pct) = seven.used_percentage {
            let sp = seven_pct.round() as u64;
            let sc = color::pct_color(sp);
            let sc_fg = sc.fg();
            let sr = crate::usage::format_time_until(seven.resets_at);
            let bar7 = make_bar(sp, 15, &sc, &L2_DIM);
            segments.push(format!("{txt}7d{RST}{bg} {bar7} {sc_fg}{sp}%{RST}{bg} {dim}{sr}{RST}{bg}"));
        }
    }

    Some(render_segments(&segments, max_width, &bg, &dim))
}

// ── Shared helpers ──────────────────────────────────────────────────────────

/// Simple left-to-right rendering: segments separated by │, padded with spaces.
/// No flex, no proportional distribution — just natural widths.
fn render_segments(segments: &[String], max_width: usize, bg: &str, sep_clr: &str) -> String {
    let mut out = format!("{RST}{bg}");
    let mut used = 0;

    for (i, seg) in segments.iter().enumerate() {
        let seg_width = visible_width(seg);
        let needed = if i > 0 { seg_width + 3 } else { seg_width + 1 }; // sep + spaces

        if used + needed > max_width {
            break; // don't render segments that won't fit
        }

        if i > 0 {
            out.push_str(&format!(" {sep_clr}│{RST}{bg} "));
            used += 3;
        } else {
            out.push(' ');
            used += 1;
        }

        out.push_str(seg);
        used += seg_width;
    }

    out
}

fn make_bar(pct: u64, width: u64, fill_clr: &Rgb, empty_clr: &Rgb) -> String {
    let filled = if pct > 0 { (pct * width / 100).max(1).min(width) } else { 0 };
    let empty = width - filled;
    let fc = fill_clr.fg();
    let ec = empty_clr.fg();
    let mut bar = String::new();
    for _ in 0..filled { bar.push_str(&fc); bar.push_str(BAR_FILLED); }
    for _ in 0..empty { bar.push_str(&ec); bar.push_str(BAR_EMPTY); }
    bar
}

fn visible_width(s: &str) -> usize {
    let mut width = 0;
    let mut in_escape = false;
    for c in s.chars() {
        if in_escape { if c.is_ascii_alphabetic() { in_escape = false; } continue; }
        if c == '\x1b' { in_escape = true; continue; }
        width += 1;
    }
    width
}
