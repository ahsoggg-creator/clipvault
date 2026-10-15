//! Hotkey strings such as `Ctrl+Alt+V` <-> the (modifiers, virtual key) pair `RegisterHotKey` wants.

pub const MOD_ALT: u32 = 0x1;
pub const MOD_CONTROL: u32 = 0x2;
pub const MOD_SHIFT: u32 = 0x4;
pub const MOD_WIN: u32 = 0x8;

const NAMED: [(&str, u32); 16] = [
    ("Space", 0x20),
    ("Enter", 0x0D),
    ("Tab", 0x09),
    ("Insert", 0x2D),
    ("Delete", 0x2E),
    ("Home", 0x24),
    ("End", 0x23),
    ("PageUp", 0x21),
    ("PageDown", 0x22),
    ("Left", 0x25),
    ("Up", 0x26),
    ("Right", 0x27),
    ("Down", 0x28),
    ("Backspace", 0x08),
    ("Pause", 0x13),
    ("PrintScreen", 0x2C),
];

/// Parses `Ctrl+Alt+V`, `ctrl+shift+f5`, `Win+Space`. At least one modifier and exactly one key.
pub fn parse_hotkey(s: &str) -> Option<(u32, u32)> {
    let mut mods = 0;
    let mut key = None;
    for part in s.split('+').map(str::trim) {
        let m = match part.to_ascii_lowercase().as_str() {
            "ctrl" | "control" => MOD_CONTROL,
            "alt" => MOD_ALT,
            "shift" => MOD_SHIFT,
            "win" | "super" | "meta" => MOD_WIN,
            _ => 0,
        };
        if m != 0 {
            mods |= m;
        } else if key.replace(key_code(part)?).is_some() {
            return None;
        }
    }
    (mods != 0).then_some((mods, key?))
}

fn key_code(name: &str) -> Option<u32> {
    let n = name.trim();
    let b = n.as_bytes();
    if b.len() == 1 && b[0].is_ascii_alphanumeric() {
        return Some(u32::from(b[0].to_ascii_uppercase()));
    }
    if let Some(num) = n.strip_prefix(['F', 'f'])
        && let Ok(f) = num.parse::<u32>()
        && (1..=24).contains(&f)
    {
        return Some(0x6F + f);
    }
    NAMED.iter().find(|(k, _)| k.eq_ignore_ascii_case(n)).map(|(_, v)| *v)
}

/// Canonical text for a hotkey, e.g. `Ctrl+Alt+V`.
pub fn format_hotkey(mods: u32, vk: u32) -> String {
    let mut parts: Vec<String> = Vec::new();
    for (flag, name) in [
        (MOD_CONTROL, "Ctrl"),
        (MOD_ALT, "Alt"),
        (MOD_SHIFT, "Shift"),
        (MOD_WIN, "Win"),
    ] {
        if mods & flag != 0 {
            parts.push(name.into());
        }
    }
    parts.push(match vk {
        0x30..=0x39 | 0x41..=0x5A => char::from(vk as u8).to_string(),
        0x70..=0x87 => format!("F{}", vk - 0x6F),
        _ => NAMED
            .iter()
            .find(|(_, v)| *v == vk)
            .map_or_else(|| format!("0x{vk:X}"), |(k, _)| (*k).to_owned()),
    });
    parts.join("+")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_and_formats() {
        assert_eq!(parse_hotkey("Ctrl+Alt+V"), Some((MOD_CONTROL | MOD_ALT, 0x56)));
        assert_eq!(parse_hotkey("shift + ctrl + f5"), Some((MOD_CONTROL | MOD_SHIFT, 0x74)));
        assert_eq!(parse_hotkey("Win+Space"), Some((MOD_WIN, 0x20)));
        assert_eq!(parse_hotkey("Ctrl+1"), Some((MOD_CONTROL, 0x31)));
        for bad in ["V", "Ctrl", "Ctrl+Alt", "Ctrl+A+B", "Ctrl+F25", "Ctrl+Nope", ""] {
            assert_eq!(parse_hotkey(bad), None, "{bad:?}");
        }
        for s in ["Ctrl+Alt+V", "Ctrl+Shift+F12", "Alt+Space", "Ctrl+Alt+Shift+PageDown"] {
            let (m, k) = parse_hotkey(s).unwrap();
            assert_eq!(format_hotkey(m, k), s);
        }
    }
}
