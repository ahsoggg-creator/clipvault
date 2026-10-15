//! The settings view inside the popup window.

use std::time::{Duration, Instant};

use clipvault_core::config::Lang;
use clipvault_core::i18n::S;
use eframe::egui::{self, RichText, Ui};

use crate::{App, diag, platform, update, winint};

/// Writes the settings and re-registers the global hotkeys, but only when something changed.
fn drop_and_apply(st: std::sync::MutexGuard<'_, clipvault_core::config::Settings>, changed: bool) {
    if changed {
        st.save();
    }
    drop(st);
    if changed {
        platform::post(platform::WM_REHOTKEY);
    }
}

/// A button that shows a hotkey and records a new one when it is clicked: the next combination pressed on the
/// physical keyboard (modifiers + a key) is taken, Esc cancels and, when `clearable`, Backspace empties the field.
/// Returns true when the value changed.
pub(crate) fn hotkey_field(
    ui: &mut Ui,
    capturing: &mut Option<egui::Id>,
    salt: impl std::hash::Hash + std::fmt::Debug,
    value: &mut String,
    clearable: bool,
    lang: Lang,
) -> bool {
    let me = ui.make_persistent_id(salt);
    let mut changed = false;
    if *capturing == Some(me) {
        let focused = ui.ctx().input(|i| i.viewport().focused.unwrap_or(true));
        let seen = if focused {
            platform::poll_capture()
        } else {
            platform::Capture::Cancel
        };
        let done = match seen {
            platform::Capture::Got(spec) => {
                *value = spec;
                changed = true;
                true
            }
            platform::Capture::Clear if clearable => {
                value.clear();
                changed = true;
                true
            }
            platform::Capture::Cancel => true,
            _ => false,
        };
        if done {
            *capturing = None;
            platform::capture_end();
        } else {
            let hint = if clearable { S::PressKeysClear } else { S::PressKeys };
            ui.label(RichText::new(hint.get(lang)).color(crate::theme::ACCENT));
            if ui.button(S::Cancel.get(lang)).clicked() {
                *capturing = None;
                platform::capture_end();
            }
            // nothing else would ask for a frame while the keys are being pressed
            ui.ctx().request_repaint_after(Duration::from_millis(30));
        }
    }
    if *capturing != Some(me) {
        let shown = if value.trim().is_empty() {
            S::HotkeyNotSet.get(lang).to_owned()
        } else {
            value.clone()
        };
        if ui
            .button(RichText::new(shown).monospace().strong())
            .on_hover_text(S::RecordHotkey.get(lang))
            .clicked()
        {
            *capturing = Some(me);
            platform::capture_begin();
        }
        if clearable && !value.trim().is_empty() && ui.small_button("×").on_hover_text(S::Remove.get(lang)).clicked() {
            value.clear();
            changed = true;
        }
    }
    changed
}

