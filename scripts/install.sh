#!/usr/bin/env bash
# macOS / Linux one-step installer: builds (or uses) the binary, makes a new token, auto-starts at login.
set -euo pipefail
DIR="$(cd "$(dirname "$0")/.." && pwd)"
cd "$DIR"
BIN="$DIR/barcode-bridge"

# Build from source when the Rust toolchain is here; otherwise use a prebuilt binary placed in this folder.
BUILT=""
if [[ -d src ]] && command -v cargo >/dev/null; then
  cargo build --release
  BUILT="target/release/barcode-bridge"
elif [[ -f "$BIN" && ! -L "$BIN" ]]; then
  BUILT="$BIN"   # prebuilt binary
elif [[ ! -x "$BIN" ]]; then
  echo "No barcode-bridge binary found. Put the prebuilt binary in $DIR, or install Rust (https://rustup.rs) and re-run."
  exit 1
fi
RUN="$BIN"   # what auto-start runs
if [[ "$(uname)" == "Darwin" ]]; then
  # Package as BarcodeBridge.app with a stable signature, so the Accessibility permission is kept
  # across updates. ./barcode-bridge becomes a link to the program inside the app.
  if [[ -n "$BUILT" ]]; then
    cp "$BUILT" "$DIR/.barcode-bridge.build"
    "$DIR/scripts/macos-bundle.sh" "$DIR/.barcode-bridge.build"
    rm -f "$DIR/.barcode-bridge.build"
  fi
  RUN="$DIR/BarcodeBridge.app/Contents/MacOS/barcode-bridge"
elif [[ -n "$BUILT" && "$BUILT" != "$BIN" ]]; then
  cp "$BUILT" "$BIN.new"
  mv -f "$BIN.new" "$BIN"   # new file, so a running copy is never overwritten in place
fi
"$BIN" new-token --no-restart

if [[ "$(uname)" == "Darwin" ]]; then
  P="$HOME/Library/LaunchAgents/com.barcodebridge.plist"
  mkdir -p "$(dirname "$P")"
  cat > "$P" <<EOF
<?xml version="1.0" encoding="UTF-8"?>
<plist version="1.0"><dict>
  <key>Label</key><string>com.barcodebridge</string>
  <key>ProgramArguments</key><array><string>$RUN</string></array>
  <key>WorkingDirectory</key><string>$DIR</string>
  <key>RunAtLoad</key><true/><key>KeepAlive</key><true/>
  <key>ThrottleInterval</key><integer>2</integer>
</dict></plist>
EOF
  launchctl unload "$P" 2>/dev/null || true
  launchctl load "$P"
  OPEN=open
  echo "macOS: when asked, allow Barcode Bridge in System Settings > Privacy & Security > Accessibility (needed to type)."
else
  U="$HOME/.config/systemd/user"; mkdir -p "$U"
  cat > "$U/barcode-bridge.service" <<EOF
[Unit]
Description=Barcode bridge
[Service]
WorkingDirectory=$DIR
ExecStart=$RUN
Restart=always
[Install]
WantedBy=default.target
EOF
  systemctl --user daemon-reload
  systemctl --user enable barcode-bridge
  systemctl --user restart barcode-bridge
  OPEN=xdg-open
fi
sleep 2
"$OPEN" https://localhost:8443/ >/dev/null 2>&1 || true
echo "Done. Scan the QR code on the page with your phone."
