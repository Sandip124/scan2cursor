#!/usr/bin/env bash
# macOS / Linux uninstaller: stops the server and removes auto-start.
DIR="$(cd "$(dirname "$0")/.." && pwd)"
if [[ "$(uname)" == "Darwin" ]]; then
  for n in com.barcodebridge com.barcodetyper; do
    P="$HOME/Library/LaunchAgents/$n.plist"
    [[ -f "$P" ]] && { launchctl unload "$P" 2>/dev/null; rm -f "$P"; echo "Removed auto-start $n"; }
  done
else
  for n in barcode-bridge barcode-typer; do
    U="$HOME/.config/systemd/user/$n.service"
    [[ -f "$U" ]] && { systemctl --user disable --now "$n" 2>/dev/null; rm -f "$U"; echo "Removed auto-start $n"; }
  done
  systemctl --user daemon-reload 2>/dev/null || true
fi
pkill -f "$DIR/barcode-bridge" 2>/dev/null && echo "Stopped server"
pkill -f "$DIR/BarcodeBridge.app/Contents/MacOS/barcode-bridge" 2>/dev/null && echo "Stopped server"
pkill -f "$DIR/server.js" 2>/dev/null   # older Node version
pkill -f "cloudflared tunnel --no-autoupdate --url https://127.0.0.1:8443" 2>/dev/null && echo "Stopped internet tunnel"
pkill -f "$DIR/typer.js" 2>/dev/null
command -v lsof >/dev/null && for p in $(lsof -ti tcp:8443 -sTCP:LISTEN 2>/dev/null); do kill "$p" && echo "Stopped process $p"; done
echo "Uninstalled. You can now delete this folder."
