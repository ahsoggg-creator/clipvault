//! User settings (`settings.json`, no clipboard content in it) and where the data lives.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::guard::{SecretPolicy, default_blacklist};
use crate::history::Limits;
use crate::secrets::Sensitivity;

/// `CLIPVAULT_DATA_DIR` overrides the folder (tests, portable use); otherwise `%APPDATA%\Clipvault`.
pub fn data_dir() -> Option<PathBuf> {
    if let Some(d) = std::env::var_os("CLIPVAULT_DATA_DIR") {
        return Some(PathBuf::from(d));
    }
    if cfg!(windows) {
        std::env::var_os("APPDATA").map(|p| PathBuf::from(p).join("Clipvault"))
    } else {
        std::env::var_os("HOME").map(|p| PathBuf::from(p).join(".config").join("clipvault"))
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Lang {
    #[default]
    Ru,
    En,
}

/// Where the compact quick-paste window opens.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum CompactPosition {
    /// Under the mouse cursor, like the Windows clipboard history.
    #[default]
    Cursor,
    /// The middle of the screen the mouse is on.
    Center,
    /// The bottom-right corner of that screen, by the tray.
    Corner,
    /// Where the user last dragged it.
    Last,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub lang: Lang,
    /// Check GitHub for a new release once a day and offer to install it.
    pub auto_update: bool,
    /// Unix seconds of the last update check.
    pub last_update_check: u64,
    /// Global hotkey that opens the popup, e.g. `Ctrl+Alt+V`.
    pub hotkey: String,
    /// Hotkey of the elevated helper window (`Clipvault.exe --admin`).
    pub admin_hotkey: String,
    /// Recording is paused (survives restarts).
    pub paused: bool,
    /// Largest entry that is stored, in kilobytes.
    pub max_item_kb: u32,
    /// Entries unused for longer than this many days are removed (0 = keep forever).
    pub max_age_days: u32,
    /// Keep at most this many unpinned entries (0 = unlimited).
    pub max_items: u32,
    /// Look for secrets before anything is written.
    pub secrets_enabled: bool,
    /// What happens to a recognised secret.
    pub secret_policy: SecretPolicy,
    /// With `MemoryOnly`: wipe the clipboard after this many seconds (if it still holds the secret).
    pub secret_clear_secs: u32,
    /// How eagerly single strings are taken for passwords.
    pub password_sensitivity: Sensitivity,
    /// The user's own regular expressions for secrets.
    pub custom_patterns: Vec<String>,
    /// Obey `ExcludeClipboardContentFromMonitorProcessing`, `CanIncludeInClipboardHistory`, ...
    pub respect_app_flags: bool,
    /// Programs whose copies are never recorded (by executable name).
    pub blacklist: Vec<String>,
    /// Pastes the clipboard as plain text and puts the previous clipboard content back afterwards.
    pub plain_paste_hotkey: String,
    /// Hotkeys that transform the clipboard text and paste the result.
    pub transform_hotkeys: Vec<TransformHotkey>,
    /// Clipvault answers Win+V instead of the built-in Windows clipboard history.
    pub replace_win_v: bool,
    /// The popup opens next to the mouse cursor instead of the middle of the screen.
    pub popup_at_cursor: bool,
    /// The hotkey opens a small quick-paste window (click an entry to paste it); the tray opens the full window.
    pub compact_popup: bool,
    /// Where the compact window opens.
    pub compact_position: CompactPosition,
    /// Top-left corner (screen pixels) where the compact window was left after being dragged.
    pub compact_last: Option<(i32, i32)>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TransformHotkey {
    pub hotkey: String,
    /// A name from `Transform::name`, e.g. `json-format`.
    pub transform: String,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            lang: Lang::Ru,
            auto_update: true,
            last_update_check: 0,
            hotkey: "Ctrl+Alt+V".into(),
            admin_hotkey: "Ctrl+Alt+Shift+V".into(),
            paused: false,
            max_item_kb: 16 * 1024,
            max_age_days: 0,
            max_items: 0,
            secrets_enabled: true,
            secret_policy: SecretPolicy::MemoryOnly,
            secret_clear_secs: 30,
            password_sensitivity: Sensitivity::Medium,
            custom_patterns: Vec::new(),
            respect_app_flags: true,
            blacklist: default_blacklist(),
            plain_paste_hotkey: "Ctrl+Shift+V".into(),
            transform_hotkeys: vec![
                TransformHotkey {
                    hotkey: "Ctrl+Alt+J".into(),
                    transform: "json-format".into(),
                },
                TransformHotkey {
                    hotkey: "Ctrl+Alt+L".into(),
                    transform: "layout-auto".into(),
                },
            ],
            replace_win_v: false,
            popup_at_cursor: false,
            compact_popup: true,
            compact_position: CompactPosition::Cursor,
            compact_last: None,
        }
    }
}

impl Settings {
    pub fn limits(&self) -> Limits {
        Limits {
            max_item_bytes: u64::from(self.max_item_kb) * 1024,
            max_age_days: self.max_age_days,
            max_items: self.max_items,
        }
    }

    fn file() -> Option<PathBuf> {
        data_dir().map(|d| d.join("settings.json"))
    }

    pub fn load() -> Settings {
        Self::file()
            .and_then(|f| std::fs::read(f).ok())
            .and_then(|b| serde_json::from_slice(&b).ok())
            .unwrap_or_default()
    }

    /// Best effort: a settings file is a convenience, failing to write it is not worth an error dialog.
    pub fn save(&self) {
        let Some(f) = Self::file() else { return };
        if let Some(d) = f.parent() {
            let _ = std::fs::create_dir_all(d);
        }
        if let Ok(json) = serde_json::to_vec_pretty(self) {
            let tmp = f.with_extension("json.tmp");
            if std::fs::write(&tmp, json).is_ok() {
                let _ = std::fs::rename(&tmp, &f);
            }
        }
    }
}

/// Opens the real vault of this user: the key from `vault.key` (DPAPI-wrapped), the log in `vault.bin`.
pub fn open_history() -> crate::error::Result<crate::history::History> {
    let dir = data_dir().ok_or_else(|| crate::error::Error::other("не найдена папка данных"))?;
    let key = crate::crypto::load_or_create_key(&dir.join("vault.key"), &*crate::crypto::default_wrap())?;
    crate::history::History::open(&dir, &key)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn partial_json_gets_defaults() {
        let s: Settings = serde_json::from_str(r#"{"lang":"en","max_items":50}"#).unwrap();
        assert_eq!(s.lang, Lang::En);
        assert_eq!(s.max_items, 50);
        assert_eq!(s.hotkey, "Ctrl+Alt+V");
        assert!(s.auto_update);
        assert!(
            s.compact_popup,
            "a settings file from an older version gets the compact window"
        );
        assert_eq!(s.compact_position, CompactPosition::Cursor);
        let s: Settings = serde_json::from_str(r#"{"compact_position":"last","compact_last":[-1900,40]}"#).unwrap();
        assert_eq!(
            (s.compact_position, s.compact_last),
            (CompactPosition::Last, Some((-1900, 40)))
        );
    }
}
