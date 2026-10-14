//! What a piece of plain text is: a colour, a link, a path or just text. Colours are also parsed for the preview.

use crate::model::Kind;

pub fn classify_text(text: &str) -> Kind {
    let t = text.trim();
    if t.is_empty() || t.len() > 2048 || t.contains(['\n', '\r', '\t']) {
        return Kind::Text;
    }
    if parse_color(t).is_some() {
        Kind::Color
    } else if is_link(t) {
        Kind::Link
    } else if is_path(t) {
        Kind::Path
    } else {
        Kind::Text
    }
}

pub fn is_link(t: &str) -> bool {
    if t.contains(char::is_whitespace) {
        return false;
    }
    let lower = t.to_ascii_lowercase();
    ["http://", "https://", "ftp://", "ftps://", "file://", "mailto:"]
        .iter()
        .any(|p| lower.starts_with(p) && lower.len() > p.len())
        || (lower.starts_with("www.") && lower[4..].contains('.') && lower.len() > 6)
}

/// Absolute Windows paths: `C:\dir\file`, `\\server\share`, `%APPDATA%\x`, `~\x`.
pub fn is_path(t: &str) -> bool {
    if t.contains(['<', '>', '"', '|', '?', '*']) || t.len() < 3 {
        return false;
    }
    let b = t.as_bytes();
    let drive = b[0].is_ascii_alphabetic() && b[1] == b':' && (b[2] == b'\\' || b[2] == b'/');
    let unc = t.starts_with("\\\\") && t.len() > 3 && !t[2..].starts_with('\\');
    let env = t.starts_with('%') && t[1..].find("%\\").is_some_and(|i| i > 0);
    let home = t.starts_with("~\\") || t.starts_with("~/");
    (drive || unc || env || home) && !t[2..].contains(':')
}

/// `#RGB`, `#RGBA`, `#RRGGBB`, `#RRGGBBAA`, `rgb()/rgba()` (comma or space separated) and `hsl()/hsla()`.
pub fn parse_color(s: &str) -> Option<[u8; 4]> {
    let s = s.trim();
    if let Some(hex) = s.strip_prefix('#') {
        return parse_hex(hex);
    }
    let lower = s.to_ascii_lowercase();
    let (name, rest) = lower.split_once('(')?;
    let inner = rest.strip_suffix(')')?;
    let parts: Vec<&str> = inner
        .split(|c: char| c == ',' || c == '/' || c.is_whitespace())
        .filter(|p| !p.is_empty())
        .collect();
    if parts.len() != 3 && parts.len() != 4 {
        return None;
    }
    let alpha = match parts.get(3) {
        None => 255,
        Some(a) => {
            let v = if let Some(p) = a.strip_suffix('%') {
                p.parse::<f64>().ok()? / 100.0
            } else {
                a.parse::<f64>().ok()?
            };
            (v.clamp(0.0, 1.0) * 255.0).round() as u8
        }
    };
    match name.trim() {
        "rgb" | "rgba" => {
            let mut c = [0u8; 4];
            for (i, p) in parts.iter().take(3).enumerate() {
                c[i] = if let Some(pct) = p.strip_suffix('%') {
                    (pct.parse::<f64>().ok()?.clamp(0.0, 100.0) * 2.55).round() as u8
                } else {
                    let v: u16 = p.parse().ok()?;
                    if v > 255 {
                        return None;
                    }
                    v as u8
                };
            }
            c[3] = alpha;
            Some(c)
        }
        "hsl" | "hsla" => {
            let h: f64 = parts[0].trim_end_matches("deg").parse().ok()?;
            let sat: f64 = parts[1].strip_suffix('%')?.parse().ok()?;
            let l: f64 = parts[2].strip_suffix('%')?.parse().ok()?;
            if !(0.0..=100.0).contains(&sat) || !(0.0..=100.0).contains(&l) {
                return None;
            }
            let [r, g, b] = hsl_to_rgb(h.rem_euclid(360.0), sat / 100.0, l / 100.0);
            Some([r, g, b, alpha])
        }
        _ => None,
    }
}

