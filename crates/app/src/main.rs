#![windows_subsystem = "windows"]
//! Clipvault: a clipboard manager that keeps its history encrypted.
//!
//! One executable, several roles: the resident app (tray icon, hotkey, popup), the installer
//! (`Clipvault-Setup.exe` / `--install`), the uninstaller (`--uninstall`), a silent updater (`--update`)
//! and the demo (`--demo`, `--screenshots <dir>`).

mod capture;
mod compact_ui;
mod demo;
mod diag;
mod installer;
mod paste;
mod platform;
mod settings_ui;
mod shared;
mod snippets_ui;
mod theme;
mod ui;
mod update;
mod winclip;
mod winint;

use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex};

use clipvault_core::config::{Settings, open_history};
use clipvault_core::i18n::S;
use clipvault_core::{History, now_unix};
use eframe::egui::{self, Ui};
use windows_sys::Win32::Foundation::{ERROR_ALREADY_EXISTS, GetLastError};
use windows_sys::Win32::System::Threading::CreateMutexW;
use windows_sys::Win32::UI::WindowsAndMessaging::{FindWindowW, MB_ICONERROR, MB_OK, MessageBoxW, PostMessageW};

use crate::shared::Shared;
use crate::winclip::wide;

pub enum Mode {
    App,
    Install(installer::Install),
    Uninstall(installer::Uninstall),
}

pub struct App {
    pub shared: Arc<Shared>,
    pub mode: Mode,
    pub popup: ui::Popup,
    pub icon: Option<egui::TextureHandle>,
    pub update: update::Shared,
    pub demo: Option<demo::Demo>,
    restarting: bool,
    background: bool,
    frames: u32,
}

impl App {
    fn new(
        cc: &eframe::CreationContext<'_>,
        shared: Arc<Shared>,
        mode: Mode,
        update: update::Shared,
        background: bool,
    ) -> App {
        theme::install(&cc.egui_ctx);
        let icon = image_from_rgba(include_bytes!("../assets/icon64.rgba"), 64)
            .map(|img| cc.egui_ctx.load_texture("icon", img, egui::TextureOptions::LINEAR));
        let _ = shared.ctx.set(cc.egui_ctx.clone());
        let screenshots = std::env::args().any(|a| a == "--screenshots");
        App {
            shared,
            mode,
            popup: ui::Popup::default(),
            icon,
            update,
            demo: screenshots.then(demo::Demo::from_args),
            restarting: false,
            background,
            frames: 0,
        }
    }

    /// Checks GitHub now; with `install`, also downloads and installs.
    pub fn start_update_check(&self, install: bool) {
        let ctx = self.shared.ctx.get().cloned();
        update::spawn(self.update.clone(), install, move || {
            if let Some(c) = &ctx {
                c.request_repaint();
            }
        });
    }

    fn app_ui(&mut self, ui: &mut Ui) {
        let ctx = ui.ctx().clone();
        self.frames = self.frames.saturating_add(1);
        if self.frames < 6 {
            ctx.request_repaint();
        }
        // eframe shows the window after its first frames even when it was created hidden: hide it once that is done
        if self.frames == 4 && !self.shared.demo {
            platform::hide_popup();
            if !self.background || self.shared.admin {
                platform::post(platform::WM_SHOW);
            }
        }
        if ctx.input(|i| i.viewport().close_requested())
            && !self.shared.demo
            && !self.shared.quit.load(Ordering::SeqCst)
        {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            platform::close_popup();
        }
        let ready = matches!(
            *self.update.lock().unwrap_or_else(|e| e.into_inner()),
            update::State::Ready { .. }
        );
        if ready && !self.restarting && !self.shared.demo {
            self.restarting = true;
            if update::restart().is_ok() {
                return;
            }
        }
        self.demo_step(&ctx);
        self.popup_ui(ui);
        let rect = ctx.content_rect();
        ctx.layer_painter(egui::LayerId::new(egui::Order::Foreground, egui::Id::new("frame")))
            .rect_stroke(
                rect,
                0.0,
                egui::Stroke::new(1.0, theme::ACCENT.gamma_multiply(0.6)),
                egui::StrokeKind::Inside,
            );
    }
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut Ui, _frame: &mut eframe::Frame) {
        match &self.mode {
            Mode::App => self.app_ui(ui),
            Mode::Install(_) | Mode::Uninstall(_) => self.installer_ui(ui),
        }
    }
}

fn image_from_rgba(bytes: &[u8], side: usize) -> Option<egui::ColorImage> {
    (bytes.len() == side * side * 4).then(|| egui::ColorImage::from_rgba_unmultiplied([side, side], bytes))
}

