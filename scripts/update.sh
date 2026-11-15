#!/usr/bin/env bash
# macOS / Linux: rebuild and swap in a new scan2cursor, then restart it.
# Keeps the pairing token (phones stay paired) and the auto-start setup. Run install.sh the first time.
set -euo pipefail
DIR="$(cd "$(dirname "$0")/.." && pwd)"
cd "$DIR"
cargo build --release
if [[ "$(uname)" == "Darwin" ]]; then
  # Same app ID and signature every time, so macOS keeps the Accessibility permission.
  ./scripts/macos-bundle.sh target/release/scan2cursor
else
  cp target/release/scan2cursor .scan2cursor.new
  mv -f .scan2cursor.new scan2cursor   # new file, so the running copy is not overwritten in place
fi
./scan2cursor restart
