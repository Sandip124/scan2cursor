# Scan2Cursor — user guide

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

1. builds the program if Rust is installed, otherwise uses the `scan2cursor` (or
   `scan2cursor.exe`) file already in this folder,
2. creates a new pairing token for this computer,
3. sets the server to start automatically when you log in,
4. opens the pairing page `http://localhost:8443/`.

**macOS only:** when asked, switch on **Scan2Cursor** in **System Settings → Privacy & Security →
Accessibility**. Without this it cannot type. If it is not listed, click **+** and choose
`Scan2Cursor.app` in this folder. Until it is allowed, the pairing page shows a red warning; the
warning disappears by itself once it works. No restart is needed.

**Windows only:** if a firewall prompt appears, click **Allow** for private networks.

## 2. Pair a phone

1. On the computer, open `http://localhost:8443/`.
2. Pick a tab and scan its QR code with the phone's camera, then open the link:
   - **Same Wi-Fi** (Wi-Fi icon in the code): phone on the same network as the computer. Fastest.
   - **From anywhere** (cloud icon in the code): phone on mobile data or another Wi-Fi. Needs the
     tunnel turned on, see [Different networks](#different-networks).

   The icon in the middle does not stop the code from scanning. `http://localhost:8443/#net`
   opens straight on the **From anywhere** tab.
3. On the **Same Wi-Fi** code, the phone warns once that the connection is not private. This is
   expected: the certificate is made by this computer. Choose *Advanced → Proceed* (Android) or
   *Show details → visit this website* (iPhone). See [Certificate warning](#certificate-warning).
4. Tap **SCAN**, point at a barcode. A beep means it was typed on the computer.

Tip: add the page to the phone's home screen (Safari: Share → Add to Home Screen; Chrome: menu →
Install app / Add to Home screen). It then opens full screen like an app, with the Scan2Cursor icon.
On an iPhone the home-screen app pairs once more on its first launch (it keeps its own storage),
using the QR code that is current at that moment.

### The scanner page on the phone

- **Top bar:** the Scan2Cursor logo, a status dot with text (green "ready" when scans will be
  typed), an icon for how the phone is connected (Wi-Fi symbol = same Wi-Fi, cloud = internet
  tunnel), and the phone's name.
- **Camera view:** only the code inside the white strip is read (yellow while scanning), so a
  label full of barcodes does not give the wrong one. A code is accepted after two matching reads.
- **🔦 and 1× 2× 3×:** torch and zoom, shown only when the phone allows them. On Android, tap the
  camera view to focus there.
- **SCAN button:** tap it, then point at a barcode. It sits a little above the bottom edge so the
  thumb reaches it easily. Tap again to cancel.

**iPhone Pro models:** the main camera cannot focus closer than about 9 cm, so close-up codes look
blurry. The page therefore starts at **2×**: hold the phone about 15–20 cm away and the code still
fills the strip, in focus.

Reading uses the phone's built-in barcode reader where the browser has one (Chrome on Android),
otherwise a decoder (zxing-cpp) that the computer serves to the phone. It never downloads anything
from the internet. If a phone reads badly, adding `&engine=zxing` to the scanner link forces the
second decoder, to compare.
- **Result line:** `✓` and the code when it was typed on the computer; `⏳` while it is waiting; `✗`
  with the reason when it was refused.

### No scan is lost

Every scan is saved on the phone first and only removed once the computer confirms it was typed
(or received by a web page using `bridge.js`). So the OK beep always means "it is in your software".

| You hear / see | Meaning |
|---|---|
| High beep, `✓ code` | Typed on the computer |
| Short soft tick, amber **N waiting** | Saved on the phone; it goes through by itself when the connection is back, or once the computer can type. No second beep when it does |
| Low double buzz | Not kept: 500 scans are already waiting, or the code is not valid |

- Phone offline (Wi-Fi gap, computer asleep): keep scanning. Scans are typed in scan order as soon
  as the phone reconnects, wherever the cursor is at that moment.
- Waiting more than 2 minutes: the phone asks first, **Send now** or **Discard**, so you can click
  into the right field on the computer before a batch of codes is typed.
- The page keeps the screen awake while it is open, and reconnects at once when the screen comes
  back on or the network returns.
- Waiting scans survive a closed browser or a restarted phone. The page itself needs the computer
  to open, so after a restart they go out as soon as the page loads again. (Opening the page with no
  connection at all is not possible on the **Same Wi-Fi** link: browsers do not allow offline pages
  on a self-made certificate.)
- Scanning the same barcode twice types it twice (on purpose: counting identical items). Only a
  scan the phone re-sends after a dropped connection is skipped if it was already typed.

### Several phones

Each phone scans the same QR code. The pairing page lists the phones connected right now, with
their address and scan count. Paired phones that are offline stay paired (they reconnect by
themselves, also when saved to the home screen) and are behind **Show N offline**.

To tell phones apart, tap the name at the right of the top bar on the phone's scanner page (e.g.
"iPhone · Safari") and type a name such as "Counter 2". The phone remembers it.

