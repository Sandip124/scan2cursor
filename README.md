# Barcode bridge
Phone scans a barcode -> it is typed + Enter wherever the desktop cursor is.
Same Wi-Fi, or from any network (mobile data) with: ./barcode-bridge tunnel on
Several phones can connect at once. One small native program (Rust): no Node or other runtime needed.

Install:   scripts\install.bat (Windows)   or  ./scripts/install.sh (macOS/Linux)
Uninstall: scripts\uninstall.bat (Windows) or  ./scripts/uninstall.sh, then delete this folder.
Then open https://localhost:8443/ and scan the QR code with each phone.

Commands (Windows: barcode-bridge.exe):
  ./barcode-bridge status | start | stop | restart | tunnel on|off|status | new-token | run | help

macOS: switch on Barcode Bridge in System Settings > Privacy & Security > Accessibility, or it cannot type.
Update after a code change (keeps phones paired): ./scripts/update.sh
Never share or commit .token or .cert.json. Logs: logs/barcode-bridge.log.

Full guide (pairing, several phones, troubleshooting, options): docs/GUIDE.md
