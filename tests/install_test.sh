#!/bin/sh
# End-to-end tests for install.sh against a fake local release (file:// URLs)
# in an isolated HOME. Nothing outside a temp dir is touched.
#
#   tests/install_test.sh [path/to/claude-statusline]
#
# The binary defaults to target/release/claude-statusline. SH picks the shell
# that runs install.sh (default: sh); use SH=dash to check Ubuntu's /bin/sh.

set -eu

root=$(cd "$(dirname "$0")/.." && pwd)
bin=${1:-$root/target/release/claude-statusline}
shell=${SH:-sh}
[ -x "$bin" ] || { echo "binary not found: $bin (run cargo build --release)"; exit 1; }

# Physical path: --configure writes the canonical binary path, and macOS
# temp dirs live behind the /var -> /private/var symlink.
work=$(cd "$(mktemp -d)" && pwd -P)
trap 'rm -rf "$work"' EXIT

sha256() {
    if command -v sha256sum >/dev/null 2>&1; then
        sha256sum "$1" | cut -d' ' -f1
    else
        shasum -a 256 "$1" | cut -d' ' -f1
    fi
}

# A release with the same binary under every asset name, so detect_asset()
# finds a match on any machine.
rel=$work/release
mkdir -p "$rel"
for a in darwin-arm64 darwin-x86_64 linux-arm64 linux-x86_64; do
    cp "$bin" "$rel/claude-statusline-$a"
done
(cd "$rel" && for f in claude-statusline-*; do printf '%s  %s\n' "$(sha256 "$f")" "$f"; done >SHA256SUMS)
"$bin" --version | awk '{ print $2 }' >"$rel/VERSION"

home=$work/home
dest=$home/.local/bin/claude-statusline
out=$work/out
passed=0
failed=0

# run [NAME=VALUE ...]: run install.sh with a clean env; sets $status.
run() {
    if env -u CLAUDE_CONFIG_DIR -u CLAUDE_STATUSLINE_CONFIG_DIRS \
        HOME="$home" CLAUDE_STATUSLINE_BASE_URL="file://$rel" "$@" \
        "$shell" "$root/install.sh" </dev/null >"$out" 2>&1; then
        status=0
    else
        status=$?
    fi
}

reset() {
    rm -rf "$home"
    mkdir -p "$home"
}

check() {
    desc=$1
    shift
    if "$@"; then
        passed=$((passed + 1))
    else
        failed=$((failed + 1))
        echo "FAIL: $desc"
        sed 's/^/    | /' "$out"
    fi
}

said() { grep -q "$1" "$out"; }
configured() { grep -q "\"command\": \"$dest\"" "$1/settings.json"; }

echo "== fresh install"
reset
run
check "exit 0" [ "$status" = 0 ]
check "binary installed" [ -x "$dest" ]
check "settings created" configured "$home/.claude"

echo "== re-run when everything is in place"
run
check "exit 0" [ "$status" = 0 ]
check "skips download" said "is up to date"
check "settings left alone" said "already set"

echo "== settings.json missing, binary fine"
rm -f "$home/.claude/settings.json"
run
check "exit 0" [ "$status" = 0 ]
check "no download" said "is up to date"
check "settings restored" configured "$home/.claude"

echo "== binary missing, settings fine"
rm -f "$dest"
run
check "exit 0" [ "$status" = 0 ]
check "binary reinstalled" [ -x "$dest" ]

echo "== binary damaged"
printf 'garbage' >>"$dest"
run
check "exit 0" [ "$status" = 0 ]
check "reports update" said "Updating"
check "binary repaired" [ "$(sha256 "$dest")" = "$(sha256 "$bin")" ]

echo "== binary not executable"
chmod 644 "$dest"
run
check "exit 0" [ "$status" = 0 ]
check "binary executable again" [ -x "$dest" ]

echo "== settings point at another command"
printf '{"statusLine":{"type":"command","command":"/gone/claude-statusline","padding":2}}\n' \
    >"$home/.claude/settings.json"
run
check "exit 0" [ "$status" = 0 ]
check "command repointed" configured "$home/.claude"
check "padding kept" grep -q '"padding": 2' "$home/.claude/settings.json"

echo "== several config dirs, with ~"
reset
run CLAUDE_STATUSLINE_CONFIG_DIRS="~/.claude-a:$home/.claude-b"
check "exit 0" [ "$status" = 0 ]
check "first dir configured" configured "$home/.claude-a"
check "second dir configured" configured "$home/.claude-b"
check "default dir untouched" [ ! -e "$home/.claude" ]

echo "== CLAUDE_CONFIG_DIR alone"
reset
run CLAUDE_CONFIG_DIR="$home/.claude-x"
check "exit 0" [ "$status" = 0 ]
check "dir configured" configured "$home/.claude-x"

echo "== one config dir broken, the other still configured"
reset
mkdir -p "$home/.claude-bad"
printf '{ "a": 1, }\n' >"$home/.claude-bad/settings.json"
run CLAUDE_STATUSLINE_CONFIG_DIRS="$home/.claude-bad:$home/.claude-good"
check "exit non-zero" [ "$status" != 0 ]
check "good dir configured" configured "$home/.claude-good"
check "bad file untouched" grep -q '"a": 1, }' "$home/.claude-bad/settings.json"

echo "== checksum mismatch keeps the existing binary"
reset
run
cp "$rel/SHA256SUMS" "$work/sums.bak"
sed 's/^[0-9a-f]*/0000000000000000000000000000000000000000000000000000000000000000/' \
    "$work/sums.bak" >"$rel/SHA256SUMS"
printf 'garbage' >>"$dest"
before=$(sha256 "$dest")
run
check "exit non-zero" [ "$status" != 0 ]
check "reports mismatch" said "checksum mismatch"
check "existing binary untouched" [ "$(sha256 "$dest")" = "$before" ]
cp "$work/sums.bak" "$rel/SHA256SUMS"

echo
echo "$passed passed, $failed failed (shell: $shell)"
[ "$failed" = 0 ]
