//! The Windows clipboard: reading what was copied, putting an entry back and pasting it into another window.
//! Everything here talks to the real system; it is exercised by `scripts/e2e.ps1`, not by unit tests.

use std::sync::OnceLock;
use std::time::Duration;

use clipvault_core::clipfmt;
use clipvault_core::guard::{ClipFlags, dword_value};
use windows_sys::Win32::Foundation::{CloseHandle, HANDLE, HWND};
use windows_sys::Win32::Security::{GetTokenInformation, TOKEN_ELEVATION, TOKEN_QUERY, TokenElevation};
use windows_sys::Win32::System::DataExchange::{
    CloseClipboard, EmptyClipboard, EnumClipboardFormats, GetClipboardData, GetClipboardOwner,
    IsClipboardFormatAvailable, OpenClipboard, RegisterClipboardFormatW, SetClipboardData,
};
use windows_sys::Win32::System::Memory::{GMEM_MOVEABLE, GlobalAlloc, GlobalLock, GlobalSize, GlobalUnlock};
use windows_sys::Win32::System::Ole::{CF_DIB, CF_HDROP, CF_UNICODETEXT};
use windows_sys::Win32::System::Threading::{
    GetCurrentProcess, OpenProcess, OpenProcessToken, PROCESS_QUERY_LIMITED_INFORMATION, QueryFullProcessImageNameW,
};
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
    GetAsyncKeyState, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYEVENTF_EXTENDEDKEY, KEYEVENTF_KEYUP, SendInput,
    VK_CONTROL, VK_INSERT, VK_LWIN, VK_MENU, VK_RWIN, VK_SHIFT,
};
use windows_sys::Win32::UI::Shell::DragQueryFileW;
use windows_sys::Win32::UI::WindowsAndMessaging::{
    GetForegroundWindow, GetWindowTextW, GetWindowThreadProcessId, IsWindow,
};

pub fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

struct Formats {
    html: u32,
    rtf: u32,
    png: u32,
    marker: u32,
    exclude_monitor: u32,
    can_include_history: u32,
    can_upload_cloud: u32,
    viewer_ignore: u32,
}

fn formats() -> &'static Formats {
    static F: OnceLock<Formats> = OnceLock::new();
    F.get_or_init(|| {
        let reg = |n: &str| {
            let w = wide(n);
            // SAFETY: `w` is NUL-terminated and outlives the call.
            unsafe { RegisterClipboardFormatW(w.as_ptr()) }
        };
        Formats {
            html: reg("HTML Format"),
            rtf: reg("Rich Text Format"),
            png: reg("PNG"),
            marker: reg("ClipvaultInternal"),
            exclude_monitor: reg("ExcludeClipboardContentFromMonitorProcessing"),
            can_include_history: reg("CanIncludeInClipboardHistory"),
            can_upload_cloud: reg("CanUploadToCloudClipboard"),
            viewer_ignore: reg("Clipboard Viewer Ignore"),
        }
    })
}

/// Holds the clipboard open; closes it on drop.
pub struct Clipboard(());

impl Clipboard {
    pub fn open(owner: HWND) -> Option<Clipboard> {
        for _ in 0..12 {
            // SAFETY: plain Win32 call; `owner` is a window of this thread or null.
            if unsafe { OpenClipboard(owner) } != 0 {
                return Some(Clipboard(()));
            }
            std::thread::sleep(Duration::from_millis(25));
        }
        None
    }
}

impl Drop for Clipboard {
    fn drop(&mut self) {
        // SAFETY: the clipboard is open in this thread (guaranteed by construction).
        unsafe { CloseClipboard() };
    }
}

fn available(fmt: u32) -> bool {
    // SAFETY: no pointers involved.
    unsafe { IsClipboardFormatAvailable(fmt) != 0 }
}

