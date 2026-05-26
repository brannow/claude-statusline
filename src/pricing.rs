use std::collections::HashMap;
use std::fs;
use std::time::{SystemTime, UNIX_EPOCH};

// DeepSeek V4 pricing per 1M tokens.
// Source: https://api-docs.deepseek.com/quick_start/pricing
// Prices reflect the permanent rates (75% promo discount becomes permanent 2026-05-31).

pub struct TokenPricing {
    pub input_per_1m: f64,
    pub cache_hit_per_1m: f64,
    pub output_per_1m: f64,
}

pub const DEEPSEEK_V4_FLASH: TokenPricing = TokenPricing {
    input_per_1m: 0.14,
    cache_hit_per_1m: 0.0028,
    output_per_1m: 0.28,
};

pub const DEEPSEEK_V4_PRO: TokenPricing = TokenPricing {
    input_per_1m: 0.435,
    cache_hit_per_1m: 0.003625,
    output_per_1m: 0.87,
};

pub fn detect(model_id: &str) -> Option<&'static TokenPricing> {
    let lower = model_id.to_lowercase();
    if !lower.contains("deepseek") {
        return None;
    }
    if lower.contains("pro") {
        Some(&DEEPSEEK_V4_PRO)
    } else {
        Some(&DEEPSEEK_V4_FLASH)
    }
}

// ── Session-level cost accumulation ──────────────────────────────────────────
//
// Claude Code's total_cost_usd uses Anthropic pricing, which is wrong when a
// DeepSeek backend is in use. We accumulate our own running total by tracking
// the delta of token counts between invocations and applying DeepSeek pricing.
// The cache is keyed by session_id so concurrent sessions don't interfere.

const COST_CACHE: &str = "/tmp/claude-statusline-deepseek-cost";
const CACHE_TTL: u64 = 86_400; // 24h

fn now_epoch() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn read_cache() -> HashMap<String, (u64, u64, u64, f64, u64)> {
    let mut map = HashMap::new();
    let Ok(content) = fs::read_to_string(COST_CACHE) else { return map };
    for line in content.lines() {
        let fields: Vec<&str> = line.splitn(6, ' ').collect();
        if fields.len() < 6 { continue; }
        let Ok(last_in) = fields[1].parse::<u64>() else { continue };
        let Ok(last_cr) = fields[2].parse::<u64>() else { continue };
        let Ok(last_out) = fields[3].parse::<u64>() else { continue };
        let Ok(acc) = fields[4].parse::<f64>() else { continue };
        let Ok(ts) = fields[5].parse::<u64>() else { continue };
        map.insert(fields[0].to_string(), (last_in, last_cr, last_out, acc, ts));
    }
    map
}

fn write_cache(entries: &HashMap<String, (u64, u64, u64, f64, u64)>) {
    let now = now_epoch();
    let mut content = String::new();
    for (sid, (li, lcr, lo, acc, ts)) in entries {
        if now.saturating_sub(*ts) < CACHE_TTL {
            content.push_str(&format!("{sid} {li} {lcr} {lo} {acc} {ts}\n"));
        }
    }
    let tmp = format!("{COST_CACHE}.tmp");
    if fs::write(&tmp, &content).is_ok() {
        let _ = fs::rename(&tmp, COST_CACHE);
    }
}

fn cost_for_tokens(p: &TokenPricing, input: u64, cache_read: u64, output: u64) -> f64 {
    let cache_miss = input.saturating_sub(cache_read);
    (cache_miss as f64 / 1_000_000.0) * p.input_per_1m
        + (cache_read as f64 / 1_000_000.0) * p.cache_hit_per_1m
        + (output as f64 / 1_000_000.0) * p.output_per_1m
}

/// Returns the accumulated DeepSeek cost for this session.
/// On each call, calculates the token delta since the last call and adds the
/// corresponding cost to the running total. If token counts decreased (e.g.
/// new conversation in same session), starts a fresh baseline from the
/// current tokens.
pub fn get_accumulated_cost(
    model_id: &str,
    session_id: &str,
    input_tokens: u64,
    cache_read_tokens: u64,
    output_tokens: u64,
) -> f64 {
    let p = match detect(model_id) {
        Some(p) => p,
        None => return 0.0,
    };

    let now = now_epoch();
    let mut cache = read_cache();

    let (last_in, last_cr, last_out, mut acc, _ts) =
        cache.get(session_id).copied().unwrap_or((0, 0, 0, 0.0, 0));

    if input_tokens > last_in || output_tokens > last_out {
        let di = input_tokens.saturating_sub(last_in);
        let dc = cache_read_tokens.saturating_sub(last_cr);
        let do_ = output_tokens.saturating_sub(last_out);
        acc += cost_for_tokens(p, di, dc, do_);
    } else if input_tokens < last_in || output_tokens < last_out {
        // Token counts shrank — fresh conversation. Rebase from current.
        acc = cost_for_tokens(p, input_tokens, cache_read_tokens, output_tokens);
    }

    cache.insert(
        session_id.to_string(),
        (input_tokens, cache_read_tokens, output_tokens, acc, now),
    );
    write_cache(&cache);

    acc
}
