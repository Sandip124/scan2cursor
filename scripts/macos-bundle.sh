#!/usr/bin/env bash
# macOS: package a built scan2cursor binary as Scan2Cursor.app, sign it, and link
# ./scan2cursor to it so the commands keep working.
#
# Why an app: macOS then tracks the Accessibility permission by app ID (com.scan2cursor) plus
# our signing certificate, instead of by file path + build fingerprint. The permission survives
# updates, shows as "Scan2Cursor" in System Settings, and can be reset with:
#   tccutil reset Accessibility com.scan2cursor
# Usage: scripts/macos-bundle.sh path/to/built/scan2cursor
set -euo pipefail
DIR="$(cd "$(dirname "$0")/.." && pwd)"
SRC="${1:?usage: macos-bundle.sh path/to/built/scan2cursor}"
APP="$DIR/Scan2Cursor.app"
EXE="$APP/Contents/MacOS/scan2cursor"

mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources"
cp "$DIR/assets/AppIcon.icns" "$APP/Contents/Resources/AppIcon.icns"
cat > "$APP/Contents/Info.plist" <<'EOF'
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
  <key>CFBundleIdentifier</key><string>com.scan2cursor</string>
  <key>CFBundleName</key><string>Scan2Cursor</string>
  <key>CFBundleDisplayName</key><string>Scan2Cursor</string>
  <key>CFBundleExecutable</key><string>scan2cursor</string>
  <key>CFBundleIconFile</key><string>AppIcon</string>
  <key>CFBundlePackageType</key><string>APPL</string>
  <key>CFBundleShortVersionString</key><string>2.0.0</string>
  <key>CFBundleVersion</key><string>2.0.0</string>
  <key>LSUIElement</key><true/>
</dict></plist>
EOF
cp "$SRC" "$EXE.new"
mv -f "$EXE.new" "$EXE"   # new file, so a running copy is never overwritten in place
"$DIR/scripts/macos-sign.sh" "$APP"
ln -sfn "Scan2Cursor.app/Contents/MacOS/scan2cursor" "$DIR/scan2cursor"