/// Copies the bytes of the global memory block behind clipboard format `fmt`.
fn global_bytes(fmt: u32) -> Option<Vec<u8>> {
    // SAFETY: the clipboard is open (caller holds a `Clipboard`); the handle stays owned by the system, the
    // block is locked for the copy and unlocked afterwards.
    unsafe {
        let h: HANDLE = GetClipboardData(fmt);
        if h.is_null() {
            return None;
        }
        let size = GlobalSize(h);
        let p = GlobalLock(h);
        if p.is_null() {
            return None;
        }
        let v = std::slice::from_raw_parts(p as *const u8, size).to_vec();
        GlobalUnlock(h);
        Some(v)
    }
}

fn wide_text(bytes: &[u8]) -> String {
    let units: Vec<u16> = bytes
        .as_chunks::<2>()
        .0
        .iter()
        .map(|c| u16::from_le_bytes(*c))
        .take_while(|&u| u != 0)
        .collect();
    String::from_utf16_lossy(&units)
}

/// Who copied, and the markers that decide whether it is recorded at all.
pub struct Meta {
    /// `chrome.exe` — the clipboard owner's process (the foreground window's if there is no owner).
    pub app: String,
    pub title: String,
    /// Put there by Clipvault itself when pasting.
    pub internal: bool,
    /// The "do not record me" formats the copying program set.
    pub flags: ClipFlags,
}

/// The raw content of the clipboard in the formats Clipvault stores.
#[derive(Default)]
pub struct Content {
    pub text: Option<String>,
    pub html: Option<String>,
    pub rtf: Option<Vec<u8>>,
    pub png: Option<Vec<u8>>,
    pub dib: Option<Vec<u8>>,
    pub files: Vec<String>,
}

pub fn process_exe(pid: u32) -> Option<String> {
    // SAFETY: the handle is closed below; the buffer length is passed and updated by the call.
    unsafe {
        let h = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
        if h.is_null() {
            return None;
        }
        let mut buf = vec![0u16; 1024];
        let mut len = buf.len() as u32;
        let ok = QueryFullProcessImageNameW(h, 0, buf.as_mut_ptr(), &mut len);
        CloseHandle(h);
        if ok == 0 {
            return None;
        }
        let path = String::from_utf16_lossy(&buf[..len as usize]);
        path.rsplit(['\\', '/']).next().map(str::to_owned)
    }
}

fn window_pid(hwnd: HWND) -> Option<u32> {
    if hwnd.is_null() {
        return None;
    }
    let mut pid = 0u32;
    // SAFETY: `pid` is a live local; a stale window handle just yields 0.
    unsafe { GetWindowThreadProcessId(hwnd, &mut pid) };
    (pid != 0).then_some(pid)
}

pub fn window_title(hwnd: HWND) -> String {
    if hwnd.is_null() {
        return String::new();
    }
    let mut buf = [0u16; 256];
    // SAFETY: the buffer length is passed.
    let n = unsafe { GetWindowTextW(hwnd, buf.as_mut_ptr(), buf.len() as i32) };
    String::from_utf16_lossy(&buf[..n.max(0) as usize])
}

/// Looks at who owns the clipboard and at the markers, without reading any content.
pub fn inspect(owner: HWND) -> Option<Meta> {
    let _clip = Clipboard::open(owner)?;
    // SAFETY: no pointers involved.
    let (clip_owner, fg) = unsafe { (GetClipboardOwner(), GetForegroundWindow()) };
    let app = window_pid(clip_owner)
        .or_else(|| window_pid(fg))
        .and_then(process_exe)
        .unwrap_or_default();
    let f = formats();
    // a DWORD format that is present and 0 means "no"
    let denied = |fmt: u32| available(fmt) && global_bytes(fmt).and_then(|b| dword_value(&b)) == Some(0);
    let flags = ClipFlags {
        exclude_monitor: available(f.exclude_monitor),
        history_denied: denied(f.can_include_history),
        cloud_denied: denied(f.can_upload_cloud),
        viewer_ignore: available(f.viewer_ignore),
    };
    Some(Meta {
        app,
        title: window_title(fg),
        internal: available(f.marker),
        flags,
    })
}

