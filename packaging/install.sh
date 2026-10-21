#!/bin/sh
# Build OnedriveLinux and install it for the current user (or system-wide with PREFIX=/usr/local).
#
#   ./packaging/install.sh              build (release) and install into ~/.local
#   PREFIX=/usr/local sudo -E ./packaging/install.sh
#   ./packaging/install.sh --uninstall  remove what was installed
#
# Installs: $PREFIX/bin/onedrivelinux, the desktop entry, the AppStream metadata and the icon.
set -eu

PREFIX="${PREFIX:-$HOME/.local}"
ROOT="$(cd "$(dirname "$0")/.." && pwd)"

BIN="$PREFIX/bin/onedrivelinux"
DESKTOP="$PREFIX/share/applications/OnedriveLinux.desktop"
METAINFO="$PREFIX/share/metainfo/io.github.onedrivelinux.metainfo.xml"
ICON="$PREFIX/share/icons/hicolor/96x96/apps/OnedriveLinux.png"

refresh_caches() {
    command -v update-desktop-database >/dev/null 2>&1 && update-desktop-database -q "$PREFIX/share/applications" || true
    command -v gtk-update-icon-cache >/dev/null 2>&1 && gtk-update-icon-cache -q -t "$PREFIX/share/icons/hicolor" || true
}

if [ "${1:-}" = "--uninstall" ]; then
    rm -f "$BIN" "$DESKTOP" "$METAINFO" "$ICON"
    refresh_caches
    echo "OnedriveLinux removed from $PREFIX (settings in ~/.config/onedrive-gui were kept)."
    exit 0
fi

if [ ! -x "$ROOT/target/release/onedrivelinux" ] || [ "${1:-}" != "--no-build" ]; then
    echo "Building OnedriveLinux (release)…"
    (cd "$ROOT" && cargo build --release --locked)
fi

install -Dm755 "$ROOT/target/release/onedrivelinux" "$BIN"
install -Dm644 "$ROOT/packaging/OnedriveLinux.desktop" "$DESKTOP"
install -Dm644 "$ROOT/packaging/io.github.onedrivelinux.metainfo.xml" "$METAINFO"
install -Dm644 "$ROOT/assets/icons/icons8-cloud-80.png" "$ICON"
refresh_caches

echo "Installed $BIN"
case ":$PATH:" in
    *":$PREFIX/bin:"*) ;;
    *) echo "Note: $PREFIX/bin is not on your PATH; start it as $BIN or from the application menu." ;;
esac
echo "Start it from the application menu (OnedriveLinux) or by running: onedrivelinux"
