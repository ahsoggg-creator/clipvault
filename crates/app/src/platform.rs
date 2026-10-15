//! The Windows thread: a hidden window that receives clipboard updates, the global hotkey and tray clicks.
//! Hotkeys use `RegisterHotKey` — there is no keyboard hook anywhere in Clipvault.

use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex, OnceLock};

use clipvault_core::config::CompactPosition;
use clipvault_core::config::Settings;
use clipvault_core::hotkey::{MOD_ALT, MOD_CONTROL, MOD_SHIFT, MOD_WIN, format_hotkey, parse_hotkey, recordable_keys};
use clipvault_core::i18n::S;
use clipvault_core::transform::Transform;
use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, RECT, WPARAM};
use windows_sys::Win32::Graphics::Gdi::{
    GetMonitorInfoW, MONITOR_DEFAULTTONEAREST, MONITORINFO, MonitorFromPoint, MonitorFromWindow,
};
use windows_sys::Win32::System::DataExchange::{
    AddClipboardFormatListener, GetClipboardSequenceNumber, RemoveClipboardFormatListener,
};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::System::Threading::{AttachThreadInput, GetCurrentThreadId};
use windows_sys::Win32::UI::HiDpi::GetDpiForWindow;
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{GetAsyncKeyState, RegisterHotKey, UnregisterHotKey};
use windows_sys::Win32::UI::Shell::{
    NIF_ICON, NIF_INFO, NIF_MESSAGE, NIF_TIP, NIIF_INFO, NIM_ADD, NIM_DELETE, NIM_MODIFY, NOTIFYICONDATAW,
    Shell_NotifyIconW,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    AppendMenuW, BringWindowToTop, CreatePopupMenu, CreateWindowExW, DefWindowProcW, DestroyMenu, DestroyWindow,
    DispatchMessageW, FindWindowW, GetCursorPos, GetForegroundWindow, GetMessageW, GetSystemMetrics, GetWindowRect,
    GetWindowThreadProcessId, HWND_TOPMOST, IMAGE_ICON, IsWindowVisible, LR_DEFAULTCOLOR, LoadImageW, MF_CHECKED,
    MF_SEPARATOR, MF_STRING, MSG, PostMessageW, PostQuitMessage, RegisterClassW, RegisterWindowMessageW, SM_CXSMICON,
    SW_HIDE, SW_SHOW, SWP_NOACTIVATE, SWP_NOSIZE, SWP_NOZORDER, SetForegroundWindow, SetTimer, SetWindowPos,
    ShowWindow, TPM_RIGHTBUTTON, TrackPopupMenu, TranslateMessage, WM_APP, WM_CLIPBOARDUPDATE, WM_COMMAND, WM_DESTROY,
    WM_HOTKEY, WM_LBUTTONUP, WM_NULL, WM_RBUTTONUP, WM_TIMER, WNDCLASSW,
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
/// `wParam` 1 = the compact shape, 0 = the full window.
pub const WM_MODE: u32 = WM_APP + 7;
/// Empties the system clipboard (after "clear everything").
pub const WM_CLEAR_CLIPBOARD: u32 = WM_APP + 8;

const ID_HOTKEY_POPUP: i32 = 1;
const ID_HOTKEY_PLAIN: i32 = 2;
const ID_HOTKEY_WINV: i32 = 3;
const ID_HOTKEY_TRANSFORM: i32 = 100;
const ID_HOTKEY_SNIPPET: i32 = 300;
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
        for (id, _) in HOTKEYS.lock().unwrap_or_else(|e| e.into_inner()).iter() {
            UnregisterHotKey(hwnd, *id);
        }
        tray(hwnd, NIM_DELETE, None);
    }
}

/// The hotkeys that are registered right now, and what each one does.
static HOTKEYS: Mutex<Vec<(i32, paste::HotkeyAction)>> = Mutex::new(Vec::new());

