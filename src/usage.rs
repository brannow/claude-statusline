use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use crate::platform;

const CACHE_TTL_SECS: u64 = 120;
const CACHE_STALE_MAX_SECS: u64 = 300; // serve stale up to 5min, then force sync refresh
const FETCH_TIMEOUT: Duration = Duration::from_secs(4);

/// Parsed usage limits from the Anthropic API.
#[derive(Clone, serde::Serialize, serde::Deserialize)]
pub struct UsageLimits {
    pub five_hour_pct: Option<f64>,
    pub five_hour_resets_at: String,
    pub seven_day_pct: Option<f64>,
    pub seven_day_resets_at: String,
}

/// Three-tier cache strategy (matches bash example.sh approach):
///  1. Cache fresh (< 60s)        → serve immediately, no API call
///  2. Cache stale (60s–300s)      → serve stale + fire-and-forget bg refresh
///  3. Cache very stale (>300s)    → sync fetch with timeout, fallback to stale
///     or missing
pub fn get_usage_limits(transcript_path: &str) -> Option<UsageLimits> {
    let path = Path::new(transcript_path);

    // Check cache age
    return match read_cache_age(path) {
        // Tier 1: fresh cache — serve immediately
        Some((data, age)) if age < CACHE_TTL_SECS => {
            Some(data)
        }
        // Tier 2: stale but usable — serve stale, refresh in background
        Some((data, age)) if age < CACHE_STALE_MAX_SECS => {
            spawn_bg_refresh(path);
            Some(data)
        }
        // Tier 3: very stale — we have data but it's old, try sync refresh
        Some((stale_data, _)) => {
            sync_fetch_or(path, Some(stale_data))
        }
        // Tier 3: no cache at all — must sync fetch
        None => {
            sync_fetch_or(path, None)
        }
    }
}

/// Sync fetch with timeout. Returns fallback on failure.
fn sync_fetch_or(transcript_path: &Path, fallback: Option<UsageLimits>) -> Option<UsageLimits> {
    let token = match platform::get_oauth_token().ok() {
        Some(t) => t,
        None => return fallback,
    };

    let (tx, rx) = std::sync::mpsc::channel();
    let path_owned = transcript_path.to_path_buf();
    std::thread::spawn(move || {
        let result = fetch_usage_limits(&token);
        if let Ok(ref data) = result {
            write_cache(&path_owned, data);
        }
        let _ = tx.send(result);
    });

    match rx.recv_timeout(FETCH_TIMEOUT) {
        Ok(Ok(data)) => Some(data),
        _ => fallback,
    }
}

/// Fire-and-forget background refresh — does not block the caller.
fn spawn_bg_refresh(transcript_path: &Path) {
    let token = match platform::get_oauth_token().ok() {
        Some(t) => t,
        None => return,
    };
    let path_owned = transcript_path.to_path_buf();
    std::thread::spawn(move || {
        if let Ok(data) = fetch_usage_limits(&token) {
            write_cache(&path_owned, &data);
        }
    });
}

/// Fetch usage limits from the Anthropic API.
fn fetch_usage_limits(token: &str) -> Result<UsageLimits, String> {
    let agent = ureq::Agent::new_with_config(
        ureq::config::Config::builder()
            .timeout_global(Some(Duration::from_secs(5)))
            .build(),
    );

    let mut response = agent
        .get("https://api.anthropic.com/api/oauth/usage")
        .header("Authorization", &format!("Bearer {token}"))
        .header("anthropic-beta", "oauth-2025-04-20")
        .call()
        .map_err(|e| format!("network error: {e}"))?;

    if response.status() != 200 {
        return Err(format!("API returned {}", response.status()));
    }

    let api: ApiResponse = response
        .body_mut()
        .read_json()
        .map_err(|e| format!("unexpected response: {e}"))?;

    Ok(UsageLimits {
        five_hour_pct: api.five_hour.as_ref().map(|p| p.utilization),
        five_hour_resets_at: api.five_hour.and_then(|p| p.resets_at).unwrap_or_default(),
        seven_day_pct: api.seven_day.as_ref().map(|p| p.utilization),
        seven_day_resets_at: api.seven_day.and_then(|p| p.resets_at).unwrap_or_default(),
    })
}

#[derive(serde::Deserialize)]
struct ApiResponse {
    five_hour: Option<UsagePeriod>,
    seven_day: Option<UsagePeriod>,
}

#[derive(serde::Deserialize, Default)]
struct UsagePeriod {
    #[serde(default)]
    utilization: f64,
    resets_at: Option<String>,
}

// ── Cache ────────────────────────────────────────────────────────────────────

