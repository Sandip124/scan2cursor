#!/usr/bin/env bash
# macOS: package a built barcode-bridge binary as BarcodeBridge.app, sign it, and link
# ./barcode-bridge to it so the commands keep working.
#
# Why an app: macOS then tracks the Accessibility permission by app ID (com.barcodebridge) plus
# our signing certificate, instead of by file path + build fingerprint. The permission survives
# updates, shows as "Barcode Bridge" in System Settings, and can be reset with:
#   tccutil reset Accessibility com.barcodebridge
# Usage: scripts/macos-bundle.sh path/to/built/barcode-bridge
set -euo pipefail
DIR="$(cd "$(dirname "$0")/.." && pwd)"
SRC="${1:?usage: macos-bundle.sh path/to/built/barcode-bridge}"
APP="$DIR/BarcodeBridge.app"
EXE="$APP/Contents/MacOS/barcode-bridge"

mkdir -p "$APP/Contents/MacOS"
cat > "$APP/Contents/Info.plist" <<'EOF'
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
  <key>CFBundleIdentifier</key><string>com.barcodebridge</string>
  <key>CFBundleName</key><string>Barcode Bridge</string>
  <key>CFBundleDisplayName</key><string>Barcode Bridge</string>
  <key>CFBundleExecutable</key><string>barcode-bridge</string>
  <key>CFBundlePackageType</key><string>APPL</string>
  <key>CFBundleShortVersionString</key><string>2.0.0</string>
  <key>CFBundleVersion</key><string>2.0.0</string>
  <key>LSUIElement</key><true/>
</dict></plist>
EOF
cp "$SRC" "$EXE.new"
mv -f "$EXE.new" "$EXE"   # new file, so a running copy is never overwritten in place
"$DIR/scripts/macos-sign.sh" "$APP"
ln -sfn "BarcodeBridge.app/Contents/MacOS/barcode-bridge" "$DIR/barcode-bridge"
