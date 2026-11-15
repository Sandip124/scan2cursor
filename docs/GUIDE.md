# Barcode bridge — user guide

Your phone becomes a barcode scanner for your computer. Scan a barcode on the phone and it is typed,
followed by Enter, wherever the cursor is on the computer: a billing app, a spreadsheet, a web form.

- **Same Wi-Fi, or anywhere.** On the same network it works directly. A phone on mobile data or
  another Wi-Fi can connect through the internet tunnel (see [Different networks](#different-networks)).
- **Several phones at once.** Any number of phones can be connected; every scan is typed.
- **Nothing to install on the phone.** It runs in the phone's browser.

---

## 1. Install

| OS | Run |
|---|---|
| Windows | double-click `scripts\install.bat` |
| macOS / Linux | `./scripts/install.sh` in a terminal, from this folder |

The installer:

1. builds the program if Rust is installed, otherwise uses the `barcode-bridge` (or
   `barcode-bridge.exe`) file already in this folder,
2. creates a new pairing token for this computer,
3. sets the server to start automatically when you log in,
4. opens the pairing page `https://localhost:8443/`.

**macOS only:** when asked, switch on **Barcode Bridge** in **System Settings → Privacy & Security →
Accessibility**. Without this it cannot type. If it is not listed, click **+** and choose
`BarcodeBridge.app` in this folder. The pairing page shows "Typing at cursor: ready" once it works;
no restart is needed.

**Windows only:** if a firewall prompt appears, click **Allow** for private networks.

## 2. Pair a phone

1. On the computer, open `https://localhost:8443/`.
2. Scan the QR code on that page with the phone's camera and open the link.
3. The first time, the phone warns that the connection is not private. This is expected: the
   certificate is made by this computer, not bought from a company. Choose *Advanced → Proceed*
   (Android) or *Show details → visit this website* (iPhone).
4. Tap **SCAN**, point at a barcode. A beep means it was typed on the computer.

Tip: add the page to the phone's home screen to open it like an app next time.

### Several phones

Each phone scans the same QR code. The pairing page lists every connected device with its address,
how long it has been connected, and how many scans it sent.

To tell phones apart, tap the name in the top-right corner of the scanner page on the phone (e.g.
"iPhone · Safari") and type a name such as "Counter 2". The phone remembers it.

## 3. Everyday commands

Run these from this folder. On Windows use `barcode-bridge.exe` instead of `./barcode-bridge`.

| Command | What it does |
|---|---|
| `./barcode-bridge status` | Is it running? Shows the page address, network address and log file |
| `./barcode-bridge start` | Start the background server |
| `./barcode-bridge stop` | Stop it. It starts again at next login, or with `start` |
| `./barcode-bridge restart` | Stop and start again (try this first when something is wrong) |
| `./barcode-bridge tunnel on` | Let phones on other networks connect (see [Different networks](#different-networks)) |
| `./barcode-bridge tunnel off` | Same Wi-Fi only again |
| `./barcode-bridge tunnel status` | Is the tunnel on, and its current internet address |
| `./barcode-bridge new-token` | New pairing token; every phone must scan the QR code again |
| `./barcode-bridge run` | Run in this terminal instead of the background (shows errors; Ctrl+C stops) |
| `./barcode-bridge help` | List the commands |

The server starts by itself at login and restarts itself if it crashes, so normally you never need
these.

### After the computer was shut down

Nothing to do: it starts when you log in. If the router gave the computer a new address, the phone's
link stops working. Open the pairing page and scan the QR code again (the phone shows the
certificate warning once more). To avoid this, give the computer a fixed address in the router
settings ("DHCP reservation").

## 4. Troubleshooting

| Problem | Fix |
|---|---|
| Pairing page does not open | `./barcode-bridge status`; if not running, `./barcode-bridge start`. Still failing: `./barcode-bridge run` shows the error |
| `address already in use` | Another program uses port 8443. Stop it, or see [Options](#6-options) |
| Phone says "offline" or cannot open the link | Phone on the same Wi-Fi as the computer? Not a *guest* network (those block devices from seeing each other)? Windows: firewall allowed? |
| Page says "Typing at cursor: not allowed", phone says "not typed" (macOS) | Switch on **Barcode Bridge** under Accessibility (see Install). If it is on but still not working, see [macOS keeps asking](#macos-keeps-asking-for-accessibility) |
| Typed into the wrong window | It types wherever the cursor is. Click into the right field first |
| Phone shows "desktop not connected" | The server restarted; wait a second, it reconnects by itself |
| Internet QR code stuck on "Connecting…" | Computer online? `./barcode-bridge tunnel status`; see the log. Some office networks block Cloudflare Tunnel |
| Internet link stopped working | The address changes when the server restarts. Scan the new "From anywhere" QR code |
| Someone else may have the link | `./barcode-bridge new-token`, then pair your phones again |

Problems are written to `logs/barcode-bridge.log`.

### macOS keeps asking for Accessibility

macOS remembers the permission by the app's identity. On a Mac the program is therefore packaged
as `BarcodeBridge.app` (app ID `com.barcodebridge`) and signed with a local certificate, "Barcode
Bridge Local Signing", made once in your login keychain. The identity stays the same across
updates, so the permission is kept.

If the entry is switched on but typing still does not work, the entry is stale (it belongs to an
older build). Start fresh:

```
tccutil reset Accessibility com.barcodebridge
./barcode-bridge restart
```

macOS asks again; switch on **Barcode Bridge**. An old entry named `barcode-bridge` (from before
the app packaging) can be removed from the list with **−**; it is no longer used.

### Updating after a code change

macOS/Linux: `./scripts/update.sh` rebuilds, packages and signs (macOS), swaps in the new program
and restarts it. Phones stay paired: the token is not changed. (Re-running the installer also works,
but it makes a new token.)

## 5. Files

| File | What it is |
|---|---|
| `barcode-bridge` / `barcode-bridge.exe` | The program (on a Mac: a link into `BarcodeBridge.app`) |
| `BarcodeBridge.app` | macOS only: the program packaged as an app, so the Accessibility permission sticks |
| `.token` | Pairing secret. Anyone with it can type on this computer. Do not share or commit |
| `.cert.json` | HTTPS certificate and private key for this computer. Do not share or commit |
| `logs/` | Log files (rolled over at 1 MB) |
| `settings.json` | Saved settings (tunnel on/off) |
| `bin/cloudflared` | Cloudflare's tunnel program, downloaded by `tunnel on` |
| `.tunnel.json` | Current internet address, while the tunnel runs |
| `scripts/` | Installers and uninstallers |
| `src/`, `public/` | Source code (Rust server, web pages) |

## 6. Options

Set as environment variables before `./barcode-bridge run` (or in the auto-start entry):

| Variable | Default | Meaning |
|---|---|---|
| `PORT` | `8443` | Port to listen on. If you change it, also change the installers and firewall rule |
| `ENTER` | on | `ENTER=0`: do not press Enter after each scan |
| `HOST_IP` | auto | Force the network address used in the QR code |
| `TUNNEL` | saved setting | `TUNNEL=1` / `TUNNEL=0`: tunnel on/off for this run only |
| `DRY_RUN` | off | `DRY_RUN=1`: log scans instead of typing (testing) |

## 7. Uninstall

Windows: `scripts\uninstall.bat`. macOS/Linux: `./scripts/uninstall.sh`. Then delete this folder.

## Different networks

A phone on mobile data cannot reach a computer on a shop or home Wi-Fi directly: routers do not
accept incoming connections from the internet. The **internet tunnel** solves this with nothing to
install on the phone and no router settings.

### Turn it on

```
./barcode-bridge tunnel on
```

The first time, this downloads Cloudflare's small `cloudflared` program into `bin/` (from
Cloudflare's official GitHub releases). Already have it installed (e.g. `brew install cloudflared`)?
Then that one is used. The server restarts and gets an address like
`https://calm-river-1234.trycloudflare.com`.

The pairing page now shows a second QR code, **From anywhere**. Scan it with a phone on any network.
It uses a normal certificate, so there is no "not private" warning.

Turn it off again with `./barcode-bridge tunnel off`.

### How it works

`cloudflared` connects *out* from this computer to Cloudflare, so no router or firewall change is
needed. The phone connects to Cloudflare, which passes the traffic through that connection:

```
phone (mobile data) --> Cloudflare (nearest data center) --> cloudflared on this computer --> server --> typed at cursor
```

Each scan typically takes 50–150 ms longer than on the same Wi-Fi, which is hard to notice. Phones on
the same Wi-Fi can keep using the **Same Wi-Fi** QR code, which is faster and does not need the
internet.

### Know before you use it

- **The address changes whenever the server restarts** (reboot, `restart`, `new-token`). Phones
  then need to scan the new **From anywhere** code. This is how Cloudflare's free, account-less
  "quick tunnel" works. A fixed address needs a free Cloudflare account and your own domain (not
  built in yet).
- **Cloudflare can see the traffic.** It handles the HTTPS connection, so it sees the scanned codes
  and the pairing token in the link. Fine for product barcodes. Do not scan secrets.
- **Quick tunnels have no uptime guarantee.** Cloudflare offers them free, for testing and light
  use. If it fails, phones on the same Wi-Fi keep working.
- **The address is public, but useless without the token.** Anyone who opens it without the
  token's link is refused. If a link leaked, run `./barcode-bridge new-token`.

### Without the tunnel

Turn on the phone's **hotspot** and connect the computer to it. Both are then on the same network
and the **Same Wi-Fi** QR code works.