#[derive(serde::Serialize, serde::Deserialize)]
struct CacheEnvelope {
    data: UsageLimits,
    expires_at: u64,
    five_hour_resets_at: u64,
    seven_day_resets_at: u64,
}

fn cache_path(transcript_path: &Path) -> Option<PathBuf> {
    let dir = transcript_path.parent()?;
    let stem = transcript_path.file_stem()?.to_str()?;
    Some(dir.join("statusline-cache").join(format!("{stem}-usage-limits")))
}

fn now_epoch() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Read cache and return (data, age_seconds). Returns None if no cache file.
/// Early-invalidation (usage window reset) forces age to look expired.
fn read_cache_age(transcript_path: &Path) -> Option<(UsageLimits, u64)> {
    let path = cache_path(transcript_path)?;
    let raw = std::fs::read_to_string(&path).ok()?;
    let envelope: CacheEnvelope = serde_json::from_str(&raw).ok()?;

    let now = now_epoch();
    let age = now.saturating_sub(envelope.expires_at.saturating_sub(CACHE_TTL_SECS));

    // Early invalidation if a usage window has reset — treat as very stale
    if now >= envelope.five_hour_resets_at || now >= envelope.seven_day_resets_at {
        return Some((envelope.data, CACHE_STALE_MAX_SECS + 1));
    }

    Some((envelope.data, age))
}

fn write_cache(transcript_path: &Path, data: &UsageLimits) {
    let Some(path) = cache_path(transcript_path) else { return };
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let now = now_epoch();
    let envelope = CacheEnvelope {
        data: data.clone(),
        expires_at: now + CACHE_TTL_SECS,
        five_hour_resets_at: iso8601_to_epoch(&data.five_hour_resets_at).unwrap_or(u64::MAX),
        seven_day_resets_at: iso8601_to_epoch(&data.seven_day_resets_at).unwrap_or(u64::MAX),
    };
    if let Ok(json) = serde_json::to_string(&envelope) {
        let _ = std::fs::write(path, json);
    }
}

