//! Quick transformations of text: JSON, Base64, URL encoding, keyboard layout, extraction, case, whitespace,
//! sorting. Every transformation is a pure function `&str -> Result<String, TransformError>`.

use std::collections::HashSet;
use std::sync::OnceLock;

use regex::Regex;

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum TransformError {
    #[error("JSON: {0}")]
    Json(String),
    #[error("Base64: {0}")]
    Base64(String),
    #[error("URL: {0}")]
    Url(String),
    #[error("ничего не найдено")]
    NothingFound,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Transform {
    JsonFormat,
    JsonMinify,
    Base64Encode,
    Base64Decode,
    UrlEncode,
    UrlDecode,
    /// Latin typed on the Russian layout -> Cyrillic (`ghbdtn` -> `привет`).
    LayoutEnToRu,
    /// Cyrillic typed on the English layout -> Latin (`руддщ` -> `hello`).
    LayoutRuToEn,
    /// Picks the direction from the letters in the text.
    LayoutAuto,
    ExtractLinks,
    ExtractEmails,
    Upper,
    Lower,
    Title,
    Trim,
    SortLines,
    SortLinesDesc,
    UniqueLines,
}

impl Transform {
    pub const ALL: [Transform; 18] = [
        Transform::JsonFormat,
        Transform::JsonMinify,
        Transform::Base64Encode,
        Transform::Base64Decode,
        Transform::UrlEncode,
        Transform::UrlDecode,
        Transform::LayoutEnToRu,
        Transform::LayoutRuToEn,
        Transform::LayoutAuto,
        Transform::ExtractLinks,
        Transform::ExtractEmails,
        Transform::Upper,
        Transform::Lower,
        Transform::Title,
        Transform::Trim,
        Transform::SortLines,
        Transform::SortLinesDesc,
        Transform::UniqueLines,
    ];

    /// The name used on the command line and in `settings.json`.
    pub fn name(self) -> &'static str {
        match self {
            Transform::JsonFormat => "json-format",
            Transform::JsonMinify => "json-minify",
            Transform::Base64Encode => "base64-encode",
            Transform::Base64Decode => "base64-decode",
            Transform::UrlEncode => "url-encode",
            Transform::UrlDecode => "url-decode",
            Transform::LayoutEnToRu => "layout-en-ru",
            Transform::LayoutRuToEn => "layout-ru-en",
            Transform::LayoutAuto => "layout-auto",
            Transform::ExtractLinks => "extract-links",
            Transform::ExtractEmails => "extract-emails",
            Transform::Upper => "upper",
            Transform::Lower => "lower",
            Transform::Title => "title",
            Transform::Trim => "trim",
            Transform::SortLines => "sort",
            Transform::SortLinesDesc => "sort-desc",
            Transform::UniqueLines => "unique",
        }
    }

    pub fn parse(name: &str) -> Option<Transform> {
        let n = name.trim().to_ascii_lowercase().replace('_', "-");
        Transform::ALL.into_iter().find(|t| t.name() == n)
    }

    pub fn apply(self, input: &str) -> Result<String, TransformError> {
        match self {
            Transform::JsonFormat => json(input, true),
            Transform::JsonMinify => json(input, false),
            Transform::Base64Encode => Ok(base64_encode(input.as_bytes())),
            Transform::Base64Decode => base64_decode(input),
            Transform::UrlEncode => Ok(url_encode(input)),
            Transform::UrlDecode => url_decode(input),
            Transform::LayoutEnToRu => Ok(map_layout(input, true)),
            Transform::LayoutRuToEn => Ok(map_layout(input, false)),
            Transform::LayoutAuto => Ok(layout_auto(input)),
            Transform::ExtractLinks => extract_links(input),
            Transform::ExtractEmails => extract_emails(input),
            Transform::Upper => Ok(input.to_uppercase()),
            Transform::Lower => Ok(input.to_lowercase()),
            Transform::Title => Ok(title_case(input)),
            Transform::Trim => Ok(trim_lines(input)),
            Transform::SortLines => Ok(sort_lines(input, false)),
            Transform::SortLinesDesc => Ok(sort_lines(input, true)),
            Transform::UniqueLines => Ok(unique_lines(input)),
        }
    }
}

