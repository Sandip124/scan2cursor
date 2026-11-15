#!/usr/bin/env bash
# macOS: sign barcode-bridge with a stable local certificate so the Accessibility permission survives
# rebuilds and updates. Without this, every build gets a new ad-hoc signature and macOS treats it
# as a new app that must be allowed again.
#
# Creates a self-signed code-signing certificate "Barcode Bridge Local Signing" in the login keychain
# (once; only usable on this Mac), then signs the binary with it.
# Usage: scripts/macos-sign.sh path/to/BarcodeBridge.app   (or a plain binary)
set -euo pipefail
BIN="${1:?usage: macos-sign.sh path/to/BarcodeBridge.app}"
NAME="Barcode Bridge Local Signing"
KEYCHAIN="$HOME/Library/Keychains/login.keychain-db"
OPENSSL=/usr/bin/openssl   # system LibreSSL: its .p12 format is what `security import` expects

if ! security find-certificate -c "$NAME" "$KEYCHAIN" >/dev/null 2>&1; then
  echo "Creating local signing certificate \"$NAME\" (one time)..."
  T="$(mktemp -d)"; trap 'rm -rf "$T"' EXIT
  cat > "$T/cfg" <<EOF
[req]
distinguished_name = dn
x509_extensions = ext
prompt = no
[dn]
CN = $NAME
[ext]
basicConstraints = critical, CA:false
keyUsage = critical, digitalSignature
extendedKeyUsage = critical, codeSigning
EOF
  "$OPENSSL" req -x509 -newkey rsa:2048 -nodes -days 3650 -config "$T/cfg" \
    -keyout "$T/key.pem" -out "$T/cert.pem" 2>/dev/null
  PASS="$("$OPENSSL" rand -hex 16)"
  "$OPENSSL" pkcs12 -export -inkey "$T/key.pem" -in "$T/cert.pem" -name "$NAME" \
    -out "$T/id.p12" -passout "pass:$PASS"
  # -T: let codesign use the key without a keychain prompt on every build.
  security import "$T/id.p12" -k "$KEYCHAIN" -P "$PASS" -T /usr/bin/codesign >/dev/null
fi

codesign --force --sign "$NAME" --identifier com.barcodebridge "$BIN"
echo "Signed $BIN as com.barcodebridge."
