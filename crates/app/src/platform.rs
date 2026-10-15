//! The Windows thread: a hidden window that receives clipboard updates, the global hotkey and tray clicks.
//! Hotkeys use `RegisterHotKey` — there is no keyboard hook anywhere in Clipvault.

use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, OnceLock};

use clipvault_core::config::Settings;
use clipvault_core::hotkey::parse_hotkey;
use clipvault_core::i18n::S;
use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, WPARAM};
use windows_sys::Win32::Graphics::Gdi::{GetMonitorInfoW, MONITOR_DEFAULTTONEAREST, MONITORINFO, MonitorFromPoint};
use windows_sys::Win32::System::DataExchange::{
    AddClipboardFormatListener, GetClipboardSequenceNumber, RemoveClipboardFormatListener,
};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::System::Threading::{AttachThreadInput, GetCurrentThreadId};
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{RegisterHotKey, UnregisterHotKey};
use windows_sys::Win32::UI::Shell::{
    NIF_ICON, NIF_INFO, NIF_MESSAGE, NIF_TIP, NIIF_INFO, NIM_ADD, NIM_DELETE, NIM_MODIFY, NOTIFYICONDATAW,
    Shell_NotifyIconW,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    AppendMenuW, BringWindowToTop, CreatePopupMenu, CreateWindowExW, DefWindowProcW, DestroyMenu, DestroyWindow,
    DispatchMessageW, FindWindowW, GetCursorPos, GetForegroundWindow, GetMessageW, GetSystemMetrics, GetWindowRect,
    GetWindowThreadProcessId, HWND_TOPMOST, IMAGE_ICON, IsWindowVisible, LR_DEFAULTCOLOR, LoadImageW, MF_CHECKED,
    MF_SEPARATOR, MF_STRING, MSG, PostMessageW, PostQuitMessage, RegisterClassW, RegisterWindowMessageW, SM_CXSMICON,
    SW_HIDE, SW_SHOW, SWP_NOSIZE, SetForegroundWindow, SetTimer, SetWindowPos, ShowWindow, TPM_RIGHTBUTTON,
    TrackPopupMenu, TranslateMessage, WM_APP, WM_CLIPBOARDUPDATE, WM_COMMAND, WM_DESTROY, WM_HOTKEY, WM_LBUTTONUP,
    WM_NULL, WM_RBUTTONUP, WM_TIMER, WNDCLASSW,
};

use crate::shared::Shared;
use crate::winclip::wide;
use crate::{capture, diag, paste};

pub const CLASS: &str = "ClipvaultMessageWindow";
pub const CLASS_ADMIN: &str = "ClipvaultAdminMessageWindow";
/// Title of the popup window; the elevated helper has its own so the two never get mixed up.
pub fn popup_title(admin: bool) -> &'static str {
    if admin { "Clipvault (admin)" } else { "Clipvault" }
}
const WM_TRAY: u32 = WM_APP + 1;
pub const WM_PASTE: u32 = WM_APP + 2;
pub const WM_REHOTKEY: u32 = WM_APP + 3;
pub const WM_SHOW: u32 = WM_APP + 4;
pub const WM_BALLOON: u32 = WM_APP + 5;
pub const WM_QUIT_APP: u32 = WM_APP + 6;

const ID_HOTKEY_POPUP: i32 = 1;
const TIMER_ERASE: usize = 1;
const CMD_OPEN: usize = 1001;
const CMD_PAUSE: usize = 1002;
const CMD_INCOGNITO: usize = 1003;
const CMD_SETTINGS: usize = 1004;
const CMD_QUIT: usize = 1005;
const TRAY_ID: u32 = 1;
const MOD_NOREPEAT: u32 = 0x4000;

static SHARED: OnceLock<Arc<Shared>> = OnceLock::new();
static LAST_SEQ: AtomicU32 = AtomicU32::new(0);
static TASKBAR_CREATED: AtomicU32 = AtomicU32::new(0);

