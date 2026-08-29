#!/usr/bin/env bash
# install.sh — Install Chronos Screentime for the current user (no sudo needed).
# Copies the release binary to ~/.local/bin and registers it in the app drawer.
set -euo pipefail

BINARY_NAME="chronos-screentime"
INSTALL_DIR="$HOME/.local/bin"
ICON_DIR="$HOME/.local/share/icons"
APP_DIR="$HOME/.local/share/applications"
AUTOSTART_DIR="$HOME/.config/autostart"
ICON_SRC="$(dirname "$0")/icon-9.png"
BINARY_SRC="$(dirname "$0")/target/release/$BINARY_NAME"

# ── 1. Build if the release binary is missing ────────────────────────────────
if [ ! -f "$BINARY_SRC" ]; then
  echo "⚙  Release binary not found — building (this takes ~1 minute on first run)..."
  cargo build --release --manifest-path "$(dirname "$0")/Cargo.toml"
fi

# ── 2. Copy binary ────────────────────────────────────────────────────────────
mkdir -p "$INSTALL_DIR"
cp "$BINARY_SRC" "$INSTALL_DIR/$BINARY_NAME"
chmod +x "$INSTALL_DIR/$BINARY_NAME"
echo "✅ Binary installed → $INSTALL_DIR/$BINARY_NAME"

# ── 3. Install icon ───────────────────────────────────────────────────────────
if [ -f "$ICON_SRC" ]; then
  mkdir -p "$HOME/.local/share/icons/hicolor/256x256/apps"
  mkdir -p "$HOME/.local/share/icons/hicolor/48x48/apps"
  mkdir -p "$HOME/.local/share/pixmaps"
  mkdir -p "$ICON_DIR"
  cp "$ICON_SRC" "$HOME/.local/share/icons/hicolor/256x256/apps/$BINARY_NAME.png"
  cp "$ICON_SRC" "$HOME/.local/share/icons/hicolor/48x48/apps/$BINARY_NAME.png"
  cp "$ICON_SRC" "$HOME/.local/share/pixmaps/$BINARY_NAME.png"
  cp "$ICON_SRC" "$ICON_DIR/$BINARY_NAME.png"
  echo "✅ Icon installed to hicolor icon theme and pixmaps"
else
  echo "⚠  icon-9.png not found next to install.sh — no icon installed."
fi

# ── 4. Write .desktop entry (app drawer) ─────────────────────────────────────
mkdir -p "$APP_DIR"
cat > "$APP_DIR/$BINARY_NAME.desktop" <<EOF
[Desktop Entry]
Type=Application
Name=Chronos Screentime
GenericName=Screen Time Tracker
Comment=Lightweight screen-time tracker with Supabase sync
Exec="$INSTALL_DIR/$BINARY_NAME"
Icon=$HOME/.local/share/icons/hicolor/256x256/apps/$BINARY_NAME.png
Terminal=false
Categories=Utility;Clock;Monitor;
StartupWMClass=$BINARY_NAME
Keywords=screentime;productivity;tracker;chronos;time;
StartupNotify=true
EOF
echo "✅ App drawer entry → $APP_DIR/$BINARY_NAME.desktop"

# ── 5. Refresh desktop database & icon caches ────────────────────────────────
if command -v update-desktop-database &>/dev/null; then
  update-desktop-database "$APP_DIR" 2>/dev/null || true
fi
if command -v gtk-update-icon-cache &>/dev/null; then
  gtk-update-icon-cache -f -t "$HOME/.local/share/icons/hicolor" 2>/dev/null || true
  gtk-update-icon-cache -f -t "$ICON_DIR" 2>/dev/null || true
fi

# ── 6. Ensure ~/.local/bin is on PATH ────────────────────────────────────────
if [[ ":$PATH:" != *":$INSTALL_DIR:"* ]]; then
  echo ""
  echo "⚠  $INSTALL_DIR is not in your PATH."
  echo "   Add this line to your ~/.bashrc or ~/.zshrc and restart your terminal:"
  echo "   export PATH=\"\$HOME/.local/bin:\$PATH\""
fi

echo ""
echo "🎉 Chronos Screentime is installed!"
echo "   • Run from terminal:  chronos-screentime"
echo "   • Or open the Activities / app drawer and search for 'Chronos'"
echo ""
echo "   To also enable autostart (start with login), open Chronos → ⚙ Preferences"
echo "   and tick 'Start Chronos automatically at system logon'."
echo ""
echo "   To uninstall, run:  ./uninstall.sh"
