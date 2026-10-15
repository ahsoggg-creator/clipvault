//! The localisation table: every user-visible string, in Russian (default) and English.
//! A missing translation is a compile error, not a blank label.

use crate::config::Lang;

macro_rules! strings {
    ($($id:ident => ($ru:expr, $en:expr)),* $(,)?) => {
        #[derive(Clone, Copy, Debug, PartialEq, Eq)]
        pub enum S { $($id),* }

        impl S {
            pub const ALL: &'static [S] = &[$(S::$id),*];

            pub fn get(self, lang: Lang) -> &'static str {
                match self {
                    $(S::$id => match lang { Lang::Ru => $ru, Lang::En => $en }),*
                }
            }
        }
    };
}

strings! {
    // popup
    SearchHint => ("Поиск по истории…", "Search the history…"),
    All => ("Все", "All"),
    KindText => ("Текст", "Text"),
    KindRich => ("Формат.", "Rich"),
    KindImage => ("Картинка", "Image"),
    KindFiles => ("Файлы", "Files"),
    KindColor => ("Цвет", "Color"),
    KindLink => ("Ссылка", "Link"),
    KindPath => ("Путь", "Path"),
    AllApps => ("Все приложения", "All apps"),
    Application => ("Приложение", "App"),
    DateAny => ("За всё время", "Any time"),
    DateToday => ("Сегодня", "Today"),
    DateWeek => ("7 дней", "7 days"),
    DateMonth => ("30 дней", "30 days"),
    PinnedOnly => ("Закреплённые", "Pinned"),
    Paste => ("Вставить", "Paste"),
    Pin => ("Закрепить", "Pin"),
    Unpin => ("Открепить", "Unpin"),
    Delete => ("Удалить", "Delete"),
    ClearAll => ("Очистить всё", "Clear all"),
    ClearConfirm => ("Удалить всю историю?", "Delete the whole history?"),
    ClearKeepPinned => ("Кроме закреплённых", "Keep pinned"),
    ClearEverything => ("Вместе с закреплёнными", "Including pinned"),
    Cancel => ("Отмена", "Cancel"),
    EmptyHistory => ("История пуста — скопируйте что-нибудь", "The history is empty — copy something"),
    NothingFound => ("Ничего не найдено", "Nothing found"),
    SelectEntry => ("Выберите запись слева", "Select an entry on the left"),
    Source => ("Источник", "Source"),
    Copied => ("Скопировано", "Copied"),
    Repeats => ("Копирований", "Copies"),
    Size => ("Размер", "Size"),
    Type => ("Тип", "Type"),
    ImageLabel => ("Изображение", "Image"),
    FilesLabel => ("ещё", "more"),
    InMemoryOnly => ("только в памяти", "memory only"),
    Masked => ("скрыто", "masked"),
    HintsLine => (
        "Enter — вставить · 1–9 — вставить по номеру · Ctrl+P — закрепить · Ctrl+D — удалить · Esc — закрыть",
        "Enter — paste · 1–9 — paste by number · Ctrl+P — pin · Ctrl+D — delete · Esc — close"
    ),
    AgoNow => ("только что", "just now"),
    AgoMin => ("{0} мин назад", "{0} min ago"),
    AgoHour => ("{0} ч назад", "{0} h ago"),
    AgoDay => ("{0} дн. назад", "{0} d ago"),
    EntriesCount => ("записей", "entries"),
    PausedBanner => ("Запись приостановлена", "Recording is paused"),
    IncognitoBanner => ("Инкогнито: история не пишется", "Incognito: nothing is recorded"),
    Resume => ("Продолжить запись", "Resume recording"),
    PauseRecording => ("Приостановить запись", "Pause recording"),
    Incognito => ("Режим инкогнито", "Incognito mode"),
    // toasts and notifications
    ElevatedTarget => (
        "Это окно запущено от администратора: Windows не даёт обычной программе нажимать в нём клавиши. Запись скопирована — нажмите Ctrl+V сами.",
        "That window runs as administrator: Windows does not let a normal program send keys to it. The entry is on the clipboard — press Ctrl+V yourself."
    ),
    PasteFailed => ("Не удалось вставить", "Could not paste"),
    HotkeyBusy => ("Сочетание уже занято другой программой", "That hotkey is already used by another program"),
    HotkeyInvalid => ("Некорректное сочетание", "Invalid hotkey"),
    StoreError => ("Ошибка хранилища", "Storage error"),
    AdminBadge => ("Администратор", "Administrator"),
    AdminOpen => ("Открыть окно от администратора", "Open the administrator window"),
    AdminNote => (
        "Обычная копия не может вставлять в окна, запущенные от администратора (защита Windows). Отдельное окно с правами администратора — может; оно только вставляет и ничего не записывает. Один запрос UAC, основная копия остаётся без повышения.",
        "The normal copy cannot paste into windows running as administrator (a Windows protection). A separate window with administrator rights can; it only pastes and records nothing. One UAC prompt; the main copy stays unelevated."
    ),
    AdminHotkey => ("Сочетание окна администратора", "Administrator window hotkey"),
    // secrets
    SecApiKey => ("ключ API", "API key"),
    SecToken => ("токен", "token"),
    SecPrivateKey => ("закрытый ключ", "private key"),
    SecConnection => ("строка подключения", "connection string"),
    SecSeed => ("сид-фраза", "seed phrase"),
    SecCard => ("номер карты", "card number"),
    SecPassword => ("пароль", "password"),
    SecCustom => ("свой шаблон", "custom pattern"),
    SecretsSection => ("Защита секретов", "Secret protection"),
    SecretsEnabled => ("Искать секреты до записи на диск", "Look for secrets before writing to disk"),
    PolicyLabel => ("Что делать с секретом", "What to do with a secret"),
    PolicyDont => ("Не сохранять", "Do not store"),
    PolicyMemory => ("Держать в памяти и стереть из буфера", "Keep in memory, wipe from the clipboard"),
    PolicyMask => ("Сохранить, но замаскировать", "Store, but masked"),
    ClearAfter => ("Стереть из буфера через, секунд", "Wipe from the clipboard after, seconds"),
    SensitivityLabel => ("Чувствительность к «похожим на пароль» строкам", "Sensitivity to password-looking strings"),
    SensOff => ("Выкл.", "Off"),
    SensLow => ("Низкая", "Low"),
    SensMedium => ("Средняя", "Medium"),
    SensHigh => ("Высокая (больше ложных срабатываний)", "High (more false alarms)"),
    CustomPatterns => ("Свои регулярные выражения (по одному в строке)", "Your own regular expressions (one per line)"),
    BadPatterns => ("Не скомпилировались строки:", "These lines did not compile:"),
    RespectFlags => (
        "Уважать флаги приложений (менеджеры паролей просят не записывать)",
        "Obey the apps' own flags (password managers ask not to be recorded)"
    ),
    BlacklistLabel => ("Программы, из которых не записывать (по одной в строке, имя exe)", "Programs never recorded (one per line, exe name)"),
    SecretsNote => (
        "Секрет, оставленный в памяти, виден в списке скрытым и пропадает при выходе из программы.",
        "A secret kept in memory shows hidden in the list and is gone when the program exits."
    ),
    Reveal => ("Показать", "Show"),
    HideSecret => ("Скрыть", "Hide"),
    // tray
    TrayOpen => ("Открыть Clipvault", "Open Clipvault"),
    TraySettings => ("Настройки", "Settings"),
    TrayQuit => ("Выход", "Exit"),
    // settings
    SettingsTitle => ("Настройки", "Settings"),
    Back => ("К истории", "Back"),
    SecGeneral => ("Основное", "General"),
    Language => ("Язык", "Language"),
    PopupHotkey => ("Сочетание для окна", "Popup hotkey"),
    Change => ("Изменить", "Change"),
    PressKeys => ("Нажмите сочетание клавиш…", "Press the key combination…"),
    HotkeyNote => (
        "Win+V зарезервировано Windows, поэтому окно вызывается другим сочетанием.",
        "Win+V is reserved by Windows, so the popup uses a different hotkey."
    ),
    Autostart => ("Запускать при входе в Windows", "Start with Windows"),
    SecLimits => ("Лимиты хранения", "Storage limits"),
    MaxItemKb => ("Максимальный размер записи, КБ", "Largest entry, KB"),
    MaxAgeDays => ("Хранить не дольше, дней (0 — всегда)", "Keep for at most, days (0 = forever)"),
    MaxItems => ("Максимум записей (0 — без лимита)", "Most entries (0 = unlimited)"),
    LimitsNote => ("Закреплённые записи лимитами не затрагиваются.", "Pinned entries are never removed by limits."),
    SecUpdates => ("Обновления", "Updates"),
    AutoUpdate => ("Проверять обновления раз в сутки (единственный сетевой запрос)", "Check for updates once a day (the only network request)"),
    CheckNow => ("Проверить сейчас", "Check now"),
    UpToDate => ("Установлена последняя версия", "You have the latest version"),
    Checking => ("Проверка…", "Checking…"),
    UpdateAvailable => ("Доступна версия", "Version available"),
    UpdateInstall => ("Обновить", "Update"),
    UpdateDownloading => ("Скачивание…", "Downloading…"),
    UpdateReady => ("Обновление установлено — перезапуск…", "Update installed — restarting…"),
    UpdateFailed => ("Не удалось обновить", "Update failed"),
    SecDiagnostics => ("Диагностика", "Diagnostics"),
    OpenLogs => ("Открыть папку журналов", "Open the logs folder"),
    CopyDiag => ("Скопировать диагностику", "Copy diagnostics"),
    DiagCopied => ("Диагностика скопирована (без содержимого буфера)", "Diagnostics copied (no clipboard content)"),
    DiagNote => (
        "В журнале и отчётах о сбое нет содержимого буфера обмена — только типы и счётчики.",
        "Logs and crash reports never contain clipboard content — only types and counters."
    ),
    SecSystem => ("Установка", "Installation"),
    InstallHere => ("Установить в профиль пользователя", "Install for this user"),
    Installed => ("Установлено", "Installed"),
    Version => ("Версия", "Version"),
    DemoBanner => ("Демо: выдуманные записи, ничего не читается и не сохраняется", "Demo: made-up entries, nothing is read or saved"),
    // installer
    SetupTitle => ("Установка Clipvault", "Install Clipvault"),
    SetupFolder => ("Папка установки", "Install folder"),
    SetupDesktop => ("Ярлык на рабочем столе", "Desktop shortcut"),
    SetupNoAdmin => (
        "Ярлык в меню «Пуск» создаётся всегда. Права администратора не нужны.",
        "A Start menu shortcut is always created. No administrator rights needed."
    ),
    SetupInstall => ("Установить", "Install"),
    SetupUpgrade => ("Обновить", "Upgrade"),
    SetupRunning => ("Установка…", "Installing…"),
    SetupDone => ("Готово", "Done"),
    SetupDoneText => (
        "Clipvault установлен и запущен. Окно вызывается сочетанием клавиш; значок — в области уведомлений.",
        "Clipvault is installed and running. Open the popup with the hotkey; the icon is in the notification area."
    ),
    SetupFailed => ("Не удалось установить", "Installation failed"),
    SetupRetry => ("Попробовать ещё раз", "Try again"),
    SetupLaunch => ("Запустить Clipvault", "Launch Clipvault"),
    Close => ("Закрыть", "Close"),
    UninstallTitle => ("Удаление Clipvault", "Uninstall Clipvault"),
    UninstallQuestion => ("Удалить Clipvault с этого компьютера?", "Remove Clipvault from this computer?"),
    UninstallNote => (
        "Будут удалены программа, ярлыки и запись в «Приложениях». История и ключ шифрования останутся, если не отметить пункт ниже.",
        "The program, shortcuts and the Apps & features entry are removed. The history and its key stay unless you tick the box below."
    ),
    UninstallData => ("Удалить также историю, ключ и настройки", "Also delete the history, key and settings"),
    UninstallDone => ("Clipvault удалён", "Clipvault was removed"),
    UninstallFailed => ("Не удалось удалить", "Could not uninstall"),
    CrashNotice => ("В прошлый раз программа завершилась аварийно. Отчёт без содержимого буфера:", "The program crashed last time. A report without clipboard content:"),
}

/// `tr!(lang, Key)`-style helper with `{0}`-style substitution for the few strings that carry a value.
pub fn fmt(s: S, lang: Lang, args: &[&str]) -> String {
    let mut out = s.get(lang).to_owned();
    for (i, a) in args.iter().enumerate() {
        out = out.replace(&format!("{{{i}}}"), a);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_string_is_filled_in_both_languages() {
        for s in S::ALL {
            assert!(!s.get(Lang::Ru).is_empty() && !s.get(Lang::En).is_empty(), "{s:?}");
        }
    }

    #[test]
    fn russian_is_cyrillic_and_english_is_not() {
        for s in S::ALL {
            let ru = s.get(Lang::Ru);
            let en = s.get(Lang::En);
            assert!(
                !en.chars().any(|c| ('а'..='я').contains(&c.to_ascii_lowercase())),
                "{s:?} en has Cyrillic"
            );
            // short technical labels (Pin, Type...) may legitimately match; only flag clear misses
            if ru.chars().count() > 12 {
                assert!(
                    ru.chars()
                        .any(|c| ('а'..='я').contains(&c.to_lowercase().next().unwrap_or(c))),
                    "{s:?}"
                );
            }
        }
    }
}
