#!/bin/sh
# Linux, for running Alt Ledger from source (cargo run): adds it to your app
# menu and gives its window the Alt Ledger icon in the taskbar/dock instead of
# a generic one. Installed packages (.deb/.rpm/AppImage) already include this.
#
#   sh dev/install-desktop-entry.sh            install or update
#   sh dev/install-desktop-entry.sh --remove   undo
#
# Only writes to your own user folders; no sudo needed.
set -eu

here=$(cd "$(dirname "$0")/.." && pwd)
data="${XDG_DATA_HOME:-$HOME/.local/share}"
apps="$data/applications"
icons="$data/icons/hicolor"
# Must match the window's class / Wayland app ID, which is the binary name.
id="alt-ledger"

if [ "${1:-}" = "--remove" ]; then
    rm -f "$apps/$id.desktop"
    for size in 32 128 256 512; do rm -f "$icons/${size}x${size}/apps/$id.png"; done
    echo "Removed the Alt Ledger menu entry and icons."
    exit 0
fi

for pair in "32:32x32.png" "128:128x128.png" "256:128x128@2x.png" "512:icon.png"; do
    size=${pair%%:*}
    file=${pair#*:}
    mkdir -p "$icons/${size}x${size}/apps"
    cp "$here/app/src-tauri/icons/$file" "$icons/${size}x${size}/apps/$id.png"
done

# Prefer a release build if there is one.
bin="$here/target/release/$id"
[ -x "$bin" ] || bin="$here/target/debug/$id"

mkdir -p "$apps"
cat > "$apps/$id.desktop" <<EOF
[Desktop Entry]
Type=Application
Name=Alt Ledger
Comment=See your World of Warcraft characters on any screen
Exec="$bin"
Icon=$id
StartupWMClass=$id
Categories=Utility;Game;
Terminal=false
EOF

command -v update-desktop-database >/dev/null 2>&1 && update-desktop-database "$apps" 2>/dev/null || true
command -v gtk-update-icon-cache >/dev/null 2>&1 && gtk-update-icon-cache -q -t "$icons" 2>/dev/null || true

echo "Installed. Launches: $bin"
[ -x "$bin" ] || echo "(Build it first with: cargo build -p alt-ledger)"
