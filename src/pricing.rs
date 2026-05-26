// DeepSeek V4 pricing per 1M tokens.
// Source: https://api-docs.deepseek.com/quick_start/pricing
// Prices reflect the permanent rates (75% promo discount becomes permanent 2026-05-31).

pub struct TokenPricing {
    pub input_per_1m: f64,      // cache miss
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

/// Detect DeepSeek model variant from the model ID.
pub fn detect(model_id: &str) -> Option<&'static TokenPricing> {
    let lower = model_id.to_lowercase();
    if !lower.contains("deepseek") {
        return None;
    }
    if lower.contains("pro") {
        Some(&DEEPSEEK_V4_PRO)
    } else {
        // flash, chat, reasoner all default to flash pricing
        Some(&DEEPSEEK_V4_FLASH)
    }
}

/// Calculate cost from token counts using DeepSeek pricing.
pub fn calculate_cost(
    model_id: &str,
    input_tokens: u64,
    cache_read_tokens: u64,
    output_tokens: u64,
) -> Option<f64> {
    let p = detect(model_id)?;
    let cache_miss = input_tokens.saturating_sub(cache_read_tokens);
    let input_cost = (cache_miss as f64 / 1_000_000.0) * p.input_per_1m
        + (cache_read_tokens as f64 / 1_000_000.0) * p.cache_hit_per_1m;
    let output_cost = (output_tokens as f64 / 1_000_000.0) * p.output_per_1m;
    Some(input_cost + output_cost)
}
