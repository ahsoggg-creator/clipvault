//! Demo mode: a made-up history instead of the real one.
//!
//! `Clipvault.exe --demo` opens the window on invented entries (nothing real is read or written), and
//! `Clipvault.exe --screenshots <dir>` additionally walks through the views and saves a PNG of each — that
//! is how the screenshots in the README are made, without a single real clipboard entry in them.

use std::path::PathBuf;
use std::time::{Duration, Instant};

use clipvault_core::clipfmt::{Rgba, png_encode};
use clipvault_core::config::Lang;
use clipvault_core::guard::mask_text;
use clipvault_core::snippets::Snippet;
use clipvault_core::{BlobKind, History, Kind, Limits, NewItem, SecretKind};
use eframe::egui::{self, Event, ViewportCommand};

use crate::App;
use crate::ui::View;

/// Time to let animations finish before a screenshot.
const SETTLE: Duration = Duration::from_millis(1400);

/// What the screenshot tour does, in order.
const STEPS: [&str; 10] = [
    "history", "search", "image", "color", "secret", "snippets", "settings", "hotkeys", "secrets", "keyword",
];

pub struct Demo {
    dir: Option<PathBuf>,
    step: usize,
    prepared: bool,
    asked: bool,
    since: Instant,
}

impl Demo {
    pub fn from_args() -> Demo {
        let mut args = std::env::args().skip_while(|a| a != "--screenshots");
        Demo {
            dir: args.nth(1).map(PathBuf::from),
            step: 0,
            prepared: false,
            asked: false,
            since: Instant::now(),
        }
    }
}

fn gradient(w: u32, h: u32, a: [u8; 3], b: [u8; 3]) -> Vec<u8> {
    let mut px = Vec::with_capacity((w * h * 4) as usize);
    for y in 0..h {
        for x in 0..w {
            let t = (x as f32 / w as f32 + y as f32 / h as f32) / 2.0;
            let mix = |i: usize| (f32::from(a[i]) * (1.0 - t) + f32::from(b[i]) * t) as u8;
            // a few soft stripes so it does not look like an empty rectangle
            let stripe = if (x / 40 + y / 40) % 2 == 0 { 0 } else { 14 };
            px.extend_from_slice(&[
                mix(0).saturating_add(stripe),
                mix(1).saturating_add(stripe),
                mix(2),
                255,
            ]);
        }
    }
    px
}