fn register_hotkeys(hwnd: HWND) {
    let s = sh();
    let (popup_spec, plain_spec, transforms, win_v) = {
        let st = s.settings.lock().unwrap_or_else(|e| e.into_inner());
        (
            if s.admin {
                st.admin_hotkey.clone()
            } else {
                st.hotkey.clone()
            },
            st.plain_paste_hotkey.clone(),
            st.transform_hotkeys.clone(),
            st.replace_win_v && !s.admin,
        )
    };
    *s.hotkey_error.lock().unwrap_or_else(|e| e.into_inner()) = None;
    let mut actions = HOTKEYS.lock().unwrap_or_else(|e| e.into_inner());
    // SAFETY: unregistering an id that is not registered just fails.
    unsafe {
        UnregisterHotKey(hwnd, ID_HOTKEY_POPUP);
        UnregisterHotKey(hwnd, ID_HOTKEY_WINV);
        for (id, _) in actions.iter() {
            UnregisterHotKey(hwnd, *id);
        }
    }
    actions.clear();
    if s.capturing.load(Ordering::SeqCst) {
        // a combination is being recorded: it must reach the recorder, not trigger what it is bound to
        s.repaint();
        return;
    }
    let mut busy: Vec<String> = Vec::new();
    let mut try_register = |id: i32, spec: &str| -> bool {
        let Some((mods, vk)) = parse_hotkey(spec) else {
            busy.push(spec.to_owned());
            return false;
        };
        // SAFETY: `hwnd` belongs to this thread.
        let ok = unsafe { RegisterHotKey(hwnd, id, mods | MOD_NOREPEAT, vk) } != 0;
        if !ok {
            busy.push(spec.to_owned());
        }
        ok
    };
    if !try_register(ID_HOTKEY_POPUP, &popup_spec) {
        *s.hotkey_error.lock().unwrap_or_else(|e| e.into_inner()) = Some(s.tr(S::HotkeyBusy).to_owned());
    }
    if win_v {
        try_register(ID_HOTKEY_WINV, "Win+V");
    }
    if !s.admin {
        if !plain_spec.trim().is_empty() && try_register(ID_HOTKEY_PLAIN, &plain_spec) {
            actions.push((ID_HOTKEY_PLAIN, paste::HotkeyAction::PlainPaste));
        }
        for (i, th) in transforms.iter().enumerate() {
            let Some(t) = Transform::parse(&th.transform) else {
                continue;
            };
            let id = ID_HOTKEY_TRANSFORM + i as i32;
            if !th.hotkey.trim().is_empty() && try_register(id, &th.hotkey) {
                actions.push((id, paste::HotkeyAction::Transform(t)));
            }
        }
        let snippets: Vec<(u64, String)> = s
            .history
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .snippets()
            .iter()
            .filter(|sn| !sn.hotkey.trim().is_empty())
            .map(|sn| (sn.id, sn.hotkey.clone()))
            .collect();
        for (i, (sid, spec)) in snippets.iter().enumerate() {
            let id = ID_HOTKEY_SNIPPET + i as i32;
            if try_register(id, spec) {
                actions.push((id, paste::HotkeyAction::Snippet(*sid)));
            }
        }
    }
    if !busy.is_empty() {
        diag::warn(format!("{} hotkey(s) could not be registered", busy.len()));
        let text = format!("{}: {}", s.tr(S::HotkeyBusy), busy.join(", "));
        *s.hotkey_error.lock().unwrap_or_else(|e| e.into_inner()) = Some(text.clone());
        balloon(hwnd, &text);
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
    capture_end();
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

/// Size of the popup in logical points: the compact quick-paste shape and the full window.
const FULL_SIZE: (f32, f32) = (980.0, 700.0);
const COMPACT_SIZE: (f32, f32) = (380.0, 540.0);

/// The popup's size in physical pixels for the monitor the window is on now, never larger than the work area.
fn popup_size_px(h: HWND, compact: bool, work: &RECT) -> (i32, i32) {
    // SAFETY: plain handle.
    let dpi = unsafe { GetDpiForWindow(h) };
    let scale = if dpi == 0 { 1.0 } else { dpi as f32 / 96.0 };
    let (w, hh) = if compact { COMPACT_SIZE } else { FULL_SIZE };
    (
        ((w * scale).round() as i32).min(work.right - work.left),
        ((hh * scale).round() as i32).min(work.bottom - work.top),
    )
}

/// Shows the popup in the compact (hotkey) or the full shape, on the monitor the mouse is on.
fn show_popup(compact: bool) {
    let s = sh();
    let h = popup_hwnd();
    if h.is_null() {
        return;
    }
    s.compact.store(compact, Ordering::SeqCst);
    // SAFETY: all calls take plain handles / out-pointers to live locals.
    unsafe {
        let fg = GetForegroundWindow();
        if fg != h {
            s.prev_foreground.store(fg as isize, Ordering::SeqCst);
        }
        let mut cur = POINT { x: 0, y: 0 };
        GetCursorPos(&mut cur);
        let (at_cursor, position, last) = {
            let st = s.settings.lock().unwrap_or_else(|e| e.into_inner());
            (st.popup_at_cursor, st.compact_position, st.compact_last)
        };
        // "where it was left" lives on the monitor it was left on; everything else follows the mouse
        let last = last.filter(|_| compact && position == CompactPosition::Last);
        let anchor = last.map_or(cur, |(x, y)| POINT { x, y });
        let mon = MonitorFromPoint(anchor, MONITOR_DEFAULTTONEAREST);
        let mut mi: MONITORINFO = std::mem::zeroed();
        mi.cbSize = std::mem::size_of::<MONITORINFO>() as u32;
        if GetMonitorInfoW(mon, &mut mi) != 0 {
            let r = mi.rcWork;
            // first hop onto the monitor: the size is worked out with the scale factor of the screen it lands on
            SetWindowPos(
                h,
                HWND_TOPMOST,
                r.left + 40,
                r.top + 40,
                0,
                0,
                SWP_NOSIZE | SWP_NOACTIVATE,
            );
            let (w, hh) = popup_size_px(h, compact, &r);
            let (x, y) = if compact {
                match (position, last) {
                    (CompactPosition::Center, _) => {
                        (r.left + (r.right - r.left - w) / 2, r.top + (r.bottom - r.top - hh) / 2)
                    }
                    (CompactPosition::Corner, _) => ((r.right - w - 12).max(r.left), (r.bottom - hh - 12).max(r.top)),
                    (CompactPosition::Last, Some((lx, ly))) => (
                        lx.clamp(r.left, (r.right - w).max(r.left)),
                        ly.clamp(r.top, (r.bottom - hh).max(r.top)),
                    ),
                    // like Win+V: right under the mouse, pushed back inside the screen when it does not fit
                    _ => (
                        (cur.x - w / 2).clamp(r.left, (r.right - w).max(r.left)),
                        (cur.y + 16).clamp(r.top, (r.bottom - hh).max(r.top)),
                    ),
                }
            } else if at_cursor {
                (
                    (cur.x - w / 2).clamp(r.left, (r.right - w).max(r.left)),
                    (cur.y - 24).clamp(r.top, (r.bottom - hh).max(r.top)),
                )
            } else {
                (r.left + (r.right - r.left - w) / 2, r.top + (r.bottom - r.top - hh) / 3)
            };
            SetWindowPos(h, HWND_TOPMOST, x, y, w, hh, 0);
        } else {
            let mut rc = std::mem::zeroed();
            GetWindowRect(h, &mut rc);
            SetWindowPos(h, HWND_TOPMOST, rc.left, rc.top, 0, 0, SWP_NOSIZE);
        }
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
            "popup shown: compact={compact} set_foreground={ok} attached={attached} is_foreground={}",
            GetForegroundWindow() == h
        ));
    }
    s.show_seq.fetch_add(1, Ordering::SeqCst);
    s.repaint();
}

