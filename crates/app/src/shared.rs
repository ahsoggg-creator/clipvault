//! State shared by the GUI thread and the Windows thread (clipboard listener, hotkeys, tray).

use std::sync::atomic::{AtomicBool, AtomicIsize, AtomicU64};
use std::sync::{Mutex, OnceLock};

use std::sync::Arc;
use std::time::Instant;

use clipvault_core::config::{Lang, Settings};
use clipvault_core::i18n::S;
use clipvault_core::secrets::{Detector, Sensitivity};
use clipvault_core::{History, Kind};
use eframe::egui;

/// Counters for the diagnostics: types and numbers only, never content.
#[derive(Default, Clone)]
pub struct Stats {
    pub captured: u64,
    pub repeated: u64,
    pub skipped_paused: u64,
    pub rejected: u64,
    pub unreadable: u64,
    pub skipped_flag: u64,
    pub skipped_blacklist: u64,
    pub secrets: u64,
}

pub struct PasteRequest {
    pub id: u64,
    /// Paste this text instead of the entry (a transformed entry, an expanded snippet).
    pub text: Option<String>,
}

pub struct Shared {
    pub history: Mutex<History>,
    pub settings: Mutex<Settings>,
    pub incognito: AtomicBool,
    pub demo: bool,
    /// The separate elevated instance (`--admin`): no clipboard capture, its own hotkey, pastes into admin windows.
    pub admin: bool,
    pub stats: Mutex<Stats>,
    /// The eframe window of the popup (an `HWND` as an integer).
    pub popup_hwnd: AtomicIsize,
    /// The window that had the focus when the popup opened: the paste target.
    pub prev_foreground: AtomicIsize,
    /// Bumped every time the popup is shown; the UI resets its search when it changes.
    pub show_seq: AtomicU64,
    pub open_settings: AtomicBool,
    pub paste: Mutex<Option<PasteRequest>>,
    pub toast: Mutex<Option<(String, std::time::Instant)>>,
    pub hotkey_error: Mutex<Option<String>>,
    pub ctx: OnceLock<egui::Context>,
    /// The Windows thread's hidden window, so the GUI can post messages to it.
    pub msg_hwnd: AtomicIsize,
    pub quit: AtomicBool,
    /// Secrets kept by the "memory only" policy: wipe the clipboard at `Instant` if it still holds this text (hash).
    pub erase: Mutex<Vec<(Instant, String)>>,
    detector: Mutex<Option<(DetectorKey, Arc<Detector>)>>,
}

type DetectorKey = (Sensitivity, Vec<String>);

impl Shared {
    pub fn new(history: History, settings: Settings, demo: bool, admin: bool) -> Shared {
        Shared {
            history: Mutex::new(history),
            settings: Mutex::new(settings),
            incognito: AtomicBool::new(false),
            demo,
            admin,
            stats: Mutex::new(Stats::default()),
            popup_hwnd: AtomicIsize::new(0),
            prev_foreground: AtomicIsize::new(0),
            show_seq: AtomicU64::new(0),
            open_settings: AtomicBool::new(false),
            paste: Mutex::new(None),
            toast: Mutex::new(None),
            hotkey_error: Mutex::new(None),
            ctx: OnceLock::new(),
            msg_hwnd: AtomicIsize::new(0),
            quit: AtomicBool::new(false),
            erase: Mutex::new(Vec::new()),
            detector: Mutex::new(None),
        }
    }

    /// The secret detector for the current settings (rebuilt only when they change).
    pub fn detector(&self, st: &Settings) -> Arc<Detector> {
        let key: DetectorKey = (st.password_sensitivity, st.custom_patterns.clone());
        let mut slot = self.detector.lock().unwrap_or_else(|e| e.into_inner());
        if let Some((k, d)) = slot.as_ref()
            && *k == key
        {
            return d.clone();
        }
        let d = Arc::new(Detector::new(key.0, &key.1).0);
        *slot = Some((key, d.clone()));
        d
    }

    pub fn lang(&self) -> Lang {
        self.settings.lock().unwrap_or_else(|e| e.into_inner()).lang
    }

    pub fn tr(&self, s: S) -> &'static str {
        s.get(self.lang())
    }

    pub fn repaint(&self) {
        if let Some(c) = self.ctx.get() {
            c.request_repaint();
        }
    }

    pub fn set_toast(&self, text: impl Into<String>) {
        *self.toast.lock().unwrap_or_else(|e| e.into_inner()) = Some((text.into(), std::time::Instant::now()));
        self.repaint();
    }

    pub fn kind_label(&self, k: Kind) -> &'static str {
        self.tr(match k {
            Kind::Text => S::KindText,
            Kind::Rich => S::KindRich,
            Kind::Image => S::KindImage,
            Kind::Files => S::KindFiles,
            Kind::Color => S::KindColor,
            Kind::Link => S::KindLink,
            Kind::Path => S::KindPath,
        })
    }

    pub fn recording_enabled(&self) -> bool {
        !self.settings.lock().unwrap_or_else(|e| e.into_inner()).paused
            && !self.incognito.load(std::sync::atomic::Ordering::Relaxed)
    }
}
