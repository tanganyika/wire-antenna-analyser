#!/bin/sh
# Install wireant for the current user (no root needed).
# Mirrors what the original MMANA-GAL basic installer does (its aisetup.ini, see MECHANICS.md §11):
#   program            -> ~/.local/bin/wireant
#   menu entry         -> "wireant"                (original: program-group shortcut)
#   uninstall entry    -> "Uninstall wireant"      (original: "Uninstall <AppName>" shortcut)
#   desktop shortcut   -> on your desktop                    (original: <Desktop> shortcut; skip with NO_DESKTOP=1)
#   start it at the end, offered with "yes" as the default   (original: LaunchFile=1, LaunchFileChecked=1)
# Extras the original doesn't have: icon, and .maa / .nec / .ez files open with it.
# Uses an existing release build if there is one, otherwise builds it with cargo.
# Set PREFIX to install somewhere else (default ~/.local).
set -e
cd "$(dirname "$0")"
PREFIX="${PREFIX:-$HOME/.local}"
APP="wireant"

if [ ! -x target/release/wireant ] || [ "${REBUILD:-0}" = 1 ]; then
    if ! command -v cargo >/dev/null 2>&1; then
        echo "Rust isn't installed. Install it with:" >&2
        echo "  curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh" >&2
        exit 1
    fi
    echo "Building the program (first time takes a few minutes)..."
    cargo build --release
fi

SHARE="$PREFIX/share"
mkdir -p "$PREFIX/bin" "$SHARE/applications" "$SHARE/icons/hicolor/scalable/apps" "$SHARE/mime/packages" "$SHARE/wireant"
install -m 755 target/release/wireant "$PREFIX/bin/wireant"
install -m 644 packaging/wireant.svg "$SHARE/icons/hicolor/scalable/apps/wireant.svg"
install -m 644 packaging/wireant-mime.xml "$SHARE/mime/packages/wireant.xml"
install -m 755 uninstall.sh "$SHARE/wireant/uninstall.sh"
sed "s|^Exec=wireant|Exec=$PREFIX/bin/wireant|" packaging/wireant.desktop > "$SHARE/applications/wireant.desktop"
cat > "$SHARE/applications/wireant-uninstall.desktop" <<EOF
[Desktop Entry]
Type=Application
Name=Uninstall $APP
Exec=sh -c 'PREFIX="$PREFIX" "$SHARE/wireant/uninstall.sh"; sleep 2'
Icon=wireant
Terminal=true
Categories=HamRadio;Science;
NoDisplay=false
EOF

if [ "${NO_DESKTOP:-0}" != 1 ]; then
    DESK="$(xdg-user-dir DESKTOP 2>/dev/null || true)"
    [ -n "$DESK" ] || DESK="$HOME/Desktop"
    if [ -d "$DESK" ] && [ -w "$DESK" ]; then
        cp "$SHARE/applications/wireant.desktop" "$DESK/wireant.desktop"
        chmod +x "$DESK/wireant.desktop"
        command -v gio >/dev/null 2>&1 && gio set "$DESK/wireant.desktop" metadata::trusted true >/dev/null 2>&1 || true
        echo "Desktop shortcut: $DESK/wireant.desktop"
    fi
fi

command -v update-mime-database >/dev/null 2>&1 && update-mime-database "$SHARE/mime" >/dev/null 2>&1 || true
command -v update-desktop-database >/dev/null 2>&1 && update-desktop-database "$SHARE/applications" >/dev/null 2>&1 || true
command -v gtk-update-icon-cache >/dev/null 2>&1 && gtk-update-icon-cache -q -t "$SHARE/icons/hicolor" >/dev/null 2>&1 || true

echo "Installed: $PREFIX/bin/wireant"
echo "Start it from your applications menu ($APP) or run: wireant"
case ":$PATH:" in
    *":$PREFIX/bin:"*) ;;
    *) echo "Note: $PREFIX/bin isn't on your PATH; the menu entry still works." ;;
esac

# like the original's finish page: offer to start it, ticked by default
if [ -t 0 ] && [ "${NO_LAUNCH:-0}" != 1 ]; then
    printf "Start %s now? [Y/n] " "$APP"
    read -r answer || answer=n
    case "$answer" in
        n|N|no|No) ;;
        *) nohup "$PREFIX/bin/wireant" >/dev/null 2>&1 & ;;
    esac
fi
