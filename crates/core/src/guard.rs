//! Decisions taken *before* a copied entry is read or stored: the apps' own "do not record" flags, the process
//! blacklist, what to do with a secret, and how a masked secret looks.

use serde::{Deserialize, Serialize};

use crate::config::Lang;
use crate::i18n::S;
use crate::model::SecretKind;

/// The clipboard formats password managers set (Windows' own "do not keep this" protocol).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ClipFlags {
    /// `ExcludeClipboardContentFromMonitorProcessing` is present.
    pub exclude_monitor: bool,
    /// `CanIncludeInClipboardHistory` is present and equals 0.
    pub history_denied: bool,
    /// `CanUploadToCloudClipboard` is present and equals 0.
    pub cloud_denied: bool,
    /// `Clipboard Viewer Ignore` is present.
    pub viewer_ignore: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SkipReason {
    /// The program that copied asked not to be recorded.
    AppFlag,
    /// The owner process is on the blacklist.
    BlacklistedApp,
}

pub fn default_blacklist() -> Vec<String> {
    [
        "KeePass.exe",
        "KeePassXC.exe",
        "Bitwarden.exe",
        "1Password.exe",
        "Dashlane.exe",
    ]
    .into_iter()
    .map(str::to_owned)
    .collect()
}

fn same_exe(a: &str, b: &str) -> bool {
    let norm = |s: &str| {
        let s = s.trim().to_lowercase();
        s.strip_suffix(".exe").unwrap_or(&s).to_owned()
    };
    !a.trim().is_empty() && norm(a) == norm(b)
}

/// Should this copy be ignored without even reading its content?
pub fn skip_reason(
    flags: &ClipFlags,
    owner_app: &str,
    blacklist: &[String],
    respect_flags: bool,
) -> Option<SkipReason> {
    if respect_flags && (flags.exclude_monitor || flags.history_denied || flags.cloud_denied || flags.viewer_ignore) {
        return Some(SkipReason::AppFlag);
    }
    blacklist
        .iter()
        .any(|b| same_exe(b, owner_app))
        .then_some(SkipReason::BlacklistedApp)
}

/// The value of a `DWORD` clipboard format (the flag formats carry 0 or 1).
pub fn dword_value(bytes: &[u8]) -> Option<u32> {
    bytes.get(..4).map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
}

/// What to do when the filter recognises a secret.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SecretPolicy {
    /// Do not record it at all.
    DontStore,
    /// Keep it in memory only and wipe it from the clipboard after N seconds.
    #[default]
    MemoryOnly,
    /// Store a masked placeholder instead of the content.
    Mask,
}

pub fn secret_label(kind: SecretKind, lang: Lang) -> &'static str {
    match kind {
        SecretKind::ApiKey => S::SecApiKey,
        SecretKind::Token => S::SecToken,
        SecretKind::PrivateKey => S::SecPrivateKey,
        SecretKind::ConnectionString => S::SecConnection,
        SecretKind::SeedPhrase => S::SecSeed,
        SecretKind::Card => S::SecCard,
        SecretKind::Password => S::SecPassword,
        SecretKind::Custom => S::SecCustom,
    }
    .get(lang)
}

/// `•••••••• [API key]` — the same for every secret of that kind, so it reveals neither length nor content.
pub fn mask_text(kind: SecretKind, lang: Lang) -> String {
    format!("•••••••• [{}]", secret_label(kind, lang))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flags_and_blacklist() {
        let bl = default_blacklist();
        let none = ClipFlags::default();
        assert_eq!(skip_reason(&none, "chrome.exe", &bl, true), None);
        for f in [
            ClipFlags {
                exclude_monitor: true,
                ..none
            },
            ClipFlags {
                history_denied: true,
                ..none
            },
            ClipFlags {
                cloud_denied: true,
                ..none
            },
            ClipFlags {
                viewer_ignore: true,
                ..none
            },
        ] {
            assert_eq!(skip_reason(&f, "chrome.exe", &bl, true), Some(SkipReason::AppFlag));
            assert_eq!(
                skip_reason(&f, "chrome.exe", &bl, false),
                None,
                "flags can be switched off"
            );
        }
        for app in [
            "KeePass.exe",
            "keepassxc.EXE",
            "Bitwarden",
            "1password.exe",
            "Dashlane.exe",
        ] {
            assert_eq!(
                skip_reason(&none, app, &bl, true),
                Some(SkipReason::BlacklistedApp),
                "{app}"
            );
        }
        assert_eq!(skip_reason(&none, "", &bl, true), None);
        assert_eq!(skip_reason(&none, "notepad.exe", &bl, true), None);
        assert_eq!(skip_reason(&none, "KeePass.exe", &[], true), None);
    }

    #[test]
    fn dword_flags() {
        assert_eq!(dword_value(&[0, 0, 0, 0]), Some(0));
        assert_eq!(dword_value(&[1, 0, 0, 0, 9]), Some(1));
        assert_eq!(dword_value(&[1]), None);
    }

    #[test]
    fn mask_hides_everything() {
        let m = mask_text(SecretKind::ApiKey, Lang::En);
        assert_eq!(m, "•••••••• [API key]");
        assert_ne!(mask_text(SecretKind::Card, Lang::Ru), m);
    }
}
