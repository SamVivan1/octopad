#!/usr/bin/env bash
#
# Build a Debian package (.deb) for the native Linux application.
#
# Usage: build_deb.sh <version> [output_dir]
#   version     Debian version string, e.g. "2.5.0" or "2.5.0-1"
#   output_dir  where to write the .deb (default: ./dist)
#
# The package installs the application under /usr/lib/octopad with a launcher at
# /usr/bin/octopad, a desktop entry, and hicolor icons. It is architecture
# independent (pure Python + distro GI typelibs).
set -euo pipefail

VERSION="${1:?usage: build_deb.sh <version> [output_dir]}"
OUT_DIR="${2:-dist}"

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
APP_SRC="$REPO_ROOT/linux-app"

PKG="octopad"
ARCH="all"
STAGE="$OUT_DIR/${PKG}_${VERSION}_${ARCH}"
DEB_PATH="$OUT_DIR/${PKG}_${VERSION}_${ARCH}.deb"

echo "Building $PKG $VERSION ($ARCH)"

rm -rf "$STAGE" "$DEB_PATH"
mkdir -p "$STAGE/DEBIAN"
LIB="$STAGE/usr/lib/$PKG"
mkdir -p "$LIB" "$STAGE/usr/bin" \
         "$STAGE/usr/share/applications" \
         "$STAGE/usr/share/icons/hicolor" \
         "$STAGE/usr/share/pixmaps" \
         "$STAGE/usr/share/metainfo" \
         "$STAGE/etc/xdg/autostart" \
         "$STAGE/usr/share/doc/$PKG"

# --- Application payload ---------------------------------------------------
cp -a "$APP_SRC/." "$LIB/"
rm -rf "$LIB/.venv"
find "$LIB" -type d -name __pycache__ -prune -exec rm -rf {} + 2>/dev/null || true
find "$LIB" -name '*.pyc' -delete 2>/dev/null || true
rm -rf "$LIB/tray-icons/out"
chmod +x "$LIB/main.py" "$LIB/run.sh" "$LIB/install.sh" "$LIB/uninstall.sh" 2>/dev/null || true

# --- Launcher --------------------------------------------------------------
cat > "$STAGE/usr/bin/$PKG" <<'EOF'
#!/usr/bin/env bash
exec /usr/bin/python3 /usr/lib/octopad/main.py "$@"
EOF
chmod 0755 "$STAGE/usr/bin/$PKG"

# --- Desktop entry ---------------------------------------------------------
cat > "$STAGE/usr/share/applications/io.github.samvivan1.octopad.desktop" <<'EOF'
[Desktop Entry]
Version=1.0
Type=Application
Name=OctoPad
GenericName=OctoPad Peripheral Manager
Comment=Configure keys, actions, and profiles for OctoPad
Exec=octopad
Icon=octopad
Terminal=false
Categories=Utility;HardwareSettings;Settings;
StartupWMClass=io.github.samvivan1.octopad
Keywords=macropad;keyboard;macro;hardware;peripheral;octopad;
EOF
chmod 644 "$STAGE/usr/share/applications/io.github.samvivan1.octopad.desktop"

# Autostart in the background (tray only) for every user session.
cat > "$STAGE/etc/xdg/autostart/io.github.samvivan1.octopad.desktop" <<'EOF'
[Desktop Entry]
Version=1.0
Type=Application
Name=OctoPad
GenericName=OctoPad Peripheral Manager
Comment=Start OctoPad in the background (tray)
Exec=octopad --hidden
Icon=octopad
Terminal=false
StartupWMClass=io.github.samvivan1.octopad
X-GNOME-Autostart-enabled=true
EOF

# --- Icons -----------------------------------------------------------------
install -Dm644 "$APP_SRC/octopad.svg" "$STAGE/usr/share/icons/hicolor/scalable/apps/octopad.svg"
install -Dm644 "$APP_SRC/octopad.png" "$STAGE/usr/share/pixmaps/octopad.png"

# Tray icons (3 statuses) into the hicolor theme.
if [ -d "$APP_SRC/tray-icons/hicolor" ]; then
    cp -a "$APP_SRC/tray-icons/hicolor/." "$STAGE/usr/share/icons/hicolor/"
fi

# Generate raster app-icon sizes when Pillow is available; the scalable SVG is
# always installed, so this is purely a fallback for themes that ignore SVG.
python3 - "$APP_SRC/octopad.png" "$STAGE/usr/share/icons/hicolor" <<'PY' || true
import os
import sys
try:
    from PIL import Image
except Exception:
    sys.exit(0)