fn json(input: &str, pretty: bool) -> Result<String, TransformError> {
    let v: serde_json::Value = serde_json::from_str(input.trim()).map_err(|e| {
        let what = match e.classify() {
            serde_json::error::Category::Eof => "неожиданный конец текста",
            serde_json::error::Category::Syntax => "синтаксическая ошибка",
            serde_json::error::Category::Data => "неверные данные",
            serde_json::error::Category::Io => "ошибка чтения",
        };
        TransformError::Json(format!("{what} (строка {}, столбец {})", e.line(), e.column()))
    })?;
    let out = if pretty {
        serde_json::to_string_pretty(&v)
    } else {
        serde_json::to_string(&v)
    };
    out.map_err(|e| TransformError::Json(e.to_string()))
}

const B64: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

pub fn base64_encode(data: &[u8]) -> String {
    let mut s = String::with_capacity(data.len().div_ceil(3) * 4);
    for c in data.chunks(3) {
        let n =
            (u32::from(c[0]) << 16) | (u32::from(*c.get(1).unwrap_or(&0)) << 8) | u32::from(*c.get(2).unwrap_or(&0));
        for i in 0..4 {
            if i <= c.len() {
                s.push(B64[((n >> (18 - 6 * i)) & 63) as usize] as char);
            } else {
                s.push('=');
            }
        }
    }
    s
}

fn base64_decode(input: &str) -> Result<String, TransformError> {
    let mut bits = 0u32;
    let mut nbits = 0;
    let mut out = Vec::new();
    for c in input.chars().filter(|c| !c.is_whitespace()) {
        let v = match c {
            'A'..='Z' => c as u32 - 'A' as u32,
            'a'..='z' => c as u32 - 'a' as u32 + 26,
            '0'..='9' => c as u32 - '0' as u32 + 52,
            '+' | '-' => 62,
            '/' | '_' => 63,
            '=' => break,
            _ => return Err(TransformError::Base64(format!("недопустимый символ «{c}»"))),
        };
        bits = (bits << 6) | v;
        nbits += 6;
        if nbits >= 8 {
            nbits -= 8;
            out.push((bits >> nbits) as u8);
            bits &= (1 << nbits) - 1;
        }
    }
    if out.is_empty() && !input.trim().is_empty() {
        return Err(TransformError::Base64("слишком короткая строка".into()));
    }
    String::from_utf8(out).map_err(|_| TransformError::Base64("результат не является текстом UTF-8".into()))
}

fn url_encode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b'~') {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

fn url_decode(s: &str) -> Result<String, TransformError> {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' {
            let hex = s
                .get(i + 1..i + 3)
                .ok_or_else(|| TransformError::Url("обрыв после «%»".into()))?;
            let v = u8::from_str_radix(hex, 16)
                .map_err(|_| TransformError::Url(format!("«%{hex}» — не шестнадцатеричное число")))?;
            out.push(v);
            i += 3;
        } else {
            out.push(b[i]);
            i += 1;
        }
    }
    String::from_utf8(out).map_err(|_| TransformError::Url("результат не является текстом UTF-8".into()))
}

