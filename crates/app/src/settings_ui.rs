//! The settings view inside the popup window.

use std::time::Instant;

use clipvault_core::config::Lang;
use clipvault_core::hotkey::{MOD_ALT, MOD_CONTROL, MOD_SHIFT, MOD_WIN, format_hotkey};
use clipvault_core::i18n::S;
use eframe::egui::{self, Key, RichText, Ui};

use crate::{App, diag, platform, update, winint};

fn vk_of(key: Key) -> Option<u32> {
    let n = key.name();
    let b = n.as_bytes();
    if b.len() == 1 && b[0].is_ascii_alphanumeric() {
        return Some(u32::from(b[0].to_ascii_uppercase()));
    }
    let named = [
        (Key::Space, 0x20),
        (Key::Enter, 0x0D),
        (Key::Tab, 0x09),
        (Key::Insert, 0x2D),
        (Key::Delete, 0x2E),
        (Key::Home, 0x24),
        (Key::End, 0x23),
        (Key::PageUp, 0x21),
        (Key::PageDown, 0x22),
    ];
    if let Some((_, v)) = named.iter().find(|(k, _)| *k == key) {
        return Some(*v);
    }
    n.strip_prefix('F')
        .and_then(|f| f.parse::<u32>().ok())
        .filter(|f| (1..=24).contains(f))
        .map(|f| 0x6F + f)
}

impl App {
    /// Records the next key combination while "Change" is active.
    fn capture_hotkey(&mut self, ctx: &egui::Context) {
        let found = ctx.input(|i| {
            i.events.iter().find_map(|e| match e {
                egui::Event::Key {
                    key,
                    pressed: true,
                    modifiers,
                    ..
                } => {
                    let mut mods = 0;
                    if modifiers.ctrl {
                        mods |= MOD_CONTROL;
                    }
                    if modifiers.alt {
                        mods |= MOD_ALT;
                    }
                    if modifiers.shift {
                        mods |= MOD_SHIFT;
                    }
                    if modifiers.mac_cmd {
                        mods |= MOD_WIN;
                    }
                    (mods != 0).then(|| vk_of(*key).map(|vk| (mods, vk))).flatten()
                }
                _ => None,
            })
        });
        if let Some((mods, vk)) = found {
            let spec = format_hotkey(mods, vk);
            let mut st = self.shared.settings.lock().unwrap_or_else(|e| e.into_inner());
            st.hotkey = spec;
            st.save();
            drop(st);
            self.popup.capture_hotkey = false;
            platform::post(platform::WM_REHOTKEY);
        } else if ctx.input(|i| i.key_pressed(Key::Escape)) {
            self.popup.capture_hotkey = false;
        }
    }

    pub fn settings_ui(&mut self, ui: &mut Ui) {
        let lang = self.shared.lang();
        let ctx = ui.ctx().clone();
        if self.popup.capture_hotkey {
            self.capture_hotkey(&ctx);
        }
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
                if self.popup.capture_hotkey {
                    ui.label(RichText::new(S::PressKeys.get(lang)).color(crate::theme::ACCENT));
                } else {
                    ui.label(RichText::new(&st.hotkey).monospace().strong());
                    if ui.button(S::Change.get(lang)).clicked() {
                        self.popup.capture_hotkey = true;
                    }
                }
            });
            if let Some(e) = self
                .shared
                .hotkey_error
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .as_ref()
            {
                ui.label(RichText::new(e).color(crate::theme::BAD));
            }
            ui.label(RichText::new(S::HotkeyNote.get(lang)).weak().small());
            drop(st);
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
        use clipvault_core::hotkey::parse_hotkey;
        use clipvault_core::transform::Transform;
        let card = crate::theme::card().show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.label(RichText::new(S::PlainHotkey.get(lang)).strong());
            let mut st = self.shared.settings.lock().unwrap_or_else(|e| e.into_inner());
            let mut reregister = false;
            ui.horizontal(|ui| {
                let r = ui.add(egui::TextEdit::singleline(&mut st.plain_paste_hotkey).desired_width(180.0));
                if r.changed() {
                    st.save();
                }
                if r.lost_focus() {
                    reregister = true;
                }
                if !st.plain_paste_hotkey.trim().is_empty() && parse_hotkey(&st.plain_paste_hotkey).is_none() {
                    ui.label(RichText::new(S::HotkeyInvalid.get(lang)).color(crate::theme::BAD));
                }
            });
            ui.label(RichText::new(S::PlainNote.get(lang)).weak().small());
            ui.add_space(6.0);
            ui.label(RichText::new(S::TransformHotkeys.get(lang)).strong());
            let mut remove: Option<usize> = None;
            for (i, th) in st.transform_hotkeys.iter_mut().enumerate() {
                ui.horizontal(|ui| {
                    let r = ui.add(
                        egui::TextEdit::singleline(&mut th.hotkey)
                            .desired_width(150.0)
                            .id_salt(("th", i)),
                    );
                    let bad = !th.hotkey.trim().is_empty() && parse_hotkey(&th.hotkey).is_none();
                    if r.lost_focus() {
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
                    if bad {
                        ui.label(RichText::new(S::HotkeyInvalid.get(lang)).color(crate::theme::BAD));
                    }
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
            }
            st.save();
            drop(st);
            if reregister {
                platform::post(platform::WM_REHOTKEY);
            }
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
            let hotkey = self
                .shared
                .settings
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .admin_hotkey
                .clone();
            ui.horizontal(|ui| {
                ui.label(S::AdminHotkey.get(lang));
                ui.label(RichText::new(hotkey).monospace().strong());
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
