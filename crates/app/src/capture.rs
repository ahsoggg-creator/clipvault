//! What happens when the clipboard changes: look, decide, record.

use std::time::{Duration, Instant};

use clipvault_core::clipfmt;
use clipvault_core::guard::{self, SecretPolicy, SkipReason};
use clipvault_core::{AddError, Added, BlobKind, Kind, NewItem, dedup, now_unix};
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
    let st = shared.settings.lock().unwrap_or_else(|e| e.into_inner()).clone();
    // the program's own "do not record" flags and the blacklist are checked before the content is even read
    if let Some(reason) = guard::skip_reason(&meta.flags, &meta.app, &st.blacklist, st.respect_app_flags) {
        let mut stats = shared.stats.lock().unwrap_or_else(|e| e.into_inner());
        match reason {
            SkipReason::AppFlag => stats.skipped_flag += 1,
            SkipReason::BlacklistedApp => stats.skipped_blacklist += 1,
        }
        return;
    }
    let Some(content) = winclip::read_content(hwnd) else {
        shared.stats.lock().unwrap_or_else(|e| e.into_inner()).unreadable += 1;
        diag::warn("clipboard stayed busy: an update was skipped");
        return;
    };
    let Some(mut item) = build_item(content, &meta) else {
        return;
    };

    if st.secrets_enabled
        && !matches!(item.kind, Some(Kind::Image | Kind::Files))
        && let Some(kind) = shared.detector(&st).detect(&item.text)
    {
        shared.stats.lock().unwrap_or_else(|e| e.into_inner()).secrets += 1;
        diag::info(format!("secret recognised: {kind:?}, policy {:?}", st.secret_policy));
        match st.secret_policy {
            SecretPolicy::DontStore => return,
            SecretPolicy::Mask => {
                item = NewItem {
                    kind: Some(Kind::Text),
                    text: guard::mask_text(kind, st.lang),
                    masked: true,
                    secret: Some(kind),
                    source_app: item.source_app,
                    source_title: String::new(),
                    ..Default::default()
                };
            }
            SecretPolicy::MemoryOnly => {
                let wipe = dedup::content_hash(Kind::Text, &item.text, &[]);
                item.secret = Some(kind);
                item.source_title = String::new();
                record(shared, item, true);
                let due = Instant::now() + Duration::from_secs(u64::from(st.secret_clear_secs));
                shared.erase.lock().unwrap_or_else(|e| e.into_inner()).push((due, wipe));
                return;
            }
        }
    }
    record(shared, item, false);
}

/// Wipes the clipboard where a memory-only secret is still sitting after its time ran out.
pub fn process_erasures(shared: &Shared, hwnd: HWND) {
    let now = Instant::now();
    let due: Vec<String> = {
        let mut list = shared.erase.lock().unwrap_or_else(|e| e.into_inner());
        let (due, keep): (Vec<_>, Vec<_>) = list.drain(..).partition(|(t, _)| *t <= now);
        *list = keep;
        due.into_iter().map(|(_, h)| h).collect()
    };
    for hash in due {
        if winclip::clear_if_text(hwnd, &hash) {
            diag::info("a memory-only secret was wiped from the clipboard");
        }
    }
}

fn record(shared: &Shared, item: NewItem, volatile: bool) {
    let limits = shared.settings.lock().unwrap_or_else(|e| e.into_inner()).limits();
    let result = {
        let mut h = shared.history.lock().unwrap_or_else(|e| e.into_inner());
        if volatile {
            h.add_volatile(item, now_unix(), &limits)
        } else {
            h.add(item, now_unix(), &limits)
        }
    };
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
