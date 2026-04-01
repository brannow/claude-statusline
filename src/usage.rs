use std::time::{SystemTime, UNIX_EPOCH};

/// Format reset time (unix epoch seconds) as human-readable relative string.
/// None/0 → "?", past → "now", <1h → "45m", <1d → "4h12m", ≥1d → "3d2h"
pub fn format_time_until(resets_at: Option<u64>) -> String {
    let reset_epoch = match resets_at {
        Some(0) | None => return "?".to_string(),
        Some(e) => e,
    };

    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);

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
}