/// Reads the content. Call only after `inspect` said the entry may be recorded.
pub fn read_content(owner: HWND) -> Option<Content> {
    let f = formats();
    let _clip = Clipboard::open(owner)?;
    let mut c = Content::default();
    if available(CF_UNICODETEXT.into()) {
        c.text = global_bytes(CF_UNICODETEXT.into()).map(|b| wide_text(&b));
    }
    if available(f.html) {
        c.html = global_bytes(f.html).and_then(|b| clipfmt::html_fragment(&b));
    }
    if available(f.rtf) {
        c.rtf = global_bytes(f.rtf).map(|mut b| {
            while b.last() == Some(&0) {
                b.pop();
            }
            b
        });
    }
    if available(f.png) {
        c.png = global_bytes(f.png);
    } else if available(CF_DIB.into()) {
        c.dib = global_bytes(CF_DIB.into());
    }
    if available(CF_HDROP.into()) {
        c.files = hdrop_files();
    }
    Some(c)
}

/// Everything that was on the clipboard (the formats that live in global memory), to put back later.
pub struct Snapshot(Vec<(u32, Vec<u8>)>);

/// Copies the clipboard's memory-backed formats. Bitmap handles are skipped: Windows makes them from the DIB.
pub fn snapshot(owner: HWND) -> Option<Snapshot> {
    let _clip = Clipboard::open(owner)?;
    let mut out = Vec::new();
    let mut fmt = 0u32;
    loop {
        // SAFETY: the clipboard is open.
        fmt = unsafe { EnumClipboardFormats(fmt) };
        if fmt == 0 {
            break;
        }
        let handle_only =
            matches!(fmt, 2 | 3 | 9 | 14) || (0x80..=0x8F).contains(&fmt) || (0x200..=0x3FF).contains(&fmt);
        if handle_only {
            continue;
        }
        if let Some(bytes) = global_bytes(fmt) {
            out.push((fmt, bytes));
        }
    }
    Some(Snapshot(out))
}

/// Puts a snapshot back (with the internal marker, so Clipvault does not record the restoration).
pub fn restore(owner: HWND, snap: &Snapshot) -> bool {
    let Some(_clip) = Clipboard::open(owner) else {
        return false;
    };
    // SAFETY: the clipboard is open.
    unsafe { EmptyClipboard() };
    let mut ok = true;
    for (fmt, bytes) in &snap.0 {
        ok &= put(*fmt, bytes);
    }
    put(formats().marker, &[1]);
    ok
}

/// The plain text on the clipboard right now.
pub fn read_text(owner: HWND) -> Option<String> {
    let _clip = Clipboard::open(owner)?;
    if !available(CF_UNICODETEXT.into()) {
        return None;
    }
    global_bytes(CF_UNICODETEXT.into()).map(|b| wide_text(&b))
}

/// Local date and time for the snippet placeholders.
pub fn local_clock() -> clipvault_core::snippets::Clock {
    use windows_sys::Win32::System::SystemInformation::GetLocalTime;
    // SAFETY: SYSTEMTIME is plain data filled by the call.
    let t = unsafe {
        let mut t = std::mem::zeroed();
        GetLocalTime(&mut t);
        t
    };
    clipvault_core::snippets::Clock {
        year: i32::from(t.wYear),
        month: u32::from(t.wMonth),
        day: u32::from(t.wDay),
        hour: u32::from(t.wHour),
        minute: u32::from(t.wMinute),
    }
}

/// Empties the clipboard if it still holds exactly this plain text (compared by its BLAKE3 hash).
pub fn clear_if_text(owner: HWND, hash: &str) -> bool {
    let Some(_clip) = Clipboard::open(owner) else {
        return false;
    };
    if !available(CF_UNICODETEXT.into()) {
        return false;
    }
    let Some(text) = global_bytes(CF_UNICODETEXT.into()).map(|b| wide_text(&b)) else {
        return false;
    };
    if clipvault_core::dedup::content_hash(clipvault_core::Kind::Text, &text, &[]) != hash {
        return false;
    }
    // SAFETY: the clipboard is open.
    unsafe { EmptyClipboard() };
    true
}

