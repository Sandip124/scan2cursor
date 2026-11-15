<img src="public/logo.svg" width="96" alt="Scan2Cursor logo">

# Scan2Cursor

Use your phone as a barcode scanner for your computer. Scan a barcode with the phone and it is
typed, followed by Enter, wherever the cursor is on the computer: a billing app, a spreadsheet, a
web form.

- **Nothing to install on the phone.** It runs in the phone's browser.
- **Several phones at once.** Each one is listed on the pairing page and can be removed there.
- **Fast, offline barcode reading.** Uses the phone's built-in reader where available, otherwise a
  WebAssembly decoder served by the computer; torch and zoom buttons; installs to the home screen.
- **No scan is lost.** Scans wait on the phone when the connection drops and are typed, in order,
  when it is back. The OK beep only sounds once the code is really typed.
- **Same Wi-Fi, or any network.** An optional internet tunnel lets a phone on mobile data connect.
- **One small program.** Written in Rust, about 2 MB, no runtime to install.

Tested on macOS. Windows and Linux are supported by the code and scripts but have not been tested.

## Install

You need either [Rust](https://rustup.rs) (the installer builds the program) or a prebuilt
`scan2cursor` binary placed in this folder.

| OS | Run from this folder |
|---|---|
| macOS / Linux | `./scripts/install.sh` |
| Windows | `scripts\install.bat` |

The installer creates a pairing token for this computer, sets the server to start at login, and
opens the pairing page.

**macOS:** when asked, switch on **Scan2Cursor** in System Settings → Privacy & Security →
Accessibility. Without it the program cannot type.

## Pair a phone

1. Open <http://localhost:8443/> on the computer.
2. Scan the QR code with the phone's camera and open the link. Use the **Same Wi-Fi** tab, or
   **From anywhere** for a phone on mobile data (see below).
3. On the **Same Wi-Fi** code, the phone warns once that the connection is not private. That is
   expected (the certificate is made by your computer); choose to continue. The **From anywhere**
   code and the page on the computer have no warning.
4. Tap **SCAN** and point at a barcode.

## Remove a phone

Click **Remove** next to it on the pairing page. That phone is disconnected and its link stops
working; the QR code changes, and your other phones keep working without scanning again.

## Phone on mobile data or another Wi-Fi

```
./scan2cursor tunnel on
```

The pairing page's **From anywhere** tab then shows a QR code. It goes through a free
Cloudflare tunnel: no account, no router settings. The address changes whenever the server
restarts, so phones scan the new code after a reboot. Turn it off with `tunnel off`.

## Commands

Run from this folder. On Windows use `scan2cursor.exe`.

| Command | What it does |
|---|---|
| `./scan2cursor status` | Is it running? Shows the page address, internet address and log file |
| `./scan2cursor start` / `stop` / `restart` | Control the background server |
| `./scan2cursor tunnel on` / `off` / `status` | Internet tunnel for phones on other networks |
| `./scan2cursor new-token` | Start over: new QR code, all paired phones forgotten |
| `./scan2cursor run` | Run in this terminal instead of the background (shows errors) |
| `./scan2cursor help` | List the commands |

The server starts by itself at login and restarts if it crashes.

## Update

After changing the code, on macOS or Linux:

```
./scripts/update.sh
```

It rebuilds, swaps in the new program and restarts it. Phones stay paired.

## Uninstall

1. Run `./scripts/uninstall.sh` (Windows: `scripts\uninstall.bat`). This stops the server and the
   tunnel and removes the auto-start.
2. Delete this whole folder (the one this README is in).

On a Mac, two things live outside the folder and can be removed by hand:

- System Settings → Privacy & Security → Accessibility: select **Scan2Cursor** and click **−**.
- Keychain Access → login keychain: delete the certificate **Scan2Cursor Local Signing**.

## Security

- `.token` is the pairing secret in the QR code: anyone who has it can pair a phone and type on
  this computer. `.devices.json` holds each paired phone's key, and `.cert.json` the HTTPS private
  key. All three stay on this computer and are ignored by git: never share or commit them.
- A phone that should lose access: **Remove** it on the pairing page. If the QR link may have
  leaked, run `./scan2cursor new-token`.
- With the tunnel on, traffic passes through Cloudflare, which can see the scanned codes.

## More

- Full guide (several phones, troubleshooting, options, files): [docs/GUIDE.md](docs/GUIDE.md)
- Logs: `logs/scan2cursor.log`
