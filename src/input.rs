use serde::Deserialize;

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
}

#[derive(Deserialize, Default)]
pub struct RateLimits {
    pub five_hour: Option<RateLimitWindow>,
    pub seven_day: Option<RateLimitWindow>,
}

#[derive(Deserialize, Default)]
pub struct RateLimitWindow {
    pub used_percentage: Option<f64>,
    pub resets_at: Option<u64>,
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
