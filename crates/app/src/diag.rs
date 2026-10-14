//! Log and crash reports, for working out what went wrong on somebody else's machine.
//!
//! * `%APPDATA%\Clipvault\logs\clipvault.log` — start-ups, counts, update problems; rotated at 512 KB.
//! * `%APPDATA%\Clipvault\logs\crashes\crash-<time>.txt` — written by the panic hook: version, location,
//!   backtrace and the tail of the log. Nothing is ever sent anywhere; the user attaches the file.
//!
//! Clipboard content never reaches either file. Log lines are built from fixed texts, kinds and counters
//! only, and the panic hook deliberately leaves out the panic *message*: the standard library's messages
//! (a bad string slice, say) can quote the data being processed.

use std::fmt::Write as _;
use std::io::Write as _;
use std::path::PathBuf;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};

use clipvault_core::config::data_dir;
use clipvault_core::now_unix;
use clipvault_core::timefmt::{file_stamp, format_utc};

const MAX_LOG: u64 = 512 << 10;
const TAIL_LINES: usize = 60;

static WRITE_LOCK: Mutex<()> = Mutex::new(());

/// Demo mode (`--demo`): nothing real is read, written or logged.
static DEMO: AtomicBool = AtomicBool::new(false);

pub fn set_demo() {
    DEMO.store(true, Ordering::Relaxed);
}

pub fn is_demo() -> bool {
    DEMO.load(Ordering::Relaxed)
}

#[derive(Clone, Copy)]
pub enum Level {
    Info,
    Warn,
    Error,
}

pub fn logs_dir() -> Option<PathBuf> {
    data_dir().map(|d| d.join("logs"))
}

fn crashes_dir() -> Option<PathBuf> {
    logs_dir().map(|d| d.join("crashes"))
}

fn log_file() -> Option<PathBuf> {
    logs_dir().map(|d| d.join("clipvault.log"))
}

/// Appends a line to the log. Never fails loudly: a broken log must not break the program.
/// Callers pass fixed texts and counters only — never clipboard content.
pub fn log(level: Level, msg: impl AsRef<str>) {
    if is_demo() {
        return;
    }
    let (Some(dir), Some(file)) = (logs_dir(), log_file()) else {
        return;
    };
    let _guard = WRITE_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let _ = std::fs::create_dir_all(&dir);
    if std::fs::metadata(&file).is_ok_and(|m| m.len() > MAX_LOG) {
        let _ = std::fs::rename(&file, dir.join("clipvault.log.1"));
    }
    let tag = match level {
        Level::Info => "INFO ",
        Level::Warn => "WARN ",
        Level::Error => "ERROR",
    };
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(&file) {
        let msg = msg.as_ref().replace('\n', "\n    ");
        let _ = writeln!(f, "{} {tag} {msg}", format_utc(now_unix()));
    }
}

pub fn info(msg: impl AsRef<str>) {
    log(Level::Info, msg);
}

pub fn warn(msg: impl AsRef<str>) {
    log(Level::Warn, msg);
}

pub fn error(msg: impl AsRef<str>) {
    log(Level::Error, msg);
}

fn log_tail(lines: usize) -> String {
    let (Some(file), Some(dir)) = (log_file(), logs_dir()) else {
        return String::new();
    };
    let mut text = std::fs::read_to_string(dir.join("clipvault.log.1")).unwrap_or_default();
    text.push_str(&std::fs::read_to_string(file).unwrap_or_default());
    let all: Vec<&str> = text.lines().collect();
    all[all.len().saturating_sub(lines)..].join("\n")
}

fn header() -> String {
    format!(
        "Clipvault {}\nOS: {} {}\nRelease build: {}\n",
        env!("CARGO_PKG_VERSION"),
        std::env::consts::OS,
        std::env::consts::ARCH,
        cfg!(not(debug_assertions)),
    )
}

/// Text for the "copy diagnostics" button: versions, entry counts per type, the log tail. No content.
pub fn report(counts: &[(clipvault_core::Kind, usize)]) -> String {
    let mut s = header();
    s.push_str("\n--- entries by type ---\n");
    for (k, n) in counts {
        let _ = writeln!(s, "{}: {n}", k.as_str());
    }
    let _ = write!(s, "\n--- last log lines ---\n{}\n", log_tail(TAIL_LINES));
    if let Some(c) = latest_crash() {
        let _ = write!(s, "\n--- latest crash report: {} ---\n", c.display());
        s.push_str(&std::fs::read_to_string(&c).unwrap_or_default());
    }
    s
}

/// Installs the panic hook. Call first thing in `main`.
pub fn install_panic_hook() {
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let thread = std::thread::current();
        let name = thread.name().unwrap_or("<unnamed>").to_owned();
        let location = info
            .location()
            .map_or("unknown".to_owned(), |l| format!("{}:{}", l.file(), l.line()));
        let mut text = header();
        let _ = write!(
            text,
            "Time (UTC): {}\nThread: {name}\nLocation: {location}\nMessage: (left out on purpose: panic messages can quote clipboard data)\n\n--- backtrace ---\n{}\n\n--- last log lines ---\n{}\n",
            format_utc(now_unix()),
            std::backtrace::Backtrace::force_capture(),
            log_tail(TAIL_LINES),
        );
        error(format!("PANIC in thread {name} at {location}"));
        let saved = save_crash(&text);
        if name == "main" {
            show_crash_dialog(saved.as_deref());
        }
        // the previous hook would print the message to stderr: harmless for a GUI app without a console, but
        // keep it out of any attached terminal as well
        let _ = &previous;
    }));
}

fn save_crash(text: &str) -> Option<PathBuf> {
    if is_demo() {
        return None;
    }
    let dir = crashes_dir()?;
    std::fs::create_dir_all(&dir).ok()?;
    let path = dir.join(format!("crash-{}.txt", file_stamp(now_unix())));
    std::fs::write(&path, text).ok()?;
    Some(path)
}

fn latest_crash() -> Option<PathBuf> {
    let mut v: Vec<PathBuf> = std::fs::read_dir(crashes_dir()?)
        .ok()?
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|e| e == "txt"))
        .collect();
    v.sort();
    v.pop()
}

/// A crash report that the user has not been told about yet (from the previous run). Marks it as seen.
pub fn take_unseen_crash() -> Option<PathBuf> {
    let latest = latest_crash()?;
    let marker = logs_dir()?.join("last_seen_crash");
    let name = latest.file_name()?.to_string_lossy().into_owned();
    if std::fs::read_to_string(&marker).is_ok_and(|s| s.trim() == name) {
        return None;
    }
    let _ = std::fs::write(&marker, &name);
    Some(latest)
}

fn show_crash_dialog(saved: Option<&std::path::Path>) {
    #[cfg(windows)]
    {
        use windows_sys::Win32::UI::WindowsAndMessaging::{MB_ICONERROR, MB_OK, MessageBoxW};
        let w = |s: &str| s.encode_utf16().chain(std::iter::once(0)).collect::<Vec<u16>>();
        let body = match saved {
            Some(p) => format!(
                "Clipvault аварийно завершился.\n\nОтчёт о сбое сохранён:\n{}\n\nПришлите этот файл разработчику.",
                p.display()
            ),
            None => "Clipvault аварийно завершился.".to_owned(),
        };
        // SAFETY: both strings are NUL-terminated and outlive the call; no owner window.
        unsafe {
            MessageBoxW(
                std::ptr::null_mut(),
                w(&body).as_ptr(),
                w("Clipvault — сбой").as_ptr(),
                MB_OK | MB_ICONERROR,
            );
        }
    }
    #[cfg(not(windows))]
    let _ = saved;
}
