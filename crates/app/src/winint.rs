//! Windows integration: per-user installation (no administrator rights needed), Start menu and desktop
//! shortcuts, autostart, and the "Apps & features" entry.
//!
//! Everything is written under HKCU / the user's profile, so installing never needs UAC.

use std::os::windows::process::CommandExt;
use std::path::{Path, PathBuf};

use windows_sys::Win32::Foundation::ERROR_SUCCESS;
use windows_sys::Win32::System::Registry::{
    HKEY, HKEY_CURRENT_USER, KEY_READ, KEY_WRITE, REG_DWORD, REG_OPTION_NON_VOLATILE, REG_SZ, RegCloseKey,
    RegCreateKeyExW, RegDeleteTreeW, RegDeleteValueW, RegEnumKeyExW, RegOpenKeyExW, RegQueryValueExW, RegSetValueExW,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{FindWindowW, PostMessageW};

use crate::platform::{CLASS, WM_QUIT_APP};
use crate::winclip::wide;

pub const APP: &str = "Clipvault";
pub const REPO_URL: &str = "https://github.com/ahsoggg-creator/clipvault";

const CREATE_NO_WINDOW: u32 = 0x0800_0000;
const UNINSTALL_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Uninstall\Clipvault";
const RUN_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";

#[derive(Clone, Copy)]
pub struct InstallOptions {
    pub desktop_shortcut: bool,
    pub autostart: bool,
}

/// `%LOCALAPPDATA%\Programs\Clipvault`.
pub fn install_dir() -> Option<PathBuf> {
    std::env::var_os("LOCALAPPDATA").map(|p| PathBuf::from(p).join("Programs").join(APP))
}

pub fn installed_exe() -> Option<PathBuf> {
    install_dir().map(|d| d.join(format!("{APP}.exe")))
}

fn same_file(a: &Path, b: &Path) -> bool {
    let norm = |p: &Path| p.to_string_lossy().to_lowercase().replace('/', "\\");
    norm(a) == norm(b)
}

/// The running executable is the installed copy.
pub fn running_installed() -> bool {
    match (std::env::current_exe(), installed_exe()) {
        (Ok(cur), Some(inst)) => same_file(&cur, &inst),
        _ => false,
    }
}

/// The executable was started as the installer (`Clipvault-Setup.exe` or `--install`).
pub fn is_setup_launch() -> bool {
    std::env::args().any(|a| a == "--install")
        || std::env::current_exe()
            .ok()
            .and_then(|p| p.file_stem().map(|s| s.to_string_lossy().to_lowercase()))
            .is_some_and(|s| s.contains("setup"))
}

struct Key(HKEY);

impl Drop for Key {
    fn drop(&mut self) {
        // SAFETY: the key was opened by RegCreateKeyExW / RegOpenKeyExW and is closed once.
        unsafe { RegCloseKey(self.0) };
    }
}

fn create_key(path: &str) -> Result<Key, String> {
    let p = wide(path);
    let mut h: HKEY = std::ptr::null_mut();
    // SAFETY: `p` is NUL-terminated; the out-pointer references a live local; no class / security attributes.
    let rc = unsafe {
        RegCreateKeyExW(
            HKEY_CURRENT_USER,
            p.as_ptr(),
            0,
            std::ptr::null(),
            REG_OPTION_NON_VOLATILE,
            KEY_WRITE,
            std::ptr::null(),
            &mut h,
            std::ptr::null_mut(),
        )
    };
    if rc == ERROR_SUCCESS {
        Ok(Key(h))
    } else {
        Err(format!("реестр: не удалось создать {path} (код {rc})"))
    }
}

fn open_key(path: &str, access: u32) -> Option<Key> {
    let p = wide(path);
    let mut h: HKEY = std::ptr::null_mut();
    // SAFETY: `p` is NUL-terminated; the out-pointer references a live local.
    let rc = unsafe { RegOpenKeyExW(HKEY_CURRENT_USER, p.as_ptr(), 0, access, &mut h) };
    (rc == ERROR_SUCCESS).then_some(Key(h))
}

fn set_str(key: &Key, name: &str, value: &str) -> Result<(), String> {
    let n = wide(name);
    let v = wide(value);
    // SAFETY: `v` is a NUL-terminated UTF-16 buffer whose byte length is passed; the name is NUL-terminated.
    let rc = unsafe {
        RegSetValueExW(
            key.0,
            n.as_ptr(),
            0,
            REG_SZ,
            v.as_ptr() as *const u8,
            (v.len() * 2) as u32,
        )
    };
    if rc == ERROR_SUCCESS {
        Ok(())
    } else {
        Err(format!("реестр: запись значения не удалась (код {rc})"))
    }
}

fn set_dword(key: &Key, name: &str, value: u32) -> Result<(), String> {
    let n = wide(name);
    // SAFETY: the data pointer references a live u32 of the stated size.
    let rc = unsafe { RegSetValueExW(key.0, n.as_ptr(), 0, REG_DWORD, &value as *const u32 as *const u8, 4) };
    if rc == ERROR_SUCCESS {
        Ok(())
    } else {
        Err(format!("реестр: запись значения не удалась (код {rc})"))
    }
}

pub fn autostart_enabled() -> bool {
    let Some(k) = open_key(RUN_KEY, KEY_READ) else {
        return false;
    };
    let n = wide(APP);
    let mut size = 0u32;
    // SAFETY: only the size of the value is queried.
    unsafe {
        RegQueryValueExW(
            k.0,
            n.as_ptr(),
            std::ptr::null(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            &mut size,
        ) == ERROR_SUCCESS
    }
}

const NOTIFY_KEY: &str = r"Control Panel\NotifyIconSettings";

fn same_path(a: &str, b: &str) -> bool {
    let norm = |s: &str| s.replace('/', "\\").to_lowercase();
    norm(a) == norm(b)
}

/// Windows 11 hides a new tray icon in the overflow flyout. Marks Clipvault's icon as "promoted" (shown on the
/// taskbar) in `NotifyIconSettings`. Returns true when the icon's entry exists and is promoted.
///
/// The entry only appears after the icon has been shown once, so the caller tries again for a while.
pub fn promote_tray_icon() -> bool {
    let Ok(exe) = std::env::current_exe() else {
        return false;
    };
    let exe = exe.display().to_string();
    let Some(root) = open_key(NOTIFY_KEY, KEY_READ) else {
        return false; // an older Windows: the icon is not hidden by default
    };
    let mut index = 0u32;
    loop {
        let mut name = [0u16; 256];
        let mut len = name.len() as u32;
        // SAFETY: the buffer and its length describe the same live array; the other out-parameters are optional.
        let rc = unsafe {
            RegEnumKeyExW(
                root.0,
                index,
                name.as_mut_ptr(),
                &mut len,
                std::ptr::null(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
            )
        };
        if rc != ERROR_SUCCESS {
            return false;
        }
        index += 1;
        let sub = format!("{NOTIFY_KEY}\\{}", String::from_utf16_lossy(&name[..len as usize]));
        let Some(k) = open_key(&sub, KEY_READ | KEY_WRITE) else {
            continue;
        };
        let n = wide("ExecutablePath");
        let mut buf = [0u16; 1024];
        let mut size = std::mem::size_of_val(&buf) as u32;
        // SAFETY: the buffer and its byte size describe the same live array.
        let rc = unsafe {
            RegQueryValueExW(
                k.0,
                n.as_ptr(),
                std::ptr::null(),
                std::ptr::null_mut(),
                buf.as_mut_ptr().cast(),
                &mut size,
            )
        };
        if rc != ERROR_SUCCESS {
            continue;
        }
        let path = String::from_utf16_lossy(&buf[..(size as usize / 2).min(buf.len())]);
        if same_path(path.trim_end_matches('\0'), &exe) {
            return set_dword(&k, "IsPromoted", 1).is_ok();
        }
    }
}

const ADVANCED_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Explorer\Advanced";
const DISABLED_HOTKEYS: &str = "DisabledHotkeys";

/// Adds or removes `letter` in the Explorer `DisabledHotkeys` string (the Win+<letter> shortcuts it gives up).
fn with_letter(current: &str, letter: char, on: bool) -> String {
    let rest: String = current.chars().filter(|c| !c.eq_ignore_ascii_case(&letter)).collect();
    if on { format!("{rest}{letter}") } else { rest }
}

fn disabled_hotkeys() -> String {
    let Some(k) = open_key(ADVANCED_KEY, KEY_READ) else {
        return String::new();
    };
    let n = wide(DISABLED_HOTKEYS);
    let mut buf = [0u16; 128];
    let mut size = std::mem::size_of_val(&buf) as u32;
    // SAFETY: the buffer and its byte size describe the same live array.
    let rc = unsafe {
        RegQueryValueExW(
            k.0,
            n.as_ptr(),
            std::ptr::null(),
            std::ptr::null_mut(),
            buf.as_mut_ptr().cast(),
            &mut size,
        )
    };
    if rc != ERROR_SUCCESS {
        return String::new();
    }
    let len = (size as usize / 2).min(buf.len());
    String::from_utf16_lossy(&buf[..len]).trim_end_matches('\0').to_owned()
}

/// Hands Win+V to Clipvault (`true`) or back to Windows (`false`).
pub fn set_win_v_released(on: bool) -> Result<(), String> {
    let next = with_letter(&disabled_hotkeys(), 'V', on);
    if next.is_empty() {
        if let Some(k) = open_key(ADVANCED_KEY, KEY_WRITE) {
            let n = wide(DISABLED_HOTKEYS);
            // SAFETY: deleting a missing value just returns an error code.
            unsafe { RegDeleteValueW(k.0, n.as_ptr()) };
        }
        Ok(())
    } else {
        set_str(&create_key(ADVANCED_KEY)?, DISABLED_HOTKEYS, &next)
    }
}

/// Restarts the shell so it re-reads `DisabledHotkeys`.
pub fn restart_explorer() {
    let _ = std::process::Command::new("cmd.exe")
        .raw_arg("/C taskkill /f /im explorer.exe & start explorer.exe")
        .creation_flags(CREATE_NO_WINDOW)
        .spawn();
}

pub fn set_autostart(enable: bool, exe: &Path) -> Result<(), String> {
    if enable {
        let k = create_key(RUN_KEY)?;
        set_str(&k, APP, &format!("\"{}\" --background", exe.display()))
    } else {
        if let Some(k) = open_key(RUN_KEY, KEY_WRITE) {
            let n = wide(APP);
            // SAFETY: deleting a missing value just returns an error code.
            unsafe { RegDeleteValueW(k.0, n.as_ptr()) };
        }
        Ok(())
    }
}

fn delete_tree(path: &str) {
    let p = wide(path);
    // SAFETY: `p` is NUL-terminated; deleting a missing key just returns an error code.
    unsafe { RegDeleteTreeW(HKEY_CURRENT_USER, p.as_ptr()) };
}

/// Runs a PowerShell snippet without a console window. The script travels base64-encoded, so paths with
/// quotes or non-ASCII characters need no escaping beyond PowerShell's own single quotes.
fn powershell(script: &str) -> Result<(), String> {
    let utf16: Vec<u8> = script.encode_utf16().flat_map(|u| u.to_le_bytes()).collect();
    let encoded = base64(&utf16);
    let out = std::process::Command::new("powershell.exe")
        .args([
            "-NoProfile",
            "-NonInteractive",
            "-ExecutionPolicy",
            "Bypass",
            "-EncodedCommand",
            &encoded,
        ])
        .creation_flags(CREATE_NO_WINDOW)
        .output()
        .map_err(|e| format!("не удалось запустить PowerShell: {e}"))?;
    if out.status.success() {
        Ok(())
    } else {
        Err(format!("PowerShell: {}", String::from_utf8_lossy(&out.stderr).trim()))
    }
}

fn base64(data: &[u8]) -> String {
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut s = String::with_capacity(data.len().div_ceil(3) * 4);
    for c in data.chunks(3) {
        let n =
            (u32::from(c[0]) << 16) | (u32::from(*c.get(1).unwrap_or(&0)) << 8) | u32::from(*c.get(2).unwrap_or(&0));
        for i in 0..4 {
            if i <= c.len() {
                s.push(T[((n >> (18 - 6 * i)) & 63) as usize] as char);
            } else {
                s.push('=');
            }
        }
    }
    s
}

fn ps_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', "''"))
}

/// Start menu (searchable as "Clipvault") and, optionally, desktop shortcuts.
fn create_shortcuts(exe: &Path, desktop: bool) -> Result<(), String> {
    let exe = ps_quote(&exe.to_string_lossy());
    let mut script = format!(
        "$ws = New-Object -ComObject WScript.Shell\n\
         function Make($dir) {{ $l = $ws.CreateShortcut((Join-Path $dir 'Clipvault.lnk')); \
         $l.TargetPath = {exe}; $l.IconLocation = {exe} + ',0'; \
         $l.Description = 'Clipvault — менеджер буфера обмена'; $l.Save() }}\n\
         Make ([Environment]::GetFolderPath('Programs'))\n"
    );
    if desktop {
        script.push_str("Make ([Environment]::GetFolderPath('Desktop'))\n");
    }
    powershell(&script)
}

fn remove_shortcuts() {
    let _ = powershell(
        "foreach ($f in 'Programs','Desktop') { \
         $p = Join-Path ([Environment]::GetFolderPath($f)) 'Clipvault.lnk'; \
         if (Test-Path -LiteralPath $p) { Remove-Item -LiteralPath $p -Force } }",
    );
}

/// Starts a second, elevated copy (`--admin`) with one UAC prompt.
pub fn open_as_admin() -> Result<(), String> {
    use windows_sys::Win32::UI::Shell::ShellExecuteW;
    use windows_sys::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    // SAFETY: all strings are NUL-terminated and outlive the call.
    let r = unsafe {
        ShellExecuteW(
            std::ptr::null_mut(),
            wide("runas").as_ptr(),
            wide(&exe.to_string_lossy()).as_ptr(),
            wide("--admin").as_ptr(),
            std::ptr::null(),
            SW_SHOWNORMAL,
        )
    };
    if r as isize > 32 {
        Ok(())
    } else {
        Err("запрос прав администратора отклонён".into())
    }
}

/// Asks a running Clipvault to exit and waits for it to let go of its files.
pub fn stop_running() {
    let class = wide(CLASS);
    for _ in 0..40 {
        // SAFETY: plain window lookup / post.
        let h = unsafe { FindWindowW(class.as_ptr(), std::ptr::null()) };
        if h.is_null() {
            return;
        }
        // SAFETY: posting to a window of another process is allowed for same-user, same-integrity windows.
        unsafe { PostMessageW(h, WM_QUIT_APP, 0, 0) };
        std::thread::sleep(std::time::Duration::from_millis(150));
    }
}

fn registry_entry(exe: &Path, dir: &Path) -> Result<(), String> {
    let k = create_key(UNINSTALL_KEY)?;
    set_str(&k, "DisplayName", APP)?;
    set_str(&k, "DisplayVersion", env!("CARGO_PKG_VERSION"))?;
    set_str(&k, "Publisher", "ahsoggg")?;
    set_str(&k, "InstallLocation", &dir.to_string_lossy())?;
    set_str(&k, "DisplayIcon", &format!("{},0", exe.display()))?;
    set_str(&k, "UninstallString", &format!("\"{}\" --uninstall", exe.display()))?;
    set_str(&k, "URLInfoAbout", REPO_URL)?;
    set_dword(&k, "NoModify", 1)?;
    set_dword(&k, "NoRepair", 1)?;
    let size_kb = std::fs::metadata(exe).map_or(0, |m| (m.len() / 1024) as u32);
    set_dword(&k, "EstimatedSize", size_kb)
}

/// Copies the running executable to the install folder, creates shortcuts, registers the uninstaller.
pub fn install(opts: InstallOptions, stop_old: bool) -> Result<PathBuf, String> {
    let dir = install_dir().ok_or("не найдена папка LOCALAPPDATA")?;
    let target = installed_exe().ok_or("не найдена папка LOCALAPPDATA")?;
    let me = std::env::current_exe().map_err(|e| e.to_string())?;
    if !same_file(&me, &target) {
        if stop_old {
            stop_running();
        }
        std::fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
        let old = target.with_extension("exe.old");
        let _ = std::fs::remove_file(&old);
        if target.exists() {
            let _ = std::fs::rename(&target, &old);
        }
        std::fs::copy(&me, &target).map_err(|e| format!("не удалось скопировать файл: {e}"))?;
        let _ = std::fs::remove_file(&old);
    }
    create_shortcuts(&target, opts.desktop_shortcut)?;
    registry_entry(&target, &dir)?;
    set_autostart(opts.autostart, &target)?;
    Ok(target)
}

/// Removes shortcuts, registry entries and the install folder (and, on request, the history and settings).
pub fn uninstall(delete_data: bool) -> Result<(), String> {
    stop_running();
    remove_shortcuts();
    delete_tree(UNINSTALL_KEY);
    set_autostart(false, Path::new(""))?;
    let _ = set_win_v_released(false);
    if delete_data && let Some(d) = clipvault_core::config::data_dir() {
        let _ = std::fs::remove_dir_all(d);
    }
    if let Some(dir) = install_dir() {
        // the running executable lives in that folder: a detached shell removes it a moment after we exit
        let _ = std::process::Command::new("cmd.exe")
            .raw_arg(format!(
                "/C ping -n 3 127.0.0.1 >nul & rmdir /s /q \"{}\"",
                dir.display()
            ))
            .creation_flags(CREATE_NO_WINDOW)
            .spawn();
    }
    Ok(())
}

#[cfg(test)]
mod win_v_tests {
    use super::with_letter;

    #[test]
    fn letter_is_added_once_and_removed_cleanly() {
        assert_eq!(with_letter("", 'V', true), "V");
        assert_eq!(with_letter("V", 'V', true), "V");
        assert_eq!(with_letter("EV", 'V', false), "E");
        assert_eq!(with_letter("v", 'V', false), "");
    }
}
