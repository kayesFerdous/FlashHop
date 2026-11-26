#!/bin/sh
# FlashHop installer for Linux (x86_64) and macOS. Downloads the latest GitHub Release.
#   curl -fsSL https://raw.githubusercontent.com/kayesFerdous/FlashHop/main/install.sh | sh
# Uninstall:  ... | sh -s -- --uninstall
set -eu

REPO=kayesFerdous/FlashHop
URL="${FLASHHOP_URL:-https://github.com/$REPO/releases/latest/download}"  # override to test or pin a version
BIN="$HOME/.local/bin"
APPS="$HOME/.local/share/applications"
ICONS="$HOME/.local/share/icons/hicolor/1024x1024/apps"

die() { echo "flashhop: $*" >&2; exit 1; }
need() { command -v "$1" >/dev/null 2>&1 || die "needs '$1'"; }

uninstall() {
    rm -f "$BIN/flashhop" "$APPS/flashhop.desktop" "$ICONS/flashhop.png"
    rm -rf "/Applications/FlashHop.app" "$HOME/Applications/FlashHop.app" 2>/dev/null || true
    echo "FlashHop removed. Settings are kept in ~/.config/flashhop."
}

[ "${1:-}" = "--uninstall" ] && { uninstall; exit 0; }

need curl
TMP=$(mktemp -d)
trap 'rm -rf "$TMP"' EXIT

case "$(uname -s)" in
Linux)
    [ "$(uname -m)" = x86_64 ] || die "only x86_64 Linux builds exist (this is $(uname -m)); build from source: cargo run --release -p flashhop"
    need tar
    echo "Downloading FlashHop for Linux..."
    curl -fL --progress-bar "$URL/flashhop-linux-x86_64.tar.gz" -o "$TMP/f.tar.gz"
    tar -xzf "$TMP/f.tar.gz" -C "$TMP"
    mkdir -p "$BIN" "$APPS"
    install -m 755 "$TMP/flashhop" "$BIN/flashhop"
    if [ -f "$TMP/flashhop.png" ]; then
        mkdir -p "$ICONS" && install -m 644 "$TMP/flashhop.png" "$ICONS/flashhop.png"
    fi
    cat > "$APPS/flashhop.desktop" <<EOF
[Desktop Entry]
Type=Application
Name=FlashHop
Comment=Share files and text with devices on your network
Exec=$BIN/flashhop
Icon=flashhop
Categories=Network;FileTransfer;
EOF
    command -v update-desktop-database >/dev/null 2>&1 && update-desktop-database "$APPS" 2>/dev/null || true
    echo "Installed to $BIN/flashhop and added to your app menu."
    case ":$PATH:" in *":$BIN:"*) ;; *) echo "Note: $BIN is not in your PATH; launch it from the app menu or add it." ;; esac
    echo "If a firewall is on, allow port 47821 (e.g. sudo ufw allow 47821)."
    ;;
Darwin)
    need ditto
    echo "Downloading FlashHop for macOS..."
    curl -fL --progress-bar "$URL/flashhop-macos.zip" -o "$TMP/f.zip"
    ditto -x -k "$TMP/f.zip" "$TMP"
    DEST=/Applications
    [ -w "$DEST" ] || { DEST="$HOME/Applications"; mkdir -p "$DEST"; }
    rm -rf "$DEST/FlashHop.app"
    mv "$TMP/FlashHop.app" "$DEST/"
    # Unsigned app: files fetched by curl carry no quarantine flag, but clear it in case.
    xattr -dr com.apple.quarantine "$DEST/FlashHop.app" 2>/dev/null || true
    echo "Installed to $DEST/FlashHop.app. Open it from Launchpad or: open -a FlashHop"
    ;;
*)
    die "unsupported OS $(uname -s). Windows: in PowerShell run  irm https://raw.githubusercontent.com/$REPO/main/install.ps1 | iex   Android: $URL/flashhop-android.apk"
    ;;
esac