/// The invented history (Russian, as the default UI language).
pub fn history(now: u64) -> History {
    let mut h = History::memory();
    let lim = Limits::default();
    let mins = |m: u64| now - m * 60;
    let txt = |t: &str, app: &str, title: &str| NewItem {
        text: t.into(),
        source_app: app.into(),
        source_title: title.into(),
        ..Default::default()
    };
    let img = |w: u32, hh: u32, a, b, app: &str| {
        let px = gradient(w, hh, a, b);
        NewItem {
            kind: Some(Kind::Image),
            blob: png_encode(&Rgba {
                width: w,
                height: hh,
                pixels: px,
            })
            .unwrap_or_default(),
            blob_kind: BlobKind::Png,
            img: Some((w, hh)),
            source_app: app.into(),
            ..Default::default()
        }
    };
    let mut add = |n: NewItem, at: u64| h.add(n, at, &lim).map(|a| a.id()).unwrap_or(0);

    let addr = add(
        txt(
            "г. Новая Земля, ул. Примерная, д. 1, кв. 23",
            "chrome.exe",
            "Заказ доставки — Пример",
        ),
        mins(2400),
    );
    add(
        txt(
            "Созвон с командой в пятницу в 15:00 — подготовить демонстрацию",
            "Telegram.exe",
            "Рабочий чат",
        ),
        mins(3),
    );
    add(
        txt(
            "https://example.com/docs/getting-started",
            "chrome.exe",
            "Начало работы — Пример",
        ),
        mins(9),
    );
    add(txt("#7C6CF0", "Figma.exe", "Палитра"), mins(14));
    add(
        img(640, 360, [124, 108, 240], [40, 190, 160], "SnippingTool.exe"),
        mins(22),
    );
    add(
        txt(r"C:\Users\demo\Documents\Отчёт_Q3.xlsx", "explorer.exe", "Документы"),
        mins(35),
    );
    add(
        txt(
            "SELECT id, name, email\nFROM customers\nWHERE active = 1\nORDER BY name;",
            "Code.exe",
            "queries.sql — demo",
        ),
        mins(51),
    );
    add(
        NewItem {
            kind: Some(Kind::Files),
            text: "C:\\Users\\demo\\Pictures\\отпуск-01.jpg\nC:\\Users\\demo\\Pictures\\отпуск-02.jpg\nC:\\Users\\demo\\Pictures\\отпуск-03.jpg".into(),
            source_app: "explorer.exe".into(),
            ..Default::default()
        },
        mins(75),
    );
    add(
        NewItem {
            kind: Some(Kind::Rich),
            text: "Итоги недели: релиз вышел, три бага закрыты, документация обновлена.".into(),
            blob: "<b>Итоги недели:</b> релиз вышел, <i>три бага закрыты</i>, документация обновлена."
                .as_bytes()
                .to_vec(),
            blob_kind: BlobKind::Html,
            source_app: "WINWORD.EXE".into(),
            source_title: "Итоги.docx".into(),
            ..Default::default()
        },
        mins(110),
    );
    add(txt("rgb(34, 197, 94)", "Figma.exe", "Палитра"), mins(150));
    add(
        txt(
            "https://github.com/example/project/issues/42",
            "chrome.exe",
            "Issue #42",
        ),
        mins(190),
    );
    add(
        txt(
            r#"{"name":"demo","version":"1.0.0","private":true}"#,
            "Code.exe",
            "package.json — demo",
        ),
        mins(260),
    );
    let milk = add(txt("Купить молоко, хлеб, яйца", "notepad.exe", "Без имени"), mins(300));
    add(
        txt("ivan.petrov@example.com", "OUTLOOK.EXE", "Новое сообщение"),
        mins(420),
    );
    add(img(480, 480, [240, 150, 90], [200, 60, 120], "chrome.exe"), mins(600));
    add(
        txt(
            "Пароль от Wi-Fi гостей лежит в менеджере паролей, сюда не копируем",
            "notepad.exe",
            "заметки",
        ),
        mins(900),
    );
    add(
        txt(
            r"D:\Проекты\clipvault\target\release\Clipvault.exe",
            "cmd.exe",
            "Командная строка",
        ),
        mins(1500),
    );
    add(txt("hsl(262, 83%, 66%)", "Figma.exe", "Палитра"), mins(1800));
    // repeats: the counter grows and the entry rises
    for m in [250, 120] {
        add(txt("Купить молоко, хлеб, яйца", "notepad.exe", "Без имени"), mins(m));
    }
    // what the secret filter leaves behind (the "key" is obviously made up)
    let _ = h.add_volatile(
        NewItem {
            text: "sk-demo-NOT-A-REAL-KEY-0000000000000000".into(),
            secret: Some(SecretKind::ApiKey),
            source_app: "chrome.exe".into(),
            ..Default::default()
        },
        mins(4),
        &lim,
    );
    let _ = h.add(
        NewItem {
            kind: Some(Kind::Text),
            text: mask_text(SecretKind::Card, Lang::Ru),
            masked: true,
            secret: Some(SecretKind::Card),
            source_app: "chrome.exe".into(),
            ..Default::default()
        },
        mins(33),
        &lim,
    );
    // snippets (invented)
    let snip = |name: &str, folder: &str, tags: &[&str], keyword: &str, hotkey: &str, text: &str| Snippet {
        name: name.into(),
        folder: folder.into(),
        tags: tags.iter().map(|t| (*t).to_owned()).collect(),
        keyword: keyword.into(),
        hotkey: hotkey.into(),
        text: text.into(),
        ..Default::default()
    };
    for s in [
        snip(
            "Подпись в письме",
            "Почта",
            &["работа"],
            "sig",
            "Ctrl+Alt+1",
            "С уважением,\nИван Петров\nОтдел поддержки",
        ),
        snip(
            "Дата для заметки",
            "Почта",
            &["дата"],
            "dt",
            "",
            "Заметка от {date}, {time}",
        ),
        snip(
            "Ответ клиенту",
            "Поддержка",
            &["клиенты", "шаблон"],
            "thx",
            "Ctrl+Alt+2",
            "Здравствуйте!\nСпасибо за обращение, вернёмся с ответом сегодня.\n\n{clipboard}",
        ),
        snip(
            "Адрес офиса",
            "Поддержка",
            &["адрес"],
            "addr",
            "",
            "г. Новая Земля, ул. Примерная, д. 1, офис 12",
        ),
        snip(
            "SQL: активные клиенты",
            "Разработка",
            &["sql"],
            "sqlc",
            "",
            "SELECT id, name FROM customers WHERE active = 1;",
        ),
    ] {
        let _ = h.put_snippet(s, now);
    }
    let _ = (milk, h.pin(addr, true));
    h
}

