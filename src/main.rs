// Scan2Cursor: the phone scans a barcode, and it is typed + Enter wherever the desktop cursor is.
// Phone on the same Wi-Fi, or anywhere via the optional internet tunnel (tunnel.rs).
// One small binary: HTTPS + WebSocket server and keyboard typer.
//
// Usage:  scan2cursor [run]                    run the server in this terminal
//         scan2cursor start|stop|restart|status control the auto-start background server
//         scan2cursor tunnel on|off|status     reach this computer from any network
//         scan2cursor new-token [--no-restart] make a new pairing token (restarts the server)
// Env:    PORT (default 8443), HOST_IP (override LAN IP), ENTER=0 (no Enter after scan), TUNNEL=1/0,
//         DRY_RUN=1 (log instead of typing), TOKEN (fixed token), BB_HOME (data folder; default: next to binary)

mod devices;
mod tunnel;

use axum::{
    extract::{
        ws::{CloseFrame, Message, WebSocket, WebSocketUpgrade},
        ConnectInfo, Query, State,
    },
    http::{header, StatusCode},
    response::{IntoResponse, Response},
    routing::get,
    Json, Router,
};
use base64::Engine;
use futures_util::{SinkExt, StreamExt};
use hyper_util::rt::TokioIo;
use std::{
    collections::HashMap,
    fs,
    io::Write,
    net::{IpAddr, SocketAddr, UdpSocket},
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        mpsc as std_mpsc, Arc, Mutex,
    },
    time::Duration,
};
use tokio::{net::TcpListener, sync::mpsc};
use tokio_rustls::{
    rustls::{
        self,
        pki_types::{pem::PemObject, CertificateDer, PrivateKeyDer},
    },
    TlsAcceptor,
};
use tower::ServiceExt;

const INDEX_HTML: &str = include_str!("../public/index.html");
const SCANNER_HTML: &str = include_str!("../public/scanner.html");
const BRIDGE_JS: &str = include_str!("../public/bridge.js");
const HTML5_QRCODE_JS: &str = include_str!("../public/vendor/html5-qrcode.min.js");
const LOGO_SVG: &str = include_str!("../public/logo.svg");
const ICON_PNG: &[u8] = include_bytes!("../public/icon-180.png");
const MAX_CODE_CHARS: usize = 512;
const MAX_LOG_BYTES: u64 = 1024 * 1024;

// ---------- data folder, logging ----------

fn home_dir() -> PathBuf {
    if let Ok(d) = std::env::var("BB_HOME") {
        return PathBuf::from(d);
    }
    let Ok(exe) = std::env::current_exe() else { return PathBuf::from(".") };
    // macOS app bundle (Scan2Cursor.app/Contents/MacOS/scan2cursor): data lives next to the .app.
    let bundle = exe.ancestors().find(|p| p.extension().is_some_and(|e| e == "app"));
    bundle
        .and_then(Path::parent)
        .or_else(|| exe.parent())
        .map_or_else(|| PathBuf::from("."), Path::to_path_buf)
}

#[derive(Clone)]
struct Logger(Arc<Mutex<Option<fs::File>>>);

impl Logger {
    fn open(home: &Path) -> Self {
        let dir = home.join("logs");
        let _ = fs::create_dir_all(&dir);
        let path = dir.join("scan2cursor.log");
        // Keep the log small: roll over to .old once it passes 1 MB.
        if fs::metadata(&path).map(|m| m.len() > MAX_LOG_BYTES).unwrap_or(false) {
            let _ = fs::rename(&path, dir.join("scan2cursor.log.old"));
        }
        let file = fs::OpenOptions::new().create(true).append(true).open(path).ok();
        Logger(Arc::new(Mutex::new(file)))
    }

    fn log(&self, msg: &str) {
        let ts = time::OffsetDateTime::now_utc()
            .format(&time::format_description::well_known::Rfc3339)
            .unwrap_or_default();
        let line = format!("{ts} {msg}");
        eprintln!("{line}");
        if let Some(f) = self.0.lock().unwrap().as_mut() {
            let _ = writeln!(f, "{line}");
        }
    }
}

// ---------- pairing token ----------

fn write_private(path: &Path, data: &str) -> std::io::Result<()> {
    let mut opts = fs::OpenOptions::new();
    opts.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
        opts.mode(0o600);
        if path.exists() {
            fs::set_permissions(path, fs::Permissions::from_mode(0o600))?;
        }
    }
    opts.open(path)?.write_all(data.as_bytes())
}

/// `bytes` random bytes as hex, from the operating system's secure random source.
fn random_hex(bytes: usize) -> std::io::Result<String> {
    let mut buf = vec![0u8; bytes];
    getrandom::fill(&mut buf).map_err(|e| std::io::Error::other(e.to_string()))?;
    Ok(buf.iter().map(|b| format!("{b:02x}")).collect())
}

fn generate_token(home: &Path) -> std::io::Result<String> {
    let token = random_hex(16)?;
    write_private(&home.join(".token"), &token)?;
    Ok(token)
}

fn load_token(home: &Path) -> std::io::Result<String> {
    if let Ok(t) = std::env::var("TOKEN") {
        if !t.is_empty() {
            return Ok(t);
        }
    }
    let saved = fs::read_to_string(home.join(".token")).unwrap_or_default();
    let saved = saved.trim();
    if saved.is_empty() {
        generate_token(home)
    } else {
        Ok(saved.to_string())
    }
}