/// Keyboard pairs: (English layout key, Russian layout key), lower and upper case and the punctuation keys.
const LAYOUT: &[(char, char)] = &[
    ('`', 'ё'),
    ('q', 'й'),
    ('w', 'ц'),
    ('e', 'у'),
    ('r', 'к'),
    ('t', 'е'),
    ('y', 'н'),
    ('u', 'г'),
    ('i', 'ш'),
    ('o', 'щ'),
    ('p', 'з'),
    ('[', 'х'),
    (']', 'ъ'),
    ('a', 'ф'),
    ('s', 'ы'),
    ('d', 'в'),
    ('f', 'а'),
    ('g', 'п'),
    ('h', 'р'),
    ('j', 'о'),
    ('k', 'л'),
    ('l', 'д'),
    (';', 'ж'),
    ('\'', 'э'),
    ('z', 'я'),
    ('x', 'ч'),
    ('c', 'с'),
    ('v', 'м'),
    ('b', 'и'),
    ('n', 'т'),
    ('m', 'ь'),
    (',', 'б'),
    ('.', 'ю'),
    ('/', '.'),
    ('~', 'Ё'),
    ('Q', 'Й'),
    ('W', 'Ц'),
    ('E', 'У'),
    ('R', 'К'),
    ('T', 'Е'),
    ('Y', 'Н'),
    ('U', 'Г'),
    ('I', 'Ш'),
    ('O', 'Щ'),
    ('P', 'З'),
    ('{', 'Х'),
    ('}', 'Ъ'),
    ('A', 'Ф'),
    ('S', 'Ы'),
    ('D', 'В'),
    ('F', 'А'),
    ('G', 'П'),
    ('H', 'Р'),
    ('J', 'О'),
    ('K', 'Л'),
    ('L', 'Д'),
    (':', 'Ж'),
    ('"', 'Э'),
    ('Z', 'Я'),
    ('X', 'Ч'),
    ('C', 'С'),
    ('V', 'М'),
    ('B', 'И'),
    ('N', 'Т'),
    ('M', 'Ь'),
    ('<', 'Б'),
    ('>', 'Ю'),
    ('?', ','),
    ('@', '"'),
    ('#', '№'),
    ('$', ';'),
    ('^', ':'),
    ('&', '?'),
];

fn map_layout(s: &str, en_to_ru: bool) -> String {
    s.chars()
        .map(|c| {
            LAYOUT
                .iter()
                .find(|(en, ru)| if en_to_ru { *en == c } else { *ru == c })
                .map_or(c, |(en, ru)| if en_to_ru { *ru } else { *en })
        })
        .collect()
}

fn layout_auto(s: &str) -> String {
    let latin = s.chars().filter(char::is_ascii_alphabetic).count();
    let cyr = s
        .chars()
        .filter(|c| ('а'..='я').contains(&c.to_lowercase().next().unwrap_or(*c)) || matches!(c, 'ё' | 'Ё'))
        .count();
    map_layout(s, latin >= cyr)
}

fn links_re() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r#"(?i)\b(?:https?://|ftp://|www\.)[^\s<>"'\]\[]+"#).expect("built-in pattern"))
}

fn emails_re() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| {
        Regex::new(r"[A-Za-z0-9._%+\-]+@[A-Za-z0-9\-]+(?:\.[A-Za-z0-9\-]+)*\.[A-Za-z]{2,}").expect("built-in pattern")
    })
}

fn unique_matches(re: &Regex, s: &str, trim_end: &[char]) -> Result<String, TransformError> {
    let mut seen = HashSet::new();
    let found: Vec<&str> = re
        .find_iter(s)
        .map(|m| m.as_str().trim_end_matches(trim_end))
        .filter(|m| seen.insert(m.to_lowercase()))
        .collect();
    if found.is_empty() {
        Err(TransformError::NothingFound)
    } else {
        Ok(found.join("\n"))
    }
}

fn extract_links(s: &str) -> Result<String, TransformError> {
    unique_matches(links_re(), s, &['.', ',', ';', ':', '!', '?', ')'])
}

fn extract_emails(s: &str) -> Result<String, TransformError> {
    unique_matches(emails_re(), s, &['.'])
}