impl App {
    fn demo_prepare(&mut self, ctx: &egui::Context, step: &str) {
        let first = |app: &App, kind: Kind| {
            let h = app.shared.history.lock().unwrap_or_else(|e| e.into_inner());
            h.query(&clipvault_core::Query {
                kinds: vec![kind],
                ..Default::default()
            })
            .first()
            .map(|i| i.id)
        };
        self.popup.reset_filters();
        self.popup.focus_search = false;
        self.popup.view = View::History;
        self.popup.cache_key_reset();
        match step {
            "search" => {
                self.popup.query = "отчёт".into();
            }
            "image" => {
                self.popup.selected = first(self, Kind::Image);
            }
            "color" => {
                self.popup.kind = None;
                self.popup.selected = first(self, Kind::Color);
            }
            "secret" => {
                self.popup.selected = {
                    let h = self.shared.history.lock().unwrap_or_else(|e| e.into_inner());
                    h.query(&clipvault_core::Query::default())
                        .iter()
                        .find(|i| i.volatile)
                        .map(|i| i.id)
                };
            }
            "snippets" => {
                self.popup.view = View::Snippets;
                let first = self
                    .shared
                    .history
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .snippets()
                    .into_iter()
                    .find(|s| s.name.starts_with("Ответ"))
                    .cloned();
                if let Some(s) = first {
                    self.popup.snip_draft = crate::snippets_ui::SnipDraft::from(&s);
                    self.popup.snip_editing = true;
                }
            }
            "hotkeys" => {
                self.popup.view = View::Settings;
                self.popup.scroll_to_hotkeys = true;
            }
            "keyword" => {
                self.popup.query = "thx".into();
            }
            "settings" => self.popup.view = View::Settings,
            "secrets" => {
                self.popup.view = View::Settings;
                self.popup.scroll_to_secrets = true;
            }
            _ => {}
        }
        ctx.request_repaint();
    }

    /// Drives the screenshot tour (no-op unless started with `--screenshots <dir>`).
    pub fn demo_step(&mut self, ctx: &egui::Context) {
        let Some(mut d) = self.demo.take() else { return };
        if let Some(dir) = d.dir.clone() {
            ctx.request_repaint();
            match STEPS.get(d.step) {
                None => ctx.send_viewport_cmd(ViewportCommand::Close),
                Some(&step) => {
                    if !d.prepared {
                        self.demo_prepare(ctx, step);
                        d.prepared = true;
                        d.since = Instant::now();
                    }
                    let shot = ctx.input(|i| {
                        i.events.iter().find_map(|e| match e {
                            Event::Screenshot { image, .. } => Some(image.clone()),
                            _ => None,
                        })
                    });
                    if let Some(img) = shot {
                        let file = dir.join(format!("{:02}-{step}.png", d.step + 1));
                        match save_png(&file, &img) {
                            Ok(()) => println!("saved {}", file.display()),
                            Err(e) => eprintln!("cannot save {}: {e}", file.display()),
                        }
                        d.step += 1;
                        d.prepared = false;
                        d.asked = false;
                    } else if !d.asked && d.since.elapsed() > SETTLE {
                        ctx.send_viewport_cmd(ViewportCommand::Screenshot(egui::UserData::default()));
                        d.asked = true;
                    }
                }
            }
        }
        self.demo = Some(d);
    }
}

fn save_png(path: &std::path::Path, img: &egui::ColorImage) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let file = std::fs::File::create(path).map_err(|e| e.to_string())?;
    let mut enc = png::Encoder::new(std::io::BufWriter::new(file), img.width() as u32, img.height() as u32);
    enc.set_color(png::ColorType::Rgba);
    enc.set_depth(png::BitDepth::Eight);
    let mut w = enc.write_header().map_err(|e| e.to_string())?;
    w.write_image_data(img.as_raw()).map_err(|e| e.to_string())
}
