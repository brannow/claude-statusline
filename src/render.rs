use crate::color::{self, Palette, Rgb, ADDED_CLR, L2_BG, L2_DIM, L2_TXT, REMOVED_CLR, RST};
use crate::git::GitInfo;
use crate::icons::*;
use crate::input::Input;
use crate::usage::UsageLimits;

/// A flex column with grow semantics (like CSS flex-grow).
struct Col {
    content: String,
    content_width: usize,
    /// flex-grow: 0 = fixed (content width + small pad), >0 = absorbs remaining space
    grow: u32,
}

impl Col {
    /// Fixed column — takes only the space it needs (content + 2 char pad)
    fn fixed(content: String) -> Self {
        let content_width = visible_width(&content);
        Self { content, content_width, grow: 0 }
    }

    /// Growable column — weight derived from content width so longer content
    /// gets proportionally more space. Minimum grow of 1.
    fn grow(content: String) -> Self {
        let content_width = visible_width(&content);
        let grow = (content_width as u32).max(1);
        Self { content, content_width, grow }
    }

    /// Growable column with explicit equal weight — all columns using the same
    /// weight get the same share of remaining space regardless of content width.
    fn grow_equal(content: String, weight: u32) -> Self {
        let content_width = visible_width(&content);
        Self { content, content_width, grow: weight }
    }
}

pub fn render(input: &Input, git: &GitInfo, palette: &Palette, cols: u16, usage: Option<&UsageLimits>) -> (String, String) {
    let cols = cols as usize;
    let inner_width = if cols > 2 { cols - 2 } else { cols };

    let line1 = build_line1(input, git, palette, inner_width);
    let line2 = build_line2(input, inner_width, usage);

    let l1 = format!(
        "{RST}{fg}{EDGE_LEFT}{line1}{RST}{fg}{EDGE_RIGHT}\x1b[?7h",
        fg = palette.base.fg(),
    );
    let l2_fg = L2_BG.fg();
    let l2 = format!(
        "{RST}{l2_fg}{EDGE_LEFT}{line2}{RST}{l2_fg}{EDGE_RIGHT}\x1b[?7h",
    );

    (l1, l2)
}

/// Distribute width: fixed columns get content_width + 2, growable columns
/// split the remaining space proportionally by their grow weight.
fn distribute(cols: &[Col], total_width: usize) -> Vec<usize> {
    if cols.is_empty() {
        return vec![];
    }

    let fixed_pad = 2; // 1 space left + 1 space right for fixed columns
    let mut widths = vec![0usize; cols.len()];

    // First: allocate fixed columns (content + padding)
    let mut fixed_used = 0usize;
    for (i, col) in cols.iter().enumerate() {
        if col.grow == 0 {
            let w = col.content_width + fixed_pad;
            // Add 1 for separator on non-first columns
            let w = if i > 0 { w + 1 } else { w };
            widths[i] = w;
            fixed_used += w;
        }
    }

    // Remaining space goes to growable columns
    let remaining = total_width.saturating_sub(fixed_used);
    let total_grow: u32 = cols.iter().map(|c| c.grow).sum();

    if total_grow > 0 && remaining > 0 {
        // Count separators needed for grow columns
        let grow_seps: usize = cols.iter().enumerate()
            .filter(|(i, c)| c.grow > 0 && *i > 0)
            .count();
        let distributable = remaining.saturating_sub(grow_seps);

        let mut grow_used = 0;
        let grow_indices: Vec<usize> = cols.iter().enumerate()
            .filter(|(_, c)| c.grow > 0)
            .map(|(i, _)| i)
            .collect();

        for (j, &i) in grow_indices.iter().enumerate() {
            let share = if j == grow_indices.len() - 1 {
                // Last grow column gets the remainder to avoid rounding gaps
                distributable - grow_used
            } else {
                (distributable as u64 * cols[i].grow as u64 / total_grow as u64) as usize
            };
            // Add separator width for non-first columns
            let sep = if i > 0 { 1 } else { 0 };
            widths[i] = share + sep;
            grow_used += share;
        }
    }

    widths
}

/// Render columns into a line with separators and proportional padding.
fn render_cols(cols: &[Col], widths: &[usize], bg: &str, sep_clr: &str) -> String {
    let mut out = String::new();
    out.push_str(&format!("{RST}{bg}"));

    for (i, col) in cols.iter().enumerate() {
        let alloc = widths[i];
        if alloc == 0 { continue; }

        if i > 0 {
            out.push_str(&format!("{sep_clr}│{RST}{bg}"));
        }

        // Usable space = alloc minus separator (already included in alloc for non-first)
        let usable = if i > 0 { alloc.saturating_sub(1) } else { alloc };

        out.push(' ');
        if col.content_width + 1 <= usable {
            out.push_str(&col.content);
            let pad = usable.saturating_sub(col.content_width + 1);
            if pad > 0 {
                out.push_str(&" ".repeat(pad));
            }
        } else {
            let target = if usable > 2 { usable - 1 } else { 1 };
            out.push_str(&truncate_ansi(&col.content, target, bg));
        }
    }

    out
}