/// Constant-time comparison so the token cannot be guessed byte by byte from response timing.
fn same_secret(a: &str, b: &str) -> bool {
    a.len() == b.len() && a.bytes().zip(b.bytes()).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

fn cmd_new_token(home: &Path, restart: bool) {
    if let Err(e) = generate_token(home) {
        eprintln!("Could not write new token: {e}");
        std::process::exit(1);
    }
    devices::clear(home);
    println!("New QR code made and all paired phones forgotten. Every phone must scan the QR code again.");
    if restart {
        cmd_service(Action::Restart);
        println!("Open http://localhost:{}/ and scan the new QR code with each phone.", port());
    }
}

// ---------- start / stop / restart / status (controls the auto-start service the installer set up) ----------

#[derive(Clone, Copy, PartialEq)]
enum Action {
    Start,
    Stop,
    Restart,
}

fn port() -> u16 {
    std::env::var("PORT").ok().and_then(|p| p.parse().ok()).unwrap_or(8443)
}

fn sh(cmd: &str, args: &[&str]) -> bool {
    std::process::Command::new(cmd)
        .args(args)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .is_ok_and(|s| s.success())
}

fn server_answering() -> bool {
    std::net::TcpStream::connect_timeout(&SocketAddr::from(([127, 0, 0, 1], port())), Duration::from_millis(500)).is_ok()
}

/// Wait up to 15 s: launchd/systemd may delay a restart by a few seconds.
fn wait_for(up: bool) -> bool {
    (0..75).any(|_| {
        std::thread::sleep(Duration::from_millis(200));
        server_answering() == up
    })
}

/// Is the auto-start service installed? (Without it, start/stop have nothing to control.)
fn service_installed() -> bool {
    if cfg!(target_os = "macos") {
        std::env::var("HOME").is_ok_and(|h| Path::new(&h).join("Library/LaunchAgents/com.scan2cursor.plist").exists())
    } else if cfg!(windows) {
        sh("schtasks", &["/Query", "/TN", "Scan2Cursor"])
    } else {
        std::env::var("HOME").is_ok_and(|h| Path::new(&h).join(".config/systemd/user/scan2cursor.service").exists())
    }
}

fn service(action: Action) -> bool {
    if cfg!(target_os = "macos") {
        let uid = std::process::Command::new("id")
            .arg("-u")
            .output()
            .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
            .unwrap_or_default();
        let target = format!("gui/{uid}/com.scan2cursor");
        let plist = format!("{}/Library/LaunchAgents/com.scan2cursor.plist", std::env::var("HOME").unwrap_or_default());
        // Stop = unload (launchd would otherwise restart it). It loads again at next login or on start.
        let loaded = || sh("launchctl", &["print", &target]);
        match action {
            Action::Stop => !loaded() || sh("launchctl", &["bootout", &target]),
            Action::Start if loaded() => true,
            Action::Start => sh("launchctl", &["bootstrap", &format!("gui/{uid}"), &plist]),
            Action::Restart if loaded() => sh("launchctl", &["kickstart", "-k", &target]),
            Action::Restart => sh("launchctl", &["bootstrap", &format!("gui/{uid}"), &plist]),
        }
    } else if cfg!(windows) {
        let ps = match action {
            Action::Start => "Start-ScheduledTask -TaskName Scan2Cursor",
            Action::Stop => "Stop-ScheduledTask -TaskName Scan2Cursor",
            Action::Restart => "Stop-ScheduledTask -TaskName Scan2Cursor; Start-ScheduledTask -TaskName Scan2Cursor",
        };
        sh("powershell", &["-NoProfile", "-Command", ps])
    } else {
        let verb = match action {
            Action::Start => "start",
            Action::Stop => "stop",
            Action::Restart => "restart",
        };
        sh("systemctl", &["--user", verb, "scan2cursor"])
    }
}

fn cmd_service(action: Action) {
    if !service_installed() {
        eprintln!("Auto-start is not installed. Run scripts/install.sh (macOS/Linux) or scripts\\install.bat (Windows),");
        eprintln!("or run the server in this terminal with: scan2cursor run");
        std::process::exit(1);
    }
    let ok = service(action) && wait_for(action != Action::Stop);
    let what = match action {
        Action::Start => "started",
        Action::Stop => "stopped",
        Action::Restart => "restarted",
    };
    if ok {
        println!("Server {what}.");
        if action == Action::Stop {
            println!("It starts again at next login, or with: scan2cursor start");
        }
    } else {
        eprintln!("Could not confirm the server {what}. See logs/scan2cursor.log, or run 'scan2cursor run' to see errors.");
        std::process::exit(1);
    }
}

fn cmd_status(home: &Path) {
    let up = server_answering();
    println!("Server      : {}", if up { "running" } else { "NOT running (start it: scan2cursor start)" });
    println!("Auto-start  : {}", if service_installed() { "installed (starts at login)" } else { "not installed (run the installer)" });
    println!("Desktop page: http://localhost:{}/", port());
    println!("LAN address : {}", lan_ip());
    println!("Internet    : {}", tunnel_summary(home));
    println!("Log file    : {}", home.join("logs").join("scan2cursor.log").display());
    if !up {
        std::process::exit(1);
    }
}

fn tunnel_summary(home: &Path) -> String {
    match (tunnel::enabled(home), tunnel::current_url(home)) {
        (false, _) => "off (phones on other networks: scan2cursor tunnel on)".into(),
        (true, Some(url)) => format!("on, {url}"),
        (true, None) => "on, connecting (or server not running)".into(),
    }
}

fn cmd_tunnel(home: &Path, arg: Option<&str>) {
    let restart_if_installed = || {
        if service_installed() {
            cmd_service(Action::Restart);
        } else {
            println!("Start the server to apply: scan2cursor run");
        }
    };
    match arg {
        Some("on") => {
            if tunnel::find(home).is_none() {
                if let Err(e) = tunnel::download(home) {
                    eprintln!("Could not get cloudflared: {e}");
                    eprintln!("Install it yourself (https://developers.cloudflare.com/cloudflare-one/connections/connect-networks/downloads/) and re-run.");
                    std::process::exit(1);
                }
            }
            if let Err(e) = tunnel::set_enabled(home, true) {
                eprintln!("Could not save setting: {e}");
                std::process::exit(1);
            }
            println!("Internet tunnel turned on.");
            restart_if_installed();
            if service_installed() {
                println!("Waiting for the internet address...");
                let url = (0..60).find_map(|_| {
                    std::thread::sleep(Duration::from_millis(500));
                    tunnel::current_url(home)
                });
                match url {
                    Some(u) => println!("Ready: {u}\nOpen http://localhost:{}/ and scan the \"From anywhere\" QR code.", port()),
                    None => println!("Still connecting. Check logs/scan2cursor.log, or run: scan2cursor tunnel status"),
                }
            }
            println!("Note: the address changes whenever the server restarts; phones then re-scan the QR code.");
        }
        Some("off") => {
            if let Err(e) = tunnel::set_enabled(home, false) {
                eprintln!("Could not save setting: {e}");
                std::process::exit(1);
            }
            println!("Internet tunnel turned off. Phones must be on the same Wi-Fi.");
            restart_if_installed();
        }
        None | Some("status") => println!("Internet tunnel: {}", tunnel_summary(home)),
        Some(_) => {
            eprintln!("Usage: scan2cursor tunnel on | off | status");
            std::process::exit(2);
        }
    }
}

const USAGE: &str = "Usage: scan2cursor <command>
  run                       run the server in this terminal (Ctrl+C to stop)
  start | stop | restart    control the auto-start background server
  status                    is it running? shows page address, internet address and log file
  tunnel on | off | status  let phones on other networks (mobile data) connect, via Cloudflare
  new-token [--no-restart]  make a new pairing token (phones must re-scan the QR code)";

// ---------- network + TLS ----------

/// LAN address of the interface that has the default route (no packets are sent).
fn lan_ip() -> String {
    if let Ok(h) = std::env::var("HOST_IP") {
        return h;
    }
    UdpSocket::bind("0.0.0.0:0")
        .and_then(|s| s.connect("8.8.8.8:80").map(|_| s))
        .and_then(|s| s.local_addr())
        .map(|a| a.ip())
        .ok()
        .filter(|ip| ip.is_ipv4() && !ip.is_loopback() && !ip.is_unspecified())
        .map(|ip| ip.to_string())
        .unwrap_or_else(|| "localhost".into())
}

/// Self-signed cert for the LAN IP, cached in .cert.json and regenerated when the IP changes.
fn load_cert(home: &Path, host: &str) -> Result<(String, String), Box<dyn std::error::Error>> {
    let path = home.join(".cert.json");
    if let Ok(raw) = fs::read_to_string(&path) {
        if let Ok(c) = serde_json::from_str::<serde_json::Value>(&raw) {
            if c["host"] == host {
                if let (Some(k), Some(p)) = (c["key"].as_str(), c["cert"].as_str()) {
                    return Ok((p.to_string(), k.to_string()));
                }
            }
        }
    }
    let mut params = rcgen::CertificateParams::new(vec!["localhost".to_string()])?;
    params.distinguished_name.push(rcgen::DnType::CommonName, host);
    params.subject_alt_names.push(rcgen::SanType::IpAddress(IpAddr::from([127, 0, 0, 1])));
    if let Ok(ip) = host.parse::<IpAddr>() {
        params.subject_alt_names.push(rcgen::SanType::IpAddress(ip));
    }
    let now = time::OffsetDateTime::now_utc();
    params.not_before = now - time::Duration::days(1);
    params.not_after = now + time::Duration::days(825); // Apple's maximum for TLS certs
    let key = rcgen::KeyPair::generate()?;
    let cert = params.self_signed(&key)?;
    let (cert_pem, key_pem) = (cert.pem(), key.serialize_pem());
    let json = serde_json::json!({ "host": host, "key": key_pem, "cert": cert_pem });
    write_private(&path, &json.to_string())?;
    Ok((cert_pem, key_pem))
}

fn tls_acceptor(cert_pem: &str, key_pem: &str) -> Result<TlsAcceptor, Box<dyn std::error::Error>> {
    let certs = CertificateDer::pem_slice_iter(cert_pem.as_bytes()).collect::<Result<Vec<_>, _>>()?;
    let key = PrivateKeyDer::from_pem_slice(key_pem.as_bytes())?;
    let mut cfg = rustls::ServerConfig::builder_with_provider(Arc::new(rustls::crypto::ring::default_provider()))
        .with_safe_default_protocol_versions()?
        .with_no_client_auth()
        .with_single_cert(certs, key)?;
    cfg.alpn_protocols = vec![b"http/1.1".to_vec()];
    Ok(TlsAcceptor::from(Arc::new(cfg)))
}

// ---------- typing at the cursor ----------

/// One long-lived keyboard handle on its own thread: no process is spawned per scan.
/// `ready` tells the rest of the server whether typing actually works (macOS may not allow it yet).
fn start_typer(press_enter: bool, dry_run: bool, log: Logger, ready: Arc<AtomicBool>) -> std_mpsc::Sender<String> {
    use enigo::{Direction, Enigo, Key, Keyboard, Settings};
    let (tx, rx) = std_mpsc::channel::<String>();
    std::thread::spawn(move || {
        let mut kb: Option<Enigo> = None;
        let mut asked = false; // macOS: show the permission prompt once, not on every scan
        let mut reported = false; // log "not available" once until it works again
        let mut connect = |kb: &mut Option<Enigo>| {
            if kb.is_some() || dry_run {
                return;
            }
            let settings = Settings { open_prompt_to_get_permissions: !asked, ..Settings::default() };
            asked = true;
            match Enigo::new(&settings) {
                Ok(e) => {
                    log.log("keyboard ready: scans will be typed at the cursor");
                    reported = false;
                    *kb = Some(e);
                }
                Err(e) if !reported => {
                    reported = true;
                    log.log(&format!(
                        "keyboard not available: {e} (macOS: switch on Scan2Cursor in System Settings > Privacy & Security > Accessibility)"
                    ));
                }
                Err(_) => {}
            }
        };
        connect(&mut kb); // ask for permission at startup, not on the first scan
        loop {
            ready.store(kb.is_some() || dry_run, Ordering::Relaxed);
            // While the keyboard is not available, look again every 2 s, so it starts working by
            // itself once the permission is switched on.
            let raw = match rx.recv_timeout(Duration::from_secs(2)) {
                Ok(raw) => raw,
                Err(std_mpsc::RecvTimeoutError::Timeout) => {
                    connect(&mut kb);
                    continue;
                }
                Err(std_mpsc::RecvTimeoutError::Disconnected) => break,
            };
            let text: String = raw.chars().filter(|c| !c.is_control()).collect();
            if text.is_empty() {
                continue;
            }
            if dry_run {
                log.log(&format!("[dry-run] type {text:?}{}", if press_enter { " + Enter" } else { "" }));
                continue;
            }
            connect(&mut kb);
            let Some(e) = kb.as_mut() else { continue };
            let res = e
                .text(&text)
                .and_then(|_| if press_enter { e.key(Key::Return, Direction::Click) } else { Ok(()) });
            if let Err(err) = res {
                log.log(&format!("type failed: {err}"));
                kb = None; // reconnect on next scan
            }
        }
    });
    tx
}

// ---------- server state ----------

#[derive(Clone, Copy, PartialEq)]
enum Role {
    Phone,   // scanner page on the phone
    Desktop, // web page using bridge.js: receives scans as messages
    Monitor, // the pairing page: status only
}

struct Peer {
    role: Role,
    tx: mpsc::UnboundedSender<Message>,
    name: String,
    ip: String,
    since: i64, // ms since epoch
    scans: u64,
    last_scan: Option<i64>,
    device: Option<String>, // id of the paired phone (devices.rs); None for pages
}

fn now_ms() -> i64 {
    (time::OffsetDateTime::now_utc().unix_timestamp_nanos() / 1_000_000) as i64
}

/// Readable default name from the browser's User-Agent, e.g. "iPhone · Safari".
fn device_label(ua: &str) -> String {
    let os = [("iPhone", "iPhone"), ("iPad", "iPad"), ("Android", "Android"), ("Windows", "Windows"), ("Macintosh", "Mac"), ("Linux", "Linux")]
        .iter()
        .find(|(k, _)| ua.contains(k))
        .map_or("Device", |(_, v)| v);
    // Order matters: most browsers also claim "Chrome"/"Safari".
    let browser = [("SamsungBrowser", "Samsung Internet"), ("EdgA", "Edge"), ("EdgiOS", "Edge"), ("Edg/", "Edge"), ("CriOS", "Chrome"), ("FxiOS", "Firefox"), ("Firefox", "Firefox"), ("Chrome", "Chrome"), ("Safari", "Safari")]
        .iter()
        .find(|(k, _)| ua.contains(k))
        .map(|(_, v)| *v);
    match browser {
        Some(b) => format!("{os} · {b}"),
        None => os.to_string(),
    }
}

/// Device name chosen on the phone: printable, trimmed, at most 40 characters.
fn clean_name(raw: &str) -> Option<String> {
    let s: String = raw.chars().filter(|c| !c.is_control()).take(40).collect();
    let s = s.trim();
    (!s.is_empty()).then(|| s.to_string())
}

/// What a new phone needs to pair: the token in the QR code, and the tunnel address if there is one.
struct Pairing {
    token: String,
    tunnel_url: Option<String>,
}

struct App {
    home: PathBuf,
    log: Logger,
    lan_base: String, // https://<lan-ip>:<port>
    tunnel_enabled: bool,
    pairing: Mutex<Pairing>,
    pair_json: Mutex<serde_json::Value>, // what /api/pair returns; rebuilt when `pairing` changes
    devices: Mutex<devices::Store>,      // paired phones and their keys
    peers: Mutex<HashMap<u64, Peer>>,    // open connections
    next_id: AtomicU64,
    typer: std_mpsc::Sender<String>,
    typer_ready: Arc<AtomicBool>, // false while the OS does not let us type (macOS Accessibility)
}

fn qr_data_url(text: &str) -> String {
    // Highest error correction (about 30% of the code may be covered): the pairing page puts an icon
    // in the middle.
    let svg = qrcode::QrCode::with_error_correction_level(text.as_bytes(), qrcode::EcLevel::H)
        .map(|q| q.render::<qrcode::render::svg::Color>().min_dimensions(260, 260).build())
        .unwrap_or_default();
    format!("data:image/svg+xml;base64,{}", base64::engine::general_purpose::STANDARD.encode(svg))
}

impl App {
    fn token_ok(&self, given: Option<&String>) -> bool {
        given.is_some_and(|t| same_secret(t, &self.pairing.lock().unwrap().token))
    }

    /// Rebuild the pairing info (links + QR codes) and tell open pairing pages to reload it.
    fn refresh_pairing(&self) {
        let json = {
            let p = self.pairing.lock().unwrap();
            let lan = format!("{}/scanner.html?t={}", self.lan_base, p.token);
            let net = p.tunnel_url.as_ref().map(|u| format!("{u}/scanner.html?t={}", p.token));
            serde_json::json!({
                "token": p.token, "url": lan, "qr": qr_data_url(&lan),
                "tunnel": { "enabled": self.tunnel_enabled, "url": net, "qr": net.as_deref().map(qr_data_url) },
            })
        };
        *self.pair_json.lock().unwrap() = json;
        self.send_to(&[Role::Monitor], r#"{"type":"pair"}"#);
    }

    /// New (or lost) internet address.
    fn set_tunnel_url(&self, url: Option<String>) {
        self.pairing.lock().unwrap().tunnel_url = url;
        self.refresh_pairing();
    }

    /// Take away one phone's access: forget its key, disconnect it, and change the QR code's token
    /// so the link it already has cannot pair it again. Other phones keep their own keys.
    fn remove_device(&self, id: &str) {
        let name = self.devices.lock().unwrap().get_mut(id).map(|d| d.name.clone());
        if !self.devices.lock().unwrap().remove(id) {
            return;
        }
        {
            let mut peers = self.peers.lock().unwrap();
            let open: Vec<u64> = peers.iter().filter(|(_, p)| p.device.as_deref() == Some(id)).map(|(k, _)| *k).collect();
            for k in open {
                // Dropped from the list right away, so nothing more from it is accepted.
                if let Some(p) = peers.remove(&k) {
                    let _ = p.tx.send(Message::Close(Some(CloseFrame { code: CLOSE_REMOVED, reason: "removed".into() })));
                }
            }
        }
        match generate_token(&self.home) {
            Ok(t) => self.pairing.lock().unwrap().token = t,
            Err(e) => self.log.log(&format!("could not change the pairing token: {e}")),
        }
        self.log.log(&format!("removed device {id} ({}); pairing token changed", name.unwrap_or_default()));
        self.refresh_pairing();
        self.broadcast_status();
    }

    /// Messages from the pairing page.
    fn on_monitor(&self, raw: &str) {
        let Ok(m) = serde_json::from_str::<serde_json::Value>(raw) else { return };
        if let ("remove", Some(id)) = (m["type"].as_str().unwrap_or(""), m["id"].as_str()) {
            self.remove_device(id);
        }
    }

    fn send_to(&self, roles: &[Role], msg: &str) -> usize {
        let peers = self.peers.lock().unwrap();
        peers
            .values()
            .filter(|p| roles.contains(&p.role))
            .filter(|p| p.tx.send(Message::Text(msg.into())).is_ok())
            .count()
    }

    fn broadcast_status(&self) {
        let paired = self.devices.lock().unwrap().list.clone();
        let (phones, desktops, devices) = {
            let peers = self.peers.lock().unwrap();
            let count = |r| peers.values().filter(|p| p.role == r).count();
            // Every paired phone, connected or not, then the web pages connected right now.
            let mut devices: Vec<_> = paired
                .iter()
                .map(|d| {
                    let p = peers.values().filter(|p| p.device.as_deref() == Some(&d.id)).max_by_key(|p| p.since);
                    serde_json::json!({
                        "id": d.id, "kind": "phone", "name": d.name, "online": p.is_some(), "removable": true,
                        "ip": p.map(|p| &p.ip), "since": p.map(|p| p.since), "lastSeen": d.last_seen,
                        "scans": p.map_or(0, |p| p.scans), "lastScan": p.and_then(|p| p.last_scan),
                    })
                })
                .collect();
            devices.extend(peers.iter().filter(|(_, p)| p.role == Role::Desktop).map(|(id, p)| {
                serde_json::json!({
                    "id": format!("page-{id}"), "kind": "web page", "name": p.name, "online": true, "removable": false,
                    "ip": p.ip, "since": p.since, "lastSeen": null, "scans": 0, "lastScan": null,
                })
            }));
            (count(Role::Phone), count(Role::Desktop), devices)
        };
        // The built-in typer counts as one desktop that types at the cursor, when the OS allows it.
        let typers = usize::from(self.typer_ready.load(Ordering::Relaxed));
        let mut msg = serde_json::json!({ "type": "status", "phones": phones, "desktops": desktops + typers, "typers": typers });
        self.send_to(&[Role::Phone, Role::Desktop], &msg.to_string());
        // Only the pairing page gets the device list (names and IPs of everyone paired).
        msg["devices"] = devices.into();
        self.send_to(&[Role::Monitor], &msg.to_string());
    }

    fn on_scan(&self, id: u64, raw: &str, reply: &mpsc::UnboundedSender<Message>) {
        if !self.peers.lock().unwrap().contains_key(&id) {
            return; // removed while still connected
        }
        let Ok(m) = serde_json::from_str::<serde_json::Value>(raw) else { return };
        let Some(code) = m["code"].as_str() else { return };
        if m["type"] != "scan" || code.is_empty() || code.chars().count() > MAX_CODE_CHARS {
            return;
        }
        let ts = now_ms();
        let out = serde_json::json!({ "type": "scan", "code": code, "format": m["format"].as_str(), "ts": ts });
        let mut delivered = self.send_to(&[Role::Desktop], &out.to_string());
        // Always hand it to the typer, but only report it as delivered if typing currently works,
        // so the phone does not beep "OK" for a scan that never reached the cursor.
        if self.typer.send(code.to_string()).is_ok() && self.typer_ready.load(Ordering::Relaxed) {
            delivered += 1;
        }
        let ack = serde_json::json!({ "type": "ack", "code": code, "delivered": delivered });
        let _ = reply.send(Message::Text(ack.to_string().into()));
        if let Some(p) = self.peers.lock().unwrap().get_mut(&id) {
            p.scans += 1;
            p.last_scan = Some(ts);
        }
        self.broadcast_status(); // after the ack, so the scan itself is not delayed
    }
}

// ---------- HTTP handlers ----------

fn asset(content_type: &'static str, body: impl Into<axum::body::Body>) -> Response {
    ([(header::CONTENT_TYPE, content_type), (header::CACHE_CONTROL, "no-cache")], body.into()).into_response()
}

fn is_loopback(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => v4.is_loopback(),
        IpAddr::V6(v6) => v6.is_loopback() || v6.to_ipv4_mapped().is_some_and(|v4| v4.is_loopback()),
    }
}

/// Headers that a proxy or tunnel adds. Their presence means the request came from outside, even if
/// the connection itself is from 127.0.0.1 (cloudflared runs on this machine and connects locally).
const PROXY_HEADERS: [&str; 5] = ["cf-connecting-ip", "cf-ray", "x-forwarded-for", "forwarded", "x-real-ip"];

/// A request typed into a browser on this computer: loopback connection, no proxy headers, and
/// addressed to localhost. The real socket address is used, never a header, so it cannot be faked
/// from the network; and tunnel traffic (loopback, but public host name + Cloudflare headers) is not local.
fn is_local_request(peer: SocketAddr, headers: &axum::http::HeaderMap) -> bool {
    let host = headers.get(header::HOST).and_then(|h| h.to_str().ok()).unwrap_or("");
    let host = host.rsplit_once(':').map_or(host, |(h, p)| if p.chars().all(|c| c.is_ascii_digit()) { h } else { host });
    is_loopback(peer.ip())
        && !PROXY_HEADERS.iter().any(|h| headers.contains_key(*h))
        && matches!(host, "localhost" | "127.0.0.1" | "[::1]")
}

/// Pairing info (contains the token). Only this computer may ask without the token.
async fn pair(
    State(app): State<Arc<App>>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    Query(q): Query<HashMap<String, String>>,
    headers: axum::http::HeaderMap,
) -> Response {
    let has_token = app.token_ok(q.get("t"));
    if !is_local_request(peer, &headers) && !has_token {
        return (StatusCode::UNAUTHORIZED, Json(serde_json::json!({ "error": "token required" }))).into_response();
    }
    Json(app.pair_json.lock().unwrap().clone()).into_response()
}

/// WebSocket close codes the phone page understands.
const CLOSE_NOT_PAIRED: u16 = 4401; // unknown key and wrong/old QR token
const CLOSE_REMOVED: u16 = 4403; // removed on the pairing page

async fn ws(
    State(app): State<Arc<App>>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    Query(q): Query<HashMap<String, String>>,
    headers: axum::http::HeaderMap,
    upgrade: WebSocketUpgrade,
) -> Response {
    let role = match q.get("role").map(String::as_str) {
        Some("phone") => Role::Phone,
        Some("monitor") => Role::Monitor,
        _ => Role::Desktop,
    };
    let ua = headers.get(header::USER_AGENT).and_then(|v| v.to_str().ok()).unwrap_or("");
    let chosen_name = q.get("name").and_then(|n| clean_name(n));
    let mut name = chosen_name.clone().unwrap_or_else(|| device_label(ua));
    let token_ok = app.token_ok(q.get("t"));

    // Phones log in with their own key. A phone without one pairs with the QR code's token and is
    // given a key. Pages (pairing page, bridge.js) use the token.
    let (mut device, mut new_key) = (None, None);
    if role == Role::Phone {
        let now = now_ms();
        let mut store = app.devices.lock().unwrap();
        let known = q.get("k").and_then(|k| store.by_key(k)).map(|d| {
            d.last_seen = now;
            if let Some(n) = &chosen_name {
                d.name = n.clone();
            }
            d.clone()
        });
        if let Some(d) = known {
            store.save();
            name = d.name.clone();
            device = Some(d.id);
        } else if token_ok {
            let Ok(d) = store.enroll(name.clone(), now) else {
                return StatusCode::INTERNAL_SERVER_ERROR.into_response();
            };
            new_key = Some(d.key);
            device = Some(d.id);
        }
        drop(store);
        if device.is_none() {
            // Accept, then close with a code, so the phone can tell "not paired" from "offline".
            return upgrade.on_upgrade(|mut socket| async move {
                let _ = socket.send(Message::Close(Some(CloseFrame { code: CLOSE_NOT_PAIRED, reason: "not paired".into() }))).await;
            });
        }
    } else if !token_ok {
        return (StatusCode::UNAUTHORIZED, "bad token").into_response();
    }

    let mut ip = match addr.ip() {
        IpAddr::V6(v6) => v6.to_ipv4_mapped().map_or(v6.to_string(), |v4| v4.to_string()),
        ip => ip.to_string(),
    };
    // Through the tunnel the connection comes from cloudflared on 127.0.0.1; Cloudflare passes the
    // phone's real address in a header. Only trusted for loopback connections (i.e. from cloudflared).
    if is_loopback(addr.ip()) {
        if let Some(real) = headers.get("cf-connecting-ip").and_then(|v| v.to_str().ok()) {
            ip = format!("{} (internet)", real.chars().take(45).collect::<String>());
        }
    }
    upgrade
        .max_message_size(4096)
        .on_upgrade(move |socket| client(app, socket, role, name, ip, device, new_key))
}

async fn client(app: Arc<App>, socket: WebSocket, role: Role, name: String, ip: String, device: Option<String>, new_key: Option<String>) {
    let (mut sink, mut stream) = socket.split();
    let (tx, mut rx) = mpsc::unbounded_channel::<Message>();
    let id = app.next_id.fetch_add(1, Ordering::Relaxed);
    // Tell the phone the name it is listed under and, when it was just paired, its key to keep.
    let _ = tx.send(Message::Text(serde_json::json!({ "type": "hello", "name": name, "key": new_key }).to_string().into()));
    let peer = Peer { role, tx: tx.clone(), name, ip, since: now_ms(), scans: 0, last_scan: None, device: device.clone() };
    app.peers.lock().unwrap().insert(id, peer);
    app.broadcast_status();

    let writer = tokio::spawn(async move {
        while let Some(m) = rx.recv().await {
            let closing = matches!(m, Message::Close(_));
            if sink.send(m).await.is_err() || closing {
                break;
            }
        }
    });
    while let Some(Ok(msg)) = stream.next().await {
        let Message::Text(t) = &msg else { continue };
        match role {
            Role::Phone => app.on_scan(id, t.as_str(), &tx),
            Role::Monitor => app.on_monitor(t.as_str()),
            Role::Desktop => {}
        }
    }
    app.peers.lock().unwrap().remove(&id);
    writer.abort();
    if let Some(dev) = device {
        let mut store = app.devices.lock().unwrap();
        if let Some(d) = store.get_mut(&dev) {
            d.last_seen = now_ms();
            store.save();
        }
    }
    app.broadcast_status();
}

// ---------- main ----------

fn main() {
    let home = home_dir();
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        None | Some("run") => {}
        Some("start") => return cmd_service(Action::Start),
        Some("stop") => return cmd_service(Action::Stop),
        Some("restart") => return cmd_service(Action::Restart),
        Some("status") => return cmd_status(&home),
        Some("tunnel") => return cmd_tunnel(&home, args.get(1).map(String::as_str)),
        Some("new-token") => return cmd_new_token(&home, !args.iter().any(|a| a == "--no-restart")),
        Some("help" | "-h" | "--help") => return println!("{USAGE}"),
        Some(_) => {
            eprintln!("{USAGE}");
            std::process::exit(2);
        }
    }
    let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().expect("tokio runtime");
    if let Err(e) = rt.block_on(serve(home)) {
        eprintln!("scan2cursor failed: {e}");
        std::process::exit(1);
    }
}