### Remove a phone

Click **Remove** next to a phone on the pairing page, for example a phone that is no longer used
or belongs to someone who left.

- That phone is disconnected at once and can no longer scan.
- The QR code changes, so the link that phone already has cannot pair it again.
- Your other phones keep working. They do not need to scan again.

To use a removed phone again, scan the new QR code with it.

How it works: the QR code only *pairs* a phone. When a phone pairs, the computer gives it its own
key, which the phone keeps. Removing a phone deletes its key. To remove **all** phones at once, run
`./scan2cursor new-token`.

### Certificate warning

- **On this computer:** none. Open the pairing page as `http://localhost:8443/`. Browsers treat
  `localhost` as secure, and the page never leaves this computer.
- **On the phone, "From anywhere" code:** none. It uses a normal certificate from Cloudflare.
- **On the phone, "Same Wi-Fi" code:** once per phone. The phone's camera only works over HTTPS,
  and no certificate authority issues certificates for a home network address like
  `192.168.x.x`, so the computer makes its own. Accepting it once is safe on your own Wi-Fi. If
  the computer's address changes, the phone asks again; a fixed address in the router avoids that.

## 3. Everyday commands

Run these from this folder. On Windows use `scan2cursor.exe` instead of `./scan2cursor`.

| Command | What it does |
|---|---|
| `./scan2cursor status` | Is it running? Shows the page address, network address and log file |
| `./scan2cursor start` | Start the background server |
| `./scan2cursor stop` | Stop it. It starts again at next login, or with `start` |
| `./scan2cursor restart` | Stop and start again (try this first when something is wrong) |
| `./scan2cursor tunnel on` | Let phones on other networks connect (see [Different networks](#different-networks)) |
| `./scan2cursor tunnel off` | Same Wi-Fi only again |
| `./scan2cursor tunnel status` | Is the tunnel on, and its current internet address |
| `./scan2cursor new-token` | Start over: new QR code and all paired phones forgotten; every phone scans again |
| `./scan2cursor run` | Run in this terminal instead of the background (shows errors; Ctrl+C stops) |
| `./scan2cursor help` | List the commands |

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
| Pairing page does not open | `./scan2cursor status`; if not running, `./scan2cursor start`. Still failing: `./scan2cursor run` shows the error |
| `address already in use` | Another program uses port 8443. Stop it, or see [Options](#6-options) |
| Phone says "offline" or cannot open the link | Phone on the same Wi-Fi as the computer? Not a *guest* network (those block devices from seeing each other)? Windows: firewall allowed? |
| Page shows a red "Scans cannot be typed yet" warning, phone says "not typed" (macOS) | Switch on **Scan2Cursor** under Accessibility (see Install). If it is on but still not working, see [macOS keeps asking](#macos-keeps-asking-for-accessibility) |
| Typed into the wrong window | It types wherever the cursor is. Click into the right field first |
| Phone shows "desktop not connected" | The server restarted; wait a second, it reconnects by itself |
| Internet QR code stuck on "Connecting…" | Computer online? `./scan2cursor tunnel status`; see the log. Some office networks block Cloudflare Tunnel |
| Internet link stopped working | The address changes when the server restarts. Scan the new "From anywhere" QR code |
| A phone should no longer have access | Click **Remove** next to it on the pairing page |
| Someone else may have the link | `./scan2cursor new-token`, then pair your phones again |

Problems are written to `logs/scan2cursor.log`.

### macOS keeps asking for Accessibility

macOS remembers the permission by the app's identity. On a Mac the program is therefore packaged
as `Scan2Cursor.app` (app ID `com.scan2cursor`) and signed with a local certificate, "Barcode
Bridge Local Signing", made once in your login keychain. The identity stays the same across
updates, so the permission is kept.

If the entry is switched on but typing still does not work, the entry is stale (it belongs to an
older build). Start fresh:

```
tccutil reset Accessibility com.scan2cursor
./scan2cursor restart
```

macOS asks again; switch on **Scan2Cursor**. Entries named `barcode-bridge` or **Barcode Bridge**
(the app's earlier name) are no longer used; remove them from the list with **−**.

### Updating after a code change

macOS/Linux: `./scripts/update.sh` rebuilds, packages and signs (macOS), swaps in the new program
and restarts it. Phones stay paired: the token is not changed. (Re-running the installer also works,
but it makes a new token.)

## 5. Files

| File | What it is |
|---|---|
| `scan2cursor` / `scan2cursor.exe` | The program (on a Mac: a link into `Scan2Cursor.app`) |
| `Scan2Cursor.app` | macOS only: the program packaged as an app, so the Accessibility permission sticks |
| `.token` | Pairing secret (in the QR code). Anyone with it can pair and type on this computer. Do not share or commit |
| `.devices.json` | Paired phones and their keys. Do not share or commit |
| `.cert.json` | HTTPS certificate and private key for this computer. Do not share or commit |
| `logs/` | Log files (rolled over at 1 MB) |
| `settings.json` | Saved settings (tunnel on/off) |
| `bin/cloudflared` | Cloudflare's tunnel program, downloaded by `tunnel on` |
| `.tunnel.json` | Current internet address, while the tunnel runs |
| `scripts/` | Installers and uninstallers |
| `src/`, `public/` | Source code (Rust server, web pages) |

## 6. Options

Set as environment variables before `./scan2cursor run` (or in the auto-start entry):

| Variable | Default | Meaning |
|---|---|---|
| `PORT` | `8443` | Port to listen on. If you change it, also change the installers and firewall rule |
| `ENTER` | on | `ENTER=0`: do not press Enter after each scan |
| `HOST_IP` | auto | Force the network address used in the QR code |
| `TUNNEL` | saved setting | `TUNNEL=1` / `TUNNEL=0`: tunnel on/off for this run only |
| `DRY_RUN` | off | `DRY_RUN=1`: log scans instead of typing (testing) |
| `TYPE` | on | `TYPE=0`: never type at the cursor; only web pages using `bridge.js` receive scans |

When the same phone sends scans fast (or several phones at once), the computer types them one by
one and waits 50 ms after each Enter before the next, so the app in front can keep up.

## 7. Uninstall

Windows: `scripts\uninstall.bat`. macOS/Linux: `./scripts/uninstall.sh`. Then delete this folder.

## Different networks

A phone on mobile data cannot reach a computer on a shop or home Wi-Fi directly: routers do not
accept incoming connections from the internet. The **internet tunnel** solves this with nothing to
install on the phone and no router settings.

### Turn it on

```
./scan2cursor tunnel on
```

The first time, this downloads Cloudflare's small `cloudflared` program into `bin/` (from
Cloudflare's official GitHub releases). Already have it installed (e.g. `brew install cloudflared`)?
Then that one is used. The server restarts and gets an address like
`https://calm-river-1234.trycloudflare.com`.

The pairing page's **From anywhere** tab now shows a QR code (with a cloud icon in the middle).
Scan it with a phone on any network.
It uses a normal certificate, so there is no "not private" warning.

Turn it off again with `./scan2cursor tunnel off`.

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

- **The address changes whenever the server restarts** (reboot, `restart`, `new-token`, update). Phones
  then need to scan the new **From anywhere** code. This is how Cloudflare's free, account-less
  "quick tunnel" works. A fixed address needs a free Cloudflare account and your own domain (not
  built in yet).
- **Cloudflare can see the traffic.** It handles the HTTPS connection, so it sees the scanned codes
  and the pairing token in the link. Fine for product barcodes. Do not scan secrets.
- **Quick tunnels have no uptime guarantee.** Cloudflare offers them free, for testing and light
  use. If it fails, phones on the same Wi-Fi keep working.
- **The address is public, but useless without the token.** Anyone who opens it without the
  token's link is refused. If a link leaked, run `./scan2cursor new-token`.

### Without the tunnel

Turn on the phone's **hotspot** and connect the computer to it. Both are then on the same network
and the **Same Wi-Fi** QR code works.