fn build_line1(input: &Input, git: &GitInfo, p: &Palette, max_width: usize) -> String {
    let bg = p.base.bg();
    let sep_clr = p.sep.fg();
    let txt = p.txt.fg();
    let txt_bold = p.txt.fg_bold();

    let model_name = input.model.as_ref()
        .and_then(|m| m.display_name.as_deref()).unwrap_or("Claude");
    let dir = input.cwd.as_deref().unwrap_or("~")
        .rsplit('/').next().unwrap_or("~");
    let cost = input.cost.as_ref()
        .and_then(|c| c.total_cost_usd).unwrap_or(0.0);

    let mut cols: Vec<Col> = Vec::new();

    // Model — fixed, just needs its own width
    cols.push(Col::fixed(format!("{txt_bold}{model_name}{RST}{bg}")));

    // Folder — growable, directory names can be long
    cols.push(Col::grow(format!("{txt}{dir}{RST}{bg}")));

    // Git branch + status — growable, branch names can be long
    if let Some(branch) = &git.branch {
        let mut git_str = format!("{txt}{branch}{RST}{bg}");
        if git.has_status() {
            let mut parts = Vec::new();
            if git.staged > 0 { parts.push(format!("+{}", git.staged)); }
            if git.modified > 0 { parts.push(format!("!{}", git.modified)); }
            if git.untracked > 0 { parts.push(format!("?{}", git.untracked)); }
            git_str.push_str(&format!(" {txt}{}{RST}{bg}", parts.join(" ")));
        }
        cols.push(Col::grow(git_str));
    }

    // Agent — growable, drop at narrow widths
    if max_width >= 100 {
        if let Some(agent) = input.agent.as_ref().and_then(|a| a.name.as_deref()) {
            if !agent.is_empty() {
                cols.push(Col::grow(format!("{txt}{agent}{RST}{bg}")));
            }
        }
    }

    // Cost — fixed, always short
    if cost > 0.001 {
        let cost_str = if cost < 10.0 { format!("${:.2}", cost) }
        else { format!("${:.1}", cost) };
        cols.push(Col::fixed(format!("{txt_bold}{cost_str}{RST}{bg}")));
    }

    // Mode — fixed, drop at very narrow widths
    if max_width >= 80 {
        if let Some(mode) = &input.mode {
            if !mode.is_empty() {
                let mode_clr = Rgb(150, 100, 0).fg_bold();
                cols.push(Col::fixed(format!("{mode_clr}{mode}{RST}{bg}")));
            }
        }
    }

    let widths = distribute(&cols, max_width);
    render_cols(&cols, &widths, &bg, &sep_clr)
}

