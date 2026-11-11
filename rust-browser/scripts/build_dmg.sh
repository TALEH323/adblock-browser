#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "$0")/.."

VERSION="${1:-0.1.0}"
APP_DIR="dist/AdBlockBrowser.app"
OUT_DMG="dist/AdBlockBrowser-${VERSION}-macOS.dmg"

if [ ! -d "$APP_DIR" ]; then
  echo "=> App bundle not found, building macOS app first..."
  bash scripts/build_macos.sh
fi

# Refresh Retina TIFF if tiffutil is present and @1x/@2x PNGs exist
if command -v tiffutil >/dev/null 2>&1 \
   && [ -f "assets/installer/dmg_background.png" ] \
   && [ -f "assets/installer/dmg_background@2x.png" ]; then
  tiffutil -cathidpicheck \
    "assets/installer/dmg_background.png" \
    "assets/installer/dmg_background@2x.png" \
    -out "assets/installer/dmg_background.tiff" >/dev/null
fi

# Ad-hoc sign bundle for clean local Gatekeeper / Finder metadata
if command -v codesign >/dev/null 2>&1; then
  codesign --force --deep --sign - "$APP_DIR" >/dev/null 2>&1 || true
fi

DMGBUILD_BIN=""
if command -v dmgbuild >/dev/null 2>&1; then
  DMGBUILD_BIN="dmgbuild"
else
  VENV_DIR="${TMPDIR:-/tmp}/adblock-dmgbuild-venv"
  if [ ! -x "$VENV_DIR/bin/dmgbuild" ]; then
    echo "=> Installing dmgbuild in temporary venv: $VENV_DIR"
    python3 -m venv "$VENV_DIR"
    "$VENV_DIR/bin/pip" install --quiet --upgrade pip dmgbuild
  fi
  DMGBUILD_BIN="$VENV_DIR/bin/dmgbuild"
fi

rm -f "$OUT_DMG"
echo "=> Building styled macOS DMG: $OUT_DMG"
"$DMGBUILD_BIN" \
  -s scripts/dmg_settings.py \
  -D root="$PWD" \
  -D app="$PWD/$APP_DIR" \
  "AdBlock Tarayici" \
  "$OUT_DMG"

echo "=> Ready: $PWD/$OUT_DMG"
ls -lh "$OUT_DMG"
