<img src="public/logo.svg" width="96" alt="Scan2Cursor logo">

# Scan2Cursor

Use your phone as a barcode scanner for your computer. Scan a barcode with the phone and it is
typed, followed by Enter, wherever the cursor is: a billing app, a spreadsheet, a web form.

- **Nothing to install on the phone.** It runs in the browser and can be added to the home screen.
- **No scan is lost.** Scans wait on the phone while the connection is down and are typed, in order, when it is back.
- **Several phones at once**, on the same Wi-Fi or over the internet.
- **One small program.** Rust, about 3 MB, no runtime.

Tested on macOS. Windows and Linux are supported but not tested.

## Install

Needs [Rust](https://rustup.rs), or a prebuilt `scan2cursor` / `scan2cursor.exe` placed in this folder.

| | macOS / Linux | Windows |
|---|---|---|
| Install | `./scripts/setup.sh install` | double-click `scripts\setup.bat`, choose **I** |
| Update after a code change | `./scripts/setup.sh update` | install again |
| Uninstall | `./scripts/setup.sh uninstall` | `scripts\setup.bat`, choose **U** |

The installer makes a pairing token, starts the server at every login, and opens the pairing page.

**macOS:** switch on **Scan2Cursor** in System Settings → Privacy & Security → Accessibility.
Without it nothing is typed. After uninstalling, remove that entry and the certificate
**Scan2Cursor Local Signing** (Keychain Access → login) by hand.

## Pair a phone

1. Open <http://localhost:8443/> on the computer.
2. Scan the QR code with the phone's camera and open the link.
   - **Same Wi-Fi:** the phone warns once that the connection is not private; continue.
   - **From anywhere:** for a phone on mobile data. Turn it on with `./scan2cursor tunnel on`.
3. Tap **SCAN** and point at a barcode.

When the phone is not paired, the scanner page reads the QR code itself: point it at the computer.
The **From anywhere** address changes at every restart; tap **SCAN** in the app and point at the
new QR code to reconnect.

**Remove** on the pairing page disconnects one phone. The others keep working.

## Commands

Run from this folder. On Windows use `scan2cursor.exe`.

| Command | What it does |
|---|---|
| `./scan2cursor status` | Running? Shows addresses and the log file |
| `./scan2cursor start` / `stop` / `restart` | Control the background server |
| `./scan2cursor tunnel on` / `off` / `status` | Internet tunnel for phones on other networks |
| `./scan2cursor new-token` | New QR code, all phones forgotten |
| `./scan2cursor run` | Run in this window (shows errors) |

**Windows:** `scan2cursor.exe` alone, or a double-click, starts the server hidden in the background.

## Security

- `.token`, `.devices.json` and `.cert.json` are secrets. They stay on this computer and are
  ignored by git. Anyone with the pairing link can type on this computer.
- Link leaked: `./scan2cursor new-token`.
- With the tunnel on, Cloudflare can see the scanned codes.

## More

[docs/GUIDE.md](docs/GUIDE.md): scanner page, troubleshooting, options, files, building.
Logs: `logs/scan2cursor.log`.
