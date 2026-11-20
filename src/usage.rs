use serde::Deserialize;
use serde_json::Value;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

/// One bar on Line 3, whichever source it came from.
#[derive(Clone)]
pub struct LimitBar {
    pub label: String,
    pub pct: u64,
    pub resets_at: Option<u64>,
    /// The value is an old snapshot and may have moved since — rendered `~5%`.
    pub stale: bool,
}

/// Claude Code stops trusting its own usage snapshot after an hour; past that
/// we still show the bar (a weekly window moves slowly) but mark it as stale.
const STALE_AFTER_SECS: u64 = 3600;

const MAX_LABEL_CHARS: usize = 20;

/// Limit windows Claude Code knows about but does not put in the statusLine
/// payload (e.g. the per-model "Current week (Fable)" bar from `/usage`).
///
/// They only exist on disk, in the usage snapshot Claude Code keeps in its
/// global config file. That file is internal and undocumented, so every read
/// is lenient: anything unexpected yields no extra bars, never an error.
pub fn load_cached_limits() -> Vec<LimitBar> {
    let Some(path) = config_file() else { return Vec::new() };
    let Ok(json) = std::fs::read_to_string(path) else { return Vec::new() };
    cached_limits(&json, now_secs())
}

/// `$CLAUDE_CONFIG_DIR/.claude.json`, falling back to `~/.claude.json`.
fn config_file() -> Option<PathBuf> {
    let dir = match std::env::var_os("CLAUDE_CONFIG_DIR").filter(|v| !v.is_empty()) {
        Some(dir) => dir,
        None => std::env::var_os("HOME").filter(|v| !v.is_empty())?,
    };
    Some(PathBuf::from(dir).join(".claude.json"))
}

/// Only the two keys we need are typed; serde skips the rest of the (large)
/// file without building a tree for it.
#[derive(Deserialize)]
struct ConfigFile {
    #[serde(rename = "oauthAccount", default)]
    oauth_account: Value,
    #[serde(rename = "cachedUsageUtilization", default)]
    cached_usage: Value,
}

fn cached_limits(json: &str, now: u64) -> Vec<LimitBar> {
    let Ok(config) = serde_json::from_str::<ConfigFile>(json) else { return Vec::new() };
    let cache = &config.cached_usage;

    // A snapshot left behind by a different login describes someone else's quota.
    if let (Some(cached), Some(current)) = (
        cache["accountUuid"].as_str(),
        config.oauth_account["accountUuid"].as_str(),
    ) {
        if cached != current {
            return Vec::new();
        }
    }

    let stale = match cache["fetchedAtMs"].as_f64() {
        Some(ms) => now.saturating_sub((ms / 1000.0) as u64) > STALE_AFTER_SECS,
        None => true,
    };

    let Some(limits) = cache["utilization"]["limits"].as_array() else { return Vec::new() };
    limits.iter().filter_map(|limit| cached_limit(limit, now, stale)).collect()
}

fn cached_limit(limit: &Value, now: u64, stale: bool) -> Option<LimitBar> {
    let kind = limit["kind"].as_str()?;
    // The 5h and 7d windows arrive live on stdin; the snapshot copy is older.
    if kind == "session" || kind == "weekly_all" {
        return None;
    }

    let pct = limit["percent"].as_f64()?.round() as u64;
    if pct == 0 {
        return None;
    }

    // Once the window has rolled over, the cached number belongs to the
    // previous window — the real value is back near zero.
    let resets_at = epoch_secs(&limit["resets_at"]);
    if resets_at.is_some_and(|reset| reset <= now) {
        return None;
    }

    let scope = &limit["scope"];
    let name = scope["model"]["display_name"].as_str()
        .or_else(|| scope["surface"]["display_name"].as_str());
    let label = match (limit["group"].as_str(), name) {
        (Some("weekly"), Some(name)) => format!("7d {name}"),
        (Some("session"), Some(name)) => format!("5h {name}"),
        (_, Some(name)) => name.to_string(),
        (_, None) => kind.replace('_', " "),
    };

    Some(LimitBar { label: clean_label(&label), pct, resets_at, stale })
}

/// Labels come from the server or from JSON keys we've never seen, and go
/// straight to the terminal: drop control characters and cap the width.
pub fn clean_label(label: &str) -> String {
    label.chars().filter(|c| !c.is_control()).take(MAX_LABEL_CHARS).collect()
}

