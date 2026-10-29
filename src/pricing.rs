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

// ── Session cost from the transcript ────────────────────────────────────────
//
// Claude Code's stdin is useless for DeepSeek cost on two counts:
//
// * `cost.total_cost_usd` is computed with Anthropic pricing.
// * `context_window.current_usage` reflects the *latest* API response's usage,
//   not a session total. An earlier delta-based scheme assumed cumulative
//   counters, so every new turn looked like a shrink, triggered its rebase
//   branch, and reset the displayed cost to pennies.
//
// The session transcript, however, records `message.usage` for every request
// (input_tokens excludes cached tokens, Anthropic semantics). Summing that and
// applying DeepSeek pricing is monotonic by construction — compaction, new
// turns, nothing can reset it.
//
// Parsing is incremental: only bytes appended since the last call are read.
// The memo lives in a /tmp file keyed by transcript path; two racing
// invocations compute the same prefix sum, so last-writer-wins is harmless.

const CACHE_PATH: &str = "/tmp/claude-statusline-deepseek-cost-v2";
const CACHE_TTL: u64 = 86_400; // 24h

#[derive(Clone, Copy, Default)]
struct Totals {
    input: u64,
    creation: u64,
    cache_read: u64,
    output: u64,
}

fn now_epoch() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn cost_of(totals: Totals, p: &TokenPricing) -> f64 {
    // Cache creation is billed at the input rate; `input` already excludes
    // cached tokens, so nothing needs subtracting anywhere.
    (totals.input + totals.creation) as f64 / 1_000_000.0 * p.input_per_1m
        + totals.cache_read as f64 / 1_000_000.0 * p.cache_hit_per_1m
        + totals.output as f64 / 1_000_000.0 * p.output_per_1m
}

fn read_cache() -> HashMap<String, (u64, Totals, u64)> {
    let mut map = HashMap::new();
    let Ok(content) = fs::read_to_string(CACHE_PATH) else { return map };
    for line in content.lines() {
        // path\tlen\tinput\tcreation\tcache_read\toutput\tts
        let fields: Vec<&str> = line.splitn(7, '\t').collect();
        if fields.len() < 7 {
            continue;
        }
        let (Ok(len), Ok(input), Ok(creation), Ok(cache_read), Ok(output), Ok(ts)) = (
            fields[1].parse::<u64>(),
            fields[2].parse::<u64>(),
            fields[3].parse::<u64>(),
            fields[4].parse::<u64>(),
            fields[5].parse::<u64>(),
            fields[6].parse::<u64>(),
        ) else {
            continue;
        };
        let totals = Totals { input, creation, cache_read, output };
        map.insert(fields[0].to_string(), (len, totals, ts));
    }
    map
}

fn write_cache(entries: &HashMap<String, (u64, Totals, u64)>) {
    let now = now_epoch();
    let mut content = String::new();
    for (path, (len, totals, ts)) in entries {
        if now.saturating_sub(*ts) < CACHE_TTL {
            content.push_str(&format!(
                "{path}\t{len}\t{}\t{}\t{}\t{}\t{ts}\n",
                totals.input, totals.creation, totals.cache_read, totals.output
            ));
        }
    }
    let tmp = format!("{CACHE_PATH}.tmp");
    if fs::write(&tmp, &content).is_ok() {
        let _ = fs::rename(&tmp, CACHE_PATH);
    }
}

/// Add the `message.usage` entries of complete JSONL lines to `totals`.
/// Malformed lines and lines without usage are skipped — the transcript also
/// carries system/checkpoint lines.
fn parse_usage(chunk: &str, totals: &mut Totals) {
    for line in chunk.lines() {
        let Ok(value) = serde_json::from_str::<serde_json::Value>(line) else { continue };
        let Some(usage) = value.get("message").and_then(|m| m.get("usage")) else { continue };
        let field = |name: &str| usage.get(name).and_then(serde_json::Value::as_u64).unwrap_or(0);
        totals.input += field("input_tokens");
        totals.creation += field("cache_creation_input_tokens");
        totals.cache_read += field("cache_read_input_tokens");
        totals.output += field("output_tokens");
    }
}

