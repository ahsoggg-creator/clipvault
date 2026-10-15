//! Self-update from GitHub releases — the only network request Clipvault ever makes.
//!
//! The latest release must carry `Clipvault.exe` and `SHA256SUMS.txt`. The new exe is downloaded next to the
//! running one, its SHA-256 is checked against `SHA256SUMS.txt`, then the files are swapped: Windows lets a
//! running executable be renamed (not overwritten), so the old one becomes `Clipvault.exe.old` and is deleted
//! on the next start. The new version runs after a restart.

use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use sha2::{Digest, Sha256};

const API: &str = "https://api.github.com/repos/ahsoggg-creator/clipvault/releases/latest";
const EXE_ASSET: &str = "Clipvault.exe";
const SUM_ASSET: &str = "SHA256SUMS.txt";
/// Once a day (the anonymous GitHub API allows 60 requests per hour).
pub const CHECK_INTERVAL_SECS: u64 = 24 * 3600;

#[derive(Clone, PartialEq)]
pub enum State {
    Idle,
    Checking,
    UpToDate,
    /// A newer version exists; not downloaded (the folder is not writable).
    Available {
        version: String,
        url: String,
    },
    Downloading {
        version: String,
        done: u64,
        total: u64,
    },
    /// Installed; takes effect after a restart.
    Ready {
        version: String,
    },
    Failed(String),
}

pub type Shared = Arc<Mutex<State>>;

struct Release {
    version: String,
    page: String,
    exe_url: String,
    exe_size: u64,
    sum_url: String,
}

/// No overall deadline: a slow connection must be able to finish a 17 MB download. Every phase has its own limit
/// instead, so a dead connection is still given up on quickly.
fn agent(via_proxy: bool) -> Option<ureq::Agent> {
    let proxy = if via_proxy {
        Some(ureq::Proxy::try_from_env()?)
    } else {
        None
    };
    Some(
        ureq::Agent::config_builder()
            .proxy(proxy)
            .timeout_connect(Some(Duration::from_secs(15)))
            .timeout_send_request(Some(Duration::from_secs(30)))
            .timeout_recv_response(Some(Duration::from_secs(45)))
            .timeout_recv_body(Some(Duration::from_secs(20 * 60)))
            .tls_config(
                ureq::tls::TlsConfig::builder()
                    .provider(ureq::tls::TlsProvider::NativeTls)
                    .build(),
            )
            .build()
            .into(),
    )
}

/// GET with a fallback between routes: straight to GitHub first (a proxy that is set up for other tools often
/// cannot reach it), then through the proxy from the environment.
fn get(url: &str, json: bool) -> Result<ureq::http::Response<ureq::Body>, ureq::Error> {
    let mut last = None;
    for via_proxy in [false, true] {
        let Some(a) = agent(via_proxy) else { continue };
        let mut req = a.get(url).header("User-Agent", &ua());
        if json {
            req = req.header("Accept", "application/vnd.github+json");
        }
        match req.call() {
            Ok(r) => return Ok(r),
            // GitHub answered: another route would not change what it said
            Err(e @ ureq::Error::StatusCode(_)) => return Err(e),
            Err(e) => last = Some(e),
        }
    }
    Err(last.unwrap_or(ureq::Error::ConnectionFailed))
}

fn ua() -> String {
    format!("Clipvault/{}", env!("CARGO_PKG_VERSION"))
}

/// `"v1.2.3"` / `"1.2.3"` -> (1, 2, 3).
pub fn parse_version(s: &str) -> Option<(u64, u64, u64)> {
    let s = s.trim().trim_start_matches(['v', 'V']);
    let core = s.split(['-', '+']).next()?;
    let mut it = core.split('.').map(|p| p.parse::<u64>().ok());
    Some((
        it.next()??,
        it.next().flatten().unwrap_or(0),
        it.next().flatten().unwrap_or(0),
    ))
}

pub fn is_newer(remote: &str, local: &str) -> bool {
    matches!((parse_version(remote), parse_version(local)), (Some(r), Some(l)) if r > l)
}