/// Empties the clipboard whatever it holds.
pub fn empty(owner: HWND) -> bool {
    let Some(_clip) = Clipboard::open(owner) else {
        return false;
    };
    // SAFETY: the clipboard is open.
    unsafe { EmptyClipboard() != 0 }
}

fn hdrop_files() -> Vec<String> {
    // SAFETY: the clipboard is open; CF_HDROP data is a valid HDROP while it is.
    unsafe {
        let h = GetClipboardData(CF_HDROP.into());
        if h.is_null() {
            return Vec::new();
        }
        let count = DragQueryFileW(h, u32::MAX, std::ptr::null_mut(), 0);
        (0..count)
            .map(|i| {
                let len = DragQueryFileW(h, i, std::ptr::null_mut(), 0);
                let mut buf = vec![0u16; len as usize + 1];
                DragQueryFileW(h, i, buf.as_mut_ptr(), buf.len() as u32);
                String::from_utf16_lossy(&buf[..len as usize])
            })
            .collect()
    }
}

/// What to put on the clipboard.
#[derive(Default)]
pub struct Payload {
    pub text: String,
    /// HTML fragment (rebuilt into `CF_HTML`).
    pub html: Option<String>,
    pub rtf: Option<Vec<u8>>,
    pub png: Option<Vec<u8>>,
    pub files: Vec<String>,
}

fn put(fmt: u32, bytes: &[u8]) -> bool {
    // SAFETY: the clipboard is open and emptied by the caller; on success the system owns the block.
    unsafe {
        let h = GlobalAlloc(GMEM_MOVEABLE, bytes.len().max(1));
        if h.is_null() {
            return false;
        }
        let p = GlobalLock(h);
        if p.is_null() {
            return false;
        }
        std::ptr::copy_nonoverlapping(bytes.as_ptr(), p as *mut u8, bytes.len());
        GlobalUnlock(h);
        !SetClipboardData(fmt, h).is_null()
    }
}

/// Replaces the clipboard with `p` (and the internal marker, so Clipvault does not record its own paste).
pub fn set_clipboard(owner: HWND, p: &Payload) -> Result<(), String> {
    let f = formats();
    let _clip = Clipboard::open(owner).ok_or("буфер обмена занят другим приложением")?;
    // SAFETY: the clipboard is open.
    unsafe { EmptyClipboard() };
    let mut ok = true;
    if !p.text.is_empty() {
        let bytes: Vec<u8> = wide(&p.text).iter().flat_map(|u| u.to_le_bytes()).collect();
        ok &= put(CF_UNICODETEXT.into(), &bytes);
    }
    if let Some(h) = &p.html {
        ok &= put(f.html, &clipfmt::build_cf_html(h));
    }
    if let Some(r) = &p.rtf {
        let mut b = r.clone();
        b.push(0);
        ok &= put(f.rtf, &b);
    }
    if let Some(png) = &p.png {
        ok &= put(f.png, png);
        if let Some(img) = clipfmt::png_decode(png) {
            ok &= put(CF_DIB.into(), &clipfmt::rgba_to_dib(&img));
        }
    }
    if !p.files.is_empty() {
        let mut data = Vec::new();
        data.extend_from_slice(&20u32.to_le_bytes()); // pFiles
        data.extend_from_slice(&[0u8; 12]); // pt (8) + fNC (4)
        data.extend_from_slice(&1u32.to_le_bytes()); // fWide
        for file in &p.files {
            data.extend(wide(file).iter().flat_map(|u| u.to_le_bytes()));
        }
        data.extend_from_slice(&[0, 0]);
        ok &= put(CF_HDROP.into(), &data);
    }
    put(f.marker, &[1]);
    if ok {
        Ok(())
    } else {
        Err("не удалось записать данные в буфер обмена".into())
    }
}

