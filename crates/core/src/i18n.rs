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
    ClearConfirm => (
        "Удалить всю историю и очистить буфер обмена?",
        "Delete the whole history and empty the clipboard?"
    ),
    CopyOnly => ("Копировать (без вставки)", "Copy (do not paste)"),
    CharsLines => ("{0} симв. · {1} строк", "{0} chars · {1} lines"),
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
        "Enter — вставить · Shift+Enter — как текст · Ctrl+Enter — копировать · 1–9 — по номеру · Ctrl+P — закрепить · Ctrl+D — удалить · Esc — закрыть",
        "Enter — paste · Shift+Enter — as text · Ctrl+Enter — copy · 1–9 — by number · Ctrl+P — pin · Ctrl+D — delete · Esc — close"
    ),
    // the compact quick-paste window
    CompactHints => (
        "Клик — вставить · Ctrl+клик — выбрать несколько · Shift+клик — как текст · Ctrl+Enter — копировать",
        "Click — paste · Ctrl+click — pick several · Shift+click — as plain text · Ctrl+Enter — copy"
    ),
    BtnExpand => ("Открыть полное окно", "Open the full window"),
    PasteSelected => ("Вставить выбранные ({0})", "Paste selected ({0})"),
    ClearSelection => ("Снять выбор", "Clear selection"),
    PasteAsText => ("Вставить как текст", "Paste as plain text"),
    PinHover => ("Закрепить / открепить", "Pin / unpin"),
    DeleteHover => ("Удалить запись", "Delete the entry"),
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
    // transforms
    TransformMenu => ("Преобразовать", "Transform"),
    TransformFailed => ("Не удалось преобразовать", "Could not transform"),
    TrJsonFormat => ("JSON: форматировать", "JSON: format"),
    TrJsonMinify => ("JSON: минифицировать", "JSON: minify"),
    TrBase64Enc => ("Base64: кодировать", "Base64: encode"),
    TrBase64Dec => ("Base64: декодировать", "Base64: decode"),
    TrUrlEnc => ("URL: кодировать", "URL: encode"),
    TrUrlDec => ("URL: декодировать", "URL: decode"),
    TrLayoutEnRu => ("Раскладка: EN -> RU", "Layout: EN -> RU"),
    TrLayoutRuEn => ("Раскладка: RU -> EN", "Layout: RU -> EN"),
    TrLayoutAuto => ("Раскладка: авто", "Layout: auto"),
    TrLinks => ("Извлечь ссылки", "Extract links"),
    TrEmails => ("Извлечь e-mail", "Extract e-mails"),
    TrUpper => ("ВЕРХНИЙ РЕГИСТР", "UPPER CASE"),
    TrLower => ("нижний регистр", "lower case"),
    TrTitle => ("Каждое Слово С Заглавной", "Title Case"),
    TrTrim => ("Обрезать пробелы", "Trim whitespace"),
    TrSort => ("Сортировать строки А-Я", "Sort lines A-Z"),
    TrSortDesc => ("Сортировать строки Я-А", "Sort lines Z-A"),
    TrUnique => ("Убрать повторы строк", "Remove duplicate lines"),
    PlainHotkey => ("Вставка без форматирования", "Paste without formatting"),
    PlainNote => (
        "Сочетание перехватывается глобально, а в некоторых приложениях Ctrl+Shift+V уже значит то же самое. Прежнее содержимое буфера возвращается после вставки.",
        "The hotkey is taken globally, and in some apps Ctrl+Shift+V already means the same. The previous clipboard content is put back after pasting."
    ),
    TransformHotkeys => ("Сочетания для преобразований (текст из буфера -> вставка результата)", "Transform hotkeys (clipboard text -> paste the result)"),
    AddRow => ("Добавить", "Add"),
    Remove => ("Убрать", "Remove"),
    // snippets
    TabHistory => ("История", "History"),
    TabSnippets => ("Сниппеты", "Snippets"),
    SnipNew => ("Новый сниппет", "New snippet"),
    SnipName => ("Название", "Name"),
    SnipFolder => ("Папка", "Folder"),
    SnipTags => ("Теги (через запятую)", "Tags (comma separated)"),
    SnipKeyword => ("Ключевое слово", "Keyword"),
    SnipHotkey => ("Сочетание", "Hotkey"),
    SnipText => ("Текст ({date}, {time}, {clipboard})", "Text ({date}, {time}, {clipboard})"),
    SnipSave => ("Сохранить", "Save"),
    SnipPreview => ("Что вставится", "What will be pasted"),
    SnipEmpty => ("Сниппетов пока нет — создайте первый", "No snippets yet — create the first one"),
    SnipPinned => ("Закреплённые записи", "Pinned entries"),
    SnipSearch => ("Название, тег, папка…", "Name, tag, folder…"),
    SnipKeywordHint => ("Сниппет по слову: {0} — Enter вставит", "Snippet by keyword: {0} — Enter pastes it"),
    SnipHotkeyBad => ("Неверное сочетание", "Invalid hotkey"),
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
    PressKeys => ("Нажмите сочетание клавиш… (Esc — отмена)", "Press the key combination… (Esc cancels)"),
    PressKeysClear => (
        "Нажмите сочетание клавиш… (Esc — отмена, Backspace — очистить)",
        "Press the key combination… (Esc cancels, Backspace clears)"
    ),
    RecordHotkey => ("Нажмите, чтобы записать сочетание клавиш", "Click to record a key combination"),
    HotkeyNotSet => ("не задано", "not set"),
    HotkeyWinNote => (
        "Сочетания с Win часть Windows оставляет себе (Win+L, Win+E…) — выберите другое, если не сработает.",
        "Windows keeps some Win combinations for itself (Win+L, Win+E…) — pick another if it does not work."
    ),
    CompactPopup => (
        "По сочетанию открывать компактное окно быстрой вставки (как Win+V): клик по записи сразу вставляет",
        "Open a compact quick-paste window on the hotkey (like Win+V): a click on an entry pastes it"
    ),
    HotkeyNote => (
        "Win+V зарезервировано Windows, поэтому окно вызывается другим сочетанием.",
        "Win+V is reserved by Windows, so the popup uses a different hotkey."
    ),
    QuickPasteMode => ("Быстрая вставка по Win+V", "Quick paste on Win+V"),
    WinVWindows => ("Windows (стандартная)", "Windows (built-in)"),
    WinVClipvault => ("Clipvault", "Clipvault"),
    WinVRestart => (
        "Чтобы переключение вступило в силу, перезапустите проводник (панель задач моргнёт на секунду).",
        "Restart Explorer for the switch to take effect (the taskbar flickers for a second)."
    ),
    WinVRestartBtn => ("Перезапустить проводник", "Restart Explorer"),
    WinVBusy => (
        "Win+V пока занято Windows — перезапустите проводник.",
        "Win+V is still held by Windows — restart Explorer."
    ),
    WinVNote => (
        "Пока Clipvault не запущен, Win+V не работает вообще — держите автозапуск включённым.",
        "While Clipvault is not running Win+V does nothing — keep autostart on."
    ),
    PopupAtCursor => (
        "Открывать полное окно у курсора мыши (компактное всегда открывается у курсора)",
        "Open the full window at the mouse cursor (the compact one always opens there)"
    ),
    BtnMinimize => ("Свернуть в трей", "Minimise to tray"),
    BtnCloseWindow => (
        "Закрыть окно (программа остаётся в трее; выйти можно через значок в трее)",
        "Close the window (the app stays in the tray; quit from the tray icon)"
    ),
    CompactPosLabel => ("Где открывать компактное окно", "Where the compact window opens"),
    PosCursor => ("Возле курсора мыши (настраивается ниже)", "Near the mouse cursor (set it below)"),
    OffsetHint => (
        "Курсор мыши — в центре схемы. Перетащите окно туда, где оно должно появляться относительно курсора.",
        "The mouse cursor is in the middle of the diagram. Drag the window to where it should appear relative to the cursor."
    ),
    OffsetValue => ("Смещение окна от курсора: {0}, {1}", "Window offset from the cursor: {0}, {1}"),
    OffsetReset => ("Сбросить", "Reset"),
    CursorLabel => ("курсор", "cursor"),
    CloseAfterPaste => ("Закрывать окно после вставки", "Close the window after pasting"),
    CloseAfterPasteNote => (
        "Если выключить, окно остаётся открытым: можно вставить несколько записей подряд. Закрыть — Esc, «×» или повторное нажатие сочетания; само по себе оно не закроется.",
        "When off, the window stays open so that several entries can be pasted in a row. Close it with Esc, the cross or the hotkey; it does not close by itself."
    ),
    PosCaret => (
        "Возле места ввода текста (мигающая палочка в чате; настраивается ниже)",
        "Near the text caret (the blinking bar in a chat; set it below)"
    ),
    CaretHint => (
        "Каретка (место ввода) — в центре схемы. Перетащите окно туда, где оно должно появляться относительно неё. Если у программы нет видимой каретки, окно откроется у курсора мыши.",
        "The text caret is in the middle of the diagram. Drag the window to where it should appear relative to it. If the program exposes no caret, the window opens at the mouse cursor."
    ),
    CaretValue => ("Смещение окна от каретки: {0}, {1}", "Window offset from the caret: {0}, {1}"),
    CaretLabel => ("каретка", "caret"),
    OffsetX => ("X", "X"),
    OffsetY => ("Y", "Y"),
    CompactSize => ("Размер компактного окна (ширина × высота)", "Compact window size (width × height)"),
    CompactMargin => ("Отступ от края экрана в углу", "Gap to the screen edge in the corner"),
    RestoreFocus => (
        "После закрытия без вставки вернуть фокус в прежнее окно и поле ввода",
        "After closing without pasting, give the focus back to the previous window and input field"
    ),
    CloseOnBlur => ("Закрывать окно, когда оно теряет фокус", "Close the window when it loses the focus"),
    PasteShiftInsert => (
        "Вставлять через Shift+Insert вместо Ctrl+V (для некоторых терминалов)",
        "Paste with Shift+Insert instead of Ctrl+V (for some terminals)"
    ),
    PosCenter => ("По центру экрана", "Center of the screen"),
    PosCorner => ("В правом нижнем углу", "Bottom-right corner"),
    PosLast => ("Там, где его оставили (перетащите окно)", "Where you left it (drag the window)"),
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
    DemoBanner => ("Демо: выдуманные записи", "Demo: made-up entries"),
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