fn message_box(title: &str, text: &str) {
    // SAFETY: both strings are NUL-terminated and outlive the call; no owner window.
    unsafe {
        MessageBoxW(
            std::ptr::null_mut(),
            wide(text).as_ptr(),
            wide(title).as_ptr(),
            MB_OK | MB_ICONERROR,
        );
    }
}

/// A windowed-subsystem program has no console: borrow the parent's so `--update` can print.
fn attach_console() {
    use windows_sys::Win32::Storage::FileSystem::{CreateFileW, FILE_SHARE_READ, FILE_SHARE_WRITE, OPEN_EXISTING};
    use windows_sys::Win32::System::Console::{
        ATTACH_PARENT_PROCESS, AttachConsole, GetStdHandle, STD_ERROR_HANDLE, STD_OUTPUT_HANDLE, SetStdHandle,
    };
    // SAFETY: standard console attachment; handles are only replaced when the process has none.
    unsafe {
        if AttachConsole(ATTACH_PARENT_PROCESS) == 0 {
            return;
        }
        if GetStdHandle(STD_OUTPUT_HANDLE).is_null() {
            let h = CreateFileW(
                wide("CONOUT$").as_ptr(),
                0x4000_0000,
                FILE_SHARE_READ | FILE_SHARE_WRITE,
                std::ptr::null(),
                OPEN_EXISTING,
                0,
                std::ptr::null_mut(),
            );
            SetStdHandle(STD_OUTPUT_HANDLE, h);
            SetStdHandle(STD_ERROR_HANDLE, h);
        }
    }
}

/// Only one resident instance. Returns false if another one runs (and has been told to show itself).
fn single_instance(replace: bool, admin: bool) -> bool {
    let name = wide(if admin {
        r"Local\Clipvault.Admin"
    } else {
        r"Local\Clipvault.Main"
    });
    let class = wide(if admin { platform::CLASS_ADMIN } else { platform::CLASS });
    for attempt in 0..30 {
        // SAFETY: the mutex handle is intentionally kept for the life of the process.
        let created = unsafe {
            let h = CreateMutexW(std::ptr::null(), 0, name.as_ptr());
            (!h.is_null()).then(|| GetLastError() != ERROR_ALREADY_EXISTS)
        };
        match created {
            Some(true) | None => return true,
            Some(false) if !replace => {
                // SAFETY: plain lookup / post.
                unsafe {
                    let w = FindWindowW(class.as_ptr(), std::ptr::null());
                    if !w.is_null() {
                        PostMessageW(w, platform::WM_SHOW, 0, 0);
                    }
                }
                return false;
            }
            Some(false) => {
                if attempt == 0 {
                    winint::stop_running();
                }
                std::thread::sleep(std::time::Duration::from_millis(200));
            }
        }
    }
    false
}

/// The installed copy starts with Windows by default; this is applied once, so switching it off later sticks.
fn first_run_autostart(shared: &Shared) {
    let mut st = shared.settings.lock().unwrap_or_else(|e| e.into_inner());
    if st.autostart_default_applied || !winint::running_installed() {
        return;
    }
    if let Ok(exe) = std::env::current_exe() {
        let _ = winint::set_autostart(true, &exe);
    }
    st.autostart_default_applied = true;
    st.save();
}

/// Windows 11 puts a new tray icon into the overflow flyout; ask for it to stay on the taskbar. Windows creates
/// the icon's settings entry some time after the icon first appears, so this tries for a minute.
fn keep_tray_icon_visible() {
    std::thread::Builder::new()
        .name("clipvault-tray".into())
        .spawn(|| {
            for _ in 0..20 {
                std::thread::sleep(std::time::Duration::from_secs(3));
                if winint::promote_tray_icon() {
                    diag::info("the tray icon is set to stay on the taskbar");
                    break;
                }
            }
        })
        .ok();
}

