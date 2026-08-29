#!/usr/bin/env bash
# uninstall.sh — Remove Chronos Screentime for the current user.
set -euo pipefail

BINARY_NAME="chronos-screentime"
INSTALL_DIR="$HOME/.local/bin"
ICON_DIR="$HOME/.local/share/icons"
APP_DIR="$HOME/.local/share/applications"
AUTOSTART_DIR="$HOME/.config/autostart"

rm -f "$INSTALL_DIR/$BINARY_NAME"        && echo "🗑  Removed binary"
rm -f "$ICON_DIR/$BINARY_NAME.png"       && echo "🗑  Removed icon"
rm -f "$APP_DIR/$BINARY_NAME.desktop"    && echo "🗑  Removed app drawer entry"
rm -f "$AUTOSTART_DIR/$BINARY_NAME.desktop" && echo "🗑  Removed autostart entry"

if command -v update-desktop-database &>/dev/null; then
  update-desktop-database "$APP_DIR" 2>/dev/null || true
fi

echo ""
echo "✅ Chronos Screentime uninstalled."
