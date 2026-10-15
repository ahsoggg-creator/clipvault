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
    let Some(p) = (match req.text {
        Some(text) => Some(Payload {
            text,
            ..Default::default()
        }),
        None => payload(sh, req.id),
    }) else {
        return;
    };

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

/// Pastes `text` into the window that has the focus *now* (a hotkey was pressed, no popup), then puts the
/// previous clipboard content back. Returns a message for the user when something went wrong.
pub fn paste_text_temporarily(sh: &Shared, msg_hwnd: HWND, text: &str) -> Result<(), String> {
    let lang = sh.lang();
    let target = winclip::foreground();
    if !winclip::self_elevated() && winclip::window_elevated(target) {
        diag::info("paste target runs elevated: UIPI blocks synthetic keys");
        return Err(S::ElevatedTarget.get(lang).to_owned());
    }
    // the hotkey's own modifiers are still down: wait until they are released or they would turn Ctrl+V into something else
    winclip::wait_modifiers_released(1500);
    let saved = winclip::snapshot(msg_hwnd);
    winclip::set_clipboard(
        msg_hwnd,
        &Payload {
            text: text.to_owned(),
            ..Default::default()
        },
    )?;
    std::thread::sleep(Duration::from_millis(40));
    let sent = winclip::send_ctrl_v();
    // give the target time to read the clipboard before it changes back
    std::thread::sleep(Duration::from_millis(450));
    if let Some(s) = saved {
        winclip::restore(msg_hwnd, &s);
    }
    if sent {
        Ok(())
    } else {
        Err(S::PasteFailed.get(lang).to_owned())
    }
}

/// What a global hotkey other than the popup's does. Runs on its own thread: it sleeps while the target reads.
pub fn hotkey_action(sh: &Shared, msg_hwnd: HWND, action: &HotkeyAction) {
    let lang = sh.lang();
    diag::info(format!("hotkey action: {action:?}"));
    let result = match action {
        HotkeyAction::PlainPaste => match winclip::read_text(msg_hwnd) {
            Some(t) if !t.is_empty() => paste_text_temporarily(sh, msg_hwnd, &t),
            _ => Ok(()),
        },
        HotkeyAction::Transform(t) => match winclip::read_text(msg_hwnd) {
            Some(text) => match t.apply(&text) {
                Ok(out) => paste_text_temporarily(sh, msg_hwnd, &out),
                Err(e) => Err(format!("{}: {e}", S::TransformFailed.get(lang))),
            },
            None => Ok(()),
        },
        HotkeyAction::Snippet(id) => {
            let template = sh
                .history
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .snippet(*id)
                .map(|s| s.text.clone());
            match template {
                Some(tpl) => {
                    let clip = winclip::read_text(msg_hwnd).unwrap_or_default();
                    let out = clipvault_core::snippets::expand(&tpl, &winclip::local_clock(), &clip);
                    paste_text_temporarily(sh, msg_hwnd, &out)
                }
                None => Ok(()),
            }
        }
    };
    if let Err(e) = result {
        diag::warn("a hotkey action failed");
        platform::notify(e);
    }
}

/// The non-popup hotkeys.
#[derive(Clone, Debug)]
pub enum HotkeyAction {
    PlainPaste,
    Transform(clipvault_core::transform::Transform),
    Snippet(u64),
}
