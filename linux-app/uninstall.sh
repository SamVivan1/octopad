#!/usr/bin/env bash
set -e

APPS_DIR="$HOME/.local/share/applications"
AUTOSTART_DIR="$HOME/.config/autostart"
ICONS_BASE="$HOME/.local/share/icons/hicolor"
PIXMAPS_DIR="$HOME/.local/share/pixmaps"
SYSTEMD_USER_DIR="$HOME/.config/systemd/user"

echo "================================================================"
echo " 🗑️  Uninstalling OctoPad Desktop App & Components"
echo "================================================================"

# 1. Stop and remove the systemd service if it still exists
echo "[1/4] Checking and stopping the background service..."
if systemctl --user is-active --quiet octopad 2>/dev/null; then
    echo " -> Stopping service octopad..."
    systemctl --user stop octopad 2>/dev/null || true
fi
systemctl --user disable octopad 2>/dev/null || true
rm -f "$SYSTEMD_USER_DIR/octopad.service"
systemctl --user daemon-reload 2>/dev/null || true
echo " -> Background service cleaned up successfully."

# 2. Remove the desktop shortcut and autostart files
echo "[2/4] Removing the application menu shortcut and autostart entry..."
rm -f "$APPS_DIR/octopad.desktop" "$APPS_DIR/io.github.samvivan1.octopad.desktop"
rm -f "$AUTOSTART_DIR/octopad.desktop" "$AUTOSTART_DIR/io.github.samvivan1.octopad.desktop"

# 3. Remove all application & tray icon variations
echo "[3/4] Removing application and tray icons..."
# App icon (all installed raster sizes + scalable SVG)
for s in 48 64 128 256 512; do
    rm -f "$ICONS_BASE/${s}x${s}/apps/octopad.png"
done
rm -f "$ICONS_BASE/scalable/apps/octopad.svg"
# Tray icons (3 statuses x every installed size + scalable)
find "$ICONS_BASE" -type f \( -name 'octopad-tray*.png' -o -name 'octopad-tray*.svg' \) -delete 2>/dev/null || true
# Pixmaps fallbacks
rm -f "$PIXMAPS_DIR/octopad.png"
rm -f "$PIXMAPS_DIR/octopad.svg"
# Drop now-empty app icon directories (best effort, never fail the uninstall)
find "$ICONS_BASE" -type d -empty -delete 2>/dev/null || true

# 4. Refresh the icon & desktop cache
echo "[4/4] Refreshing the application & icon cache..."
if command -v gtk-update-icon-cache &>/dev/null; then
    # -t is required because we do not ship an index.theme in the user hicolor dir.
    gtk-update-icon-cache -f -t "$ICONS_BASE" 2>/dev/null || true
fi
if command -v update-desktop-database &>/dev/null; then
    update-desktop-database "$APPS_DIR" 2>/dev/null || true
fi

echo "================================================================"
echo " ✅ SUCCESS! OctoPad has been cleanly removed from the Ubuntu system."
echo ""
echo " Note: Your profile & key configuration files at:"
echo "   $HOME/.config/octopad/"
echo " are kept in case you want to use them again later."
echo " To remove everything including the configuration, run:"
echo "   rm -rf $HOME/.config/octopad"
echo "================================================================"