fn sh() -> &'static Arc<Shared> {
    SHARED
        .get()
        .expect("platform thread started before the shared state exists")
}

/// Demo mode: the shared state exists, but there is no hidden window, tray icon or hotkey.
pub fn init_only(shared: Arc<Shared>) {
    let _ = SHARED.set(shared);
}

/// Starts the Windows thread. `capture` = listen to the clipboard (false in demo mode).
pub fn spawn(shared: Arc<Shared>, capture: bool) -> std::thread::JoinHandle<()> {
    let _ = SHARED.set(shared);
    std::thread::Builder::new()
        .name("clipvault-win".into())
        .spawn(move || run(capture))
        .expect("cannot start the Windows thread")
}

fn run(listen: bool) {
    // SAFETY: standard window-class registration and message loop on a thread that owns the window.
    unsafe {
        let hinst = GetModuleHandleW(std::ptr::null());
        let class = wide(if sh().admin { CLASS_ADMIN } else { CLASS });
        let wc = WNDCLASSW {
            style: 0,
            lpfnWndProc: Some(wndproc),
            cbClsExtra: 0,
            cbWndExtra: 0,
            hInstance: hinst,
            hIcon: std::ptr::null_mut(),
            hCursor: std::ptr::null_mut(),
            hbrBackground: std::ptr::null_mut(),
            lpszMenuName: std::ptr::null(),
            lpszClassName: class.as_ptr(),
        };
        RegisterClassW(&wc);
        let hwnd = CreateWindowExW(
            0,
            class.as_ptr(),
            wide("ClipvaultMessage").as_ptr(),
            0,
            0,
            0,
            0,
            0,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            hinst,
            std::ptr::null(),
        );
        if hwnd.is_null() {
            diag::error("cannot create the message window");
            return;
        }
        sh().msg_hwnd.store(hwnd as isize, Ordering::SeqCst);
        TASKBAR_CREATED.store(
            RegisterWindowMessageW(wide("TaskbarCreated").as_ptr()),
            Ordering::SeqCst,
        );
        if listen && AddClipboardFormatListener(hwnd) == 0 {
            diag::error("AddClipboardFormatListener failed");
        }
        register_hotkeys(hwnd);
        tray(hwnd, NIM_ADD, None);
        SetTimer(hwnd, TIMER_ERASE, 1000, None);
        diag::info("clipboard listener started");
        let mut msg: MSG = std::mem::zeroed();
        while GetMessageW(&mut msg, std::ptr::null_mut(), 0, 0) > 0 {
            TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
        if listen {
            RemoveClipboardFormatListener(hwnd);
        }
        UnregisterHotKey(hwnd, ID_HOTKEY_POPUP);
        tray(hwnd, NIM_DELETE, None);
    }
}

fn register_hotkeys(hwnd: HWND) {
    let s = sh();
    let spec = {
        let st = s.settings.lock().unwrap_or_else(|e| e.into_inner());
        if s.admin {
            st.admin_hotkey.clone()
        } else {
            st.hotkey.clone()
        }
    };
    *s.hotkey_error.lock().unwrap_or_else(|e| e.into_inner()) = None;
    // SAFETY: unregistering an id that is not registered just fails.
    unsafe { UnregisterHotKey(hwnd, ID_HOTKEY_POPUP) };
    match parse_hotkey(&spec) {
        None => *s.hotkey_error.lock().unwrap_or_else(|e| e.into_inner()) = Some(s.tr(S::HotkeyInvalid).to_owned()),
        Some((mods, vk)) => {
            // SAFETY: `hwnd` belongs to this thread.
            if unsafe { RegisterHotKey(hwnd, ID_HOTKEY_POPUP, mods | MOD_NOREPEAT, vk) } == 0 {
                diag::warn("popup hotkey is taken by another program");
                *s.hotkey_error.lock().unwrap_or_else(|e| e.into_inner()) = Some(s.tr(S::HotkeyBusy).to_owned());
                balloon(hwnd, s.tr(S::HotkeyBusy));
            }
        }
    }
    s.repaint();
}

fn copy_wide<const N: usize>(dst: &mut [u16; N], text: &str) {
    for (d, u) in dst.iter_mut().zip(text.encode_utf16().take(N - 1)) {
        *d = u;
    }
}

fn tray_icon() -> windows_sys::Win32::UI::WindowsAndMessaging::HICON {
    // SAFETY: resource id 1 is the icon embedded by `build.rs`; MAKEINTRESOURCE passes it as a pointer value.
    unsafe {
        let hinst = GetModuleHandleW(std::ptr::null());
        let size = GetSystemMetrics(SM_CXSMICON);
        LoadImageW(
            hinst,
            std::ptr::without_provenance::<u16>(1),
            IMAGE_ICON,
            size,
            size,
            LR_DEFAULTCOLOR,
        ) as _
    }
}

/// Adds / updates / removes the tray icon; `info` shows a balloon notification.
fn tray(hwnd: HWND, action: u32, info: Option<&str>) {
    let s = sh();
    // SAFETY: NOTIFYICONDATAW is plain data; zeroed is a valid starting point.
    let mut nid: NOTIFYICONDATAW = unsafe { std::mem::zeroed() };
    nid.cbSize = std::mem::size_of::<NOTIFYICONDATAW>() as u32;
    nid.hWnd = hwnd;
    nid.uID = TRAY_ID;
    nid.uFlags = NIF_MESSAGE | NIF_ICON | NIF_TIP;
    nid.uCallbackMessage = WM_TRAY;
    nid.hIcon = tray_icon();
    let tip = if s.admin {
        "Clipvault (admin)".to_owned()
    } else if s.settings.lock().unwrap_or_else(|e| e.into_inner()).paused {
        format!("Clipvault — {}", s.tr(S::PausedBanner))
    } else if s.incognito.load(Ordering::Relaxed) {
        format!("Clipvault — {}", s.tr(S::IncognitoBanner))
    } else {
        "Clipvault".to_owned()
    };
    copy_wide(&mut nid.szTip, &tip);
    if let Some(text) = info {
        nid.uFlags |= NIF_INFO;
        nid.dwInfoFlags = NIIF_INFO;
        copy_wide(&mut nid.szInfoTitle, "Clipvault");
        copy_wide(&mut nid.szInfo, text);
    }
    // SAFETY: `nid` is fully initialised above.
    unsafe { Shell_NotifyIconW(action, &nid) };
}

pub fn balloon(hwnd: HWND, text: &str) {
    tray(hwnd, NIM_MODIFY, Some(text));
}

/// The eframe window. It does not exist yet when the app is created, so it is looked up by title on first use.
fn popup_hwnd() -> HWND {
    let cached = sh().popup_hwnd.load(Ordering::SeqCst);
    if cached != 0 {
        return cached as HWND;
    }
    // SAFETY: plain lookup by window title; the message window has a different title.
    let h = unsafe { FindWindowW(std::ptr::null(), wide(popup_title(sh().admin)).as_ptr()) };
    sh().popup_hwnd.store(h as isize, Ordering::SeqCst);
    h
}

pub fn hide_popup() {
    let h = popup_hwnd();
    if !h.is_null() {
        // SAFETY: hiding a window of this process.
        unsafe { ShowWindow(h, SW_HIDE) };
    }
}

fn popup_open() -> bool {
    let h = popup_hwnd();
    // SAFETY: no pointers involved.
    !h.is_null() && unsafe { IsWindowVisible(h) != 0 }
}

fn show_popup() {
    let s = sh();
    let h = popup_hwnd();
    if h.is_null() {
        return;
    }
    // SAFETY: all calls take plain handles / out-pointers to live locals.
    unsafe {
        let fg = GetForegroundWindow();
        if fg != h {
            s.prev_foreground.store(fg as isize, Ordering::SeqCst);
        }
        let mut cur = POINT { x: 0, y: 0 };
        GetCursorPos(&mut cur);
        let mon = MonitorFromPoint(cur, MONITOR_DEFAULTTONEAREST);
        let mut mi: MONITORINFO = std::mem::zeroed();
        mi.cbSize = std::mem::size_of::<MONITORINFO>() as u32;
        let mut rc = std::mem::zeroed();
        GetWindowRect(h, &mut rc);
        let (w, hh) = (rc.right - rc.left, rc.bottom - rc.top);
        let (x, y) = if GetMonitorInfoW(mon, &mut mi) != 0 {
            let r = mi.rcWork;
            (r.left + (r.right - r.left - w) / 2, r.top + (r.bottom - r.top - hh) / 3)
        } else {
            (rc.left, rc.top)
        };
        SetWindowPos(h, HWND_TOPMOST, x, y, 0, 0, SWP_NOSIZE);
        ShowWindow(h, SW_SHOW);
        // the hotkey press gives this process the right to take the focus; attaching to the old foreground
        // thread makes it reliable even when that window belongs to a busy program
        let cur_thread = GetCurrentThreadId();
        let fg_thread = if fg.is_null() {
            0
        } else {
            GetWindowThreadProcessId(fg, std::ptr::null_mut())
        };
        let attached = fg_thread != 0 && fg_thread != cur_thread && AttachThreadInput(cur_thread, fg_thread, 1) != 0;
        BringWindowToTop(h);
        let ok = SetForegroundWindow(h);
        if attached {
            AttachThreadInput(cur_thread, fg_thread, 0);
        }
        diag::info(format!(
            "popup shown: set_foreground={ok} attached={attached} is_foreground={}",
            GetForegroundWindow() == h
        ));
    }
    s.show_seq.fetch_add(1, Ordering::SeqCst);
    s.repaint();
}

fn toggle_popup() {
    // SAFETY: no pointers involved.
    if popup_open() && unsafe { GetForegroundWindow() } == popup_hwnd() {
        hide_popup();
    } else {
        show_popup();
    }
}

fn show_menu(hwnd: HWND) {
    let s = sh();
    let paused = s.settings.lock().unwrap_or_else(|e| e.into_inner()).paused;
    let incog = s.incognito.load(Ordering::Relaxed);
    // SAFETY: the menu is created, shown and destroyed here; strings are NUL-terminated and outlive the calls.
    unsafe {
        let menu = CreatePopupMenu();
        let add = |flags: u32, id: usize, text: &str| {
            AppendMenuW(menu, flags, id, wide(text).as_ptr());
        };
        add(MF_STRING, CMD_OPEN, s.tr(S::TrayOpen));
        if !s.admin {
            AppendMenuW(menu, MF_SEPARATOR, 0, std::ptr::null());
            add(
                MF_STRING | if paused { MF_CHECKED } else { 0 },
                CMD_PAUSE,
                s.tr(S::PauseRecording),
            );
            add(
                MF_STRING | if incog { MF_CHECKED } else { 0 },
                CMD_INCOGNITO,
                s.tr(S::Incognito),
            );
            AppendMenuW(menu, MF_SEPARATOR, 0, std::ptr::null());
            add(MF_STRING, CMD_SETTINGS, s.tr(S::TraySettings));
        }
        add(MF_STRING, CMD_QUIT, s.tr(S::TrayQuit));
        let mut pt = POINT { x: 0, y: 0 };
        GetCursorPos(&mut pt);
        SetForegroundWindow(hwnd);
        TrackPopupMenu(menu, TPM_RIGHTBUTTON, pt.x, pt.y, 0, hwnd, std::ptr::null());
        PostMessageW(hwnd, WM_NULL, 0, 0);
        DestroyMenu(menu);
    }
}

fn command(hwnd: HWND, cmd: usize) {
    let s = sh();
    match cmd {
        CMD_OPEN => show_popup(),
        CMD_PAUSE => {
            {
                let mut st = s.settings.lock().unwrap_or_else(|e| e.into_inner());
                st.paused = !st.paused;
                st.save();
            }
            diag::info("recording paused/resumed from the tray");
            tray(hwnd, NIM_MODIFY, None);
            s.repaint();
        }
        CMD_INCOGNITO => {
            s.incognito.fetch_xor(true, Ordering::SeqCst);
            diag::info("incognito toggled from the tray");
            tray(hwnd, NIM_MODIFY, None);
            s.repaint();
        }
        CMD_SETTINGS => {
            s.open_settings.store(true, Ordering::SeqCst);
            show_popup();
        }
        CMD_QUIT => quit(hwnd),
        _ => {}
    }
}

fn quit(hwnd: HWND) {
    let st = sh().stats.lock().unwrap_or_else(|e| e.into_inner()).clone();
    diag::info(format!(
        "exit: captured={} repeated={} skipped_paused={} rejected={} unreadable={}",
        st.captured, st.repeated, st.skipped_paused, st.rejected, st.unreadable
    ));
    sh().quit.store(true, Ordering::SeqCst);
    // SAFETY: destroying our own window ends the message loop; the process then exits.
    unsafe { DestroyWindow(hwnd) };
    std::process::exit(0);
}

unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    match msg {
        WM_CLIPBOARDUPDATE => {
            // SAFETY: no pointers involved.
            let seq = unsafe { GetClipboardSequenceNumber() };
            if LAST_SEQ.swap(seq, Ordering::SeqCst) != seq {
                capture::on_clipboard_update(sh(), hwnd);
            }
            0
        }
        WM_HOTKEY => {
            if wp as i32 == ID_HOTKEY_POPUP {
                toggle_popup();
            }
            0
        }
        WM_TRAY => {
            match lp as u32 {
                WM_LBUTTONUP => show_popup(),
                WM_RBUTTONUP => show_menu(hwnd),
                _ => {}
            }
            0
        }
        WM_COMMAND => {
            command(hwnd, wp & 0xFFFF);
            0
        }
        WM_TIMER => {
            if wp == TIMER_ERASE {
                capture::process_erasures(sh(), hwnd);
            }
            0
        }
        WM_PASTE => {
            paste::run(sh(), hwnd);
            0
        }
        WM_REHOTKEY => {
            register_hotkeys(hwnd);
            tray(hwnd, NIM_MODIFY, None);
            0
        }
        WM_SHOW => {
            show_popup();
            0
        }
        WM_BALLOON => {
            if let Some((text, _)) = sh().toast.lock().unwrap_or_else(|e| e.into_inner()).take() {
                balloon(hwnd, &text);
            }
            0
        }
        WM_QUIT_APP => {
            quit(hwnd);
            0
        }
        WM_DESTROY => {
            // SAFETY: ends this thread's message loop.
            unsafe { PostQuitMessage(0) };
            0
        }
        m if m != 0 && m == TASKBAR_CREATED.load(Ordering::SeqCst) => {
            tray(hwnd, NIM_ADD, None);
            0
        }
        // SAFETY: default handling of every other message.
        _ => unsafe { DefWindowProcW(hwnd, msg, wp, lp) },
    }
}

/// Asks the Windows thread to do something; safe to call from any thread.
pub fn post(msg: u32) {
    let h = sh().msg_hwnd.load(Ordering::SeqCst) as HWND;
    if !h.is_null() {
        // SAFETY: posting to a window of this process.
        unsafe { PostMessageW(h, msg, 0, 0) };
    }
}

/// Shows a balloon notification from any thread.
pub fn notify(text: impl Into<String>) {
    *sh().toast.lock().unwrap_or_else(|e| e.into_inner()) = Some((text.into(), std::time::Instant::now()));
    post(WM_BALLOON);
}

pub fn settings_changed(_: &Settings) {
    post(WM_REHOTKEY);
}
