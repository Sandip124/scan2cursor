// Internet tunnel: lets a phone on mobile data (or any other network) reach this computer without
// router settings or an app on the phone. Uses a Cloudflare "quick tunnel" (free, no account):
// Cloudflare's `cloudflared` program connects out from this computer and gets a public
// https://<random>.trycloudflare.com address that forwards to this server.
//
// Limits: the address changes every time cloudflared restarts (phones re-scan the QR code), and
// Cloudflare terminates HTTPS, so it can see the traffic (the token and the scanned codes).

use crate::Logger;
use std::{
    fs,
    path::{Path, PathBuf},
    process::Stdio,
    time::{Duration, Instant},
};
use tokio::io::{AsyncBufReadExt, BufReader};

const SETTINGS: &str = "settings.json";
const STATE: &str = ".tunnel.json"; // {pid, url} of the running cloudflared, read by `status`

fn exe_name() -> &'static str {
    if cfg!(windows) { "cloudflared.exe" } else { "cloudflared" }
}

/// Tunnel on/off. TUNNEL=1 / TUNNEL=0 overrides the saved setting.
pub fn enabled(home: &Path) -> bool {
    if let Ok(v) = std::env::var("TUNNEL") {
        return v == "1";
    }
    fs::read_to_string(home.join(SETTINGS))
        .ok()
        .and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok())
        .and_then(|v| v["tunnel"].as_bool())
        .unwrap_or(false)
}

pub fn set_enabled(home: &Path, on: bool) -> std::io::Result<()> {
    let path = home.join(SETTINGS);
    let mut v = fs::read_to_string(&path)
        .ok()
        .and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok())
        .filter(|v| v.is_object())
        .unwrap_or_else(|| serde_json::json!({}));
    v["tunnel"] = on.into();
    fs::write(path, serde_json::to_string_pretty(&v).unwrap_or_default())
}

/// cloudflared from this folder's bin/, the PATH, or the usual install folders (launchd's PATH is minimal).
pub fn find(home: &Path) -> Option<PathBuf> {
    let mut dirs = vec![home.join("bin")];
    if let Some(p) = std::env::var_os("PATH") {
        dirs.extend(std::env::split_paths(&p));
    }
    dirs.extend(["/opt/homebrew/bin", "/usr/local/bin", "/usr/bin"].map(PathBuf::from));
    dirs.into_iter().map(|d| d.join(exe_name())).find(|p| p.is_file())
}

/// Download cloudflared from Cloudflare's official GitHub releases into bin/ (uses the system curl).
pub fn download(home: &Path) -> Result<PathBuf, String> {
    let arch = match std::env::consts::ARCH {
        "aarch64" => "arm64",
        "x86_64" => "amd64",
        other => return Err(format!("no cloudflared download for CPU type {other}; install cloudflared yourself")),
    };
    let asset = match std::env::consts::OS {
        "macos" => format!("cloudflared-darwin-{arch}.tgz"),
        "linux" => format!("cloudflared-linux-{arch}"),
        "windows" => format!("cloudflared-windows-{arch}.exe"),
        other => return Err(format!("no cloudflared download for {other}")),
    };
    let url = format!("https://github.com/cloudflare/cloudflared/releases/latest/download/{asset}");
    let bin = home.join("bin");
    fs::create_dir_all(&bin).map_err(|e| e.to_string())?;
    let target = bin.join(exe_name());
    let tmp = bin.join(format!("{asset}.part"));
    println!("Downloading cloudflared from {url} ...");
    let ok = std::process::Command::new("curl")
        .args(["-fL", "--proto", "=https", "--proto-redir", "=https", "--tlsv1.2", "--retry", "2", "-o"])
        .arg(&tmp)
        .arg(&url)
        .status()
        .is_ok_and(|s| s.success());
    if !ok {
        let _ = fs::remove_file(&tmp);
        return Err("download failed (is the internet connected?)".into());
    }
    if asset.ends_with(".tgz") {
        let ok = std::process::Command::new("tar").arg("-xzf").arg(&tmp).arg("-C").arg(&bin).status().is_ok_and(|s| s.success());
        let _ = fs::remove_file(&tmp);
        if !ok || !target.is_file() {
            return Err("could not unpack cloudflared".into());
        }
    } else {
        fs::rename(&tmp, &target).map_err(|e| e.to_string())?;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&target, fs::Permissions::from_mode(0o755)).map_err(|e| e.to_string())?;
    }
    Ok(target)
}

