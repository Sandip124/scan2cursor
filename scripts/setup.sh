#!/usr/bin/env bash
# macOS / Linux: install, update or uninstall Scan2Cursor.
#   ./scripts/setup.sh install     build (or use the binary in this folder), new pairing token, auto-start at login
#   ./scripts/setup.sh update      rebuild and restart; phones stay paired
#   ./scripts/setup.sh uninstall   stop the server and remove auto-start
set -euo pipefail
DIR="$(cd "$(dirname "$0")/.." && pwd)"
cd "$DIR"
BIN="$DIR/scan2cursor"
MAC=""; [[ "$(uname)" == "Darwin" ]] && MAC=1
APP="$DIR/Scan2Cursor.app"
PLIST="$HOME/Library/LaunchAgents/com.scan2cursor.plist"
UNIT="$HOME/.config/systemd/user/scan2cursor.service"

# macOS: sign with a local certificate, made once in the login keychain. The signature is then the
# same for every build, so macOS keeps the Accessibility permission across updates.
sign() {
  local name="Scan2Cursor Local Signing" keychain="$HOME/Library/Keychains/login.keychain-db"
  local openssl=/usr/bin/openssl   # system LibreSSL: its .p12 format is what `security import` expects
  if ! security find-certificate -c "$name" "$keychain" >/dev/null 2>&1; then
    echo "Creating local signing certificate \"$name\" (one time)..."
    local t pass; t="$(mktemp -d)"
    cat > "$t/cfg" <<EOF
[req]
distinguished_name = dn
x509_extensions = ext
prompt = no
[dn]
CN = $name
[ext]
basicConstraints = critical, CA:false
keyUsage = critical, digitalSignature
extendedKeyUsage = critical, codeSigning
EOF
    "$openssl" req -x509 -newkey rsa:2048 -nodes -days 3650 -config "$t/cfg" -keyout "$t/key.pem" -out "$t/cert.pem" 2>/dev/null
    pass="$("$openssl" rand -hex 16)"
    "$openssl" pkcs12 -export -inkey "$t/key.pem" -in "$t/cert.pem" -name "$name" -out "$t/id.p12" -passout "pass:$pass"
    # -T: let codesign use the key without a keychain prompt on every build.
    security import "$t/id.p12" -k "$keychain" -P "$pass" -T /usr/bin/codesign >/dev/null
    rm -rf "$t"
  fi
  codesign --force --sign "$name" --identifier com.scan2cursor "$APP"
  echo "Signed $APP as com.scan2cursor."
}

# Put a built binary in place. macOS: packaged as Scan2Cursor.app (the Accessibility permission is
# tracked by app ID), with ./scan2cursor as a link into it. Linux: ./scan2cursor itself.
place() {
  local src="$1" exe="$BIN"
  if [[ -n "$MAC" ]]; then
    exe="$APP/Contents/MacOS/scan2cursor"
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
  fi
  cp "$src" "$exe.new"
  mv -f "$exe.new" "$exe"   # new file, so a running copy is never overwritten in place
  if [[ -n "$MAC" ]]; then
    sign
    ln -sfn "Scan2Cursor.app/Contents/MacOS/scan2cursor" "$BIN"
  fi
}

install() {
  # Build from source when Rust is here; otherwise use a prebuilt binary placed in this folder.
  if [[ -d src ]] && command -v cargo >/dev/null; then
    cargo build --release
    place target/release/scan2cursor
  elif [[ -f "$BIN" && ! -L "$BIN" ]]; then
    [[ -z "$MAC" ]] || { cp "$BIN" "$DIR/.scan2cursor.build"; place "$DIR/.scan2cursor.build"; rm -f "$DIR/.scan2cursor.build"; }
  elif [[ ! -x "$BIN" ]]; then
    echo "No scan2cursor binary found. Put the prebuilt binary in $DIR, or install Rust (https://rustup.rs) and re-run."
    exit 1
  fi
  "$BIN" new-token --no-restart

  if [[ -n "$MAC" ]]; then
    mkdir -p "$(dirname "$PLIST")"
    cat > "$PLIST" <<EOF
<?xml version="1.0" encoding="UTF-8"?>
<plist version="1.0"><dict>
  <key>Label</key><string>com.scan2cursor</string>
  <key>ProgramArguments</key><array><string>$APP/Contents/MacOS/scan2cursor</string></array>
  <key>WorkingDirectory</key><string>$DIR</string>
  <key>RunAtLoad</key><true/><key>KeepAlive</key><true/>
  <key>ThrottleInterval</key><integer>2</integer>
</dict></plist>
EOF
    launchctl unload "$PLIST" 2>/dev/null || true
    launchctl load "$PLIST"
    echo "When asked, allow Scan2Cursor in System Settings > Privacy & Security > Accessibility (needed to type)."
  else
    mkdir -p "$(dirname "$UNIT")"
    cat > "$UNIT" <<EOF
[Unit]
Description=Scan2Cursor
[Service]
WorkingDirectory=$DIR
ExecStart=$BIN
Restart=always
[Install]
WantedBy=default.target
EOF
    systemctl --user daemon-reload
    systemctl --user enable scan2cursor
    systemctl --user restart scan2cursor
  fi
  sleep 2
  if [[ -n "$MAC" ]]; then open http://localhost:8443/; else xdg-open http://localhost:8443/; fi >/dev/null 2>&1 || true
  echo "Done. Scan the QR code on the page with your phone."
}

update() {
  cargo build --release
  place target/release/scan2cursor
  "$BIN" restart
}

uninstall() {
  set +e
  if [[ -n "$MAC" ]]; then
    [[ -f "$PLIST" ]] && { launchctl unload "$PLIST" 2>/dev/null; rm -f "$PLIST"; echo "Removed auto-start"; }
  else
    [[ -f "$UNIT" ]] && { systemctl --user disable --now scan2cursor 2>/dev/null; rm -f "$UNIT"; echo "Removed auto-start"; }
    systemctl --user daemon-reload 2>/dev/null
  fi
  pkill -f "$DIR/scan2cursor" 2>/dev/null && echo "Stopped server"
  pkill -f "$APP/Contents/MacOS/scan2cursor" 2>/dev/null && echo "Stopped server"
  pkill -f "cloudflared tunnel --no-autoupdate --url https://127.0.0.1:8443" 2>/dev/null && echo "Stopped internet tunnel"
  command -v lsof >/dev/null && for p in $(lsof -ti tcp:8443 -sTCP:LISTEN 2>/dev/null); do kill "$p" && echo "Stopped process $p"; done
  echo "Uninstalled. You can now delete this folder."
}

case "${1:-}" in
  install) install ;;
  update) update ;;
  uninstall) uninstall ;;
  *) echo "Usage: ./scripts/setup.sh install | update | uninstall"; exit 2 ;;
esac
