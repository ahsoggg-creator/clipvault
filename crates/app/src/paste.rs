//! Putting an entry on the clipboard and pasting it into the window that had the focus.

use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};

use clipvault_core::i18n::S;
use clipvault_core::{BlobKind, Kind};
use windows_sys::Win32::Foundation::HWND;
use windows_sys::Win32::UI::WindowsAndMessaging::SetForegroundWindow;

use crate::shared::Shared;
use crate::winclip::{self, Payload};
use crate::{diag, platform};

/// The clipboard form of an entry.
fn payload(sh: &Shared, id: u64) -> Option<Payload> {
    let h = sh.history.lock().unwrap_or_else(|e| e.into_inner());
    let it = h.get(id)?.clone();
    let blob = h.blob(id).ok().flatten();
    Some(match it.kind {
        Kind::Files => Payload {
            files: it.text.lines().map(str::to_owned).collect(),
            ..Default::default()
        },
        Kind::Image => Payload {
            png: blob,
            ..Default::default()
        },
        Kind::Rich => match it.blob_kind {
            BlobKind::Html => Payload {
                text: it.text,
                html: blob.and_then(|b| String::from_utf8(b).ok()),
                ..Default::default()
            },
            BlobKind::Rtf => Payload {
                text: it.text,
                rtf: blob,
                ..Default::default()
            },
            _ => Payload {
                text: it.text,
                ..Default::default()
            },
        },
        _ => Payload {
            text: it.text,
            ..Default::default()
        },
    })
}

/// Handles a paste request from the popup (runs on the Windows thread).
pub fn run(sh: &Shared, msg_hwnd: HWND) {
    let Some(req) = sh.paste.lock().unwrap_or_else(|e| e.into_inner()).take() else {
        return;
    };
    let lang = sh.lang();
    platform::hide_popup();
    let target = sh.prev_foreground.load(Ordering::SeqCst) as HWND;
    let Some(p) = payload(sh, req.id) else { return };

    if winclip::window_alive(target) {
        // SAFETY: plain handle.
        unsafe { SetForegroundWindow(target) };
        let deadline = Instant::now() + Duration::from_millis(600);
        while winclip::foreground() != target && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(15));
        }
    }
    if let Err(e) = winclip::set_clipboard(msg_hwnd, &p) {
        diag::warn("could not set the clipboard for a paste");
        platform::notify(format!("{}: {e}", S::PasteFailed.get(lang)));
        return;
    }
    if !winclip::window_alive(target) {
        return; // nothing to paste into: the entry is on the clipboard
    }
    if !winclip::self_elevated() && winclip::window_elevated(target) {
        diag::info("paste target runs elevated: UIPI blocks synthetic keys");
        platform::notify(S::ElevatedTarget.get(lang));
        return;
    }
    winclip::wait_modifiers_released(400);
    std::thread::sleep(Duration::from_millis(30));
    if !winclip::send_ctrl_v() {
        diag::warn("SendInput was refused");
        platform::notify(S::PasteFailed.get(lang));
    }
}