fn title_case(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut start = true;
    for c in s.chars() {
        if c.is_alphanumeric() || c == '\'' {
            if start {
                out.extend(c.to_uppercase());
            } else {
                out.extend(c.to_lowercase());
            }
            start = false;
        } else {
            out.push(c);
            start = true;
        }
    }
    out
}

fn trim_lines(s: &str) -> String {
    let lines: Vec<&str> = s.lines().map(str::trim).collect();
    let first = lines.iter().position(|l| !l.is_empty()).unwrap_or(lines.len());
    let last = lines.iter().rposition(|l| !l.is_empty()).map_or(first, |i| i + 1);
    lines[first..last].join("\n")
}

fn sort_lines(s: &str, desc: bool) -> String {
    let mut lines: Vec<&str> = s.lines().collect();
    lines.sort_by_key(|l| l.to_lowercase());
    if desc {
        lines.reverse();
    }
    lines.join("\n")
}

fn unique_lines(s: &str) -> String {
    let mut seen = HashSet::new();
    s.lines().filter(|l| seen.insert(*l)).collect::<Vec<_>>().join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn t(tr: Transform, s: &str) -> String {
        tr.apply(s).unwrap()
    }

    #[test]
    fn layout_table_both_ways() {
        assert_eq!(t(Transform::LayoutEnToRu, "ghbdtn"), "привет");
        assert_eq!(t(Transform::LayoutRuToEn, "привет"), "ghbdtn");
        assert_eq!(t(Transform::LayoutRuToEn, "руддщ"), "hello");
        assert_eq!(t(Transform::LayoutEnToRu, "Ghbdtn vbh"), "Привет мир");
        assert_eq!(t(Transform::LayoutEnToRu, "[]';,./`"), "хъэжбю.ё");
        assert_eq!(t(Transform::LayoutEnToRu, "{}:\"<>?~"), "ХЪЖЭБЮ,Ё");
        assert_eq!(t(Transform::LayoutEnToRu, "@#$^&"), "\"№;:?");
        assert_eq!(t(Transform::LayoutEnToRu, "123 !"), "123 !");
    }

    #[test]
    fn layout_round_trip_over_the_whole_table() {
        let en: String = LAYOUT.iter().map(|(e, _)| *e).collect();
        let ru: String = LAYOUT.iter().map(|(_, r)| *r).collect();
        assert_eq!(t(Transform::LayoutEnToRu, &en), ru);
        assert_eq!(t(Transform::LayoutRuToEn, &ru), en);
        // the table is a bijection
        let mut e: Vec<char> = LAYOUT.iter().map(|p| p.0).collect();
        let mut r: Vec<char> = LAYOUT.iter().map(|p| p.1).collect();
        e.sort_unstable();
        e.dedup();
        r.sort_unstable();
        r.dedup();
        assert_eq!((e.len(), r.len()), (LAYOUT.len(), LAYOUT.len()));
    }

    #[test]
    fn layout_auto_picks_the_direction() {
        assert_eq!(t(Transform::LayoutAuto, "ghbdtn"), "привет");
        assert_eq!(t(Transform::LayoutAuto, "привет"), "ghbdtn");
        assert_eq!(t(Transform::LayoutAuto, "ghbdtn vbh 1"), "привет мир 1");
    }

    #[test]
    fn json_round_trip_and_errors() {
        let src = r#"{"b":1,"a":[1,2,{"x":null}],"s":"привет"}"#;
        let pretty = t(Transform::JsonFormat, src);
        assert!(pretty.contains("\n  \"b\": 1"), "{pretty}");
        assert!(
            pretty.find("\"b\"").unwrap() < pretty.find("\"a\"").unwrap(),
            "key order is kept"
        );
        assert_eq!(t(Transform::JsonMinify, &pretty), src);
        let err = Transform::JsonFormat.apply("{\"a\": }").unwrap_err().to_string();
        assert!(err.contains("строка 1") && err.contains("столбец"), "{err}");
        assert!(Transform::JsonMinify.apply("").is_err());
    }

    #[test]
    fn base64_round_trip_and_vectors() {
        for (plain, enc) in [
            ("", ""),
            ("f", "Zg=="),
            ("fo", "Zm8="),
            ("foo", "Zm9v"),
            ("foobar", "Zm9vYmFy"),
            ("привет", "0L/RgNC40LLQtdGC"),
        ] {
            assert_eq!(t(Transform::Base64Encode, plain), enc);
            assert_eq!(t(Transform::Base64Decode, enc), plain);
        }
        assert_eq!(
            t(Transform::Base64Decode, "Zm9v\nYmFy"),
            "foobar",
            "whitespace is ignored"
        );
        assert_eq!(t(Transform::Base64Decode, "Zm9vYg"), "foob", "padding is optional");
        assert!(Transform::Base64Decode.apply("Zm9v!").is_err());
        assert!(Transform::Base64Decode.apply("/w==").is_err(), "not UTF-8");
    }

    #[test]
    fn url_round_trip() {
        assert_eq!(t(Transform::UrlEncode, "a b&c=d/é"), "a%20b%26c%3Dd%2F%C3%A9");
        assert_eq!(t(Transform::UrlDecode, "a%20b%26c%3Dd%2F%C3%A9"), "a b&c=d/é");
        for s in ["", "plain-text_1.2~", "привет мир?x=1&y=2#z"] {
            assert_eq!(t(Transform::UrlDecode, &t(Transform::UrlEncode, s)), s);
        }
        assert!(Transform::UrlDecode.apply("100%").is_err());
        assert!(Transform::UrlDecode.apply("%zz").is_err());
        assert!(Transform::UrlDecode.apply("%FF").is_err());
    }

    #[test]
    fn extraction() {
        let text =
            "See https://example.com/a?b=1, and (http://x.org/path). Also www.site.ru. Again https://example.com/a?b=1";
        assert_eq!(
            t(Transform::ExtractLinks, text),
            "https://example.com/a?b=1\nhttp://x.org/path\nwww.site.ru"
        );
        assert_eq!(
            t(
                Transform::ExtractEmails,
                "Write to Ivan.Petrov@Example.com, or a@b.co. Also IVAN.PETROV@example.com!"
            ),
            "Ivan.Petrov@Example.com\na@b.co"
        );
        assert_eq!(
            Transform::ExtractLinks.apply("nothing here"),
            Err(TransformError::NothingFound)
        );
        assert_eq!(
            Transform::ExtractEmails.apply("nothing here"),
            Err(TransformError::NothingFound)
        );
    }

    #[test]
    fn case_trim_sort_unique() {
        assert_eq!(t(Transform::Upper, "Привет, world"), "ПРИВЕТ, WORLD");
        assert_eq!(t(Transform::Lower, "Привет, WORLD"), "привет, world");
        assert_eq!(
            t(Transform::Title, "hELLO wORLD-and don't ПРИВЕТ мир"),
            "Hello World-And Don't Привет Мир"
        );
        assert_eq!(t(Transform::Trim, "\n\n  a  \n\t b\t\n\n"), "a\nb");
        assert_eq!(t(Transform::Trim, "   "), "");
        assert_eq!(t(Transform::SortLines, "b\nA\nc\na"), "A\na\nb\nc");
        assert_eq!(t(Transform::SortLinesDesc, "b\nA\nc"), "c\nb\nA");
        assert_eq!(t(Transform::UniqueLines, "x\ny\nx\nz\ny"), "x\ny\nz");
    }

    #[test]
    fn names_round_trip() {
        for tr in Transform::ALL {
            assert_eq!(Transform::parse(tr.name()), Some(tr));
        }
        assert_eq!(Transform::parse("JSON_FORMAT"), Some(Transform::JsonFormat));
        assert_eq!(Transform::parse("nope"), None);
    }
}