/// Switches the visible popup between the compact and the full shape, keeping it on its monitor.
fn resize_popup(compact: bool) {
    let h = popup_hwnd();
    if h.is_null() {
        return;
    }
    sh().compact.store(compact, Ordering::SeqCst);
    // SAFETY: plain handles and out-pointers to live locals.
    unsafe {
        let mut rc: RECT = std::mem::zeroed();
        GetWindowRect(h, &mut rc);
        let mut mi: MONITORINFO = std::mem::zeroed();
        mi.cbSize = std::mem::size_of::<MONITORINFO>() as u32;
        if GetMonitorInfoW(MonitorFromWindow(h, MONITOR_DEFAULTTONEAREST), &mut mi) == 0 {
            return;
        }
        let r = mi.rcWork;
        let (w, hh) = popup_size_px(h, compact, &r);
        let x = rc.left.clamp(r.left, (r.right - w).max(r.left));
        let y = rc.top.clamp(r.top, (r.bottom - hh).max(r.top));
        SetWindowPos(h, std::ptr::null_mut(), x, y, w, hh, SWP_NOZORDER | SWP_NOACTIVATE);
    }
    sh().repaint();
}

/// Asks the Windows thread to switch the popup between its compact and full shape.
pub fn set_compact(compact: bool) {
    post_with(WM_MODE, usize::from(compact));
}

fn toggle_popup() {
    // SAFETY: no pointers involved.
    if popup_open() && unsafe { GetForegroundWindow() } == popup_hwnd() {
        hide_popup();
    } else {
        let compact = sh().settings.lock().unwrap_or_else(|e| e.into_inner()).compact_popup;
        show_popup(compact);
    }
}

/// The window being dragged by its title strip: where the mouse and the window were when the press began.
#[derive(Clone, Copy, Debug)]
pub struct Drag {
    cursor: (i32, i32),
    window: (i32, i32),
}

/// The popup's top-left corner on the screen.
pub fn window_pos() -> Option<(i32, i32)> {
    let h = popup_hwnd();
    if h.is_null() {
        return None;
    }
    // SAFETY: out-pointer to a live local.
    unsafe {
        let mut rc: RECT = std::mem::zeroed();
        (GetWindowRect(h, &mut rc) != 0).then_some((rc.left, rc.top))
    }
}