fn parse_hex(hex: &str) -> Option<[u8; 4]> {
    if !hex.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    let d = |i: usize| u8::from_str_radix(&hex[i..=i], 16).map(|v| v * 17);
    let p = |i: usize| u8::from_str_radix(&hex[i..i + 2], 16);
    match hex.len() {
        3 => Some([d(0).ok()?, d(1).ok()?, d(2).ok()?, 255]),
        4 => Some([d(0).ok()?, d(1).ok()?, d(2).ok()?, d(3).ok()?]),
        6 => Some([p(0).ok()?, p(2).ok()?, p(4).ok()?, 255]),
        8 => Some([p(0).ok()?, p(2).ok()?, p(4).ok()?, p(6).ok()?]),
        _ => None,
    }
}

fn hsl_to_rgb(h: f64, s: f64, l: f64) -> [u8; 3] {
    let c = (1.0 - (2.0 * l - 1.0).abs()) * s;
    let x = c * (1.0 - ((h / 60.0) % 2.0 - 1.0).abs());
    let m = l - c / 2.0;
    let (r, g, b) = match (h / 60.0) as u32 {
        0 => (c, x, 0.0),
        1 => (x, c, 0.0),
        2 => (0.0, c, x),
        3 => (0.0, x, c),
        4 => (x, 0.0, c),
        _ => (c, 0.0, x),
    };
    [r, g, b].map(|v| ((v + m) * 255.0).round().clamp(0.0, 255.0) as u8)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn colors() {
        assert_eq!(parse_color("#fff"), Some([255, 255, 255, 255]));
        assert_eq!(parse_color("#1E90FF"), Some([30, 144, 255, 255]));
        assert_eq!(parse_color("#1e90ff80"), Some([30, 144, 255, 128]));
        assert_eq!(parse_color("rgb(10, 20, 30)"), Some([10, 20, 30, 255]));
        assert_eq!(parse_color("rgb(10 20 30 / 50%)"), Some([10, 20, 30, 128]));
        assert_eq!(parse_color("rgba(255,0,0,0.5)"), Some([255, 0, 0, 128]));
        assert_eq!(parse_color("hsl(0, 100%, 50%)"), Some([255, 0, 0, 255]));
        assert_eq!(parse_color("hsl(120deg 100% 25%)"), Some([0, 128, 0, 255]));
        assert_eq!(parse_color("hsl(240, 100%, 50%)"), Some([0, 0, 255, 255]));
        for bad in [
            "#12",
            "#ggg",
            "rgb(300,0,0)",
            "rgb(1,2)",
            "hsl(1,2,3)",
            "color",
            "#12345",
        ] {
            assert_eq!(parse_color(bad), None, "{bad}");
        }
    }

    #[test]
    fn kinds() {
        assert_eq!(classify_text("#ff00aa"), Kind::Color);
        assert_eq!(classify_text("https://example.com/a?b=1"), Kind::Link);
        assert_eq!(classify_text("www.example.com"), Kind::Link);
        assert_eq!(classify_text("mailto:a@b.c"), Kind::Link);
        assert_eq!(classify_text(r"C:\Users\demo\file.txt"), Kind::Path);
        assert_eq!(classify_text(r"\\server\share\x"), Kind::Path);
        assert_eq!(classify_text(r"%APPDATA%\Clipvault"), Kind::Path);
        assert_eq!(classify_text("just some words"), Kind::Text);
        assert_eq!(classify_text("see https://example.com now"), Kind::Text);
        assert_eq!(classify_text("note: 12:30"), Kind::Text);
        assert_eq!(classify_text("#hashtag"), Kind::Text);
        assert_eq!(classify_text("a\nb"), Kind::Text);
    }
}
