// Scan2Cursor: the phone scans a barcode, and it is typed + Enter wherever the desktop cursor is.
// Phone on the same Wi-Fi, or anywhere via the optional internet tunnel (tunnel.rs).
// One small binary: HTTPS + WebSocket server and keyboard typer.
//
// Usage:  scan2cursor [run]                    run the server in this terminal
//                                              (Windows, no command: start it in the background instead)
//         scan2cursor start|stop|restart|status control the background server
//         scan2cursor tunnel [on|off|status]   reach this computer from any network
//         scan2cursor new-token [--no-restart] make a new pairing token (restarts the server)
//         scan2cursor help                     show the command list
// Env:    PORT (default 8443), HOST_IP (override LAN IP), ENTER=0 (no Enter after scan), TUNNEL=1/0,
//         DRY_RUN=<anything> (log instead of typing), TYPE=0 (never type; only bridge.js pages), TOKEN (fixed token, 16+ characters), BB_HOME (data folder; default: next to binary)

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
const BARCODE_DETECTOR_JS: &str = include_str!("../public/vendor/barcode-detector.js");
const ZXING_WASM: &[u8] = include_bytes!("../public/vendor/zxing_reader.wasm");
const SCAN_WORKER_JS: &str = include_str!("../public/scan-worker.js");
const SW_JS: &str = include_str!("../public/sw.js");
const LOGO_SVG: &str = include_str!("../public/logo.svg");
const ICON_180_PNG: &[u8] = include_bytes!("../public/icon-180.png");
const ICON_192_PNG: &[u8] = include_bytes!("../public/icon-192.png");
const ICON_512_PNG: &[u8] = include_bytes!("../public/icon-512.png");
const MAX_CODE_CHARS: usize = 512;
// Fixed limits, so memory stays flat however long the server runs.
const PEER_BUFFER: usize = 256; // messages waiting for one connection; a connection that stops reading is dropped
const PHONE_INFLIGHT: usize = 600; // scans from one phone waiting to be typed (the phone keeps at most 500)
const RECENT_PER_PHONE: usize = 500; // recent scan ids per phone, to skip scans re-sent after a dropped connection
const TYPER_BUFFER: usize = 64; // scans waiting for the keyboard
const BURST_PAUSE: Duration = Duration::from_millis(50); // after Enter, only when the next scan is already waiting
const PAGE_CONFIRM_WAIT: Duration = Duration::from_secs(2); // how long to wait for a bridge.js page to confirm
const MAX_LOG_BYTES: u64 = 1024 * 1024;
const LOG_NAME: &str = "scan2cursor.log";
const MAX_CONNECTIONS: usize = 256; // open connections at once; more wait their turn
const MIN_TOKEN_CHARS: usize = 16; // a shorter TOKEN could be guessed
const CERT_DAYS: i64 = 825; // Apple's maximum for TLS certs

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
struct Logger(Arc<Mutex<LogFile>>);

struct LogFile {
    dir: PathBuf,
    file: Option<fs::File>,
    size: u64,
}

impl LogFile {
    /// Keep the log small: once it passes 1 MB, it becomes .old (replacing the previous one).
    fn roll_if_big(&mut self) {
        if self.size <= MAX_LOG_BYTES {
            return;
        }
        let path = self.dir.join(LOG_NAME);
        let _ = fs::rename(&path, self.dir.join(format!("{LOG_NAME}.old")));
        self.file = fs::OpenOptions::new().create(true).append(true).open(&path).ok();
        self.size = 0;
    }
}

impl Logger {
    fn open(home: &Path) -> Self {
        let dir = home.join("logs");
        let _ = fs::create_dir_all(&dir);
        let path = dir.join(LOG_NAME);
        let size = fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
        let file = fs::OpenOptions::new().create(true).append(true).open(path).ok();
        let mut lf = LogFile { dir, file, size };
        lf.roll_if_big();
        Logger(Arc::new(Mutex::new(lf)))
    }