/// The hash listed for `name` in a `SHA256SUMS.txt` (`<64 hex>  <file>` per line).
pub fn checksum_for(sums: &str, name: &str) -> Option<String> {
    sums.lines().find_map(|l| {
        let mut it = l.split_whitespace();
        let (hash, file) = (it.next()?, it.next()?.trim_start_matches('*'));
        (file == name && hash.len() == 64 && hash.bytes().all(|b| b.is_ascii_hexdigit()))
            .then(|| hash.to_ascii_lowercase())
    })
}

fn latest() -> Result<Release, String> {
    let mut resp = get(API, true).map_err(|e| match e {
        ureq::Error::StatusCode(404) => "на GitHub пока нет релизов".to_owned(),
        e => format!("GitHub недоступен: {e}"),
    })?;
    let text = resp.body_mut().read_to_string().map_err(|e| e.to_string())?;
    let v: serde_json::Value = serde_json::from_str(&text).map_err(|e| e.to_string())?;
    let tag = v["tag_name"].as_str().ok_or("в ответе GitHub нет tag_name")?;
    let asset = |name: &str| {
        v["assets"].as_array().and_then(|a| {
            a.iter().find(|x| x["name"].as_str() == Some(name)).map(|x| {
                (
                    x["browser_download_url"].as_str().unwrap_or("").to_owned(),
                    x["size"].as_u64().unwrap_or(0),
                )
            })
        })
    };
    let (exe_url, exe_size) = asset(EXE_ASSET).ok_or("в релизе нет Clipvault.exe")?;
    let (sum_url, _) = asset(SUM_ASSET).ok_or("в релизе нет SHA256SUMS.txt")?;
    Ok(Release {
        version: tag.trim_start_matches('v').to_owned(),
        page: v["html_url"].as_str().unwrap_or(crate::winint::REPO_URL).to_owned(),
        exe_url,
        exe_size,
        sum_url,
    })
}

fn target_exe() -> Result<PathBuf, String> {
    std::env::current_exe().map_err(|e| e.to_string())
}

/// Deletes leftovers of a previous update.
pub fn cleanup_old() {
    if let Ok(exe) = target_exe() {
        let _ = std::fs::remove_file(exe.with_extension("exe.old"));
        let _ = std::fs::remove_file(exe.with_extension("exe.download"));
    }
}

fn dir_writable(dir: &Path) -> bool {
    let probe = dir.join(".clipvault-write-test");
    let ok = std::fs::write(&probe, b"").is_ok();
    let _ = std::fs::remove_file(&probe);
    ok
}

/// One try at downloading the exe into `tmp`; returns the SHA-256 of what was written.
fn fetch_exe(rel: &Release, tmp: &Path, state: &Shared, repaint: &dyn Fn()) -> Result<String, String> {
    let mut resp = get(&rel.exe_url, false).map_err(|e| e.to_string())?;
    let mut reader = resp.body_mut().as_reader();
    let mut file = std::fs::File::create(tmp).map_err(|e| format!("{}: {e}", tmp.display()))?;
    let mut hasher = Sha256::new();
    let mut buf = vec![0u8; 256 * 1024];
    let mut done = 0u64;
    loop {
        let n = reader.read(&mut buf).map_err(|e| e.to_string())?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
        file.write_all(&buf[..n]).map_err(|e| e.to_string())?;
        done += n as u64;
        *state.lock().unwrap_or_else(|e| e.into_inner()) = State::Downloading {
            version: rel.version.clone(),
            done,
            total: rel.exe_size,
        };
        repaint();
    }
    file.sync_all().map_err(|e| e.to_string())?;
    Ok(hasher.finalize().iter().map(|b| format!("{b:02x}")).collect())
}

fn download(rel: &Release, state: &Shared, repaint: &dyn Fn()) -> Result<(), String> {
    let exe = target_exe()?;
    let tmp = exe.with_extension("exe.download");

    let sum_text = get(&rel.sum_url, false)
        .map_err(|e| e.to_string())?
        .body_mut()
        .read_to_string()
        .map_err(|e| e.to_string())?;
    let expected = checksum_for(&sum_text, EXE_ASSET).ok_or("в SHA256SUMS.txt нет суммы для Clipvault.exe")?;

    // a connection that drops half-way is tried again from the start
    let mut attempt = 0;
    let got = loop {
        attempt += 1;
        match fetch_exe(rel, &tmp, state, repaint) {
            Ok(h) => break h,
            Err(e) if attempt >= 3 => {
                let _ = std::fs::remove_file(&tmp);
                return Err(e);
            }
            Err(_) => std::thread::sleep(Duration::from_secs(2)),
        }
    };
    if got != expected {
        let _ = std::fs::remove_file(&tmp);
        return Err("контрольная сумма не совпала — обновление отменено".into());
    }

    let old = exe.with_extension("exe.old");
    let _ = std::fs::remove_file(&old);
    std::fs::rename(&exe, &old).map_err(|e| format!("не удалось заменить exe: {e}"))?;
    if let Err(e) = std::fs::rename(&tmp, &exe) {
        let _ = std::fs::rename(&old, &exe); // roll back
        return Err(format!("не удалось заменить exe: {e}"));
    }
    Ok(())
}

