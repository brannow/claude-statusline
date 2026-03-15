#!/bin/bash
# Visual smoke tests for claude-statusline
BIN="./target/release/claude-statusline"

echo "=== Subscription session (no cost, git, 42% context) ==="
echo '{"model":{"id":"claude-opus-4-6","display_name":"Opus"},"cwd":"/Users/test/my-project","session_id":"sub-001","context_window":{"used_percentage":42,"context_window_size":200000},"cost":{"total_cost_usd":0,"total_duration_ms":3600000,"total_lines_added":156,"total_lines_removed":23}}' | $BIN
echo ""

echo "=== API session (cost, agent, 78% context) ==="
echo '{"model":{"id":"claude-sonnet-4-6","display_name":"Sonnet 4.6"},"cwd":"/Users/test/company-app","session_id":"api-002","context_window":{"used_percentage":78,"context_window_size":200000},"cost":{"total_cost_usd":6.61,"total_duration_ms":7200000,"total_lines_added":42,"total_lines_removed":8},"agent":{"name":"security-reviewer"}}' | $BIN
echo ""

echo "=== High context (92%), long session, haiku ==="
echo '{"model":{"id":"claude-haiku-4-5","display_name":"Haiku"},"cwd":"/Users/test/urgent-fix","session_id":"crit-003","context_window":{"used_percentage":92,"context_window_size":200000},"cost":{"total_cost_usd":0.03,"total_duration_ms":10800000,"total_lines_added":5,"total_lines_removed":300}}' | $BIN
echo ""

echo "=== Minimal input (empty JSON) ==="
echo '{}' | $BIN
echo ""

echo "=== Fresh session (0% context, 0 duration) ==="
echo '{"model":{"id":"claude-opus-4-6","display_name":"Opus"},"cwd":"/Users/test/new-project","session_id":"fresh-004","context_window":{"used_percentage":0,"context_window_size":200000},"cost":{"total_cost_usd":0,"total_duration_ms":5000}}' | $BIN
echo ""

echo "=== Extended context (1M window) ==="
echo '{"model":{"id":"claude-opus-4-6","display_name":"Opus"},"cwd":"/Users/test/big-project","session_id":"ext-005","context_window":{"used_percentage":15,"context_window_size":1000000},"cost":{"total_cost_usd":1.50,"total_duration_ms":1800000,"total_lines_added":500,"total_lines_removed":120},"mode":"plan"}' | $BIN
echo ""

# ── Usage limits (simulated via pre-written cache) ───────────────────────────

CACHE_DIR="/tmp/statusline-test-cache/cship"
mkdir -p "$CACHE_DIR"

echo "=== Usage limits: 5h only (subscription, no 7d) ==="
cat > "$CACHE_DIR/ul-test1-usage-limits" <<'CACHE'
{"data":{"five_hour_pct":72.0,"five_hour_resets_at":"2099-01-01T00:00:00Z","seven_day_pct":null,"seven_day_resets_at":""},"expires_at":9999999999,"five_hour_resets_at":4070908800,"seven_day_resets_at":18446744073709551615}
CACHE
echo '{"model":{"display_name":"Opus"},"cwd":"/Users/test/project","session_id":"ul-001","transcript_path":"/tmp/statusline-test-cache/ul-test1.jsonl","context_window":{"used_percentage":35,"context_window_size":200000},"cost":{"total_cost_usd":0.80,"total_duration_ms":900000}}' | $BIN
echo ""

echo "=== Usage limits: both 5h and 7d ==="
cat > "$CACHE_DIR/ul-test2-usage-limits" <<'CACHE'
{"data":{"five_hour_pct":45.0,"five_hour_resets_at":"2099-01-01T00:00:00Z","seven_day_pct":23.0,"seven_day_resets_at":"2099-01-07T00:00:00Z"},"expires_at":9999999999,"five_hour_resets_at":4070908800,"seven_day_resets_at":4071427200}
CACHE
echo '{"model":{"display_name":"Sonnet 4.6"},"cwd":"/Users/test/project","session_id":"ul-002","transcript_path":"/tmp/statusline-test-cache/ul-test2.jsonl","context_window":{"used_percentage":60,"context_window_size":200000},"cost":{"total_cost_usd":2.10,"total_duration_ms":5400000,"total_lines_added":88,"total_lines_removed":12}}' | $BIN
echo ""

echo "=== Usage limits: critical (100% 5h) ==="
cat > "$CACHE_DIR/ul-test3-usage-limits" <<'CACHE'
{"data":{"five_hour_pct":100.0,"five_hour_resets_at":"2099-01-01T00:00:00Z","seven_day_pct":null,"seven_day_resets_at":""},"expires_at":9999999999,"five_hour_resets_at":4070908800,"seven_day_resets_at":18446744073709551615}
CACHE
echo '{"model":{"display_name":"Opus"},"cwd":"/Users/test/throttled","session_id":"ul-003","transcript_path":"/tmp/statusline-test-cache/ul-test3.jsonl","context_window":{"used_percentage":88,"context_window_size":200000},"cost":{"total_cost_usd":4.50,"total_duration_ms":14400000}}' | $BIN
echo ""

echo "=== Usage limits: low usage (10% 5h, 5% 7d) ==="
cat > "$CACHE_DIR/ul-test4-usage-limits" <<'CACHE'
{"data":{"five_hour_pct":10.0,"five_hour_resets_at":"2099-01-01T00:00:00Z","seven_day_pct":5.0,"seven_day_resets_at":"2099-01-07T00:00:00Z"},"expires_at":9999999999,"five_hour_resets_at":4070908800,"seven_day_resets_at":4071427200}
CACHE
echo '{"model":{"display_name":"Haiku"},"cwd":"/Users/test/fresh-start","session_id":"ul-004","transcript_path":"/tmp/statusline-test-cache/ul-test4.jsonl","context_window":{"used_percentage":5,"context_window_size":200000},"cost":{"total_cost_usd":0.01,"total_duration_ms":60000,"total_lines_added":3}}' | $BIN
echo ""

# Cleanup
rm -rf /tmp/statusline-test-cache
