# Scan2Cursor guide

Scan a barcode with the phone; it is typed, followed by Enter, wherever the cursor is on the computer.

## 1. Install

Run from this folder. Needs [Rust](https://rustup.rs), or a prebuilt `scan2cursor` / `scan2cursor.exe` placed here.

| | macOS / Linux | Windows |
|---|---|---|
| Install | `./scripts/setup.sh install` | double-click `scripts\setup.bat`, choose **I** |
| Update after a code change | `./scripts/setup.sh update` | install again |
| Uninstall | `./scripts/setup.sh uninstall` | `scripts\setup.bat`, choose **U** |

The installer makes a new pairing token, sets the server to start at login, and opens the pairing
page. Updating keeps phones paired; installing again does not.

- **macOS:** switch on **Scan2Cursor** in System Settings → Privacy & Security → Accessibility.
  Without it nothing is typed, and the pairing page shows a red warning.
- **Windows:** click **Allow** if the firewall asks.

## 2. Pair a phone

1. On the computer, open `http://localhost:8443/`.
2. Scan a QR code with the phone's camera and open the link.
   - **Same Wi-Fi:** phone on the computer's network. The phone warns once that the connection is
     not private; continue (the certificate is made by this computer).
   - **From anywhere:** phone on mobile data or another Wi-Fi. Needs the [tunnel](#5-phones-on-other-networks).
3. Tap **SCAN** and point at a barcode.

The link is also shown under each QR code, with **Copy**. Anyone with the link can pair a phone.

**Home-screen app:** Safari: Share → Add to Home Screen. Chrome: menu → Install app.

**Pairing inside the app:** when the phone is not paired, the light blinks amber, the button reads
**PAIR**, and the camera looks for the QR code on the computer. Tap **PAIR** if the camera does not
start by itself.

**Internet address changed:** the installed app still opens and shows "Computer not reachable". Tap
**SCAN** and point at the **From anywhere** QR code. It reconnects and stays paired.

## 3. The scanner page

| Part | Meaning |
|---|---|
| Light | Green: ready. Red: no connection. Blinking amber: not paired |
| Wi-Fi / cloud icon | Connected over the same Wi-Fi / over the internet |
| Name (top right) | Tap to rename this phone |
| Corner marks | Only a code inside them is read |
| Bolt, 1× 2× 3× | Torch and zoom, where the phone has them |
| Readout | `✓` typed on the computer, `⏳` waiting, `✗` refused |

iPhone Pro: the page starts at 2×; hold the phone 15–20 cm away.

### No scan is lost

A scan stays on the phone until the computer confirms it was typed.

| Signal | Meaning |
|---|---|
| High beep, `✓` | Typed |
| Soft tick, **N waiting** | Saved on the phone; sent when the connection is back |
| Low double buzz | Refused: 500 scans already waiting, or not a valid code |

- Scans waiting longer than 2 minutes ask first: **Send now** or **Discard**.
- Waiting scans survive a closed browser or a restarted phone.
- The same barcode scanned twice is typed twice.

### Several phones, removing a phone

Every phone scans the same QR code. The pairing page lists them; offline ones are under
**Show N offline**.

**Remove** disconnects that phone and changes the QR code. Other phones keep working. To forget all
phones: `./scan2cursor new-token`.

## 4. Commands

Run from this folder. On Windows use `scan2cursor.exe`.

| Command | What it does |
|---|---|
| `./scan2cursor status` | Running? Shows addresses and the log file |
| `./scan2cursor start` / `stop` / `restart` | Control the background server |
| `./scan2cursor tunnel on` / `off` / `status` | Internet tunnel |
| `./scan2cursor new-token` | New QR code, all phones forgotten |
| `./scan2cursor run` | Run in this window (shows errors, Ctrl+C stops) |

**Windows:** `scan2cursor.exe` alone, or a double-click, starts the server hidden in the background.
Closing the command window does not stop it; `scan2cursor.exe stop` does.

## 5. Phones on other networks

```
./scan2cursor tunnel on
```

The **From anywhere** tab then shows a QR code. It uses a free Cloudflare tunnel: no account, no
router settings. `tunnel off` turns it off.

- The address changes at every server restart. See "Internet address changed" above.
- Cloudflare can see the scanned codes and the pairing link. Do not scan secrets.
- No uptime guarantee. Phones on the same Wi-Fi are not affected.

Without the tunnel: connect the computer to the phone's hotspot and use the **Same Wi-Fi** code.

## 6. Troubleshooting

Errors are written to `logs/scan2cursor.log`.

| Problem | Fix |
|---|---|
| Pairing page does not open | `./scan2cursor status`, then `start`. Still failing: `run` shows the error |
| `address already in use` | Another program uses port 8443. Stop it, or set `PORT` |
| Phone cannot open the link | Same Wi-Fi as the computer, not a guest network. Windows: firewall allowed? |
| Red "Scans cannot be typed yet" (macOS) | Switch on **Scan2Cursor** under Accessibility. Already on: see below |
| Typed into the wrong window | Click into the right field first |
| **From anywhere** code stuck on "Connecting…" | `./scan2cursor tunnel status`. Some networks block Cloudflare Tunnel |
| Internet link stopped working | The address changed. Tap **SCAN** in the app and point at the new QR code |
| Wi-Fi link stopped working | The computer got a new address. Scan the QR code again; reserve the address in the router |
| Someone else may have the link | `./scan2cursor new-token`, then pair the phones again |

**macOS, Accessibility is on but nothing is typed:**

```
tccutil reset Accessibility com.scan2cursor
./scan2cursor restart
```

Then switch on **Scan2Cursor** again.

## 7. Options

Environment variables for `./scan2cursor run`:

| Variable | Default | Meaning |
|---|---|---|
| `PORT` | `8443` | Port to listen on |
| `ENTER` | on | `ENTER=0`: no Enter after a scan |
| `HOST_IP` | auto | Network address used in the QR code |
| `TUNNEL` | saved setting | `TUNNEL=1` / `TUNNEL=0` for this run only |
| `DRY_RUN` | off | `DRY_RUN=1`: log scans instead of typing |
| `TYPE` | on | `TYPE=0`: only web pages using `bridge.js` receive scans |

Adding `&engine=zxing` to the scanner link forces the second barcode decoder.

## 8. Files

| File | What it is |
|---|---|
| `scan2cursor`, `scan2cursor.exe`, `Scan2Cursor.app` | The program |
| `.token`, `.devices.json`, `.cert.json` | Secrets: pairing token, phone keys, HTTPS key. Never share or commit |
| `settings.json`, `.tunnel.json`, `.server-id` | Tunnel setting, current internet address, this computer's id |
| `bin/cloudflared` | Tunnel program, downloaded by `tunnel on` |
| `logs/` | Log files |
| `scripts/setup.sh`, `scripts/setup.bat` (+ `setup.ps1`) | Install, update, uninstall: macOS/Linux, Windows |
| `src/`, `public/` | Source: Rust server, web pages |

## 9. Build by hand

```
cargo build --release
```

Output: `target/release/scan2cursor` (`.exe` on Windows). The pages in `public/` are compiled into
the program, so a change there needs a rebuild.

Windows program from a Mac (once: `rustup target add x86_64-pc-windows-gnu`, `brew install mingw-w64`):

```
CARGO_TARGET_X86_64_PC_WINDOWS_GNU_LINKER=x86_64-w64-mingw32-gcc \
CC_x86_64_pc_windows_gnu=x86_64-w64-mingw32-gcc \
AR_x86_64_pc_windows_gnu=x86_64-w64-mingw32-ar \
cargo build --release --target x86_64-pc-windows-gnu
```
