//! What happens when the clipboard changes: look, decide, record.

use clipvault_core::clipfmt;
use clipvault_core::{AddError, Added, BlobKind, Kind, NewItem, now_unix};
use windows_sys::Win32::Foundation::HWND;

use crate::diag;
use crate::shared::Shared;
use crate::winclip::{self, Content, Meta};

/// Turns what was on the clipboard into a history entry. Files win over text, text over a bare image.
pub fn build_item(c: Content, meta: &Meta) -> Option<NewItem> {
    let base = NewItem {
        source_app: meta.app.clone(),
        source_title: meta.title.clone(),
        ..Default::default()
    };
    if !c.files.is_empty() {
        return Some(NewItem {
            kind: Some(Kind::Files),
            text: c.files.join("\n"),
            ..base
        });
    }
    let text = c.text.filter(|t| !t.trim().is_empty());
    if let Some(text) = text {
        if let Some(html) = c.html {
            return Some(NewItem {
                kind: Some(Kind::Rich),
                text,
                blob: html.into_bytes(),
                blob_kind: BlobKind::Html,
                ..base
            });
        }
        if let Some(rtf) = c.rtf {
            return Some(NewItem {
                kind: Some(Kind::Rich),
                text,
                blob: rtf,
                blob_kind: BlobKind::Rtf,
                ..base
            });
        }
        return Some(NewItem { text, ..base });
    }
    let (png, dims) = match (c.png, c.dib) {
        (Some(png), _) => {
            let d = clipfmt::png_decode(&png).map(|i| (i.width, i.height));
            (png, d?)
        }
        (None, Some(dib)) => {
            let img = clipfmt::dib_to_rgba(&dib)?;
            (clipfmt::png_encode(&img)?, (img.width, img.height))
        }
        _ => return None,
    };
    Some(NewItem {
        kind: Some(Kind::Image),
        blob: png,
        blob_kind: BlobKind::Png,
        img: Some(dims),
        ..base
    })
}

pub fn on_clipboard_update(shared: &Shared, hwnd: HWND) {
    if shared.demo {
        return;
    }
    if !shared.recording_enabled() {
        shared.stats.lock().unwrap_or_else(|e| e.into_inner()).skipped_paused += 1;
        return;
    }
    // let the program that copied finish its own follow-up clipboard calls before we open it
    std::thread::sleep(std::time::Duration::from_millis(60));
    let Some(meta) = winclip::inspect(hwnd) else {
        shared.stats.lock().unwrap_or_else(|e| e.into_inner()).unreadable += 1;
        diag::warn("clipboard stayed busy: an update was skipped");
        return;
    };
    if meta.internal {
        return;
    }
    let Some(content) = winclip::read_content(hwnd) else {
        shared.stats.lock().unwrap_or_else(|e| e.into_inner()).unreadable += 1;
        diag::warn("clipboard stayed busy: an update was skipped");
        return;
    };
    let Some(item) = build_item(content, &meta) else { return };
    record(shared, item);
}

fn record(shared: &Shared, item: NewItem) {
    let limits = shared.settings.lock().unwrap_or_else(|e| e.into_inner()).limits();
    let result = shared
        .history
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .add(item, now_unix(), &limits);
    let mut st = shared.stats.lock().unwrap_or_else(|e| e.into_inner());
    match result {
        Ok(Added::New(_)) => st.captured += 1,
        Ok(Added::Bumped(_)) => st.repeated += 1,
        Err(AddError::Rejected(_)) => st.rejected += 1,
        Err(AddError::Store(e)) => {
            diag::error(format!("history write failed: {e}"));
            drop(st);
            shared.set_toast(format!("{}: {e}", shared.tr(clipvault_core::i18n::S::StoreError)));
            return;
        }
    }
    drop(st);
    shared.repaint();
}
