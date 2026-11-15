# Barcode Bridge

Use your phone as a barcode scanner for your computer. Scan a barcode with the phone and it is
typed, followed by Enter, wherever the cursor is on the computer: a billing app, a spreadsheet, a
web form.

- **Nothing to install on the phone.** It runs in the phone's browser.
- **Several phones at once.** Each one is listed on the pairing page.
- **Same Wi-Fi, or any network.** An optional internet tunnel lets a phone on mobile data connect.
- **One small program.** Written in Rust, about 2 MB, no runtime to install.

Tested on macOS. Windows and Linux are supported by the code and scripts but have not been tested.

## Install

You need either [Rust](https://rustup.rs) (the installer builds the program) or a prebuilt
`barcode-bridge` binary placed in this folder.

| OS | Run from this folder |
|---|---|
| macOS / Linux | `./scripts/install.sh` |
| Windows | `scripts\install.bat` |

The installer creates a pairing token for this computer, sets the server to start at login, and
opens the pairing page.

**macOS:** when asked, switch on **Barcode Bridge** in System Settings → Privacy & Security →
Accessibility. Without it the program cannot type.

## Pair a phone

1. Open <https://localhost:8443/> on the computer.
2. Scan the QR code with the phone's camera and open the link.
3. On the **Same Wi-Fi** code, the phone warns once that the connection is not private. That is
   expected (the certificate is made by your computer); choose to continue.
4. Tap **SCAN** and point at a barcode.

## Phone on mobile data or another Wi-Fi

```
./barcode-bridge tunnel on
```

The pairing page then shows a second QR code under **From anywhere**. It goes through a free
Cloudflare tunnel: no account, no router settings. The address changes whenever the server
restarts, so phones scan the new code after a reboot. Turn it off with `tunnel off`.

## Commands

Run from this folder. On Windows use `barcode-bridge.exe`.

| Command | What it does |
|---|---|
| `./barcode-bridge status` | Is it running? Shows the page address, internet address and log file |
| `./barcode-bridge start` / `stop` / `restart` | Control the background server |
| `./barcode-bridge tunnel on` / `off` / `status` | Internet tunnel for phones on other networks |
| `./barcode-bridge new-token` | New pairing token; every phone scans the QR code again |
| `./barcode-bridge run` | Run in this terminal instead of the background (shows errors) |
| `./barcode-bridge help` | List the commands |

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
2. Delete the whole `barcode-bridge` folder (the folder this README is in).

On a Mac, two things live outside the folder and can be removed by hand:

- System Settings → Privacy & Security → Accessibility: select **Barcode Bridge** and click **−**.
- Keychain Access → login keychain: delete the certificate **Barcode Bridge Local Signing**.

## Security

- `.token` is the pairing secret. Anyone who has it can type on this computer. `.cert.json` holds
  the HTTPS private key. Both stay on this computer and are ignored by git: never share or commit
  them.
- If a phone link may have leaked, run `./barcode-bridge new-token`.
- With the tunnel on, traffic passes through Cloudflare, which can see the scanned codes.

## More

- Full guide (several phones, troubleshooting, options, files): [docs/GUIDE.md](docs/GUIDE.md)
- Logs: `logs/barcode-bridge.log`