src, base = sys.argv[1], sys.argv[2]
img = Image.open(src)
for size in (48, 64, 128, 256, 512):
    target = os.path.join(base, f"{size}x{size}", "apps")
    os.makedirs(target, exist_ok=True)
    img.resize((size, size), Image.Resampling.LANCZOS).save(
        os.path.join(target, "octopad.png"), "PNG")
PY

# --- AppStream metainfo ----------------------------------------------------
cat > "$STAGE/usr/share/metainfo/io.github.samvivan1.octopad.metainfo.xml" <<EOF
<?xml version="1.0" encoding="UTF-8"?>
<component type="desktop-application">
  <id>io.github.samvivan1.octopad</id>
  <name>OctoPad</name>
  <summary>Configure keys, actions and profiles for the OctoPad macropad</summary>
  <metadata_license>MIT</metadata_license>
  <project_license>MIT</project_license>
  <developer_name>SamVivan and the OctoPad contributors</developer_name>
  <url type="homepage">https://github.com/SamVivan1/octopad</url>
  <launchable type="desktop-id">io.github.samvivan1.octopad.desktop</launchable>
  <description>
    <p>
      Native desktop application for the OctoPad 8-key macropad. Configure the
      button mappings, monitor hardware events over USB Serial or Bluetooth LE,
      and trigger Home Assistant services directly.
    </p>
  </description>
  <content_rating type="oars-1.1"/>
  <releases>
    <release version="$VERSION" date="$(date -u +%Y-%m-%d)"/>
  </releases>
</component>
EOF

# --- Copyright -------------------------------------------------------------
cat > "$STAGE/usr/share/doc/$PKG/copyright" <<'EOF'
Format: https://www.debian.org/doc/packaging-manuals/copyright-format/1.0/
Upstream-Name: OctoPad
Source: https://github.com/SamVivan1/octopad

Files: *
Copyright: 2026 SamVivan and the OctoPad contributors
License: MIT
 Permission is hereby granted, free of charge, to any person obtaining a copy
 of this software and associated documentation files (the "Software"), to deal
 in the Software without restriction, including without limitation the rights
 to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
 copies of the Software, and to permit persons to whom the Software is
 furnished to do so, subject to the following conditions:
 .
 The above copyright notice and this permission notice shall be included in
 all copies or substantial portions of the Software.
 .
 THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
 IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
 FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
 AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
 LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
 OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
 SOFTWARE.
EOF

# --- Control ---------------------------------------------------------------
INSTALLED_SIZE="$(du -sk "$STAGE" | cut -f1)"
cat > "$STAGE/DEBIAN/control" <<EOF
Package: $PKG
Version: $VERSION
Section: utils
Priority: optional
Architecture: $ARCH
Maintainer: SamVivan and the OctoPad contributors <noreply@github.com>
Installed-Size: $INSTALLED_SIZE
Depends: python3 (>= 3.9), python3-gi, gir1.2-glib-2.0, gir1.2-gtk-4.0, gir1.2-adw-1 | gir1.2-adwaita-1, python3-serial
Recommends: pipewire-bin | pulseaudio-utils
Homepage: https://github.com/SamVivan1/octopad
Description: Native desktop application for the OctoPad macropad
 Configure the buttons, actions and profiles of the OctoPad 8-key macropad,
 monitor hardware events over USB Serial or Bluetooth LE, and trigger Home
 Assistant services directly. It runs as a normal desktop application with a
 system tray icon - no background daemon and no web server.
EOF

# --- Maintainer scripts ----------------------------------------------------
cat > "$STAGE/DEBIAN/postinst" <<'EOF'
#!/bin/sh
set -e
if [ "$1" = "configure" ]; then
    command -v gtk-update-icon-cache >/dev/null 2>&1 && \
        gtk-update-icon-cache -f -t /usr/share/icons/hicolor >/dev/null 2>&1 || true
    command -v update-desktop-database >/dev/null 2>&1 && \
        update-desktop-database -q /usr/share/applications >/dev/null 2>&1 || true
fi
exit 0
EOF
chmod 755 "$STAGE/DEBIAN/postinst"

cat > "$STAGE/DEBIAN/postrm" <<'EOF'
#!/bin/sh
set -e
if [ "$1" = "remove" ] || [ "$1" = "purge" ]; then
    command -v gtk-update-icon-cache >/dev/null 2>&1 && \
        gtk-update-icon-cache -f -t /usr/share/icons/hicolor >/dev/null 2>&1 || true
    command -v update-desktop-database >/dev/null 2>&1 && \
        update-desktop-database -q /usr/share/applications >/dev/null 2>&1 || true
fi
exit 0
EOF
chmod 755 "$STAGE/DEBIAN/postrm"

# --- Package ---------------------------------------------------------------
dpkg-deb --root-owner-group --build "$STAGE" "$DEB_PATH"
rm -rf "$STAGE"
echo "Built: $DEB_PATH"
