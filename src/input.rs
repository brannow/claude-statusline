use serde::{Deserialize, Deserializer};
use serde_json::Value;

#[derive(Deserialize, Default)]
pub struct Input {
    pub cwd: Option<String>,
    pub session_id: Option<String>,
    pub model: Option<Model>,
    pub workspace: Option<Workspace>,
    pub cost: Option<Cost>,
    pub context_window: Option<ContextWindow>,
    pub agent: Option<Agent>,
    pub mode: Option<String>,
    pub worktree: Option<Worktree>,
    pub transcript_path: Option<String>,
    pub rate_limits: Option<RateLimits>,
    pub effort: Option<Effort>,
}

#[derive(Deserialize, Default)]
pub struct Effort {
    pub level: Option<String>,
}

/// Every limit window Claude Code sent, in payload order.
///
/// Deliberately an open list instead of named fields: Anthropic adds windows
/// without notice (`spend_limit` on gateways, per-model weekly buckets), and a
/// new one should show up without a release of this binary.
#[derive(Default)]
pub struct RateLimits(pub Vec<(String, RateLimitWindow)>);

pub struct RateLimitWindow {
    pub used_percentage: f64,
    pub resets_at: Option<u64>,
}

impl<'de> Deserialize<'de> for RateLimits {
    /// Entries are read leniently: anything that isn't an object with a
    /// numeric `used_percentage` is skipped. A strict parse would let one
    /// unexpected value fail the whole payload and blank the statusline.
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let Value::Object(windows) = Value::deserialize(deserializer)? else {
            return Ok(Self::default());
        };
        Ok(Self(
            windows
                .into_iter()
                .filter_map(|(key, window)| {
                    let used_percentage = window.get("used_percentage")?.as_f64()?;
                    let resets_at = window.get("resets_at").and_then(crate::usage::epoch_secs);
                    Some((key, RateLimitWindow { used_percentage, resets_at }))
                })
                .collect(),
        ))
    }
}

#[derive(Deserialize, Default)]
pub struct Model {
    pub id: Option<String>,
    pub display_name: Option<String>,
}

#[derive(Deserialize, Default)]
pub struct Workspace {
    pub current_dir: Option<String>,
    pub project_dir: Option<String>,
}

#[derive(Deserialize, Default)]
pub struct Cost {
    pub total_cost_usd: Option<f64>,
    pub total_duration_ms: Option<u64>,
    pub total_lines_added: Option<u64>,
    pub total_lines_removed: Option<u64>,
}

#[derive(Deserialize, Default)]
pub struct ContextWindow {
    pub context_window_size: Option<u64>,
    pub used_percentage: Option<f64>,
    pub remaining_percentage: Option<f64>,
    pub current_usage: Option<CurrentUsage>,
}

#[derive(Deserialize, Default)]
pub struct CurrentUsage {
    pub input_tokens: Option<u64>,
    pub cache_creation_input_tokens: Option<u64>,
    pub cache_read_input_tokens: Option<u64>,
    pub output_tokens: Option<u64>,
}

#[derive(Deserialize, Default)]
pub struct Agent {
    pub name: Option<String>,
}

#[derive(Deserialize, Default)]
pub struct Worktree {
    pub name: Option<String>,
    pub path: Option<String>,
    pub branch: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rate_limits_keep_every_usable_window_in_payload_order() {
        let input: Input = serde_json::from_str(
            r#"{ "rate_limits": {
                "five_hour": { "used_percentage": 42.0, "resets_at": 1234567890 },
                "seven_day": null,
                "model_scoped": [{ "display_name": "Fable" }],
                "broken": { "used_percentage": "n/a" },
                "spend_limit": { "used_percentage": 3.5, "resets_at": "2026-11-20T22:00:00+00:00", "used_usd": 1.2 }
            } }"#,
        )
        .unwrap();

        let windows = input.rate_limits.unwrap().0;
        let keys: Vec<&str> = windows.iter().map(|(k, _)| k.as_str()).collect();
        assert_eq!(keys, ["five_hour", "spend_limit"]);
        assert_eq!(windows[0].1.used_percentage, 42.0);
        assert_eq!(windows[0].1.resets_at, Some(1_234_567_890));
        assert_eq!(windows[1].1.resets_at, Some(1_795_212_000));
    }

    #[test]
    fn unexpected_rate_limits_shape_does_not_fail_the_payload() {
        for shape in ["null", "[]", "\"soon\"", "{}"] {
            let json = format!(r#"{{ "cwd": "/x", "rate_limits": {shape} }}"#);
            let input: Input = serde_json::from_str(&json).unwrap();
            assert_eq!(input.cwd.as_deref(), Some("/x"));
            assert!(input.rate_limits.map_or(true, |r| r.0.is_empty()), "{shape}");
        }
    }
}
