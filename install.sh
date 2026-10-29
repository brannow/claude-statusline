#!/bin/sh
# Install claude-statusline from the latest GitHub release and point
# Claude Code's statusLine setting at it. macOS and Linux, no Rust needed.
#
#   curl -fsSL https://raw.githubusercontent.com/brannow/claude-statusline/main/install.sh | sh
#
# Environment (set it on `sh`, not on `curl`):
#   CLAUDE_CONFIG_DIR              Claude Code config dir (default: ~/.claude)
#   CLAUDE_STATUSLINE_CONFIG_DIRS  several config dirs, colon-separated, e.g.
#                                  "~/.claude:~/.claude-work" (wins over CLAUDE_CONFIG_DIR)
#   CLAUDE_STATUSLINE_INSTALL_DIR  where the binary goes (default: ~/.local/bin)
#   CLAUDE_STATUSLINE_VERSION      release tag such as v0.2.0 (default: latest)
#   CLAUDE_STATUSLINE_BASE_URL     download base URL, for mirrors or testing
#   CLAUDE_STATUSLINE_BINARY       install this local file instead of downloading
#   CLAUDE_STATUSLINE_FORCE=1      reinstall even if already up to date
#
# Re-running updates and repairs: the binary is downloaded only when it's
# missing, outdated or damaged (checked by SHA-256), and statusLine is
# checked and fixed in every config dir either way.
#
# Everything runs inside main(), called on the last line, so a download cut
# off mid-script executes nothing. Child processes get </dev/null because
# stdin is this script when it's piped from curl.

set -eu

REPO="brannow/claude-statusline"
BIN="claude-statusline"

err() {
    printf 'error: %s\n' "$*" >&2
    exit 1
}

has() {
    command -v "$1" >/dev/null 2>&1
}

detect_asset() {
    os=$(uname -s </dev/null)
    arch=$(uname -m </dev/null)
    case "$os" in
        Darwin) os=darwin ;;
        Linux) os=linux ;;
        *) err "unsupported OS: $os (macOS and Linux only)" ;;
    esac
    case "$arch" in
        x86_64 | amd64) arch=x86_64 ;;
        arm64 | aarch64) arch=arm64 ;;
        *) err "unsupported architecture: $arch" ;;
    esac
    # A shell running under Rosetta reports x86_64; prefer the native binary.
    if [ "$os" = darwin ] && [ "$arch" = x86_64 ] &&
        [ "$(sysctl -n hw.optional.arm64 2>/dev/null </dev/null)" = 1 ]; then
        arch=arm64
    fi
    printf '%s-%s-%s\n' "$BIN" "$os" "$arch"
}

download() {
    if has curl; then
        curl -fsSL -o "$2" "$1" </dev/null || err "download failed: $1"
    elif has wget; then
        wget -q -O "$2" "$1" </dev/null || err "download failed: $1"
    else
        err "curl or wget is required"
    fi
}

sha256() {
    if has sha256sum; then
        sha256sum "$1" </dev/null | cut -d' ' -f1
    elif has shasum; then
        shasum -a 256 "$1" </dev/null | cut -d' ' -f1
    else
        err "sha256sum or shasum is required to verify the download"
    fi
}

# Put the release binary at $dest unless the installed one is already
# byte-identical to it. Comparing SHA-256 rather than version strings also
# repairs a binary that is damaged, truncated or built for the wrong arch.
install_binary() {
    dest="$install_dir/$BIN"

    if [ -n "${CLAUDE_STATUSLINE_BINARY:-}" ]; then
        cp "$CLAUDE_STATUSLINE_BINARY" "$tmp/$BIN"
    else
        has sha256sum || has shasum || err "sha256sum or shasum is required to verify the download"
        asset=$(detect_asset)
        if [ -n "${CLAUDE_STATUSLINE_VERSION:-}" ]; then
            base="https://github.com/$REPO/releases/download/$CLAUDE_STATUSLINE_VERSION"
        else
            base="https://github.com/$REPO/releases/latest/download"
        fi
        base=${CLAUDE_STATUSLINE_BASE_URL:-$base}

        download "$base/SHA256SUMS" "$tmp/SHA256SUMS"
        download "$base/VERSION" "$tmp/VERSION"
        target=$(tr -d ' \r\n' <"$tmp/VERSION")
        expected=$(awk -v f="$asset" '$2 == f { print $1 }' "$tmp/SHA256SUMS")
        [ -n "$expected" ] || err "$asset not listed in SHA256SUMS"

        if [ -f "$dest" ] && [ -x "$dest" ] && [ "$(sha256 "$dest")" = "$expected" ] &&
            [ "${CLAUDE_STATUSLINE_FORCE:-}" != 1 ]; then
            echo "claude-statusline $target is up to date ($dest)"
            return 0
        fi
        if [ -e "$dest" ]; then
            # Binaries older than --version, or damaged ones, print nothing here.
            installed=$("$dest" --version </dev/null 2>/dev/null | awk '{ print $2 }') || installed=""
            echo "Updating claude-statusline ${installed:-(unknown or damaged)} -> $target"
        fi

        echo "Downloading $asset ..."
        download "$base/$asset" "$tmp/$BIN"
        actual=$(sha256 "$tmp/$BIN")
        [ "$expected" = "$actual" ] || err "checksum mismatch for $asset"
    fi

    chmod 755 "$tmp/$BIN"
    mkdir -p "$install_dir"
    # Rename into place: overwriting a running binary in place fails on
    # Linux ("text file busy") and would break a live statusline.
    mv "$tmp/$BIN" "$install_dir/.$BIN.new"
    mv -f "$install_dir/.$BIN.new" "$dest"
    echo "Installed $("$dest" --version </dev/null) to $dest"
}

# Point statusLine at $dest in every config dir. Runs on every install, so a
# missing or stale settings entry is repaired even when the binary was fine.
# One broken settings.json doesn't stop the other dirs from being configured.
configure_all() {
    dirs=${CLAUDE_STATUSLINE_CONFIG_DIRS:-${CLAUDE_CONFIG_DIR:-$HOME/.claude}}
    failed=0
    old_ifs=$IFS
    IFS=:
    set -f
    for d in $dirs; do
        IFS=$old_ifs
        [ -n "$d" ] || continue
        case $d in
            "~") d=$HOME ;;
            "~/"*) d=$HOME/${d#"~/"} ;;
        esac
        CLAUDE_CONFIG_DIR=$d "$dest" --configure </dev/null || failed=1
    done
    IFS=$old_ifs
    set +f
    [ "$failed" = 0 ] || err "some config dirs were not updated (see above); fix them and re-run"
}

main() {
    install_dir=${CLAUDE_STATUSLINE_INSTALL_DIR:-$HOME/.local/bin}
    tmp=$(mktemp -d)
    trap 'rm -rf "$tmp"' EXIT

    install_binary
    configure_all
    echo "Done. The statusline appears on your next Claude Code prompt."
}

main "$@"
