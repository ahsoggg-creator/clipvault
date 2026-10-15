//! Installer / uninstaller screens. The same executable becomes the installer when it is started as
//! `Clipvault-Setup.exe` (or with `--install`) and the uninstaller with `--uninstall`.

use std::path::PathBuf;
use std::sync::mpsc::{Receiver, channel};

use clipvault_core::i18n::S;
use eframe::egui::{self, Align, Id, Layout, RichText, Ui, vec2};

use crate::{App, Mode, theme, winint};

pub struct Install {
    desktop: bool,
    autostart: bool,
    job: Option<Receiver<Result<PathBuf, String>>>,
    result: Option<Result<PathBuf, String>>,
}

impl Default for Install {
    fn default() -> Self {
        Install {
            desktop: true,
            autostart: true,
            job: None,
            result: None,
        }
    }
}

#[derive(Default)]
pub struct Uninstall {
    delete_data: bool,
    job: Option<Receiver<Result<(), String>>>,
    result: Option<Result<(), String>>,
}

impl App {
    pub fn installer_ui(&mut self, ui: &mut Ui) {
        let icon = self.icon.clone();
        let ctx = ui.ctx().clone();
        let lang = self.shared.lang();
        egui::CentralPanel::default().show(ui, |ui| {
            let appear = ctx.animate_bool_with_time(Id::new("setup_in"), true, 0.3);
            ui.multiply_opacity(appear);
            ui.add_space(18.0 + (1.0 - appear) * 12.0);
            ui.vertical_centered(|ui| {
                if let Some(icon) = &icon {
                    ui.add(egui::Image::new(icon).fit_to_exact_size(vec2(64.0, 64.0)));
                }
                ui.add_space(6.0);
                ui.label(RichText::new("Clipvault").size(24.0).strong());
                ui.label(RichText::new(format!("{} {}", S::Version.get(lang), env!("CARGO_PKG_VERSION"))).weak());
            });
            ui.add_space(14.0);
            match &mut self.mode {
                Mode::Install(s) => install_body(ui, s, lang),
                Mode::Uninstall(s) => uninstall_body(ui, s, lang),
                Mode::App => {}
            }
        });
    }
}

fn install_body(ui: &mut Ui, s: &mut Install, lang: clipvault_core::config::Lang) {
    if let Some(rx) = &s.job {
        if let Ok(r) = rx.try_recv() {
            s.result = Some(r);
            s.job = None;
        } else {
            ui.vertical_centered(|ui| {
                ui.add_space(20.0);
                ui.add(egui::Spinner::new().size(32.0).color(theme::ACCENT));
                ui.label(S::SetupRunning.get(lang));
            });
            ui.ctx().request_repaint();
            return;
        }
    }
    match &s.result {
        Some(Ok(exe)) => {
            let exe = exe.clone();
            ui.vertical_centered(|ui| {
                ui.label(
                    RichText::new(format!("✔ {}", S::SetupDone.get(lang)))
                        .size(18.0)
                        .color(theme::GOOD)
                        .strong(),
                );
                ui.add_space(4.0);
                ui.label(S::SetupDoneText.get(lang));
                ui.add_space(16.0);
                ui.horizontal(|ui| {
                    ui.add_space((ui.available_width() - 260.0).max(0.0) / 2.0);
                    if theme::primary_button(ui, S::SetupLaunch.get(lang)).clicked() {
                        let _ = std::process::Command::new(&exe).spawn();
                        ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
                    }
                    if ui.button(S::Close.get(lang)).clicked() {
                        ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
                    }
                });
            });
        }
        Some(Err(e)) => {
            ui.label(RichText::new(S::SetupFailed.get(lang)).strong().color(theme::BAD));
            ui.label(e.as_str());
            if ui.button(S::SetupRetry.get(lang)).clicked() {
                s.result = None;
            }
        }
        None => {
            let dir = winint::install_dir()
                .map(|d| d.display().to_string())
                .unwrap_or_default();
            let upgrade = winint::installed_exe().is_some_and(|e| e.exists());
            theme::card().show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.label(RichText::new(S::SetupFolder.get(lang)).weak());
                ui.label(dir);
                ui.add_space(6.0);
                ui.checkbox(&mut s.desktop, S::SetupDesktop.get(lang));
                ui.checkbox(&mut s.autostart, S::Autostart.get(lang));
                ui.label(RichText::new(S::SetupNoAdmin.get(lang)).weak().small());
            });
            ui.add_space(12.0);
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if ui.button(S::Cancel.get(lang)).clicked() {
                    ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
                }
                let label = if upgrade { S::SetupUpgrade } else { S::SetupInstall };
                if theme::primary_button(ui, label.get(lang)).clicked() {
                    let (tx, rx) = channel();
                    let opts = winint::InstallOptions {
                        desktop_shortcut: s.desktop,
                        autostart: s.autostart,
                    };
                    let ctx = ui.ctx().clone();
                    std::thread::spawn(move || {
                        let _ = tx.send(winint::install(opts, true));
                        ctx.request_repaint();
                    });
                    s.job = Some(rx);
                }
            });
        }
    }
}

fn uninstall_body(ui: &mut Ui, s: &mut Uninstall, lang: clipvault_core::config::Lang) {
    if let Some(rx) = &s.job {
        if let Ok(r) = rx.try_recv() {
            s.result = Some(r);
            s.job = None;
        } else {
            ui.vertical_centered(|ui| {
                ui.add(egui::Spinner::new().size(28.0).color(theme::ACCENT));
            });
            ui.ctx().request_repaint();
            return;
        }
    }
    match &s.result {
        None => {
            ui.vertical_centered(|ui| {
                ui.label(S::UninstallQuestion.get(lang));
                ui.label(RichText::new(S::UninstallNote.get(lang)).weak());
                ui.add_space(8.0);
                ui.checkbox(&mut s.delete_data, S::UninstallData.get(lang));
                ui.add_space(10.0);
                ui.horizontal(|ui| {
                    ui.add_space((ui.available_width() - 180.0).max(0.0) / 2.0);
                    if ui
                        .add(
                            egui::Button::new(RichText::new(S::Delete.get(lang)).color(egui::Color32::WHITE))
                                .fill(theme::BAD),
                        )
                        .clicked()
                    {
                        let (tx, rx) = channel();
                        let ctx = ui.ctx().clone();
                        let data = s.delete_data;
                        std::thread::spawn(move || {
                            let _ = tx.send(winint::uninstall(data));
                            ctx.request_repaint();
                        });
                        s.job = Some(rx);
                    }
                    if ui.button(S::Cancel.get(lang)).clicked() {
                        ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
                    }
                });
            });
        }
        Some(Ok(())) => {
            ui.vertical_centered(|ui| {
                ui.label(RichText::new(S::UninstallDone.get(lang)).size(17.0).strong());
                ui.add_space(10.0);
                if ui.button(S::Close.get(lang)).clicked() {
                    ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
                }
            });
        }
        Some(Err(e)) => {
            ui.label(RichText::new(S::UninstallFailed.get(lang)).strong().color(theme::BAD));
            ui.label(e.as_str());
        }
    }
}