/// Public address of the running tunnel, as last written by the server.
pub fn current_url(home: &Path) -> Option<String> {
    let s = fs::read_to_string(home.join(STATE)).ok()?;
    let v: serde_json::Value = serde_json::from_str(&s).ok()?;
    v["url"].as_str().map(String::from)
}

fn write_state(home: &Path, pid: Option<u32>, url: Option<&str>) {
    let _ = fs::write(home.join(STATE), serde_json::json!({ "pid": pid, "url": url }).to_string());
}

pub fn clear_state(home: &Path) {
    let _ = fs::remove_file(home.join(STATE));
}

/// If the server was killed hard last time, its cloudflared may still be running: stop it.
fn kill_stale(home: &Path, port: u16) {
    let Some(pid) = fs::read_to_string(home.join(STATE))
        .ok()
        .and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok())
        .and_then(|v| v["pid"].as_u64())
    else {
        return;
    };
    let pid = pid.to_string();
    let out = |cmd: &str, args: &[&str]| {
        std::process::Command::new(cmd)
            .args(args)
            .output()
            .map(|o| String::from_utf8_lossy(&o.stdout).to_lowercase())
            .unwrap_or_default()
    };
    // Check the name first so a reused PID of some other program is never killed.
    if cfg!(windows) {
        if out("tasklist", &["/FI", &format!("PID eq {pid}"), "/NH"]).contains("cloudflared") {
            let _ = out("taskkill", &["/PID", &pid, "/F"]);
        }
    } else {
        // Only our own tunnel, never another cloudflared that got the same number after a reboot.
        let args = out("ps", &["-p", &pid, "-o", "args="]);
        if args.contains("cloudflared") && args.contains(&format!("127.0.0.1:{port}")) {
            let _ = out("kill", &[&pid]);
        }
    }
}

/// The public address cloudflared prints, e.g. "https://calm-river-1234.trycloudflare.com".
fn parse_url(line: &str) -> Option<String> {
    line.split(|c: char| c.is_whitespace() || c == '|')
        .find(|w| w.starts_with("https://") && w.ends_with(".trycloudflare.com"))
        .map(String::from)
}

/// Keep a quick tunnel running for as long as the server runs. `on_url` gets the public address
/// when it is ready and `None` when the tunnel is down.
pub async fn run(home: PathBuf, port: u16, log: Logger, on_url: impl Fn(Option<String>)) {
    kill_stale(&home, port);
    let Some(exe) = find(&home) else {
        log.log("tunnel is on but cloudflared was not found; run: scan2cursor tunnel on");
        return;
    };
    let mut backoff = 2;
    loop {
        let mut cmd = tokio::process::Command::new(&exe);
        cmd.args(["tunnel", "--no-autoupdate", "--url", &format!("https://127.0.0.1:{port}"), "--no-tls-verify"])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .kill_on_drop(true); // stops with the server
        #[cfg(windows)]
        cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
        let started = Instant::now();
        match cmd.spawn() {
            Err(e) => log.log(&format!("could not start cloudflared: {e}")),
            Ok(mut child) => {
                write_state(&home, child.id(), None);
                if let Some(err) = child.stderr.take() {
                    let mut lines = BufReader::new(err).lines();
                    while let Ok(Some(line)) = lines.next_line().await {
                        if let Some(url) = parse_url(&line) {
                            log.log(&format!("tunnel ready: {url}"));
                            write_state(&home, child.id(), Some(&url));
                            on_url(Some(url));
                        }
                    }
                }
                let status = child.wait().await;
                log.log(&format!("tunnel stopped ({status:?}); restarting"));
            }
        }
        write_state(&home, None, None);
        on_url(None);
        if started.elapsed() > Duration::from_secs(120) {
            backoff = 2; // it ran fine for a while: retry quickly
        }
        tokio::time::sleep(Duration::from_secs(backoff)).await;
        backoff = (backoff * 2).min(60);
    }
}

#[cfg(test)]
mod tests {
    use super::parse_url;

    #[test]
    fn finds_tunnel_address_in_cloudflared_banner() {
        let line = "2026-11-15T08:00:00Z INF |  https://calm-river-1234.trycloudflare.com                                 |";
        assert_eq!(parse_url(line).as_deref(), Some("https://calm-river-1234.trycloudflare.com"));
    }

    #[test]
    fn ignores_other_links() {
        assert_eq!(parse_url("INF Thank you for trying Cloudflare Tunnel. https://www.cloudflare.com/website-terms/"), None);
    }
}
