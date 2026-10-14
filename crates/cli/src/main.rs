//! `clipvault-cli`: the history from a terminal. It opens the same encrypted vault as the app
//! (the key is unwrapped by Windows DPAPI for the current user), so it only works for the user who owns it.

use std::io::Write;
use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Parser, Subcommand};
use clipvault_core::config::open_history;
use clipvault_core::timefmt::format_utc;
use clipvault_core::{History, Item, Kind, Query};

#[derive(Parser)]
#[command(
    name = "clipvault-cli",
    version,
    about = "Clipvault: история буфера обмена из консоли"
)]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Последние записи
    List {
        /// Сколько записей показать
        #[arg(short = 'n', long, default_value_t = 20)]
        limit: usize,
        /// Только этот тип: text, rich, image, files, color, link, path
        #[arg(short, long)]
        kind: Option<String>,
        /// Только закреплённые
        #[arg(long)]
        pinned: bool,
    },
    /// Нечёткий поиск по истории
    Search {
        query: String,
        #[arg(short = 'n', long, default_value_t = 20)]
        limit: usize,
        #[arg(short, long)]
        kind: Option<String>,
    },
    /// Вывести содержимое записи (текст — в stdout; картинка — PNG в файл `--out`)
    Get {
        id: u64,
        /// Записать в файл вместо stdout (обязательно для изображений)
        #[arg(short, long)]
        out: Option<PathBuf>,
    },
    /// Удалить историю (закреплённые можно оставить)
    Purge {
        /// Оставить закреплённые записи
        #[arg(long)]
        keep_pinned: bool,
        /// Не спрашивать подтверждение
        #[arg(short, long)]
        yes: bool,
    },
}

fn preview(item: &Item) -> String {
    let t = match item.kind {
        Kind::Image => {
            let (w, h) = item.img.unwrap_or((0, 0));
            format!("[изображение {w}x{h}]")
        }
        _ => item.text.split_whitespace().collect::<Vec<_>>().join(" "),
    };
    let cut: String = t.chars().take(80).collect();
    if t.chars().count() > 80 {
        format!("{cut}…")
    } else {
        cut
    }
}

fn parse_kind(k: &Option<String>) -> Result<Vec<Kind>, String> {
    match k {
        None => Ok(Vec::new()),
        Some(s) => Kind::parse(s)
            .map(|k| vec![k])
            .ok_or_else(|| format!("неизвестный тип «{s}» (text, rich, image, files, color, link, path)")),
    }
}

fn print_items(items: &[&Item], limit: usize) {
    let mut out = std::io::stdout().lock();
    for it in items.iter().take(limit) {
        let _ = writeln!(
            out,
            "{:>6}  {:<5}  {}{}  {}  {}",
            it.id,
            it.kind.as_str(),
            if it.pinned { "*" } else { " " },
            format_utc(it.last_used),
            if it.source_app.is_empty() { "-" } else { &it.source_app },
            preview(it)
        );
    }
}

fn run(cli: Cli, h: &mut History) -> Result<(), String> {
    match cli.cmd {
        Cmd::List { limit, kind, pinned } => {
            let q = Query {
                kinds: parse_kind(&kind)?,
                pinned_only: pinned,
                ..Default::default()
            };
            print_items(&h.query(&q), limit);
        }
        Cmd::Search { query, limit, kind } => {
            let q = Query {
                text: query,
                kinds: parse_kind(&kind)?,
                ..Default::default()
            };
            let hits = h.query(&q);
            if hits.is_empty() {
                return Err("ничего не найдено".into());
            }
            print_items(&hits, limit);
        }
        Cmd::Get { id, out } => {
            let item = h.get(id).ok_or_else(|| format!("записи {id} нет"))?.clone();
            let bytes = if item.kind == Kind::Image {
                if out.is_none() {
                    return Err("это изображение: укажите файл через --out".into());
                }
                h.blob(id).map_err(|e| e.to_string())?.ok_or("у записи нет данных")?
            } else {
                item.text.clone().into_bytes()
            };
            match out {
                Some(p) => std::fs::write(&p, bytes).map_err(|e| format!("{}: {e}", p.display()))?,
                None => {
                    let mut o = std::io::stdout().lock();
                    o.write_all(&bytes).map_err(|e| e.to_string())?;
                }
            }
        }
        Cmd::Purge { keep_pinned, yes } => {
            let n = h.len();
            if !yes {
                eprint!(
                    "Удалить {n} записей{}? [y/N] ",
                    if keep_pinned {
                        " (кроме закреплённых)"
                    } else {
                        ""
                    }
                );
                let mut a = String::new();
                std::io::stdin().read_line(&mut a).map_err(|e| e.to_string())?;
                if !matches!(a.trim().to_lowercase().as_str(), "y" | "yes" | "д" | "да") {
                    return Err("отменено".into());
                }
            }
            h.clear(keep_pinned).map_err(|e| e.to_string())?;
            println!("Удалено. Осталось записей: {}", h.len());
        }
    }
    Ok(())
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    let mut history = match open_history() {
        Ok(h) => h,
        Err(e) => {
            eprintln!("Не удалось открыть историю: {e}");
            return ExitCode::from(2);
        }
    };
    match run(cli, &mut history) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("{e}");
            ExitCode::FAILURE
        }
    }
}
