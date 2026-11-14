#!/bin/sh
# Build a .deb (Linux Mint, Ubuntu, Debian, Zorin, Pop!_OS ...) from a built wireant binary.
#   packaging/linux/build-deb.sh <path/to/wireant> <version> [output dir]
# Installs /usr/bin/wireant, the menu entry, icons and the .maa/.nec/.ez file types.
# The binary should be built against an old glibc (CI uses cargo zigbuild ... .2.31) so the
# package installs on older releases too; the Depends line below says which glibc it needs.
set -e
BIN="$1"
VER="$2"
OUT="${3:-.}"
[ -x "$BIN" ] && [ -n "$VER" ] || { echo "usage: $0 <wireant binary> <version> [output dir]" >&2; exit 1; }
HERE="$(cd "$(dirname "$0")/../.." && pwd)"

# the newest glibc symbol version the binary uses -> minimum libc6
GLIBC=$(objdump -T "$BIN" 2>/dev/null | grep -o 'GLIBC_[0-9.]*' | sed 's/GLIBC_//' | sort -Vu | tail -1)
GLIBC="${GLIBC:-2.31}"

ROOT="$(mktemp -d)"
trap 'rm -rf "$ROOT"' EXIT
install -Dm755 "$BIN" "$ROOT/usr/bin/wireant"
install -Dm644 "$HERE/packaging/wireant.desktop" "$ROOT/usr/share/applications/wireant.desktop"
install -Dm644 "$HERE/packaging/wireant.svg" "$ROOT/usr/share/icons/hicolor/scalable/apps/wireant.svg"
install -Dm644 "$HERE/packaging/wireant-1024.png" "$ROOT/usr/share/pixmaps/wireant.png"
install -Dm644 "$HERE/packaging/wireant-mime.xml" "$ROOT/usr/share/mime/packages/wireant.xml"
install -Dm644 "$HERE/README.md" "$ROOT/usr/share/doc/wireant/README.md"
install -Dm644 "$HERE/LICENSE" "$ROOT/usr/share/doc/wireant/copyright"

SIZE=$(du -sk "$ROOT/usr" | cut -f1)
mkdir -p "$ROOT/DEBIAN"
cat > "$ROOT/DEBIAN/control" <<EOF
Package: wireant
Version: $VER
Section: hamradio
Priority: optional
Architecture: amd64
Installed-Size: $SIZE
Depends: libc6 (>= $GLIBC), libgl1, libegl1, libxkbcommon0, libx11-6, libxcursor1, libxrandr2, libxi6
Recommends: libxkbcommon-x11-0, libwayland-client0, libvulkan1, xdg-desktop-portal
Maintainer: wireant <https://github.com/tanganyika/wireant/issues>
Homepage: https://github.com/tanganyika/wireant
Description: Model wire antennas (MININEC, wiremom and NEC-2 engines)
 Desktop program for modelling wire antennas: impedance, SWR, gain and
 radiation patterns, frequency sweeps and an optimizer. Reads and writes
 MMANA (.maa), NEC (.nec) and EZNEC (.ez) files.
EOF
# menu, file types and icons are picked up by dpkg triggers on current systems; these
# cover older ones
cat > "$ROOT/DEBIAN/postinst" <<'EOF'
#!/bin/sh
set -e
command -v update-mime-database >/dev/null 2>&1 && update-mime-database /usr/share/mime || true
command -v update-desktop-database >/dev/null 2>&1 && update-desktop-database -q /usr/share/applications || true
command -v gtk-update-icon-cache >/dev/null 2>&1 && gtk-update-icon-cache -q -t /usr/share/icons/hicolor || true
exit 0
EOF
cp "$ROOT/DEBIAN/postinst" "$ROOT/DEBIAN/postrm"
chmod 755 "$ROOT/DEBIAN/postinst" "$ROOT/DEBIAN/postrm"

mkdir -p "$OUT"
DEB="$OUT/wireant_${VER}_amd64.deb"
fakeroot dpkg-deb --build -Zxz "$ROOT" "$DEB" >/dev/null 2>&1 || dpkg-deb --root-owner-group --build -Zxz "$ROOT" "$DEB" >/dev/null
echo "$DEB"