impl App {
    pub fn settings_ui(&mut self, ui: &mut Ui) {
        let lang = self.shared.lang();
        egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
            ui.heading(S::SettingsTitle.get(lang));
            ui.add_space(6.0);
            self.sec_general(ui, lang);
            ui.add_space(8.0);
            self.sec_hotkeys(ui, lang);
            ui.add_space(8.0);
            self.sec_secrets(ui, lang);
            ui.add_space(8.0);
            self.sec_limits(ui, lang);
            ui.add_space(8.0);
            if !self.shared.demo {
                self.sec_admin(ui, lang);
                ui.add_space(8.0);
            }
            self.sec_updates(ui, lang);
            ui.add_space(8.0);
            self.sec_diag(ui, lang);
            if !self.shared.demo {
                ui.add_space(8.0);
                self.sec_install(ui, lang);
            }
        });
    }

    fn sec_general(&mut self, ui: &mut Ui, lang: Lang) {
        crate::theme::card().show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.label(RichText::new(S::SecGeneral.get(lang)).strong());
            let mut st = self.shared.settings.lock().unwrap_or_else(|e| e.into_inner());
            let mut reregister = false;
            ui.horizontal(|ui| {
                ui.label(S::Language.get(lang));
                let before = st.lang;
                egui::ComboBox::from_id_salt("lang")
                    .selected_text(if st.lang == Lang::Ru {
                        "Русский"
                    } else {
                        "English"
                    })
                    .show_ui(ui, |ui| {
                        ui.selectable_value(&mut st.lang, Lang::Ru, "Русский");
                        ui.selectable_value(&mut st.lang, Lang::En, "English");
                    });
                if st.lang != before {
                    st.save();
                }
            });
            ui.horizontal(|ui| {
                ui.label(S::PopupHotkey.get(lang));
                if hotkey_field(
                    ui,
                    &mut self.popup.capturing,
                    "popup_hotkey",
                    &mut st.hotkey,
                    false,
                    lang,
                ) {
                    st.save();
                    reregister = true;
                }
            });
            if reregister {
                platform::post(platform::WM_REHOTKEY);
            }
            ui.label(RichText::new(S::HotkeyWinNote.get(lang)).weak().small());
            if let Some(e) = self
                .shared
                .hotkey_error
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .as_ref()
            {
                ui.label(RichText::new(e).color(crate::theme::BAD));
            }
            let win_v = st.replace_win_v;
            if !win_v {
                ui.label(RichText::new(S::HotkeyNote.get(lang)).weak().small());
            }
            let mut at_cursor = st.popup_at_cursor;
            let mut compact = st.compact_popup;
            drop(st);
            if ui.checkbox(&mut compact, S::CompactPopup.get(lang)).changed() {
                let mut st = self.shared.settings.lock().unwrap_or_else(|e| e.into_inner());
                st.compact_popup = compact;
                st.save();
            }
            if !self.shared.demo {
                let mut choice = win_v;
                ui.horizontal(|ui| {
                    ui.label(S::QuickPasteMode.get(lang));
                    ui.radio_value(&mut choice, false, S::WinVWindows.get(lang));
                    ui.radio_value(&mut choice, true, S::WinVClipvault.get(lang));
                });
                if choice != win_v && winint::set_win_v_released(choice).is_ok() {
                    let mut st = self.shared.settings.lock().unwrap_or_else(|e| e.into_inner());
                    st.replace_win_v = choice;
                    st.save();
                    drop(st);
                    self.popup.win_v_hint = true;
                    platform::post(platform::WM_REHOTKEY);
                }
                if choice {
                    ui.label(RichText::new(S::WinVNote.get(lang)).weak().small());
                }
                if self.popup.win_v_hint {
                    ui.label(RichText::new(S::WinVRestart.get(lang)).color(crate::theme::ACCENT));
                    if ui.button(S::WinVRestartBtn.get(lang)).clicked() {
                        winint::restart_explorer();
                        self.popup.win_v_hint = false;
                        // the new Explorer has to be up before the hotkey can be taken
                        std::thread::spawn(|| {
                            std::thread::sleep(std::time::Duration::from_secs(4));
                            platform::post(platform::WM_REHOTKEY);
                        });
                    }
                }
            }
            if ui.checkbox(&mut at_cursor, S::PopupAtCursor.get(lang)).changed() {
                let mut st = self.shared.settings.lock().unwrap_or_else(|e| e.into_inner());
                st.popup_at_cursor = at_cursor;
                st.save();
            }
            if !self.shared.demo {
                let mut auto = winint::autostart_enabled();
                if ui.checkbox(&mut auto, S::Autostart.get(lang)).changed()
                    && let Ok(exe) = std::env::current_exe()
                {
                    let _ = winint::set_autostart(auto, &exe);
                }
            }
        });
    }

    fn sec_hotkeys(&mut self, ui: &mut Ui, lang: Lang) {
        use clipvault_core::config::TransformHotkey;
        use clipvault_core::transform::Transform;
        let card = crate::theme::card().show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.label(RichText::new(S::PlainHotkey.get(lang)).strong());
            let mut st = self.shared.settings.lock().unwrap_or_else(|e| e.into_inner());
            let mut reregister = false;
            ui.horizontal(|ui| {
                if hotkey_field(
                    ui,
                    &mut self.popup.capturing,
                    "plain_hotkey",
                    &mut st.plain_paste_hotkey,
                    true,
                    lang,
                ) {
                    reregister = true;
                }
            });
            ui.label(RichText::new(S::PlainNote.get(lang)).weak().small());
            ui.add_space(6.0);
            ui.label(RichText::new(S::TransformHotkeys.get(lang)).strong());
            let mut remove: Option<usize> = None;
            for (i, th) in st.transform_hotkeys.iter_mut().enumerate() {
                ui.horizontal(|ui| {
                    if hotkey_field(ui, &mut self.popup.capturing, ("th", i), &mut th.hotkey, true, lang) {
                        reregister = true;
                    }
                    let current = Transform::parse(&th.transform);
                    egui::ComboBox::from_id_salt(("tt", i))
                        .selected_text(current.map_or("?", |t| crate::ui::transform_label(t).get(lang)))
                        .width(240.0)
                        .show_ui(ui, |ui| {
                            for t in Transform::ALL {
                                if ui
                                    .selectable_label(current == Some(t), crate::ui::transform_label(t).get(lang))
                                    .clicked()
                                {
                                    th.transform = t.name().to_owned();
                                    reregister = true;
                                }
                            }
                        });
                    if ui.button(S::Remove.get(lang)).clicked() {
                        remove = Some(i);
                    }
                });
            }
            if let Some(i) = remove {
                st.transform_hotkeys.remove(i);
                reregister = true;
            }
            if ui.button(S::AddRow.get(lang)).clicked() {
                st.transform_hotkeys.push(TransformHotkey {
                    hotkey: String::new(),
                    transform: Transform::Trim.name().to_owned(),
                });
                reregister = true;
            }
            drop_and_apply(st, reregister);
        });
        if std::mem::take(&mut self.popup.scroll_to_hotkeys) {
            ui.scroll_to_rect(card.response.rect, Some(egui::Align::TOP));
        }
    }

    fn sec_secrets(&mut self, ui: &mut Ui, lang: Lang) {
        use clipvault_core::guard::SecretPolicy;
        use clipvault_core::secrets::{Detector, Sensitivity};
        if !self.popup.texts_loaded {
            let st = self.shared.settings.lock().unwrap_or_else(|e| e.into_inner());
            self.popup.custom_text = st.custom_patterns.join(
                "
",
            );
            self.popup.blacklist_text = st.blacklist.join(
                "
",
            );
            self.popup.texts_loaded = true;
        }
        let card = crate::theme::card().show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.label(RichText::new(S::SecretsSection.get(lang)).strong());
            let mut changed = false;
            let mut st = self.shared.settings.lock().unwrap_or_else(|e| e.into_inner());
            changed |= ui
                .checkbox(&mut st.secrets_enabled, S::SecretsEnabled.get(lang))
                .changed();
            ui.add_enabled_ui(st.secrets_enabled, |ui| {
                ui.label(S::PolicyLabel.get(lang));
                for (p, label) in [
                    (SecretPolicy::DontStore, S::PolicyDont),
                    (SecretPolicy::MemoryOnly, S::PolicyMemory),
                    (SecretPolicy::Mask, S::PolicyMask),
                ] {
                    changed |= ui.radio_value(&mut st.secret_policy, p, label.get(lang)).changed();
                }
                if st.secret_policy == SecretPolicy::MemoryOnly {
                    ui.horizontal(|ui| {
                        ui.label(S::ClearAfter.get(lang));
                        changed |= ui
                            .add(egui::DragValue::new(&mut st.secret_clear_secs).range(1..=3600))
                            .changed();
                    });
                }
                ui.horizontal(|ui| {
                    ui.label(S::SensitivityLabel.get(lang));
                    let name = |s: Sensitivity| match s {
                        Sensitivity::Off => S::SensOff,
                        Sensitivity::Low => S::SensLow,
                        Sensitivity::Medium => S::SensMedium,
                        Sensitivity::High => S::SensHigh,
                    };
                    egui::ComboBox::from_id_salt("sens")
                        .selected_text(name(st.password_sensitivity).get(lang))
                        .show_ui(ui, |ui| {
                            for s in [
                                Sensitivity::Off,
                                Sensitivity::Low,
                                Sensitivity::Medium,
                                Sensitivity::High,
                            ] {
                                changed |= ui
                                    .selectable_value(&mut st.password_sensitivity, s, name(s).get(lang))
                                    .changed();
                            }
                        });
                });
                ui.label(S::CustomPatterns.get(lang));
                if ui
                    .add(
                        egui::TextEdit::multiline(&mut self.popup.custom_text)
                            .desired_rows(2)
                            .desired_width(f32::INFINITY),
                    )
                    .changed()
                {
                    st.custom_patterns = self.popup.custom_text.lines().map(str::to_owned).collect();
                    changed = true;
                }
                let (_, bad) = Detector::new(Sensitivity::Off, &st.custom_patterns);
                if !bad.is_empty() {
                    let nums: Vec<String> = bad.iter().map(|i| (i + 1).to_string()).collect();
                    ui.label(
                        RichText::new(format!("{} {}", S::BadPatterns.get(lang), nums.join(", ")))
                            .color(crate::theme::BAD),
                    );
                }
            });
            changed |= ui
                .checkbox(&mut st.respect_app_flags, S::RespectFlags.get(lang))
                .changed();
            ui.label(S::BlacklistLabel.get(lang));
            if ui
                .add(
                    egui::TextEdit::multiline(&mut self.popup.blacklist_text)
                        .desired_rows(3)
                        .desired_width(f32::INFINITY),
                )
                .changed()
            {
                st.blacklist = self
                    .popup
                    .blacklist_text
                    .lines()
                    .map(|l| l.trim().to_owned())
                    .filter(|l| !l.is_empty())
                    .collect();
                changed = true;
            }
            ui.label(RichText::new(S::SecretsNote.get(lang)).weak().small());
            if changed {
                st.save();
            }
        });
        if std::mem::take(&mut self.popup.scroll_to_secrets) {
            ui.scroll_to_rect(card.response.rect, Some(egui::Align::TOP));
        }
    }

    fn sec_limits(&mut self, ui: &mut Ui, lang: Lang) {
        crate::theme::card().show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.label(RichText::new(S::SecLimits.get(lang)).strong());
            let mut st = self.shared.settings.lock().unwrap_or_else(|e| e.into_inner());
            let before = (st.max_item_kb, st.max_age_days, st.max_items);
            egui::Grid::new("limits").num_columns(2).show(ui, |ui| {
                ui.label(S::MaxItemKb.get(lang));
                ui.add(egui::DragValue::new(&mut st.max_item_kb).range(1..=262_144));
                ui.end_row();
                ui.label(S::MaxAgeDays.get(lang));
                ui.add(egui::DragValue::new(&mut st.max_age_days).range(0..=36_500));
                ui.end_row();
                ui.label(S::MaxItems.get(lang));
                ui.add(egui::DragValue::new(&mut st.max_items).range(0..=1_000_000));
                ui.end_row();
            });
            ui.label(RichText::new(S::LimitsNote.get(lang)).weak().small());
            if (st.max_item_kb, st.max_age_days, st.max_items) != before {
                st.save();
                let limits = st.limits();
                drop(st);
                let _ = self
                    .shared
                    .history
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .enforce(&limits, clipvault_core::now_unix());
            }
        });
    }

    fn sec_admin(&mut self, ui: &mut Ui, lang: Lang) {
        crate::theme::card().show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.label(RichText::new(S::AdminBadge.get(lang)).strong());
            ui.label(RichText::new(S::AdminNote.get(lang)).weak().small());
            ui.horizontal(|ui| {
                ui.label(S::AdminHotkey.get(lang));
                {
                    let mut st = self.shared.settings.lock().unwrap_or_else(|e| e.into_inner());
                    if hotkey_field(
                        ui,
                        &mut self.popup.capturing,
                        "admin_hotkey",
                        &mut st.admin_hotkey,
                        false,
                        lang,
                    ) {
                        st.save();
                    }
                }
                if !self.shared.admin
                    && !crate::winclip::self_elevated()
                    && ui.button(S::AdminOpen.get(lang)).clicked()
                    && let Err(e) = winint::open_as_admin()
                {
                    self.popup.toast = Some((e, Instant::now()));
                }
            });
        });
    }

    fn sec_updates(&mut self, ui: &mut Ui, lang: Lang) {
        crate::theme::card().show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.label(RichText::new(S::SecUpdates.get(lang)).strong());
            {
                let mut st = self.shared.settings.lock().unwrap_or_else(|e| e.into_inner());
                if ui.checkbox(&mut st.auto_update, S::AutoUpdate.get(lang)).changed() {
                    st.save();
                }
            }
            ui.horizontal(|ui| {
                ui.label(format!("{} {}", S::Version.get(lang), env!("CARGO_PKG_VERSION")));
                let state = self.update.lock().unwrap_or_else(|e| e.into_inner()).clone();
                match &state {
                    update::State::Checking => {
                        ui.spinner();
                        ui.label(S::Checking.get(lang));
                    }
                    update::State::UpToDate => {
                        ui.label(RichText::new(S::UpToDate.get(lang)).color(crate::theme::GOOD));
                    }
                    update::State::Available { version, url } => {
                        ui.label(format!("{} {version}", S::UpdateAvailable.get(lang)));
                        ui.hyperlink_to(url, url);
                    }
                    update::State::Downloading { done, total, .. } => {
                        let frac = if *total > 0 { *done as f32 / *total as f32 } else { 0.0 };
                        ui.add(
                            egui::ProgressBar::new(frac)
                                .desired_width(160.0)
                                .text(S::UpdateDownloading.get(lang)),
                        );
                    }
                    update::State::Ready { .. } => {
                        ui.label(RichText::new(S::UpdateReady.get(lang)).color(crate::theme::GOOD));
                    }
                    update::State::Failed(e) => {
                        ui.label(RichText::new(format!("{}: {e}", S::UpdateFailed.get(lang))).color(crate::theme::BAD));
                    }
                    update::State::Idle => {}
                }
                let busy = matches!(state, update::State::Checking | update::State::Downloading { .. });
                if !self.shared.demo
                    && ui
                        .add_enabled(!busy, egui::Button::new(S::CheckNow.get(lang)))
                        .clicked()
                {
                    self.start_update_check(true);
                }
            });
        });
    }

    fn sec_diag(&mut self, ui: &mut Ui, lang: Lang) {
        crate::theme::card().show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.label(RichText::new(S::SecDiagnostics.get(lang)).strong());
            ui.horizontal(|ui| {
                if ui.button(S::OpenLogs.get(lang)).clicked()
                    && let Some(d) = diag::logs_dir()
                {
                    let _ = std::fs::create_dir_all(&d);
                    let _ = std::process::Command::new("explorer.exe").arg(d).spawn();
                }
                if ui.button(S::CopyDiag.get(lang)).clicked() {
                    let counts = self.shared.history.lock().unwrap_or_else(|e| e.into_inner()).counts();
                    ui.ctx().copy_text(diag::report(&counts));
                    self.popup.toast = Some((S::DiagCopied.get(lang).to_owned(), Instant::now()));
                }
            });
            ui.label(RichText::new(S::DiagNote.get(lang)).weak().small());
        });
    }

    fn sec_install(&mut self, ui: &mut Ui, lang: Lang) {
        crate::theme::card().show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.label(RichText::new(S::SecSystem.get(lang)).strong());
            if winint::running_installed() {
                ui.label(RichText::new(S::Installed.get(lang)).color(crate::theme::GOOD));
            } else if ui.button(S::InstallHere.get(lang)).clicked() {
                let opts = winint::InstallOptions {
                    desktop_shortcut: false,
                    autostart: false,
                };
                match winint::install(opts, false) {
                    Ok(exe) => {
                        // the new copy asks this one to exit and takes over
                        let _ = std::process::Command::new(exe).arg("--replace").spawn();
                    }
                    Err(e) => self.popup.toast = Some((e, Instant::now())),
                }
            }
        });
    }
}