/// Parse ISO 8601 "YYYY-MM-DDTHH:MM:SSZ" or "+00:00" to Unix epoch seconds.
fn iso8601_to_epoch(s: &str) -> Option<u64> {
    if s.is_empty() {
        return None;
    }
    let s = s
        .strip_suffix('Z')
        .or_else(|| s.strip_suffix("+00:00"))
        .unwrap_or(s);
    let (date_s, time_s) = s.split_once('T')?;
    let mut dp = date_s.split('-');
    let year: i64 = dp.next()?.parse().ok()?;
    let month: i64 = dp.next()?.parse().ok()?;
    let day: i64 = dp.next()?.parse().ok()?;
    let mut tp = time_s.split(':');
    let hour: i64 = tp.next()?.parse().ok()?;
    let min: i64 = tp.next()?.parse().ok()?;
    let sec: i64 = tp.next()?.split('.').next()?.parse().ok()?;
    // Howard Hinnant civil-to-days
    let y = if month <= 2 { year - 1 } else { year };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let doy = (153 * (month + if month > 2 { -3 } else { 9 }) + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146097 + doe - 719468;
    let total = days * 86400 + hour * 3600 + min * 60 + sec;
    u64::try_from(total).ok()
}

/// Format reset time as human-readable relative string.
/// Empty/unparseable → "?", past → "now", <1h → "45m", <1d → "4h12m", ≥1d → "3d2h"
pub fn format_time_until(resets_at: &str) -> String {
    if resets_at.is_empty() {
        return "?".to_string();
    }
    let reset_epoch = match iso8601_to_epoch(resets_at) {
        Some(e) => e,
        None => return "?".to_string(),
    };
    let now = now_epoch();
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
    use std::path::PathBuf;

    fn sample_data() -> UsageLimits {
        UsageLimits {
            five_hour_pct: Some(23.4),
            five_hour_resets_at: "2099-01-01T00:00:00Z".into(),
            seven_day_pct: Some(45.1),
            seven_day_resets_at: "2099-01-01T00:00:00Z".into(),
        }
    }

    fn sample_data_no_seven_day() -> UsageLimits {
        UsageLimits {
            five_hour_pct: Some(99.0),
            five_hour_resets_at: "2099-01-01T00:00:00Z".into(),
            seven_day_pct: None,
            seven_day_resets_at: String::new(),
        }
    }

    fn temp_transcript(subdir: &str) -> (tempfile::TempDir, PathBuf) {
        let dir = tempfile::tempdir().expect("tempdir");
        let transcript = dir.path().join(subdir).join("transcript.jsonl");
        (dir, transcript)
    }

    // ── iso8601_to_epoch ─────────────────────────────────────────────────────

    #[test]
    fn iso8601_known_epoch() {
        // 2000-01-01T00:00:00Z = 946,684,800
        assert_eq!(iso8601_to_epoch("2000-01-01T00:00:00Z"), Some(946_684_800));
    }

    #[test]
    fn iso8601_plus_offset_format() {
        // Anthropic API uses "+00:00" not "Z"
        assert_eq!(
            iso8601_to_epoch("2000-01-01T00:00:00+00:00"),
            Some(946_684_800),
        );
    }

    #[test]
    fn iso8601_fractional_seconds() {
        // Sub-second precision truncated
        assert_eq!(
            iso8601_to_epoch("2000-01-01T00:00:01.943648+00:00"),
            Some(946_684_801),
        );
    }

    #[test]
    fn iso8601_empty_returns_none() {
        assert_eq!(iso8601_to_epoch(""), None);
    }

    #[test]
    fn iso8601_garbage_returns_none() {
        assert_eq!(iso8601_to_epoch("not-a-date"), None);
    }

    // ── format_time_until ────────────────────────────────────────────────────

    #[test]
    fn format_time_empty_returns_question() {
        assert_eq!(format_time_until(""), "?");
    }

    #[test]
    fn format_time_past_returns_now() {
        assert_eq!(format_time_until("2000-01-01T00:00:00Z"), "now");
    }

    #[test]
    fn format_time_far_future_returns_days() {
        // 2099 is far enough that it includes days
        let result = format_time_until("2099-01-01T00:00:00Z");
        assert!(result.contains('d'), "expected days: {result}");
        assert!(result.contains('h'), "expected hours: {result}");
    }

    #[test]
    fn format_time_invalid_returns_question() {
        assert_eq!(format_time_until("garbage"), "?");
    }

    // ── API response deserialization ─────────────────────────────────────────

    #[test]
    fn deserialize_full_api_response() {
        let json = r#"{
            "five_hour": {"utilization": 42.5, "resets_at": "2026-03-15T14:00:00+00:00"},
            "seven_day": {"utilization": 15.3, "resets_at": "2026-03-20T00:00:00+00:00"}
        }"#;
        let api: ApiResponse = serde_json::from_str(json).unwrap();
        assert!(api.five_hour.is_some());
        assert!(api.seven_day.is_some());
        assert!((api.five_hour.unwrap().utilization - 42.5).abs() < f64::EPSILON);
    }

    #[test]
    fn deserialize_null_seven_day() {
        // Real response from subscription accounts: seven_day is null
        let json = r#"{
            "five_hour": {"utilization": 99.0, "resets_at": "2026-03-15T14:00:00+00:00"},
            "seven_day": null
        }"#;
        let api: ApiResponse = serde_json::from_str(json).unwrap();
        assert!(api.five_hour.is_some());
        assert!(api.seven_day.is_none());
    }

    #[test]
    fn deserialize_both_null() {
        let json = r#"{"five_hour": null, "seven_day": null}"#;
        let api: ApiResponse = serde_json::from_str(json).unwrap();
        assert!(api.five_hour.is_none());
        assert!(api.seven_day.is_none());
    }

    #[test]
    fn deserialize_extra_fields_ignored() {
        // Real API returns extra fields like iguana_necktie, extra_usage, etc.
        let json = r#"{
            "five_hour": {"utilization": 50.0, "resets_at": "2026-03-15T14:00:00+00:00"},
            "seven_day": null,
            "seven_day_oauth_apps": null,
            "iguana_necktie": null,
            "extra_usage": {"is_enabled": true, "monthly_limit": 5000}
        }"#;
        let api: ApiResponse = serde_json::from_str(json).unwrap();
        assert!(api.five_hour.is_some());
    }

    // ── Cache read/write ─────────────────────────────────────────────────────

    #[test]
    fn cache_roundtrip() {
        let (_dir, transcript) = temp_transcript("rt");
        write_cache(&transcript, &sample_data());
        let result = read_cache_age(&transcript);
        assert!(result.is_some());
        let (data, age) = result.unwrap();
        assert!(age < CACHE_TTL_SECS, "freshly written cache should be fresh");
        assert!((data.five_hour_pct.unwrap() - 23.4).abs() < f64::EPSILON);
        assert!((data.seven_day_pct.unwrap() - 45.1).abs() < f64::EPSILON);
    }

    #[test]
    fn cache_roundtrip_no_seven_day() {
        let (_dir, transcript) = temp_transcript("rt_no7d");
        write_cache(&transcript, &sample_data_no_seven_day());
        let result = read_cache_age(&transcript);
        assert!(result.is_some());
        let (data, age) = result.unwrap();
        assert!(age < CACHE_TTL_SECS);
        assert!((data.five_hour_pct.unwrap() - 99.0).abs() < f64::EPSILON);
        assert!(data.seven_day_pct.is_none());
    }

    #[test]
    fn cache_miss_nonexistent() {
        let (_dir, transcript) = temp_transcript("miss");
        assert!(read_cache_age(&transcript).is_none());
    }

    #[test]
    fn cache_creates_directory() {
        let dir = tempfile::tempdir().unwrap();
        let transcript = dir.path().join("deep").join("nested").join("t.jsonl");
        write_cache(&transcript, &sample_data());
        let cache_file = dir.path().join("deep").join("nested").join("statusline-cache").join("t-usage-limits");
        assert!(cache_file.exists());
    }

    #[test]
    fn cache_ttl_expired_reports_stale_age() {
        let dir = tempfile::tempdir().unwrap();
        let transcript = dir.path().join("transcript.jsonl");
        write_cache(&transcript, &sample_data());
        // Overwrite with expired envelope (expires_at = 0 means written CACHE_TTL_SECS ago from epoch 0)
        let path = dir.path().join("statusline-cache").join("transcript-usage-limits");
        let expired = serde_json::json!({
            "data": {
                "five_hour_pct": 23.4,
                "five_hour_resets_at": "2099-01-01T00:00:00Z",
                "seven_day_pct": 45.1,
                "seven_day_resets_at": "2099-01-01T00:00:00Z"
            },
            "expires_at": 0_u64,
            "five_hour_resets_at": 9_999_999_999_u64,
            "seven_day_resets_at": 9_999_999_999_u64
        });
        std::fs::write(&path, serde_json::to_string(&expired).unwrap()).unwrap();
        let result = read_cache_age(&transcript);
        assert!(result.is_some(), "expired cache still returns data");
        let (_data, age) = result.unwrap();
        assert!(age >= CACHE_TTL_SECS, "age should exceed TTL: got {age}");
    }

    #[test]
    fn cache_early_invalidation_five_hour_reset() {
        let (_dir, transcript) = temp_transcript("early5h");
        let data = UsageLimits {
            five_hour_pct: Some(50.0),
            five_hour_resets_at: "2000-01-01T00:00:00Z".into(), // past
            seven_day_pct: Some(10.0),
            seven_day_resets_at: "2099-01-01T00:00:00Z".into(),
        };
        write_cache(&transcript, &data);
        let result = read_cache_age(&transcript);
        assert!(result.is_some());
        let (_, age) = result.unwrap();
        assert!(age > CACHE_STALE_MAX_SECS, "reset window passed → forced very stale");
    }

    #[test]
    fn cache_empty_resets_at_no_invalidation() {
        let (_dir, transcript) = temp_transcript("empty_reset");
        // Subscription accounts: seven_day_resets_at is empty
        let data = UsageLimits {
            five_hour_pct: Some(50.0),
            five_hour_resets_at: "2099-01-01T00:00:00Z".into(),
            seven_day_pct: None,
            seven_day_resets_at: String::new(),
        };
        write_cache(&transcript, &data);
        let result = read_cache_age(&transcript);
        assert!(result.is_some(), "empty resets_at should not invalidate");
        let (_, age) = result.unwrap();
        assert!(age < CACHE_TTL_SECS, "should still be fresh");
    }

    #[test]
    fn cache_stale_still_returns_data() {
        let dir = tempfile::tempdir().unwrap();
        let transcript = dir.path().join("transcript.jsonl");
        write_cache(&transcript, &sample_data());
        let path = dir.path().join("statusline-cache").join("transcript-usage-limits");
        // Make it expired but data should still be accessible
        let expired = serde_json::json!({
            "data": {
                "five_hour_pct": 77.0,
                "five_hour_resets_at": "2099-01-01T00:00:00Z",
                "seven_day_pct": 88.0,
                "seven_day_resets_at": "2099-01-01T00:00:00Z"
            },
            "expires_at": 0_u64,
            "five_hour_resets_at": 9_999_999_999_u64,
            "seven_day_resets_at": 9_999_999_999_u64
        });
        std::fs::write(&path, serde_json::to_string(&expired).unwrap()).unwrap();
        let result = read_cache_age(&transcript);
        assert!(result.is_some(), "stale cache still returns data");
        let (data, _) = result.unwrap();
        assert!((data.five_hour_pct.unwrap() - 77.0).abs() < f64::EPSILON);
    }

    #[test]
    fn cache_no_file_returns_none() {
        let (_dir, transcript) = temp_transcript("no_file");
        assert!(read_cache_age(&transcript).is_none());
    }
}
