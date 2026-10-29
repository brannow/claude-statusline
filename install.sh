#!/bin/sh
# Install claude-statusline from the latest GitHub release and point
# Claude Code's statusLine setting at it. macOS and Linux, no Rust needed.
#
#   curl -fsSL https://raw.githubusercontent.com/brannow/claude-statusline/main/install.sh | sh
#
# Environment (set it on `sh`, not on `curl`):
#   CLAUDE_CONFIG_DIR              Claude Code config dir (default: ~/.claude)
#   CLAUDE_STATUSLINE_INSTALL_DIR  where the binary goes (default: ~/.local/bin)
#   CLAUDE_STATUSLINE_VERSION      release tag such as v0.2.0 (default: latest)
#   CLAUDE_STATUSLINE_BASE_URL     download base URL, for mirrors or testing
#   CLAUDE_STATUSLINE_BINARY       install this local file instead of downloading
#   CLAUDE_STATUSLINE_FORCE=1      reinstall even if already up to date
#
# Re-running updates: if the installed binary already reports the target
# release's version, the download is skipped (settings are still checked).
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

main() {
    install_dir=${CLAUDE_STATUSLINE_INSTALL_DIR:-$HOME/.local/bin}
    tmp=$(mktemp -d)
    trap 'rm -rf "$tmp"' EXIT

    if [ -n "${CLAUDE_STATUSLINE_BINARY:-}" ]; then
        cp "$CLAUDE_STATUSLINE_BINARY" "$tmp/$BIN"
    else
        asset=$(detect_asset)
        if [ -n "${CLAUDE_STATUSLINE_VERSION:-}" ]; then
            base="https://github.com/$REPO/releases/download/$CLAUDE_STATUSLINE_VERSION"
        else
            base="https://github.com/$REPO/releases/latest/download"
        fi
        base=${CLAUDE_STATUSLINE_BASE_URL:-$base}

        download "$base/VERSION" "$tmp/VERSION"
        target=$(tr -d ' \r\n' <"$tmp/VERSION")
        # Binaries older than --version print nothing here, which counts as outdated.
        installed=""
        if [ -x "$install_dir/$BIN" ]; then
            installed=$("$install_dir/$BIN" --version </dev/null 2>/dev/null | awk '{ print $2 }') || installed=""
        fi
        if [ -n "$target" ] && [ "$installed" = "$target" ] && [ "${CLAUDE_STATUSLINE_FORCE:-}" != 1 ]; then
            echo "claude-statusline $installed is up to date ($install_dir/$BIN)"
            "$install_dir/$BIN" --configure </dev/null
            return 0
        fi
        if [ -n "$installed" ]; then
            echo "Updating claude-statusline $installed -> $target"
        fi

        echo "Downloading $asset ..."
        download "$base/$asset" "$tmp/$BIN"
        download "$base/SHA256SUMS" "$tmp/SHA256SUMS"

        expected=$(awk -v f="$asset" '$2 == f { print $1 }' "$tmp/SHA256SUMS")
        [ -n "$expected" ] || err "$asset not listed in SHA256SUMS"
        actual=$(sha256 "$tmp/$BIN")
        [ "$expected" = "$actual" ] || err "checksum mismatch for $asset"
    fi

    chmod 755 "$tmp/$BIN"
    mkdir -p "$install_dir"
    # Rename into place: overwriting a running binary in place fails on
    # Linux ("text file busy") and would break a live statusline.
    mv "$tmp/$BIN" "$install_dir/.$BIN.new"
    mv -f "$install_dir/.$BIN.new" "$install_dir/$BIN"

    echo "Installed $("$install_dir/$BIN" --version </dev/null) to $install_dir/$BIN"
    "$install_dir/$BIN" --configure </dev/null
    echo "Done. The statusline appears on your next Claude Code prompt."
}

main "$@"