fn build_line2(input: &Input, max_width: usize, usage: Option<&UsageLimits>) -> String {
    let bg = L2_BG.bg();
    let txt = L2_TXT.fg();
    let dim = L2_DIM.fg();

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

    // Autocompact reserves a fixed 33k buffer — show % of usable capacity
    const COMPACT_BUFFER: u64 = 33_000;
    let ctx_size_raw = input.context_window.as_ref()
        .and_then(|c| c.context_window_size).unwrap_or(200_000);
    let ctx_usable = ctx_size_raw.saturating_sub(COMPACT_BUFFER);
    let ctx_usable_k = ctx_usable / 1000;
    // Recalculate pct against usable capacity (100% = autocompact)
    let tokens_used = input.context_window.as_ref()
        .and_then(|c| c.used_percentage).map(|p| (ctx_size_raw as f64 * p / 100.0) as u64).unwrap_or(0);
    let pct = if ctx_usable > 0 { (tokens_used * 100 / ctx_usable).min(100) } else { 0 };
    let ctx_clr = color::pct_color(pct);
    let cc = ctx_clr.fg();

    let lines_added = input.cost.as_ref().and_then(|c| c.total_lines_added).unwrap_or(0);
    let lines_removed = input.cost.as_ref().and_then(|c| c.total_lines_removed).unwrap_or(0);

    // Collect usage limit info upfront so we know how many bar columns we'll have
    let five_hour: Option<(u64, Rgb, String)> = usage.and_then(|ul| {
        ul.five_hour_pct.map(|p| {
            let pct = p.round() as u64;
            (pct, color::pct_color(pct), crate::usage::format_time_until(&ul.five_hour_resets_at))
        })
    });
    let seven_day: Option<(u64, Rgb, String)> = usage.and_then(|ul| {
        ul.seven_day_pct.map(|p| {
            let pct = p.round() as u64;
            (pct, color::pct_color(pct), crate::usage::format_time_until(&ul.seven_day_resets_at))
        })
    });

    // All bar columns use equal grow weight (100) so they get the same share
    const BAR_WEIGHT: u32 = 100;

    let mut cols: Vec<Col> = Vec::new();

    // Duration — fixed
    cols.push(Col::fixed(format!("{tc}{time_str}{RST}{bg}")));

    // Context bar — growable, placeholder bar (recalculated after distribution)
    let ctx_col_idx = cols.len();
    let bar_placeholder = make_bar(pct, 10, &ctx_clr, &L2_DIM);
    cols.push(Col::grow_equal(
        format!("{bar_placeholder} {cc}{pct}%{RST}{bg} {txt}of {ctx_usable_k}k"),
        BAR_WEIGHT,
    ));

    // 5h usage bar — growable with equal weight, placeholder bar
    let five_col_idx = five_hour.as_ref().map(|_| cols.len());
    if let Some((fp, ref fc, ref fr)) = five_hour {
        let bar = make_bar(fp, 5, fc, &L2_DIM);
        let fc_fg = fc.fg();
        cols.push(Col::grow_equal(
            format!("{txt}5h{RST}{bg} {bar} {fc_fg}{fp}%{RST}{bg} {dim}{fr}{RST}{bg}"),
            BAR_WEIGHT,
        ));
    }

    // 7d usage bar — growable with equal weight, drop at narrow widths
    let seven_col_idx = if max_width >= 100 { seven_day.as_ref().map(|_| cols.len()) } else { None };
    if max_width >= 100 {
        if let Some((sp, ref sc, ref sr)) = seven_day {
            let bar = make_bar(sp, 5, sc, &L2_DIM);
            let sc_fg = sc.fg();
            cols.push(Col::grow_equal(
                format!("{txt}7d{RST}{bg} {bar} {sc_fg}{sp}%{RST}{bg} {dim}{sr}{RST}{bg}"),
                BAR_WEIGHT,
            ));
        }
    }

    // Lines changed — fixed, drop at narrow widths
    if max_width >= 125 && (lines_added > 0 || lines_removed > 0) {
        let mut lc = String::new();
        if lines_added > 0 {
            let ac = ADDED_CLR.fg();
            lc.push_str(&format!("{ac}+{lines_added}{RST}{bg}"));
        }
        if lines_removed > 0 {
            if lines_added > 0 { lc.push(' '); }
            let rc = REMOVED_CLR.fg();
            lc.push_str(&format!("{rc}-{lines_removed}{RST}{bg}"));
        }
        cols.push(Col::fixed(lc));
    }

    let widths = distribute(&cols, max_width);

    // Recalculate all bar widths based on actual allocated space
    // Context bar: "▰▰▰▱▱ 95% of 167k"
    {
        let alloc = widths[ctx_col_idx].saturating_sub(if ctx_col_idx > 0 { 1 } else { 0 });
        let suffix_width = 1 + pct.to_string().len() + 1 + 3 + ctx_usable_k.to_string().len();
        let bar_space = alloc.saturating_sub(suffix_width + 3);
        let bar_width = bar_space.max(3);
        let bar = make_bar(pct, bar_width as u64, &ctx_clr, &L2_DIM);
        cols[ctx_col_idx] = Col::grow_equal(
            format!("{bar} {cc}{pct}%{RST}{bg} {txt}of {ctx_usable_k}k"),
            BAR_WEIGHT,
        );
    }

    // 5h bar: "5h ▰▰▱▱▱ 72% 2h30m"
    if let (Some(idx), Some((fp, ref fc, ref fr))) = (five_col_idx, &five_hour) {
        let alloc = widths[idx].saturating_sub(1);
        let suffix_width = 3 + 1 + fp.to_string().len() + 1 + 1 + fr.len();
        let bar_space = alloc.saturating_sub(suffix_width + 2);
        let bar_width = bar_space.max(3);
        let bar = make_bar(*fp, bar_width as u64, fc, &L2_DIM);
        let fc_fg = fc.fg();
        cols[idx] = Col::grow_equal(
            format!("{txt}5h{RST}{bg} {bar} {fc_fg}{fp}%{RST}{bg} {dim}{fr}{RST}{bg}"),
            BAR_WEIGHT,
        );
    }

    // 7d bar: "7d ▰▱▱▱▱ 23% 5d14h"
    if let (Some(idx), Some((sp, ref sc, ref sr))) = (seven_col_idx, &seven_day) {
        let alloc = widths[idx].saturating_sub(1);
        let suffix_width = 3 + 1 + sp.to_string().len() + 1 + 1 + sr.len();
        let bar_space = alloc.saturating_sub(suffix_width + 2);
        let bar_width = bar_space.max(3);
        let bar = make_bar(*sp, bar_width as u64, sc, &L2_DIM);
        let sc_fg = sc.fg();
        cols[idx] = Col::grow_equal(
            format!("{txt}7d{RST}{bg} {bar} {sc_fg}{sp}%{RST}{bg} {dim}{sr}{RST}{bg}"),
            BAR_WEIGHT,
        );
    }

    render_cols(&cols, &widths, &bg, &dim)
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

fn truncate_ansi(s: &str, target_width: usize, bg: &str) -> String {
    if target_width == 0 { return String::new(); }
    let mut result = String::new();
    let mut vis_count = 0;
    let mut in_escape = false;

    for c in s.chars() {
        if in_escape {
            result.push(c);
            if c.is_ascii_alphabetic() { in_escape = false; }
            continue;
        }
        if c == '\x1b' { in_escape = true; result.push(c); continue; }
        vis_count += 1;
        if vis_count >= target_width {
            result.push('…');
            result.push_str(&format!("\x1b[0m{bg}"));
            break;
        }
        result.push(c);
    }
    result
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