/// A daily look at GitHub; a found update is announced with a notification and a button in the window.
fn start_update_loop(shared: Arc<Shared>, state: update::Shared) {
    std::thread::Builder::new()
        .name("clipvault-update".into())
        .spawn(move || {
            std::thread::sleep(std::time::Duration::from_secs(3));
            if let Some(p) = diag::take_unseen_crash() {
                platform::notify(format!("{} {}", shared.tr(S::CrashNotice), p.display()));
            }
            loop {
                let (auto, last) = {
                    let s = shared.settings.lock().unwrap_or_else(|e| e.into_inner());
                    (s.auto_update, s.last_update_check)
                };
                if auto && now_unix().saturating_sub(last) >= update::CHECK_INTERVAL_SECS {
                    update::check_only(&state);
                    let st = state.lock().unwrap_or_else(|e| e.into_inner()).clone();
                    if !matches!(st, update::State::Failed(_)) {
                        let mut s = shared.settings.lock().unwrap_or_else(|e| e.into_inner());
                        s.last_update_check = now_unix();
                        s.save();
                    }
                    if let update::State::Available { version, .. } = &st {
                        diag::info("a newer release is available");
                        platform::notify(format!("{} {version}", shared.tr(S::UpdateAvailable)));
                    }
                    shared.repaint();
                }
                std::thread::sleep(std::time::Duration::from_secs(1800));
            }
        })
        .ok();
}

fn main() -> eframe::Result {
    diag::install_panic_hook();
    let args: Vec<String> = std::env::args().skip(1).collect();
    let has = |f: &str| args.iter().any(|a| a == f);

    if has("--update") {
        attach_console();
        std::process::exit(update::run_headless());
    }
    if has("--silent") && (has("--install") || has("--uninstall")) {
        attach_console();
        let r = if has("--uninstall") {
            winint::uninstall(has("--delete-data")).map(|()| "Clipvault удалён.".to_owned())
        } else {
            let opts = winint::InstallOptions {
                desktop_shortcut: has("--desktop"),
                autostart: has("--autostart"),
            };
            winint::install(opts, true).map(|p| format!("Clipvault установлен: {}", p.display()))
        };
        match r {
            Ok(m) => println!("{m}"),
            Err(e) => {
                eprintln!("{e}");
                std::process::exit(1);
            }
        }
        std::process::exit(0);
    }
    let demo = has("--demo") || has("--screenshots");
    if demo {
        diag::set_demo();
    }
    let mode = if has("--uninstall") {
        Mode::Uninstall(installer::Uninstall::default())
    } else if winint::is_setup_launch() {
        Mode::Install(installer::Install::default())
    } else {
        Mode::App
    };
    let is_app = matches!(mode, Mode::App);
    let background = has("--background");
    let admin = has("--admin");

    let settings = if demo { Settings::default() } else { Settings::load() };
    let history = if demo {
        demo::history(now_unix())
    } else if is_app {
        if !single_instance(has("--replace"), admin) {
            return Ok(());
        }
        match open_history() {
            Ok(h) => h,
            Err(e) => {
                diag::error(format!("cannot open the history: {e}"));
                message_box("Clipvault", &format!("Не удалось открыть хранилище истории:\n{e}"));
                return Ok(());
            }
        }
    } else {
        History::memory()
    };
    let shared = Arc::new(Shared::new(history, settings, demo, admin));
    let update_state: update::Shared = Arc::new(Mutex::new(update::State::Idle));

    if is_app {
        update::cleanup_old();
        if demo {
            platform::init_only(shared.clone());
        } else {
            diag::info(format!("Clipvault {} started", env!("CARGO_PKG_VERSION")));
            platform::spawn(shared.clone(), !admin);
            if !admin {
                first_run_autostart(&shared);
                keep_tray_icon_visible();
                start_update_loop(shared.clone(), update_state.clone());
            }
        }
    } else {
        platform::init_only(shared.clone());
    }

    let (size, title) = match mode {
        Mode::App => ([980.0, 700.0], platform::popup_title(admin)),
        Mode::Install(_) => ([560.0, 440.0], "Clipvault — установка"),
        Mode::Uninstall(_) => ([520.0, 360.0], "Clipvault — удаление"),
    };
    let mut viewport = egui::ViewportBuilder::default()
        .with_inner_size(size)
        .with_title(title)
        .with_app_id("Clipvault");
    if let Some(img) = image_from_rgba(include_bytes!("../assets/icon64.rgba"), 64) {
        viewport = viewport.with_icon(egui::IconData {
            rgba: img.as_raw().to_vec(),
            width: 64,
            height: 64,
        });
    }
    if is_app {
        viewport = viewport.with_min_inner_size([300.0, 300.0]).with_taskbar(demo);
        if !demo {
            viewport = viewport
                .with_decorations(false)
                .with_always_on_top()
                .with_visible(false);
        }
    } else {
        viewport = viewport.with_resizable(false).with_maximize_button(false);
    }
    let opts = eframe::NativeOptions {
        viewport,
        ..Default::default()
    };
    eframe::run_native(
        "Clipvault",
        opts,
        Box::new(move |cc| Ok(Box::new(App::new(cc, shared, mode, update_state, background)))),
    )
}