/// Accumulated DeepSeek cost for the session whose transcript is `path`.
pub fn get_accumulated_cost(model_id: &str, transcript_path: Option<&str>) -> f64 {
    let Some(pricing) = detect(model_id) else { return 0.0 };
    let Some(path) = transcript_path else { return 0.0 };

    let now = now_epoch();
    let mut cache = read_cache();
    let (mut len, mut totals, _ts) = cache
        .get(path)
        .copied()
        .unwrap_or((0, Totals::default(), now));

    let Ok(content) = fs::read_to_string(path) else { return 0.0 };
    let full_len = content.len() as u64;

    if full_len < len {
        // Transcript was replaced (new conversation in the same file) —
        // start over instead of keeping a stale prefix.
        len = 0;
        totals = Totals::default();
    }
    if full_len > len {
        let chunk = &content[len as usize..];
        // Skip a trailing partial line: the writer appends whole lines, and a
        // cut-off one is counted on the next call when it completes.
        let complete = match chunk.rfind('\n') {
            Some(at) => &chunk[..=at],
            None => "",
        };
        parse_usage(complete, &mut totals);
        len += complete.len() as u64;
    }

    cache.insert(path.to_string(), (len, totals, now));
    write_cache(&cache);

    cost_of(totals, pricing)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn write_line(file: &mut fs::File, usage: (u64, u64, u64, u64)) {
        let line = serde_json::json!({
            "type": "assistant",
            "message": {
                "usage": {
                    "input_tokens": usage.0,
                    "cache_creation_input_tokens": usage.1,
                    "cache_read_input_tokens": usage.2,
                    "output_tokens": usage.3,
                }
            }
        });
        writeln!(file, "{line}").expect("write");
    }

    /// DeepSeek pricing with round numbers: 1M fresh + 1M creation + 1M cache
    /// read + 1M output = 0.435 + 0.435 + 0.003625 + 0.87.
    fn expected_cost(in_: f64, cc: f64, cr: f64, out: f64) -> f64 {
        let p = &DEEPSEEK_V4_PRO;
        (in_ + cc) * p.input_per_1m / 1e6 + cr * p.cache_hit_per_1m / 1e6 + out * p.output_per_1m / 1e6
    }

    #[test]
    fn sums_usage_and_prices_at_deepseek_rates() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("transcript.jsonl");
        let mut file = fs::File::create(&path).expect("create");
        write_line(&mut file, (1_000_000, 500_000, 2_000_000, 100_000));
        write_line(&mut file, (0, 0, 3_000_000, 50_000));
        drop(file);
        let path_str = path.to_str().expect("utf8");

        let cost = get_accumulated_cost("deepseek-v4-pro", Some(path_str));
        assert!((cost - expected_cost(1_000_000.0, 500_000.0, 5_000_000.0, 150_000.0)).abs() < 1e-9);

        // Same file, nothing appended — memoized, identical result.
        let again = get_accumulated_cost("deepseek-v4-pro", Some(path_str));
        assert_eq!(cost, again);

        // Appending one line adds exactly that line's cost.
        let mut file = fs::File::options().append(true).open(&path).expect("append");
        write_line(&mut file, (10_000, 0, 0, 1_000));
        drop(file);
        let grown = get_accumulated_cost("deepseek-v4-pro", Some(path_str));
        assert!((grown - cost - expected_cost(10_000.0, 0.0, 0.0, 1_000.0)).abs() < 1e-9);
    }

    #[test]
    fn a_partial_tail_line_is_counted_once() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("transcript.jsonl");
        let path_str = path.to_str().expect("utf8");
        fs::write(&path, "").expect("write");

        // Write a whole line plus half of the next one.
        let mut file = fs::File::options().append(true).open(&path).expect("open");
        write_line(&mut file, (100, 0, 0, 10));
        let half = serde_json::json!({"message": {"usage": {"input_tokens": 100, "output_tokens": 10}}});
        write!(file, "{}", half).expect("write");
        drop(file);

        let first = get_accumulated_cost("deepseek-v4-pro", Some(path_str));
        assert!((first - expected_cost(100.0, 0.0, 0.0, 10.0)).abs() < 1e-9, "partial line must not count");

        // Complete the line.
        let mut file = fs::File::options().append(true).open(&path).expect("open");
        writeln!(file, "\n").expect("write");
        drop(file);
        let second = get_accumulated_cost("deepseek-v4-pro", Some(path_str));
        assert!((second - expected_cost(200.0, 0.0, 0.0, 20.0)).abs() < 1e-9, "completed line counts exactly once");
    }

    #[test]
    fn a_replaced_transcript_restarts_from_zero() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("transcript.jsonl");
        let path_str = path.to_str().expect("utf8");
        let mut file = fs::File::create(&path).expect("create");
        write_line(&mut file, (1_000_000, 0, 0, 0));
        drop(file);
        let _ = get_accumulated_cost("deepseek-v4-pro", Some(path_str));

        // Truncated and rewritten with less content than before.
        let mut file = fs::File::create(&path).expect("recreate");
        write_line(&mut file, (100, 0, 0, 0));
        drop(file);
        let cost = get_accumulated_cost("deepseek-v4-pro", Some(path_str));
        assert!((cost - expected_cost(100.0, 0.0, 0.0, 0.0)).abs() < 1e-9);
    }

    #[test]
    fn non_deepseek_models_are_not_priced() {
        assert_eq!(get_accumulated_cost("claude-opus-4-6", Some("/nonexistent")), 0.0);
    }

    #[test]
    fn missing_transcript_yields_zero() {
        let cost = get_accumulated_cost("deepseek-v4-pro", Some("/nonexistent/transcript.jsonl"));
        assert_eq!(cost, 0.0);
    }
}