/// A reset time as epoch seconds. stdin sends a number; the usage snapshot
/// sends ISO-8601 text.
pub fn epoch_secs(value: &Value) -> Option<u64> {
    match value {
        Value::Number(n) => n.as_u64().or_else(|| n.as_f64().map(|f| f as u64)),
        Value::String(s) => parse_iso8601(s),
        _ => None,
    }
}

/// `2026-11-20T22:00:00.676614+00:00` → epoch seconds. Hand-rolled for the
/// same reason as the SHA-256 in platform.rs: one timestamp format doesn't
/// justify a date crate.
fn parse_iso8601(s: &str) -> Option<u64> {
    let num = |range: std::ops::Range<usize>| s.get(range)?.parse::<i64>().ok();
    let (year, month, day) = (num(0..4)?, num(5..7)?, num(8..10)?);
    let (hour, minute, second) = (num(11..13)?, num(14..16)?, num(17..19)?);
    if !(1..=12).contains(&month) {
        return None;
    }

    // Skip fractional seconds, then read the UTC offset (Z, +HH, +HHMM, +HH:MM).
    let zone = s.get(19..)?.trim_start_matches(|c: char| c == '.' || c.is_ascii_digit());
    let offset = match zone {
        "" | "Z" | "z" => 0,
        _ => {
            let sign = match zone.as_bytes()[0] {
                b'+' => 1,
                b'-' => -1,
                _ => return None,
            };
            let hours = zone.get(1..3)?.parse::<i64>().ok()?;
            let minutes = if zone.len() > 3 {
                zone.get(zone.len() - 2..)?.parse::<i64>().ok()?
            } else {
                0
            };
            sign * (hours * 3600 + minutes * 60)
        }
    };

    // Days since 1970-01-01 (Howard Hinnant's days_from_civil).
    let y = if month <= 2 { year - 1 } else { year };
    let era = y.div_euclid(400);
    let year_of_era = y.rem_euclid(400);
    let day_of_year = (153 * (if month > 2 { month - 3 } else { month + 9 }) + 2) / 5 + day - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    let days = era * 146_097 + day_of_era - 719_468;

    u64::try_from(days * 86_400 + hour * 3600 + minute * 60 + second - offset).ok()
}

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Format reset time (unix epoch seconds) as human-readable relative string.
/// None/0 → "?", past → "now", <1h → "45m", <1d → "4h12m", ≥1d → "3d2h"
pub fn format_time_until(resets_at: Option<u64>) -> String {
    let reset_epoch = match resets_at {
        Some(0) | None => return "?".to_string(),
        Some(e) => e,
    };

    let now = now_secs();

    if now >= reset_epoch {
        return "now".to_string();
    }

    let secs = reset_epoch - now;
    let mins = secs / 60;
    let hours = mins / 60;
    let days = hours / 24;

    if days > 0 {
        format!("{}d{}h", days, hours % 24)
    } else if hours > 0 {
        format!("{}h{}m", hours, mins % 60)
    } else {
        format!("{}m", mins)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn format_time_none_returns_question() {
        assert_eq!(format_time_until(None), "?");
    }

    #[test]
    fn format_time_zero_returns_question() {
        assert_eq!(format_time_until(Some(0)), "?");
    }

    #[test]
    fn format_time_past_returns_now() {
        assert_eq!(format_time_until(Some(946_684_800)), "now"); // 2000-01-01
    }

    #[test]
    fn format_time_far_future_returns_days() {
        let result = format_time_until(Some(4_070_908_800)); // ~2099
        assert!(result.contains('d'), "expected days: {result}");
        assert!(result.contains('h'), "expected hours: {result}");
    }

    #[test]
    fn iso8601_parses_the_formats_claude_code_writes() {
        assert_eq!(parse_iso8601("1970-01-01T00:00:00Z"), Some(0));
        assert_eq!(parse_iso8601("2000-01-01T00:00:00Z"), Some(946_684_800));
        assert_eq!(parse_iso8601("2024-02-29T12:00:00Z"), Some(1_709_208_000)); // leap day
        assert_eq!(parse_iso8601("2026-11-20T22:00:00.676614+00:00"), Some(1_795_212_000));
        // Same instant written in Bangkok time.
        assert_eq!(parse_iso8601("2026-11-21T05:00:00+07:00"), Some(1_795_212_000));
        assert_eq!(parse_iso8601("2026-11-20T17:00:00-0500"), Some(1_795_212_000));
    }

    #[test]
    fn iso8601_rejects_garbage() {
        for bad in ["", "soon", "2026-11-20", "2026-13-01T00:00:00Z", "2026-11-20T22:00:00 UTC", "日本語日本語日本語日本語"] {
            assert_eq!(parse_iso8601(bad), None, "{bad}");
        }
    }

    const NOW: u64 = 1_795_200_000; // 2026-11-20T18:40:00Z

    /// The snapshot as Claude Code 2.1.329 writes it, trimmed to the fields we read.
    fn snapshot(fetched_at_ms: u64, limits: &str) -> String {
        format!(
            r#"{{
                "numStartups": 12,
                "oauthAccount": {{ "accountUuid": "acct-1", "emailAddress": "x@example.com" }},
                "cachedUsageUtilization": {{
                    "fetchedAtMs": {fetched_at_ms},
                    "accountUuid": "acct-1",
                    "utilization": {{ "five_hour": {{ "utilization": 18 }}, "limits": {limits} }}
                }}
            }}"#
        )
    }

    const FABLE: &str = r#"{ "kind": "weekly_scoped", "group": "weekly", "percent": 5,
        "resets_at": "2026-11-20T20:00:00.676879+00:00",
        "scope": { "model": { "id": null, "display_name": "Fable" }, "surface": null } }"#;

    #[test]
    fn cache_yields_only_the_windows_stdin_does_not_carry() {
        let limits = format!(
            r#"[
                {{ "kind": "session", "group": "session", "percent": 18, "resets_at": "2026-11-20T22:00:00+00:00", "scope": null }},
                {{ "kind": "weekly_all", "group": "weekly", "percent": 34, "resets_at": "2026-11-20T20:00:00+00:00", "scope": null }},
                {FABLE}
            ]"#
        );
        let bars = cached_limits(&snapshot(NOW * 1000 - 300_000, &limits), NOW);

        assert_eq!(bars.len(), 1);
        assert_eq!(bars[0].label, "7d Fable");
        assert_eq!(bars[0].pct, 5);
        assert_eq!(bars[0].resets_at, Some(1_795_204_800));
        assert!(!bars[0].stale);
    }

    #[test]
    fn cache_hides_windows_at_zero_percent() {
        let limits = FABLE.replace(r#""percent": 5"#, r#""percent": 0.4"#);
        assert!(cached_limits(&snapshot(NOW * 1000, &format!("[{limits}]")), NOW).is_empty());
    }

    #[test]
    fn cache_marks_old_snapshots_stale() {
        let two_hours_ago = (NOW - 7200) * 1000;
        let limits = FABLE.replace("2026-11-20T20:00:00", "2026-11-25T20:00:00");
        let bars = cached_limits(&snapshot(two_hours_ago, &format!("[{limits}]")), NOW);
        assert!(bars[0].stale);
    }

    #[test]
    fn cache_drops_windows_that_already_reset() {
        let after_reset = 1_795_204_800 + 60;
        assert!(cached_limits(&snapshot(NOW * 1000, &format!("[{FABLE}]")), after_reset).is_empty());
    }

    #[test]
    fn cache_from_another_account_is_ignored() {
        let json = snapshot(NOW * 1000, &format!("[{FABLE}]")).replacen("acct-1", "acct-2", 1);
        assert!(cached_limits(&json, NOW).is_empty());
    }

    #[test]
    fn cache_labels_fall_back_to_kind_and_are_sanitised() {
        let limits = r#"[
            { "kind": "monthly_design", "group": "monthly", "percent": 9, "resets_at": null },
            { "kind": "weekly_scoped", "group": "weekly", "percent": 9,
              "scope": { "surface": { "display_name": "Co\u001b[31mwork with a very long name" } } }
        ]"#;
        let bars = cached_limits(&snapshot(NOW * 1000, limits), NOW);

        assert_eq!(bars[0].label, "monthly design");
        assert_eq!(bars[0].resets_at, None);
        assert_eq!(bars[1].label, "7d Co[31mwork with a");
    }

    #[test]
    fn unexpected_cache_shapes_yield_no_bars() {
        for json in [
            "",
            "not json",
            "[]",
            "{}",
            r#"{ "cachedUsageUtilization": "nope" }"#,
            r#"{ "cachedUsageUtilization": { "utilization": { "limits": {} } } }"#,
            r#"{ "cachedUsageUtilization": { "utilization": { "limits": [1, null, "x", { "kind": 3 }, { "kind": "k" }] } } }"#,
        ] {
            assert!(cached_limits(json, NOW).is_empty(), "{json}");
        }
    }
}