fn check_and_install(state: &Shared, install: bool, repaint: &dyn Fn()) -> State {
    let result = (|| -> Result<State, String> {
        let rel = latest()?;
        if !is_newer(&rel.version, env!("CARGO_PKG_VERSION")) {
            return Ok(State::UpToDate);
        }
        let writable = target_exe()
            .ok()
            .and_then(|e| e.parent().map(dir_writable))
            .unwrap_or(false);
        if !install || !writable {
            return Ok(State::Available {
                version: rel.version,
                url: rel.page,
            });
        }
        download(&rel, state, repaint)?;
        Ok(State::Ready { version: rel.version })
    })();
    result.unwrap_or_else(State::Failed)
}

/// Checks for a newer release in the background; with `install`, downloads and installs it.
pub fn spawn(state: Shared, install: bool, repaint: impl Fn() + Send + 'static) {
    *state.lock().unwrap_or_else(|e| e.into_inner()) = State::Checking;
    std::thread::spawn(move || {
        let r = check_and_install(&state, install, &repaint);
        *state.lock().unwrap_or_else(|e| e.into_inner()) = r;
        repaint();
    });
}

/// Looks for a newer release without installing it (the daily check).
pub fn check_only(state: &Shared) {
    let r = check_and_install(state, false, &|| {});
    *state.lock().unwrap_or_else(|e| e.into_inner()) = r;
}

/// `Clipvault.exe --update`: the same thing without a window. Returns the process exit code.
pub fn run_headless() -> i32 {
    let state: Shared = Arc::new(Mutex::new(State::Idle));
    let r = check_and_install(&state, true, &|| {});
    match r {
        State::UpToDate => {
            println!("Установлена последняя версия ({}).", env!("CARGO_PKG_VERSION"));
            0
        }
        State::Ready { version } => {
            println!("Clipvault {version} установлен. Перезапустите программу, чтобы начать им пользоваться.");
            0
        }
        State::Available { version, url } => {
            println!("Доступна версия {version}: {url} (папку программы нельзя изменить — скачайте вручную).");
            0
        }
        State::Failed(e) => {
            eprintln!("Не удалось обновить: {e}");
            1
        }
        _ => 1,
    }
}

/// Starts the (new) executable; the caller then exits.
pub fn restart() -> Result<(), String> {
    let exe = target_exe()?;
    std::process::Command::new(exe)
        .args(["--replace", "--background"])
        .spawn()
        .map(|_| ())
        .map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn versions() {
        assert_eq!(parse_version("v1.2.3"), Some((1, 2, 3)));
        assert_eq!(parse_version("0.10"), Some((0, 10, 0)));
        assert_eq!(parse_version("2.0.0-beta.1"), Some((2, 0, 0)));
        assert_eq!(parse_version("x"), None);
        assert!(is_newer("v0.10.0", "0.9.9"));
        assert!(!is_newer("0.2.0", "0.2.0"));
        assert!(!is_newer("garbage", "0.1.0"));
    }

    #[test]
    fn checksum_lookup() {
        let h1 = "a".repeat(64);
        let h2 = "B".repeat(64);
        let sums = format!("{h1}  Clipvault-Setup.exe\n{h2} *Clipvault.exe\nbad  clipvault-cli.exe\n");
        assert_eq!(checksum_for(&sums, "Clipvault.exe"), Some("b".repeat(64)));
        assert_eq!(checksum_for(&sums, "Clipvault-Setup.exe"), Some(h1));
        assert_eq!(checksum_for(&sums, "clipvault-cli.exe"), None);
        assert_eq!(checksum_for(&sums, "other.exe"), None);
    }
}