fn key_input(vk: u16, up: bool) -> INPUT {
    key_input_flags(vk, up, 0)
}

fn key_input_flags(vk: u16, up: bool, extra: u32) -> INPUT {
    INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: vk,
                wScan: 0,
                dwFlags: extra | if up { KEYEVENTF_KEYUP } else { 0 },
                time: 0,
                dwExtraInfo: 0,
            },
        },
    }
}

/// Waits (up to `ms`) until the user has let go of the modifier keys, so they do not mix into the paste.
pub fn wait_modifiers_released(ms: u64) {
    let down = |vk: u16| {
        // SAFETY: no pointers involved.
        unsafe { GetAsyncKeyState(i32::from(vk)) < 0 }
    };
    let deadline = std::time::Instant::now() + Duration::from_millis(ms);
    while std::time::Instant::now() < deadline
        && [VK_CONTROL, VK_MENU, VK_SHIFT, VK_LWIN, VK_RWIN].into_iter().any(down)
    {
        std::thread::sleep(Duration::from_millis(10));
    }
}

/// Types Ctrl+V into the foreground window. Returns false if Windows refused the input.
pub fn send_paste(shift_insert: bool) -> bool {
    let inputs = if shift_insert {
        [
            key_input(VK_SHIFT, false),
            key_input_flags(VK_INSERT, false, KEYEVENTF_EXTENDEDKEY),
            key_input_flags(VK_INSERT, true, KEYEVENTF_EXTENDEDKEY),
            key_input(VK_SHIFT, true),
        ]
    } else {
        [
            key_input(VK_CONTROL, false),
            key_input(u16::from(b'V'), false),
            key_input(u16::from(b'V'), true),
            key_input(VK_CONTROL, true),
        ]
    };
    // SAFETY: `inputs` is a valid array of INPUT structures of the stated size.
    let sent = unsafe {
        SendInput(
            inputs.len() as u32,
            inputs.as_ptr(),
            std::mem::size_of::<INPUT>() as i32,
        )
    };
    sent == inputs.len() as u32
}

pub fn foreground() -> HWND {
    // SAFETY: no pointers involved.
    unsafe { GetForegroundWindow() }
}

pub fn window_alive(hwnd: HWND) -> bool {
    // SAFETY: no pointers involved.
    !hwnd.is_null() && unsafe { IsWindow(hwnd) != 0 }
}

fn elevated(process: HANDLE) -> Option<bool> {
    // SAFETY: the token handle is closed; the output buffer matches the requested class.
    unsafe {
        let mut token: HANDLE = std::ptr::null_mut();
        if OpenProcessToken(process, TOKEN_QUERY, &mut token) == 0 {
            return None;
        }
        let mut e = TOKEN_ELEVATION { TokenIsElevated: 0 };
        let mut ret = 0u32;
        let ok = GetTokenInformation(
            token,
            TokenElevation,
            &mut e as *mut _ as *mut _,
            std::mem::size_of::<TOKEN_ELEVATION>() as u32,
            &mut ret,
        );
        CloseHandle(token);
        (ok != 0).then_some(e.TokenIsElevated != 0)
    }
}

pub fn self_elevated() -> bool {
    // SAFETY: the pseudo-handle of the current process needs no closing.
    elevated(unsafe { GetCurrentProcess() }).unwrap_or(false)
}

/// Is the process that owns `hwnd` running with administrator rights? Unknown counts as "no" unless the
/// process cannot even be opened (a protected / elevated one).
pub fn window_elevated(hwnd: HWND) -> bool {
    let Some(pid) = window_pid(hwnd) else { return false };
    // SAFETY: the handle is closed below.
    unsafe {
        let h = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
        if h.is_null() {
            return true;
        }
        let r = elevated(h).unwrap_or(true);
        CloseHandle(h);
        r
    }
}