/// Starts moving the popup by hand. The system's own move loop (`StartDrag`) only works while the window is
/// focused and leaves egui believing the button is still down; following the cursor works everywhere.
pub fn drag_begin() -> Option<Drag> {
    let h = popup_hwnd();
    if h.is_null() {
        return None;
    }
    // SAFETY: out-pointers to live locals.
    unsafe {
        let mut cur = POINT { x: 0, y: 0 };
        let mut rc: RECT = std::mem::zeroed();
        (GetCursorPos(&mut cur) != 0 && GetWindowRect(h, &mut rc) != 0).then_some(Drag {
            cursor: (cur.x, cur.y),
            window: (rc.left, rc.top),
        })
    }
}

/// Puts the popup where the mouse has carried it since `drag_begin`, on whichever monitor that is.
pub fn drag_to(d: &Drag) {
    let h = popup_hwnd();
    if h.is_null() {
        return;
    }
    // SAFETY: out-pointer to a live local.
    unsafe {
        let mut cur = POINT { x: 0, y: 0 };
        if GetCursorPos(&mut cur) == 0 {
            return;
        }
        SetWindowPos(
            h,
            std::ptr::null_mut(),
            d.window.0 + cur.x - d.cursor.0,
            d.window.1 + cur.y - d.cursor.1,
            0,
            0,
            SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE,
        );
    }
}

/// Releases the global hotkeys so that a combination being recorded reaches the recorder.
pub fn capture_begin() {
    sh().capturing.store(true, Ordering::SeqCst);
    post(WM_REHOTKEY);
}

/// Takes the hotkeys back. Safe to call when nothing is being recorded.
pub fn capture_end() {
    if sh().capturing.swap(false, Ordering::SeqCst) {
        post(WM_REHOTKEY);
    }
}

/// What the recorder saw on the keyboard.
#[derive(Debug, PartialEq, Eq)]
pub enum Capture {
    Idle,
    Cancel,
    Clear,
    Got(String),
}

/// Looks at the physical keyboard (not at egui's events: egui turns Ctrl+V into a paste event and never sees the
/// Win key). A modifier plus a key is a combination; Esc alone cancels, Backspace alone clears.
pub fn poll_capture() -> Capture {
    let down = |vk: i32| {
        // SAFETY: no pointers involved.
        unsafe { GetAsyncKeyState(vk) as u16 & 0x8000 != 0 }
    };
    let mut mods = 0;
    for (vks, m) in [
        (&[0x11][..], MOD_CONTROL),
        (&[0x12], MOD_ALT),
        (&[0x10], MOD_SHIFT),
        (&[0x5B, 0x5C], MOD_WIN),
    ] {
        if vks.iter().any(|&v| down(v)) {
            mods |= m;
        }
    }
    if mods == 0 {
        return if down(0x1B) {
            Capture::Cancel
        } else if down(0x08) || down(0x2E) {
            Capture::Clear
        } else {
            Capture::Idle
        };
    }
    recordable_keys()
        .find(|&vk| down(vk as i32))
        .map_or(Capture::Idle, |vk| Capture::Got(format_hotkey(mods, vk)))
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
        CMD_OPEN => show_popup(false),
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
            show_popup(false);
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
            let id = wp as i32;
            if id == ID_HOTKEY_POPUP || id == ID_HOTKEY_WINV {
                toggle_popup();
            } else {
                let action = HOTKEYS
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .iter()
                    .find(|(i, _)| *i == id)
                    .map(|(_, a)| a.clone());
                if let Some(action) = action {
                    // the action sleeps while the target program reads the clipboard: not on the message thread
                    let h = hwnd as isize;
                    std::thread::spawn(move || paste::hotkey_action(sh(), h as HWND, &action));
                }
            }
            0
        }
        WM_TRAY => {
            match lp as u32 {
                WM_LBUTTONUP => show_popup(false),
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
            show_popup(false);
            0
        }
        WM_CLEAR_CLIPBOARD => {
            if crate::winclip::empty(hwnd) {
                diag::info("the clipboard was cleared from the popup");
            }
            0
        }
        WM_MODE => {
            resize_popup(wp != 0);
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
    post_with(msg, 0);
}

fn post_with(msg: u32, wparam: usize) {
    let h = sh().msg_hwnd.load(Ordering::SeqCst) as HWND;
    if !h.is_null() {
        // SAFETY: posting to a window of this process.
        unsafe { PostMessageW(h, msg, wparam, 0) };
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
