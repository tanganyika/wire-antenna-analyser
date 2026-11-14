#!/bin/sh
# Remove what install.sh put in place (your antenna files and settings are not touched).
PREFIX="${PREFIX:-$HOME/.local}"
SHARE="$PREFIX/share"
DESK="$(xdg-user-dir DESKTOP 2>/dev/null || true)"
[ -n "$DESK" ] || DESK="$HOME/Desktop"
rm -f "$PREFIX/bin/wireant" \
      "$SHARE/applications/wireant.desktop" \
      "$SHARE/applications/wireant-uninstall.desktop" \
      "$SHARE/icons/hicolor/scalable/apps/wireant.svg" \
      "$SHARE/mime/packages/wireant.xml" \
      "$DESK/wireant.desktop"
command -v update-mime-database >/dev/null 2>&1 && update-mime-database "$SHARE/mime" >/dev/null 2>&1 || true
command -v update-desktop-database >/dev/null 2>&1 && update-desktop-database "$SHARE/applications" >/dev/null 2>&1 || true
# the icon cache still lists the removed icon until it's rebuilt, and desktops trust it, so
# an icon installed elsewhere (e.g. by the .deb) wouldn't be found
command -v gtk-update-icon-cache >/dev/null 2>&1 && gtk-update-icon-cache -q -f -t "$SHARE/icons/hicolor" >/dev/null 2>&1 || true
rm -f "$SHARE/wireant/uninstall.sh"
rmdir "$SHARE/wireant" 2>/dev/null || true
echo "wireant removed."