    fn log(&self, msg: &str) {
        let ts = time::OffsetDateTime::now_utc()
            .format(&time::format_description::well_known::Rfc3339)
            .unwrap_or_default();
        let line = format!("{ts} {msg}");
        eprintln!("{line}");
        let mut lf = self.0.lock().unwrap();
        if let Some(f) = lf.file.as_mut() {
            if writeln!(f, "{line}").is_ok() {
                lf.size += line.len() as u64 + 1;
            }
        }
        lf.roll_if_big();
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

/// This computer's id, in the QR code next to the token. Not a secret from paired phones: a phone
/// remembers it, and only follows a QR code to a new address when the code carries the same id.
fn load_server_id(home: &Path) -> std::io::Result<String> {
    let path = home.join(".server-id");
    let saved = fs::read_to_string(&path).unwrap_or_default();
    if !saved.trim().is_empty() {
        return Ok(saved.trim().to_string());
    }
    let id = random_hex(16)?;
    write_private(&path, &id)?;
    Ok(id)
}

fn load_token(home: &Path) -> std::io::Result<String> {
    // A fixed TOKEN wins over the saved one at every start, so it also undoes the change made by "Remove".
    match std::env::var("TOKEN") {
        Ok(t) if t.len() >= MIN_TOKEN_CHARS => return Ok(t),
        Ok(t) if !t.is_empty() => eprintln!("TOKEN is shorter than {MIN_TOKEN_CHARS} characters and was ignored; using the saved token."),
        _ => {}
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
    // Stop the server first: while it runs it still knows the old phones and would save them again.
    let running = server_answering();
    let stop_first = restart && running && (cfg!(windows) || service_installed());
    if stop_first {
        service(Action::Stop);
        wait_for(false);
    }
    if let Err(e) = generate_token(home) {
        eprintln!("Could not write new token: {e}");
        std::process::exit(1);
    }
    devices::clear(home);
    println!("New QR code made and all paired phones forgotten. Every phone must scan the QR code again.");
    if restart {
        cmd_service(if stop_first { Action::Start } else { Action::Restart });
        println!("Open http://localhost:{}/ and scan the new QR code with each phone.", port());
    } else if running {
        println!("The running server still uses the old one: stop and start it to apply.");
    }
}

// ---------- start / stop / restart / status (the background server: launchd / systemd service, or a hidden process on Windows) ----------

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

/// Windows: start the server as a separate hidden process. It does not belong to the command
/// window it was started from, so closing that window (or logging the launcher off the task) leaves
/// it running.
#[cfg(windows)]
fn spawn_background() -> bool {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200; // Ctrl+C in the launching window is not passed on
    const CREATE_BREAKAWAY_FROM_JOB: u32 = 0x0100_0000; // outlive the Task Scheduler job that launched it
    let Ok(exe) = std::env::current_exe() else {
        return false;
    };
    let spawn = |flags: u32| {
        std::process::Command::new(&exe)
            .arg("run")
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .creation_flags(flags)
            .spawn()
            .is_ok()
    };
    // Leaving the job is refused in some setups: then start it inside the job.
    spawn(CREATE_NO_WINDOW | CREATE_NEW_PROCESS_GROUP | CREATE_BREAKAWAY_FROM_JOB) || spawn(CREATE_NO_WINDOW | CREATE_NEW_PROCESS_GROUP)
}
#[cfg(not(windows))]
fn spawn_background() -> bool {
    false
}

/// Windows: stop every running copy of this program (and its cloudflared), except this process.
fn stop_background() {
    let name = std::env::current_exe()
        .ok()
        .and_then(|p| p.file_name().map(|n| n.to_string_lossy().into_owned()))
        .unwrap_or_else(|| "scan2cursor.exe".into());
    sh("taskkill", &["/F", "/T", "/IM", &name, "/FI", &format!("PID ne {}", std::process::id())]);
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
        let load = || sh("launchctl", &["bootstrap", &format!("gui/{uid}"), &plist]);
        match action {
            Action::Stop => !loaded() || sh("launchctl", &["bootout", &target]),
            Action::Start => loaded() || load(),
            Action::Restart if loaded() => sh("launchctl", &["kickstart", "-k", &target]),
            Action::Restart => load(),
        }
    } else if cfg!(windows) {
        // The server is a hidden background process of its own (not the scheduled task, which only
        // launches it at logon), so it is started and stopped directly.
        match action {
            Action::Start => server_answering() || spawn_background(),
            Action::Stop => {
                stop_background();
                true
            }
            Action::Restart => {
                stop_background();
                wait_for(false);
                spawn_background()
            }
        }
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
    // Windows needs no installed service to run in the background.
    if !cfg!(windows) && !service_installed() {
        eprintln!("Auto-start is not installed. Run: scripts/setup.sh install,");
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
        } else if cfg!(windows) {
            println!("It runs in the background: this window can be closed. Open http://localhost:{}/", port());
            println!("Stop it with: scan2cursor stop");
        }
    } else {
        eprintln!("Could not confirm the server {what}. See logs/scan2cursor.log, or run 'scan2cursor run' to see errors.");
        std::process::exit(1);
    }
}

fn cmd_status(home: &Path) {
    let up = server_answering();
    println!("Server      : {}", if up { "running" } else { "NOT running (start it: scan2cursor start)" });
    println!("Auto-start  : {}", if service_installed() { "installed (starts at login)" } else { "not installed (run scripts/setup.sh install, or scripts\\setup.bat on Windows)" });
    println!("Desktop page: http://localhost:{}/", port());
    println!("LAN address : {}", lan_ip());
    println!("Internet    : {}", tunnel_summary(home));
    println!("Log file    : {}", home.join("logs").join(LOG_NAME).display());
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
        let installed = service_installed();
        if installed {
            cmd_service(Action::Restart);
        } else {
            println!("Start the server to apply: scan2cursor run");
        }
        installed
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
            if restart_if_installed() {
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
  run                       run the server in this terminal (Ctrl+C to stop); macOS/Linux: the default
  start | stop | restart    control the background server (Windows: no command = start)
  status                    is it running? shows page address, internet address and log file
  tunnel on | off | status  let phones on other networks (mobile data) connect, via Cloudflare
  new-token [--no-restart]  make a new pairing token (phones must re-scan the QR code)
  help                      show this list";

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

/// Self-signed cert for the LAN IP, cached in .cert.json and regenerated when the IP changes or
/// the cert is within 30 days of running out.
fn load_cert(home: &Path, host: &str) -> Result<(String, String), Box<dyn std::error::Error>> {
    let path = home.join(".cert.json");
    if let Ok(raw) = fs::read_to_string(&path) {
        if let Ok(c) = serde_json::from_str::<serde_json::Value>(&raw) {
            let now = time::OffsetDateTime::now_utc().unix_timestamp();
            // Files from before the date was saved: count from when the file was written.
            let expires = c["expires"].as_i64().or_else(|| {
                let made = fs::metadata(&path).ok()?.modified().ok()?.duration_since(std::time::UNIX_EPOCH).ok()?;
                Some(made.as_secs() as i64 + CERT_DAYS * 86_400)
            });
            if c["host"] == host && expires.is_some_and(|e| e > now + 30 * 86_400) {
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
    let not_after = now + time::Duration::days(CERT_DAYS);
    params.not_after = not_after;
    let key = rcgen::KeyPair::generate()?;
    let cert = params.self_signed(&key)?;
    let (cert_pem, key_pem) = (cert.pem(), key.serialize_pem());
    let json = serde_json::json!({ "host": host, "key": key_pem, "cert": cert_pem, "expires": not_after.unix_timestamp() });
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

/// A scan for the keyboard. `done` gets true once it was typed (code + Enter), false if it could not be.
struct TypeJob {
    code: String,
    done: tokio::sync::oneshot::Sender<bool>,
}

/// Printable text only: no control characters (Enter, Tab, Escape), line separators or invisible
/// direction marks. A scanned code can then never press a key beyond its own text.
fn typeable(c: char) -> bool {
    !c.is_control() && !matches!(c, '\u{2028}' | '\u{2029}' | '\u{200B}'..='\u{200F}' | '\u{202A}'..='\u{202E}' | '\u{2060}'..='\u{2069}' | '\u{FEFF}')
}

/// One long-lived keyboard handle on its own thread: no process is spawned per scan. Scans are typed
/// one at a time, each as a whole (code + Enter), so scans from several phones never mix.
/// `ready` tells the rest of the server whether typing actually works (macOS may not allow it yet).
fn start_typer(press_enter: bool, dry_run: bool, no_typing: bool, log: Logger, ready: Arc<AtomicBool>) -> std_mpsc::SyncSender<TypeJob> {
    use enigo::{Direction, Enigo, Key, Keyboard, Settings};
    let (tx, rx) = std_mpsc::sync_channel::<TypeJob>(TYPER_BUFFER);
    std::thread::spawn(move || {
        let mut kb: Option<Enigo> = None;
        let mut asked = false; // macOS: show the permission prompt once, not on every scan
        let mut reported = false; // log "not available" once until it works again
        let mut connect = |kb: &mut Option<Enigo>| {
            if kb.is_some() || dry_run || no_typing {
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
        let mut next: Option<TypeJob> = None;
        loop {
            ready.store(!no_typing && (kb.is_some() || dry_run), Ordering::Relaxed);
            let job = match next.take() {
                Some(j) => j,
                // While the keyboard is not available, look again every 2 s, so it starts working by
                // itself once the permission is switched on.
                None => match rx.recv_timeout(Duration::from_secs(2)) {
                    Ok(j) => j,
                    Err(std_mpsc::RecvTimeoutError::Timeout) => {
                        connect(&mut kb);
                        continue;
                    }
                    Err(std_mpsc::RecvTimeoutError::Disconnected) => break,
                },
            };
            let text: String = job.code.chars().filter(|c| typeable(*c)).collect();
            let typed = if text.is_empty() {
                true // nothing printable to type
            } else if dry_run {
                log.log(&format!("[dry-run] type {text:?}{}", if press_enter { " + Enter" } else { "" }));
                true
            } else {
                connect(&mut kb);
                match kb.as_mut() {
                    None => false,
                    Some(e) => match e.text(&text).and_then(|_| if press_enter { e.key(Key::Return, Direction::Click) } else { Ok(()) }) {
                        Ok(()) => true,
                        Err(err) => {
                            log.log(&format!("type failed: {err}"));
                            kb = None; // reconnect on next scan
                            false
                        }
                    },
                }
            };
            let _ = job.done.send(typed);
            // Burst (queued scans, several phones): give the app in front a moment to handle the
            // Enter before the next code arrives. A single scan is never delayed.
            if let Ok(j) = rx.try_recv() {
                if typed && !dry_run {
                    std::thread::sleep(BURST_PAUSE);
                }
                next = Some(j);
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
    tx: mpsc::Sender<Message>,           // bounded (PEER_BUFFER)
    kick: Arc<tokio::sync::Notify>,      // tells the connection to close (stuck, or phone removed)
    name: String,
    ip: String,
    local: bool, // opened on this computer (http://localhost), not from the network
    since: i64,  // ms since epoch
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
    server_id: String, // see load_server_id
    tunnel_enabled: bool,
    pairing: Mutex<Pairing>,
    pair_json: Mutex<serde_json::Value>, // what /api/pair returns; rebuilt when `pairing` changes
    devices: Mutex<devices::Store>,      // paired phones and their keys
    peers: Mutex<HashMap<u64, Peer>>,    // open connections
    next_id: AtomicU64,
    typer: std_mpsc::SyncSender<TypeJob>,
    typer_ready: Arc<AtomicBool>, // false while the OS does not let us type (macOS Accessibility)
    next_scan: AtomicU64,         // ids for scans sent to bridge.js pages
    page_waits: Mutex<HashMap<u64, tokio::sync::oneshot::Sender<()>>>, // pages' confirmations awaited
    recent: Mutex<HashMap<String, Recent>>, // per paired phone: ids of scans already delivered
}

/// The last RECENT_PER_PHONE scan ids delivered from one phone (fixed size).
#[derive(Default)]
struct Recent {
    order: std::collections::VecDeque<u64>,
    set: std::collections::HashSet<u64>,
}

impl Recent {
    fn add(&mut self, id: u64) {
        if self.set.insert(id) {
            self.order.push_back(id);
            if self.order.len() > RECENT_PER_PHONE {
                if let Some(old) = self.order.pop_front() {
                    self.set.remove(&old);
                }
            }
        }
    }
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
            let link = |base: &str| format!("{base}/scanner.html?t={}&s={}", p.token, self.server_id);
            let lan = link(&self.lan_base);
            let net = p.tunnel_url.as_deref().map(link);
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
    /// so the link it already has cannot pair it again. Other phones keep their own keys. Pages from
    /// other computers logged in with the old token, so they are disconnected too.
    fn remove_device(&self, id: &str) {
        let mut store = self.devices.lock().unwrap();
        let Some(name) = store.get_mut(id).map(|d| d.name.clone()) else { return };
        store.remove(id);
        drop(store); // locked again below, by broadcast_status
        self.recent.lock().unwrap().remove(id);
        {
            let mut peers = self.peers.lock().unwrap();
            let stale = |p: &Peer| p.device.as_deref() == Some(id) || (p.role != Role::Phone && !p.local);
            let open: Vec<u64> = peers.iter().filter(|(_, p)| stale(p)).map(|(k, _)| *k).collect();
            for k in open {
                // Dropped from the list right away, so nothing more from it is accepted.
                if let Some(p) = peers.remove(&k) {
                    let _ = p.tx.try_send(Message::Close(Some(CloseFrame { code: CLOSE_REMOVED, reason: "removed".into() })));
                    p.kick.notify_one();
                }
            }
        }
        match generate_token(&self.home) {
            Ok(t) => self.pairing.lock().unwrap().token = t,
            Err(e) => self.log.log(&format!("could not change the pairing token: {e}")),
        }
        self.log.log(&format!("removed device {id} ({}); pairing token changed", name));
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

    /// Send to every connection with one of these roles. A connection whose buffer is full has stopped
    /// reading (frozen tab, dead network): it is dropped instead of buffering without limit.
    fn send_to(&self, roles: &[Role], msg: &str) -> usize {
        let mut peers = self.peers.lock().unwrap();
        let mut sent = 0;
        let mut stuck = Vec::new();
        for (k, p) in peers.iter().filter(|(_, p)| roles.contains(&p.role)) {
            match p.tx.try_send(Message::Text(msg.into())) {
                Ok(()) => sent += 1,
                Err(mpsc::error::TrySendError::Full(_)) => stuck.push(*k),
                Err(mpsc::error::TrySendError::Closed(_)) => {}
            }
        }
        for k in stuck {
            if let Some(p) = peers.remove(&k) {
                p.kick.notify_one();
            }
        }
        sent
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

    /// Deliver one scan: type it at the cursor, and send it to bridge.js pages. True only when it was
    /// typed, or a page confirmed it received it.
    async fn deliver(&self, code: &str, format: Option<&str>) -> bool {
        let scan_id = self.next_scan.fetch_add(1, Ordering::Relaxed);
        let (page_tx, page_rx) = tokio::sync::oneshot::channel();
        self.page_waits.lock().unwrap().insert(scan_id, page_tx);
        let out = serde_json::json!({ "type": "scan", "id": scan_id, "code": code, "format": format, "ts": now_ms() });
        let pages = self.send_to(&[Role::Desktop], &out.to_string());

        let mut typed = false;
        if self.typer_ready.load(Ordering::Relaxed) {
            let (done_tx, done_rx) = tokio::sync::oneshot::channel();
            if self.typer.try_send(TypeJob { code: code.to_string(), done: done_tx }).is_ok() {
                typed = done_rx.await.unwrap_or(false);
            }
        }
        let confirmed = typed || (pages > 0 && matches!(tokio::time::timeout(PAGE_CONFIRM_WAIT, page_rx).await, Ok(Ok(()))));
        self.page_waits.lock().unwrap().remove(&scan_id);
        confirmed
    }

    /// A bridge.js page confirms it received a scan.
    fn on_page(&self, raw: &str) {
        let Ok(m) = serde_json::from_str::<serde_json::Value>(raw) else { return };
        if let ("got", Some(id)) = (m["type"].as_str().unwrap_or(""), m["id"].as_u64()) {
            if let Some(w) = self.page_waits.lock().unwrap().remove(&id) {
                let _ = w.send(());
            }
        }
    }

    fn scan_done(&self, conn: u64) {
        if let Some(p) = self.peers.lock().unwrap().get_mut(&conn) {
            p.scans += 1;
            p.last_scan = Some(now_ms());
        }
        self.broadcast_status();
    }
}

/// Scans from one phone connection, handled strictly in order. The phone sends without waiting; each
/// scan is confirmed only once delivered. If one cannot be typed, it and every later scan are refused
/// until the phone sends that one again, so the phone's order is never broken.
async fn phone_scans(app: Arc<App>, conn: u64, device: String, mut rx: mpsc::Receiver<serde_json::Value>, reply: mpsc::Sender<Message>) {
    let mut blocked: Option<u64> = None;
    let ack = |v: serde_json::Value| reply.try_send(Message::Text(v.to_string().into())).is_ok();
    while let Some(m) = rx.recv().await {
        if !app.peers.lock().unwrap().contains_key(&conn) {
            return; // removed or dropped while scans were waiting
        }
        let code = m["code"].as_str().unwrap_or("");
        let format = m["format"].as_str();
        let valid = !code.is_empty() && code.chars().count() <= MAX_CODE_CHARS;
        let Some(id) = m["id"].as_u64() else {
            // Older phone page without scan ids: deliver once, no ordering or duplicate check.
            if valid {
                let ok = app.deliver(code, format).await;
                if ok {
                    app.scan_done(conn);
                }
                ack(serde_json::json!({ "type": "ack", "code": code, "delivered": u8::from(ok) }));
            }
            continue;
        };
        if !valid {
            ack(serde_json::json!({ "type": "ack", "id": id, "code": code, "delivered": false, "rejected": true }));
            continue;
        }
        if let Some(b) = blocked {
            if id != b {
                ack(serde_json::json!({ "type": "ack", "id": id, "code": code, "delivered": false }));
                continue;
            }
            blocked = None;
        }
        // Re-sent after a dropped connection, but already delivered: confirm without typing it again.
        if app.recent.lock().unwrap().get(&device).is_some_and(|r| r.set.contains(&id)) {
            ack(serde_json::json!({ "type": "ack", "id": id, "code": code, "delivered": true }));
            continue;
        }
        let ok = app.deliver(code, format).await;
        if ok {
            app.recent.lock().unwrap().entry(device.clone()).or_default().add(id);
        } else {
            blocked = Some(id);
        }
        ack(serde_json::json!({ "type": "ack", "id": id, "code": code, "delivered": ok }));
        if ok {
            app.scan_done(conn);
        }
    }
}

// ---------- HTTP handlers ----------

fn asset(content_type: &'static str, body: impl Into<axum::body::Body>) -> Response {
    ([(header::CONTENT_TYPE, content_type), (header::CACHE_CONTROL, "no-cache")], body.into()).into_response()
}

/// Added to every answer: no guessing of file types, the pairing code in the address is never passed
/// on to other sites, and the pages cannot be shown inside another site's page.
async fn security_headers(mut res: Response) -> Response {
    use axum::http::HeaderValue;
    let h = res.headers_mut();
    h.insert(header::X_CONTENT_TYPE_OPTIONS, HeaderValue::from_static("nosniff"));
    h.insert(header::REFERRER_POLICY, HeaderValue::from_static("no-referrer"));
    if h.get(header::CONTENT_TYPE).is_some_and(|v| v.as_bytes().starts_with(b"text/html")) {
        h.insert(header::X_FRAME_OPTIONS, HeaderValue::from_static("DENY"));
        h.insert(header::CONTENT_SECURITY_POLICY, HeaderValue::from_static("frame-ancestors 'none'; base-uri 'none'; object-src 'none'; form-action 'none'"));
    }
    res
}

fn is_loopback(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => v4.is_loopback(),
        IpAddr::V6(v6) => v6.is_loopback() || v6.to_ipv4_mapped().is_some_and(|v4| v4.is_loopback()),
    }
}

/// "Add to Home Screen" description for the scanner page. The installed app opens the scanner link;
/// the pairing code is only included when it is the current one (a phone that already has its own
/// key does not need it).
async fn manifest(State(app): State<Arc<App>>, Query(q): Query<HashMap<String, String>>) -> Response {
    let start = match q.get("t") {
        Some(t) if app.token_ok(Some(t)) => format!("/scanner.html?t={t}"),
        _ => "/scanner.html".to_string(),
    };
    let body = serde_json::json!({
        "name": "Scan2Cursor", "short_name": "Scan2Cursor", "description": "Scan on your phone, typed on the computer",
        "start_url": start, "scope": "/", "display": "standalone", "orientation": "portrait",
        "background_color": "#111111", "theme_color": "#111111",
        "icons": [
            { "src": "/icon-192.png", "sizes": "192x192", "type": "image/png" },
            { "src": "/icon-512.png", "sizes": "512x512", "type": "image/png" },
            { "src": "/logo.svg", "sizes": "any", "type": "image/svg+xml" },
        ],
    });
    ([(header::CONTENT_TYPE, "application/manifest+json"), (header::CACHE_CONTROL, "no-store")], body.to_string()).into_response()
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
    ([(header::CACHE_CONTROL, "no-store")], Json(app.pair_json.lock().unwrap().clone())).into_response()
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
            (d.id.clone(), d.name.clone())
        });
        if let Some((id, known_name)) = known {
            store.save();
            name = known_name;
            device = Some(id);
        } else if token_ok {
            let Ok(d) = store.enroll(name.clone(), now) else {
                drop(store);
                app.log.log("pairing refused: could not save, or too many paired phones (remove unused ones on the pairing page)");
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
    let local = is_local_request(addr, &headers);
    upgrade
        .max_message_size(4096)
        .max_frame_size(4096)
        .on_upgrade(move |socket| client(app, socket, role, name, ip, local, device, new_key))
}

#[allow(clippy::too_many_arguments)]
async fn client(app: Arc<App>, socket: WebSocket, role: Role, name: String, ip: String, local: bool, device: Option<String>, new_key: Option<String>) {
    let (mut sink, mut stream) = socket.split();
    let (tx, mut rx) = mpsc::channel::<Message>(PEER_BUFFER);
    let kick = Arc::new(tokio::sync::Notify::new());
    let id = app.next_id.fetch_add(1, Ordering::Relaxed);
    // Tell the phone the name it is listed under and, when it was just paired, its key to keep.
    let _ = tx.try_send(Message::Text(serde_json::json!({ "type": "hello", "name": name, "key": new_key, "sid": app.server_id }).to_string().into()));
    let peer = Peer { role, tx: tx.clone(), kick: kick.clone(), name, ip, local, since: now_ms(), scans: 0, last_scan: None, device: device.clone() };
    app.peers.lock().unwrap().insert(id, peer);
    app.broadcast_status();

    let mut writer = tokio::spawn(async move {
        while let Some(m) = rx.recv().await {
            let closing = matches!(m, Message::Close(_));
            if sink.send(m).await.is_err() || closing {
                break;
            }
        }
    });
    // A phone's scans go to their own task, handled in order without blocking this read loop.
    let (scan_tx, scan_rx) = mpsc::channel::<serde_json::Value>(PHONE_INFLIGHT);
    let scans = match (role, &device) {
        (Role::Phone, Some(dev)) => Some(tokio::spawn(phone_scans(app.clone(), id, dev.clone(), scan_rx, tx.clone()))),
        _ => None,
    };
    loop {
        let msg = tokio::select! {
            m = stream.next() => m,
            _ = kick.notified() => break,
        };
        let Some(Ok(msg)) = msg else { break };
        let Message::Text(t) = &msg else { continue };
        match role {
            Role::Phone => {
                let Ok(v) = serde_json::from_str::<serde_json::Value>(t.as_str()) else { continue };
                if v["type"] == "scan" && scan_tx.try_send(v).is_err() {
                    break; // far more scans in flight than a phone ever keeps: drop the connection
                }
            }
            Role::Monitor => app.on_monitor(t.as_str()),
            Role::Desktop => app.on_page(t.as_str()),
        }
    }
    if let Some(s) = scans {
        s.abort();
    }
    app.peers.lock().unwrap().remove(&id);
    drop(tx);
    // Let a queued close message (e.g. "removed") go out, but never wait long on a stuck connection.
    if tokio::time::timeout(Duration::from_secs(1), &mut writer).await.is_err() {
        writer.abort();
    }
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
        // Windows, no command (double-click, or the logon task): run hidden in the background.
        None if cfg!(windows) => return cmd_service(Action::Start),
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
    let no_typing = std::env::var("TYPE").is_ok_and(|v| v == "0"); // only deliver to bridge.js pages

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
        server_id: load_server_id(&home)?,
        tunnel_enabled: tunnel_on,
        pairing: Mutex::new(Pairing { token, tunnel_url: None }),
        pair_json: Mutex::new(serde_json::Value::Null),
        devices: Mutex::new(devices::Store::load(&home)),
        peers: Mutex::new(HashMap::new()),
        next_id: AtomicU64::new(0),
        typer: start_typer(press_enter, dry_run, no_typing, log.clone(), typer_ready.clone()),
        typer_ready: typer_ready.clone(),
        next_scan: AtomicU64::new(0),
        page_waits: Mutex::new(HashMap::new()),
        recent: Mutex::new(HashMap::new()),
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
        .route("/vendor/barcode-detector.js", get(|| async { asset("text/javascript; charset=utf-8", BARCODE_DETECTOR_JS) }))
        .route("/vendor/zxing_reader.wasm", get(|| async { asset("application/wasm", ZXING_WASM) }))
        .route("/scan-worker.js", get(|| async { asset("text/javascript; charset=utf-8", SCAN_WORKER_JS) }))
        .route("/sw.js", get(|| async { asset("text/javascript; charset=utf-8", SW_JS) }))
        .route("/icon-192.png", get(|| async { asset("image/png", ICON_192_PNG) }))
        .route("/icon-512.png", get(|| async { asset("image/png", ICON_512_PNG) }))
        .route("/icon-180.png", get(|| async { asset("image/png", ICON_180_PNG) }))
        .route("/logo.svg", get(|| async { asset("image/svg+xml", LOGO_SVG) }))
        .route("/manifest.webmanifest", get(manifest))
        .route("/api/pair", get(pair))
        .route("/ws", get(ws))
        .layer(axum::middleware::map_response(security_headers))
        .with_state(app.clone());

    let listener = TcpListener::bind(("0.0.0.0", port)).await?;
    println!("\nDesktop page : http://localhost:{port}/");
    println!("Phone URL    : https://{host}:{port}/scanner.html?t=<token>  (scan the QR on the desktop page)");
    println!("Phone must be on the same Wi-Fi{}.\n", if tunnel_on { ", or use the internet QR code" } else { "" });
    log.log(&format!("listening on port {port}, LAN address {host}, tunnel {}", if tunnel_on { "on" } else { "off" }));

    if tunnel_on {
        tokio::spawn(tunnel::run(home.clone(), port, log.clone(), move |url| app.set_tunnel_url(url)));
    }

    // Plain HTTP from another device is only ever answered with a redirect to HTTPS.
    let to_https = Router::new().fallback(|headers: axum::http::HeaderMap, uri: axum::http::Uri| async move {
        let host = headers.get(header::HOST).and_then(|h| h.to_str().ok()).unwrap_or("localhost");
        axum::response::Redirect::temporary(&format!("https://{host}{uri}"))
    });

    let slots = Arc::new(tokio::sync::Semaphore::new(MAX_CONNECTIONS));
    let accept_loop = async {
        loop {
            let Ok(slot) = slots.clone().acquire_owned().await else { return };
            let Ok((tcp, peer)) = listener.accept().await else {
                tokio::time::sleep(Duration::from_millis(100)).await; // e.g. out of file handles: do not spin
                continue;
            };
            let _ = tcp.set_nodelay(true); // send each scan immediately, no Nagle batching
            let (acceptor, router, to_https) = (acceptor.clone(), router.clone(), to_https.clone());
            tokio::spawn(async move {
                let _slot = slot; // freed when the connection ends
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
    // A connection that never sends its request is dropped after 10 s.
    let _ = hyper::server::conn::http1::Builder::new()
        .timer(hyper_util::rt::TokioTimer::new())
        .header_read_timeout(Duration::from_secs(10))
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