async fn serve(home: PathBuf) -> Result<(), Box<dyn std::error::Error>> {
    let log = Logger::open(&home);
    let port = port();
    let press_enter = std::env::var("ENTER").map(|v| v != "0").unwrap_or(true);
    let dry_run = std::env::var("DRY_RUN").is_ok_and(|v| !v.is_empty());

    let token = load_token(&home)?;
    let host = lan_ip();
    let (cert_pem, key_pem) = load_cert(&home, &host)?;
    let acceptor = tls_acceptor(&cert_pem, &key_pem)?;

    let tunnel_on = tunnel::enabled(&home);
    let typer_ready = Arc::new(AtomicBool::new(false));
    let app = Arc::new(App {
        home: home.clone(),
        log: log.clone(),
        lan_base: format!("https://{host}:{port}"),
        tunnel_enabled: tunnel_on,
        pairing: Mutex::new(Pairing { token, tunnel_url: None }),
        pair_json: Mutex::new(serde_json::Value::Null),
        devices: Mutex::new(devices::Store::load(&home)),
        peers: Mutex::new(HashMap::new()),
        next_id: AtomicU64::new(0),
        typer: start_typer(press_enter, dry_run, log.clone(), typer_ready.clone()),
        typer_ready: typer_ready.clone(),
    });
    app.refresh_pairing();

    // Tell open pages when typing starts or stops working (e.g. the permission was switched on).
    {
        let app = app.clone();
        tokio::spawn(async move {
            let mut last = typer_ready.load(Ordering::Relaxed);
            loop {
                tokio::time::sleep(Duration::from_secs(1)).await;
                let now = typer_ready.load(Ordering::Relaxed);
                if now != last {
                    last = now;
                    app.broadcast_status();
                }
            }
        });
    }

    let router = Router::new()
        .route("/", get(|| async { asset("text/html; charset=utf-8", INDEX_HTML) }))
        .route("/index.html", get(|| async { asset("text/html; charset=utf-8", INDEX_HTML) }))
        .route("/scanner.html", get(|| async { asset("text/html; charset=utf-8", SCANNER_HTML) }))
        .route("/bridge.js", get(|| async { asset("text/javascript; charset=utf-8", BRIDGE_JS) }))
        .route("/vendor/html5-qrcode.js", get(|| async { asset("text/javascript; charset=utf-8", HTML5_QRCODE_JS) }))
        .route("/logo.svg", get(|| async { asset("image/svg+xml", LOGO_SVG) }))
        .route("/icon-180.png", get(|| async { asset("image/png", ICON_PNG) }))
        .route("/api/pair", get(pair))
        .route("/ws", get(ws))
        .with_state(app.clone());

    let listener = TcpListener::bind(("0.0.0.0", port)).await?;
    println!("\nDesktop page : http://localhost:{port}/");
    println!("Phone URL    : https://{host}:{port}/scanner.html?t=<token>  (scan the QR on the desktop page)");
    println!("Phone must be on the same Wi-Fi{}.\n", if tunnel_on { ", or use the internet QR code" } else { "" });
    log.log(&format!("listening on port {port}, LAN address {host}, tunnel {}", if tunnel_on { "on" } else { "off" }));

    if tunnel_on {
        let (home, log, app) = (home.clone(), log.clone(), app.clone());
        tokio::spawn(tunnel::run(home, port, log, move |url| app.set_tunnel_url(url)));
    }

    // Plain HTTP from another device is only ever answered with a redirect to HTTPS.
    let to_https = Router::new().fallback(|headers: axum::http::HeaderMap, uri: axum::http::Uri| async move {
        let host = headers.get(header::HOST).and_then(|h| h.to_str().ok()).unwrap_or("localhost");
        axum::response::Redirect::temporary(&format!("https://{host}{uri}"))
    });

    let accept_loop = async {
        loop {
            let Ok((tcp, peer)) = listener.accept().await else { continue };
            let _ = tcp.set_nodelay(true); // send each scan immediately, no Nagle batching
            let (acceptor, router, to_https) = (acceptor.clone(), router.clone(), to_https.clone());
            tokio::spawn(async move {
                // One port, two protocols: a TLS handshake starts with byte 0x16, plain HTTP with a letter.
                let mut first = [0u8; 1];
                let Ok(Ok(1)) = tokio::time::timeout(Duration::from_secs(10), tcp.peek(&mut first)).await else { return };
                if first[0] == 0x16 {
                    let Ok(Ok(tls)) = tokio::time::timeout(Duration::from_secs(10), acceptor.accept(tcp)).await else { return };
                    serve_conn(tls, router, peer).await;
                } else if is_loopback(peer.ip()) {
                    // Plain HTTP only for this computer: http://localhost needs no certificate, so the
                    // desktop page opens without a browser warning, and nothing crosses the network.
                    serve_conn(tcp, router, peer).await;
                } else {
                    serve_conn(tcp, to_https, peer).await; // never send the token over the network unencrypted
                }
            });
        }
    };
    // Exit cleanly on Ctrl+C / stop, so the tunnel's cloudflared is stopped with us.
    tokio::select! {
        _ = accept_loop => {}
        _ = shutdown_signal() => log.log("stopping"),
    }
    tunnel::clear_state(&home);
    Ok(())
}

/// Serve one connection (TLS or plain) with the given router.
async fn serve_conn<IO>(io: IO, router: Router, peer: SocketAddr)
where
    IO: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + Send + 'static,
{
    let svc = hyper::service::service_fn(move |mut req: hyper::Request<hyper::body::Incoming>| {
        req.extensions_mut().insert(ConnectInfo(peer));
        router.clone().oneshot(req)
    });
    let _ = hyper::server::conn::http1::Builder::new()
        .serve_connection(TokioIo::new(io), svc)
        .with_upgrades()
        .await;
}

async fn shutdown_signal() {
    #[cfg(unix)]
    {
        use tokio::signal::unix::{signal, SignalKind};
        match signal(SignalKind::terminate()) {
            Ok(mut term) => tokio::select! { _ = term.recv() => {}, _ = tokio::signal::ctrl_c() => {} },
            Err(_) => { let _ = tokio::signal::ctrl_c().await; }
        }
    }
    #[cfg(not(unix))]
    {
        let _ = tokio::signal::ctrl_c().await;
    }
}
