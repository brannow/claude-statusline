use crate::color::{self, Palette, Rgb, L2_BG, L2_DIM, L2_TXT, RST};
use crate::git::GitInfo;
use crate::icons::*;
use crate::input::{Input, RateLimits};
use crate::usage::LimitBar;

/// Returns 2 or 3 lines depending on whether rate limits are available.
/// Line 1: Model, project dir, git branch+status (acts as spacer for Claude Code overlay)
/// Line 2: Duration, context bar, cost, lines changed
/// Line 3 (optional): usage bars — 5h, 7d, plus any other window with usage.
///   `cached_limits` are the windows Claude Code doesn't send on stdin.
pub fn render(
    input: &Input,
    git: &GitInfo,
    palette: &Palette,
    cols: u16,
    rate_limits: Option<&RateLimits>,
    cached_limits: &[LimitBar],
) -> Vec<String> {
    let cols = cols as usize;
    let inner_width = if cols > 2 { cols - 2 } else { cols };

    let line1 = build_line1(input, git, palette, inner_width);
    let line2 = build_line2(input, inner_width);
    let line3 = build_line3(inner_width, rate_limits, cached_limits);

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
    let model_id = input.model.as_ref().and_then(|m| m.id.as_deref()).unwrap_or("");
    let dir = input.cwd.as_deref().unwrap_or("~")
        .rsplit('/').next().unwrap_or("~");

    let mut segments: Vec<String> = Vec::new();

    // Model (+ reasoning effort, if reported). DeepSeek doesn't support the
    // effort parameter so Claude Code never populates `effort.level` for it —
    // fall back to CLAUDE_CODE_EFFORT_LEVEL, which the DeepSeek launch wrapper
    // sets directly since there's no real session value to read.
    let effort = input.effort.as_ref()
        .and_then(|e| e.level.clone())
        .or_else(|| {
            if crate::pricing::detect(model_id).is_some() {
                std::env::var("CLAUDE_CODE_EFFORT_LEVEL").ok()
            } else {
                None
            }
        });
    match effort {
        Some(level) if !level.is_empty() => {
            segments.push(format!("{txt_bold}{model_name}{RST}{bg} {txt}({level}){RST}{bg}"));
        }
        _ => {
            segments.push(format!("{txt_bold}{model_name}{RST}{bg}"));
        }
    }

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

    // Cost — DeepSeek sessions sum the transcript's per-message usage and price
    // it at DeepSeek rates; everything else uses Claude Code's own total.
    let model_id = input.model.as_ref().and_then(|m| m.id.as_deref());
    let token_cost = model_id.and_then(|id| {
        crate::pricing::detect(id)?;
        Some(crate::pricing::get_accumulated_cost(id, input.transcript_path.as_deref()))
    });
    let cost = token_cost.unwrap_or_else(||
        input.cost.as_ref().and_then(|c| c.total_cost_usd).unwrap_or(0.0)
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

/// Bar widths to try, widest first; 0 means label + percentage only.
const LIMIT_BAR_WIDTHS: [u64; 4] = [15, 10, 6, 0];

fn build_line3(max_width: usize, rate_limits: Option<&RateLimits>, cached_limits: &[LimitBar]) -> Option<String> {
    let bars = limit_bars(rate_limits?, cached_limits);
    if bars.is_empty() {
        return None;
    }

    // Shrink all bars together until the line fits: an extra window should
    // cost bar resolution, not knock a whole segment off a narrow terminal.
    let mut segments = Vec::new();
    for bar_width in LIMIT_BAR_WIDTHS {
        segments = bars.iter().map(|bar| limit_segment(bar, bar_width)).collect();
        if segments_width(&segments) <= max_width {
            break;
        }
    }

    Some(render_segments(&segments, max_width, &L2_BG.bg(), &L2_DIM.fg()))
}

/// 5h and 7d are always shown; every other window only once it has usage —
/// most accounts sit at 0% on most of them, and an empty bar is just noise.
fn limit_bars(limits: &RateLimits, cached_limits: &[LimitBar]) -> Vec<LimitBar> {
    let rank = |key: &str| match key {
        "five_hour" => 0,
        "seven_day" => 1,
        _ => 2,
    };
    let mut windows: Vec<_> = limits.0.iter().collect();
    windows.sort_by_key(|(key, _)| rank(key)); // stable: the rest keep payload order

    let mut bars: Vec<LimitBar> = windows
        .into_iter()
        .filter_map(|(key, window)| {
            let pct = window.used_percentage.round() as u64;
            if rank(key) == 2 && pct == 0 {
                return None;
            }
            Some(LimitBar {
                label: window_label(key),
                pct,
                resets_at: window.resets_at,
                stale: false,
            })
        })
        .collect();

    // The snapshot never opens Line 3 on its own: without live data from
    // stdin there'd be a lone, possibly stale bar and no 5h/7d beside it.
    if !bars.is_empty() {
        bars.extend(cached_limits.iter().cloned());
    }
    bars
}

/// `five_hour` → `5h`, `seven_day_opus` → `7d opus`, `spend_limit` → `spend limit`.
/// Unknown keys can't have a hand-picked label, so the key itself has to do.
fn window_label(key: &str) -> String {
    let label = key.replace("five_hour", "5h").replace("seven_day", "7d").replace('_', " ");
    crate::usage::clean_label(&label)
}

fn limit_segment(bar: &LimitBar, bar_width: u64) -> String {
    let bg = L2_BG.bg();
    let txt = L2_TXT.fg();
    let dim = L2_DIM.fg();

    let clr = color::pct_color(bar.pct);
    let clr_fg = clr.fg();
    let gauge = if bar_width > 0 {
        format!("{} ", make_bar(bar.pct, bar_width, &clr, &L2_DIM))
    } else {
        String::new()
    };
    let approx = if bar.stale { "~" } else { "" };
    let reset = crate::usage::format_time_until(bar.resets_at);

    format!(
        "{txt}{label}{RST}{bg} {gauge}{clr_fg}{approx}{pct}%{RST}{bg} {dim}{reset}{RST}{bg}",
        label = bar.label,
        pct = bar.pct,
    )
}

// ── Shared helpers ──────────────────────────────────────────────────────────

/// Columns `render_segments` needs to show every segment.
fn segments_width(segments: &[String]) -> usize {
    let content: usize = segments.iter().map(|s| visible_width(s)).sum();
    content + 1 + 3 * segments.len().saturating_sub(1) // leading space + separators
}

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

#[cfg(test)]
mod tests {
    use super::*;

    fn limits(json: &str) -> RateLimits {
        serde_json::from_str(json).unwrap()
    }

    /// Two days out, so countdowns have a realistic width (`2d0h`).
    fn reset() -> u64 {
        let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs();
        now + 172_830
    }

    fn fable(stale: bool) -> LimitBar {
        LimitBar { label: "7d Fable".into(), pct: 5, resets_at: Some(reset()), stale }
    }

    /// What stdin carries today: the two live windows.
    fn base() -> RateLimits {
        let reset = reset();
        limits(&format!(
            r#"{{ "five_hour": {{ "used_percentage": 18, "resets_at": {reset} }},
                 "seven_day": {{ "used_percentage": 34, "resets_at": {reset} }} }}"#
        ))
    }

    /// The line as the terminal shows it: escape sequences removed.
    fn plain(line: &str) -> String {
        let mut out = String::new();
        let mut in_escape = false;
        for c in line.chars() {
            if in_escape { if c.is_ascii_alphabetic() { in_escape = false; } continue; }
            if c == '\x1b' { in_escape = true; continue; }
            out.push(c);
        }
        out
    }

    fn bar_cells(line: &str) -> usize {
        line.matches(BAR_FILLED).count() + line.matches(BAR_EMPTY).count()
    }

    #[test]
    fn base_windows_show_at_zero_other_windows_only_with_usage() {
        let limits = limits(
            r#"{ "seven_day_opus": { "used_percentage": 0 },
                 "spend_limit": { "used_percentage": 12.4 },
                 "seven_day": { "used_percentage": 0 },
                 "five_hour": { "used_percentage": 0 } }"#,
        );
        let line = plain(&build_line3(200, Some(&limits), &[]).unwrap());

        let (five, seven, spend) = (line.find("5h ").unwrap(), line.find("7d ").unwrap(), line.find("spend limit ").unwrap());
        assert!(five < seven && seven < spend, "5h, 7d, then the rest: {line}");
        assert!(line.contains("12%"), "{line}");
        assert!(!line.contains("opus"), "{line}");
    }

    #[test]
    fn cached_windows_follow_the_live_ones() {
        let line = plain(&build_line3(200, Some(&base()), &[fable(false)]).unwrap());
        assert!(line.contains("34%"), "{line}");
        assert!(line.contains("7d Fable"), "{line}");
        assert!(line.contains(" 5% "), "{line}");
        assert_eq!(bar_cells(&line), 45, "three full-width bars: {line}");
    }

    #[test]
    fn stale_snapshot_values_are_marked() {
        let line = plain(&build_line3(200, Some(&base()), &[fable(true)]).unwrap());
        assert!(line.contains("~5%"), "{line}");
        assert!(line.contains(" 34%"), "live values stay unmarked: {line}");
    }

    #[test]
    fn snapshot_alone_does_not_open_the_line() {
        assert!(build_line3(200, None, &[fable(false)]).is_none());
        assert!(build_line3(200, Some(&limits("{}")), &[fable(false)]).is_none());
    }

    #[test]
    fn bars_shrink_before_a_window_is_dropped() {
        let limits = base();
        let cached = [fable(false)];

        // 78 = inner width of an 80-column terminal.
        let narrow = plain(&build_line3(78, Some(&limits), &cached).unwrap());
        assert!(narrow.contains("7d Fable"), "{narrow}");
        assert!(narrow.chars().count() <= 78, "{narrow}");
        assert!(bar_cells(&narrow) < 45, "{narrow}");

        // Too narrow even for bars: percentages only, all three still there.
        let tiny = plain(&build_line3(50, Some(&limits), &cached).unwrap());
        assert_eq!(bar_cells(&tiny), 0, "{tiny}");
        assert!(tiny.contains("18%") && tiny.contains("34%") && tiny.contains("7d Fable 5%"), "{tiny}");
    }
}
