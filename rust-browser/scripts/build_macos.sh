#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "$0")/.."
export PATH="$HOME/.cargo/bin:$PATH"

echo "=> Building Rust Release binary for macOS..."
cargo build --release

APP_DIR="dist/AdBlockBrowser.app"
CONTENTS="$APP_DIR/Contents"
rm -rf "$APP_DIR"
mkdir -p "$CONTENTS/MacOS" "$CONTENTS/Resources"

cp "target/release/adblock-browser" "$CONTENTS/MacOS/adblock-browser"
chmod +x "$CONTENTS/MacOS/adblock-browser"
if [ -f "assets/AppIcon.icns" ]; then
  cp "assets/AppIcon.icns" "$CONTENTS/Resources/AppIcon.icns"
fi

cat > "$CONTENTS/Info.plist" << 'EOF'
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleName</key>
  <string>AdBlock Browser</string>
  <key>CFBundleDisplayName</key>
  <string>AdBlock Tarayıcı</string>
  <key>CFBundleIdentifier</key>
  <string>com.adblock.browser.rust</string>
  <key>CFBundleVersion</key>
  <string>0.1.0</string>
  <key>CFBundleShortVersionString</key>
  <string>0.1.0</string>
  <key>CFBundleExecutable</key>
  <string>adblock-browser</string>
  <key>CFBundleIconFile</key>
  <string>AppIcon</string>
  <key>CFBundlePackageType</key>
  <string>APPL</string>
  <key>NSHighResolutionCapable</key>
  <true/>
</dict>
</plist>
EOF

touch "$APP_DIR"
echo "=> Bundled: $APP_DIR"
