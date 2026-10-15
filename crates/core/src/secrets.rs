//! Finds secrets in text *before* anything is written to disk: API keys and tokens, private keys, connection
//! strings, BIP-39 seed phrases, payment card numbers and password-looking strings.
//!
//! Every detector is a pure function over the text, so the whole table of positives and false positives is
//! unit-tested. Nothing here logs or stores what it looks at.

use std::collections::HashSet;
use std::sync::OnceLock;

use regex::Regex;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::classify::{classify_text, parse_color};
use crate::model::{Kind, SecretKind};

/// How eager the "looks like a password" heuristic is. Higher = catches more, wrongly flags more.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Sensitivity {
    Off,
    Low,
    #[default]
    Medium,
    High,
}

fn words() -> &'static HashSet<&'static str> {
    static W: OnceLock<HashSet<&'static str>> = OnceLock::new();
    W.get_or_init(|| {
        include_str!("bip39_en.txt")
            .lines()
            .map(str::trim)
            .filter(|l| !l.is_empty())
            .collect()
    })
}

fn word_list() -> &'static [&'static str] {
    static L: OnceLock<Vec<&'static str>> = OnceLock::new();
    L.get_or_init(|| {
        include_str!("bip39_en.txt")
            .lines()
            .map(str::trim)
            .filter(|l| !l.is_empty())
            .collect()
    })
}

struct Patterns {
    api: Vec<Regex>,
    jwt: Regex,
    pem: Regex,
    conn: Vec<Regex>,
    card: Regex,
}

fn patterns() -> &'static Patterns {
    static P: OnceLock<Patterns> = OnceLock::new();
    P.get_or_init(|| {
        let r = |s: &str| Regex::new(s).expect("built-in pattern");
        Patterns {
            api: vec![
                r(r"\bsk-[A-Za-z0-9_\-]{20,}"),                      // OpenAI / Anthropic style
                r(r"\bgh[pousr]_[A-Za-z0-9]{30,}"),                  // GitHub tokens
                r(r"\bgithub_pat_[A-Za-z0-9_]{20,}"),                // GitHub fine-grained
                r(r"\b(?:AKIA|ASIA|AGPA|AIDA|AROA|ANPA|ANVA)[A-Z0-9]{16}\b"), // AWS access key id
                r(r"\bxox[abprs]-[A-Za-z0-9\-]{10,}"),               // Slack
                r(r"\bxapp-[A-Za-z0-9\-]{10,}"),
                r(r"\bAIza[0-9A-Za-z_\-]{35}\b"),                    // Google API key
                r(r"\b[sr]k_(?:live|test)_[0-9A-Za-z]{20,}"),        // Stripe
            ],
            jwt: r(r"\beyJ[A-Za-z0-9_\-]{8,}\.eyJ[A-Za-z0-9_\-]{8,}\.[A-Za-z0-9_\-]{8,}"),
            pem: r(r"-----BEGIN [A-Z0-9 ]*PRIVATE KEY(?: BLOCK)?-----"),
            conn: vec![
                // scheme://user:password@host
                r(r"(?i)\b(?:postgres(?:ql)?|mysql|mariadb|mongodb(?:\+srv)?|redis|rediss|amqps?|mssql|sqlserver|ftp|sftp)://[^\s:@/]+:[^\s@/]+@"),
                // Server=...;...;Password=...
                r(r"(?i)\b(?:server|data source|host|provider)\s*=[^;\n]+;[^\n]*\b(?:password|pwd)\s*=[^;\s]+"),
                r(r"(?i)\b(?:user id|uid|username)\s*=[^;\n]+;[^\n]*\b(?:password|pwd)\s*=[^;\s]+"),
            ],
            card: r(r"\b\d(?:[ \-]?\d){12,18}\b"),
        }
    })
}

pub fn luhn(digits: &str) -> bool {
    let mut sum = 0;
    let mut n = 0;
    for (i, c) in digits.chars().rev().enumerate() {
        let Some(mut d) = c.to_digit(10) else { return false };
        if i % 2 == 1 {
            d *= 2;
            if d > 9 {
                d -= 9;
            }
        }
        sum += d;
        n += 1;
    }
    n >= 2 && sum % 10 == 0
}

/// Card networks by leading digits and length; keeps long Luhn-valid IDs and phone numbers out.
fn plausible_card(d: &str) -> bool {
    let len = d.len();
    if !(13..=19).contains(&len) || d.bytes().all(|b| b == d.as_bytes()[0]) {
        return false;
    }
    let p2: u32 = d[..2].parse().unwrap_or(0);
    let p4: u32 = d[..4].parse().unwrap_or(0);
    match d.as_bytes()[0] {
        b'4' => matches!(len, 13 | 16 | 19),
        b'5' => (51..=55).contains(&p2) && len == 16 || p2 == 50 || (56..=59).contains(&p2),
        b'2' => ((2221..=2720).contains(&p4) || (2200..=2204).contains(&p4)) && (16..=19).contains(&len),
        b'3' => matches!(p2, 34 | 37) && len == 15 || (3528..=3589).contains(&p4) || matches!(p2, 30 | 36 | 38 | 39),
        b'6' => true,
        _ => false,
    }
}

fn has_card(text: &str) -> bool {
    patterns().card.find_iter(text).any(|m| {
        let digits: String = m.as_str().chars().filter(char::is_ascii_digit).collect();
        plausible_card(&digits) && luhn(&digits)
    })
}

/// Result of looking at a possible seed phrase.
#[derive(Debug, PartialEq, Eq)]
pub enum Seed {
    /// Valid BIP-39 phrase (checksum matches).
    Valid,
    /// Every word is from the list and the count is right, but the checksum fails: a mistyped phrase.
    BadChecksum,
    No,
}

pub fn seed_phrase(text: &str) -> Seed {
    let ws: Vec<String> = text
        .split(|c: char| c.is_whitespace() || c == ',')
        .filter(|w| !w.is_empty())
        .map(str::to_lowercase)
        .collect();
    if !matches!(ws.len(), 12 | 15 | 18 | 21 | 24) {
        return Seed::No;
    }
    let set = words();
    if !ws.iter().all(|w| set.contains(w.as_str())) {
        return Seed::No;
    }
    let list = word_list();
    let mut bits: Vec<bool> = Vec::with_capacity(ws.len() * 11);
    for w in &ws {
        let idx = list.iter().position(|x| x == w).unwrap_or(0);
        for i in (0..11).rev() {
            bits.push(idx >> i & 1 == 1);
        }
    }
    let cs = bits.len() / 33;
    let ent = bits.len() - cs;
    let entropy: Vec<u8> = bits[..ent]
        .chunks(8)
        .map(|c| c.iter().fold(0u8, |a, &b| a << 1 | u8::from(b)))
        .collect();
    let hash = Sha256::digest(&entropy);
    let ok = (0..cs).all(|i| (hash[i / 8] >> (7 - i % 8) & 1 == 1) == bits[ent + i]);
    if ok { Seed::Valid } else { Seed::BadChecksum }
}

fn shannon(s: &str) -> f64 {
    let n = s.chars().count() as f64;
    let mut counts = std::collections::HashMap::new();
    for c in s.chars() {
        *counts.entry(c).or_insert(0u32) += 1;
    }
    counts
        .values()
        .map(|&c| {
            let p = f64::from(c) / n;
            -p * p.log2()
        })
        .sum()
}

fn longest_lower_run(s: &str) -> usize {
    let (mut best, mut cur) = (0, 0);
    for c in s.chars() {
        if c.is_lowercase() {
            cur += 1;
            best = best.max(cur);
        } else {
            cur = 0;
        }
    }
    best
}

fn is_hex(s: &str) -> bool {
    s.len() >= 16 && s.bytes().all(|b| b.is_ascii_hexdigit())
}

fn is_uuid(s: &str) -> bool {
    s.len() == 36
        && s.bytes().enumerate().all(|(i, b)| {
            if [8, 13, 18, 23].contains(&i) {
                b == b'-'
            } else {
                b.is_ascii_hexdigit()
            }
        })
}

/// `1.2.3`, `v2.0.0-beta+build.5`
fn is_version(s: &str) -> bool {
    static V: OnceLock<Regex> = OnceLock::new();
    V.get_or_init(|| Regex::new(r"^[vV]?\d+\.\d+(?:\.\d+)*(?:[-+][0-9A-Za-z.\-+]*)?$").expect("built-in pattern"))
        .is_match(s)
}

fn is_email(s: &str) -> bool {
    let mut it = s.split('@');
    matches!((it.next(), it.next(), it.next()), (Some(u), Some(d), None) if !u.is_empty() && d.contains('.') && !d.ends_with('.'))
}

/// A single "word" with no spaces that has the shape of a password.
pub fn password_like(text: &str, level: Sensitivity) -> bool {
    let t = text.trim();
    if level == Sensitivity::Off
        || t.chars().count() < 8
        || t.chars().count() > 128
        || t.contains(char::is_whitespace)
        || !t.is_ascii()
    {
        return false;
    }
    if classify_text(t) != Kind::Text || parse_color(t).is_some() || is_hex(t) || is_uuid(t) || is_email(t) {
        return false;
    }
    if is_version(t) {
        return false;
    }
    if t.chars()
        .all(|c| c.is_ascii_digit() || matches!(c, '.' | ',' | '-' | ':' | '/'))
    {
        return false; // numbers, dates, IPs, versions
    }
    let lower = t.chars().any(|c| c.is_ascii_lowercase());
    let upper = t.chars().any(|c| c.is_ascii_uppercase());
    let digit = t.chars().any(|c| c.is_ascii_digit());
    let symbol = t
        .chars()
        .any(|c| c.is_ascii_punctuation() && !matches!(c, '-' | '_' | '.'));
    let classes = [lower, upper, digit, symbol].into_iter().filter(|&b| b).count();
    let len = t.chars().count();
    let entropy = shannon(t);
    let run = longest_lower_run(t);
    match level {
        Sensitivity::Off => false,
        // clearly random: all four classes, long
        Sensitivity::Low => len >= 12 && classes == 4 && entropy >= 3.2 && run <= 6,
        Sensitivity::Medium => {
            (symbol && classes >= 3 && len >= 8 && entropy >= 2.8 && run <= 8)
                || (classes >= 3 && len >= 14 && entropy >= 3.5 && run <= 5)
        }
        Sensitivity::High => classes >= 3 && len >= 8 && entropy >= 2.6 && run <= 8,
    }
}

/// Compiled detectors plus the user's own patterns.
pub struct Detector {
    level: Sensitivity,
    custom: Vec<Regex>,
}

impl Detector {
    /// Invalid custom patterns are skipped; their indexes are returned so the settings can say which.
    pub fn new(level: Sensitivity, custom: &[String]) -> (Detector, Vec<usize>) {
        let mut ok = Vec::new();
        let mut bad = Vec::new();
        for (i, p) in custom.iter().enumerate() {
            if p.trim().is_empty() {
                continue;
            }
            match Regex::new(p) {
                Ok(r) => ok.push(r),
                Err(_) => bad.push(i),
            }
        }
        (Detector { level, custom: ok }, bad)
    }

    pub fn detect(&self, text: &str) -> Option<SecretKind> {
        let p = patterns();
        if p.pem.is_match(text) {
            return Some(SecretKind::PrivateKey);
        }
        if p.jwt.is_match(text) {
            return Some(SecretKind::Token);
        }
        if p.api.iter().any(|r| r.is_match(text)) {
            return Some(SecretKind::ApiKey);
        }
        if p.conn.iter().any(|r| r.is_match(text)) {
            return Some(SecretKind::ConnectionString);
        }
        if seed_phrase(text) != Seed::No {
            return Some(SecretKind::SeedPhrase);
        }
        if has_card(text) {
            return Some(SecretKind::Card);
        }
        if self.custom.iter().any(|r| r.is_match(text)) {
            return Some(SecretKind::Custom);
        }
        if password_like(text, self.level) {
            return Some(SecretKind::Password);
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn det() -> Detector {
        Detector::new(Sensitivity::Medium, &[]).0
    }

    #[test]
    fn luhn_table() {
        for ok in [
            "4111111111111111",
            "5555555555554444",
            "378282246310005",
            "6011111111111117",
        ] {
            assert!(luhn(ok), "{ok}");
        }
        for bad in ["4111111111111112", "1234567890123456", "", "4"] {
            assert!(!luhn(bad), "{bad}");
        }
    }

    #[test]
    fn detects_keys_and_tokens() {
        let cases = [
            ("sk-proj-AbCdEfGhIjKlMnOpQrStUvWxYz012345", SecretKind::ApiKey),
            ("export OPENAI_API_KEY=sk-abcdefghijklmnopqrstuvwx", SecretKind::ApiKey),
            ("ghp_0123456789abcdefghijklmnopqrstuvwxyz", SecretKind::ApiKey),
            (
                "github_pat_11ABCDEFG0123456789_abcdefghijklmnopqrstuvwxyz",
                SecretKind::ApiKey,
            ),
            ("AKIAIOSFODNN7EXAMPLE", SecretKind::ApiKey),
            ("xoxb-123456789012-abcdefghijkl", SecretKind::ApiKey),
            ("AIzaSyA1234567890abcdefghijklmnopqrstuv", SecretKind::ApiKey),
            (
                "eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9.eyJzdWIiOiIxMjM0NTY3ODkwIn0.dozjgNryP4J3jVmNHl0w5N_XgL0n3I9PlFUP0THsR8U",
                SecretKind::Token,
            ),
            ("-----BEGIN RSA PRIVATE KEY-----\nMIIEow...", SecretKind::PrivateKey),
            ("-----BEGIN OPENSSH PRIVATE KEY-----", SecretKind::PrivateKey),
            ("-----BEGIN PRIVATE KEY-----", SecretKind::PrivateKey),
            (
                "postgres://admin:s3cret@db.internal:5432/app",
                SecretKind::ConnectionString,
            ),
            (
                "mongodb+srv://user:pa55@cluster0.example.net/test",
                SecretKind::ConnectionString,
            ),
            (
                "Server=tcp:db.example.com;Database=app;User Id=sa;Password=hunter2;",
                SecretKind::ConnectionString,
            ),
        ];
        // assembled at run time so that secret scanners do not mistake a test value for a real key
        let stripe = format!("sk_{}_0123456789abcdefghijklmn", "live");
        assert_eq!(det().detect(&stripe), Some(SecretKind::ApiKey));
        for (text, kind) in cases {
            assert_eq!(det().detect(text), Some(kind), "{text}");
        }
    }

    #[test]
    fn detects_cards_and_seed_phrases() {
        for t in [
            "4111 1111 1111 1111",
            "5555-5555-5555-4444",
            "card: 378282246310005 exp 12/30",
            "6011111111111117",
        ] {
            assert_eq!(det().detect(t), Some(SecretKind::Card), "{t}");
        }
        let valid12 = "abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon about";
        let valid18 = format!("{} agent", "abandon ".repeat(17).trim_end());
        let valid24 = format!("{} art", "abandon ".repeat(23).trim_end());
        assert_eq!(seed_phrase(valid12), Seed::Valid);
        assert_eq!(seed_phrase(&valid18), Seed::Valid);
        assert_eq!(seed_phrase(&valid24), Seed::Valid);
        assert_eq!(seed_phrase(&valid12.to_uppercase()), Seed::Valid);
        let bad = "abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon";
        assert_eq!(seed_phrase(bad), Seed::BadChecksum);
        for t in [valid12, bad] {
            assert_eq!(det().detect(t), Some(SecretKind::SeedPhrase));
        }
        // not seed phrases: wrong count, a word outside the list
        assert_eq!(seed_phrase("abandon abandon abandon"), Seed::No);
        assert_eq!(
            seed_phrase(
                "abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon zzzzz"
            ),
            Seed::No
        );
        assert_eq!(
            seed_phrase("the quick brown fox jumps over the lazy dog and then it sleeps"),
            Seed::No
        );
    }

    #[test]
    fn no_false_positives_on_ordinary_text() {
        let fine = [
            "Hello, this is just a normal sentence about the weekend.",
            "Созвон в пятницу в 15:00, не забыть отчёт",
            "+7 (900) 123-45-67",
            "8 495 123 45 67",
            "79001234567",
            "+1 202-555-0143",
            "12345678901",
            "1 234 567,89",
            "1700000000000",
            "2026-10-15 18:04:09",
            "192.168.100.200",
            "v1.2.3-beta+build.5",
            "https://example.com/path?token=abc&x=1",
            r"C:\Users\demo\Documents\Report_Q3_final.xlsx",
            "ivan.petrov@example.com",
            "550e8400-e29b-41d4-a716-446655440000",
            "da39a3ee5e6b4b0d3255bfef95601890afd80709",
            "#7C6CF0",
            "getUserNameFromDatabase",
            "README.md",
            "Clipvault.exe",
            "hello_world_function",
            "SELECT id, name FROM customers WHERE active = 1;",
            "The quick brown fox jumps over the lazy dog",
            "4111",
            "ISBN 978-3-16-148410-0",
            "ORDER-2026-000123",
        ];
        for t in fine {
            assert_eq!(det().detect(t), None, "false positive: {t}");
        }
    }

    #[test]
    fn password_shapes_by_sensitivity() {
        let passwords = ["Tr0ub4dor&3x", "xK9#mP2$vL7@", "P@ssw0rd!2026", "qZ7!rT4&nB9^wE1"];
        for p in passwords {
            assert!(password_like(p, Sensitivity::Medium), "{p}");
        }
        for p in [
            "correct horse battery staple",
            "password",
            "Hello World",
            "simpleword",
            "getUserNameFromDatabase2",
        ] {
            assert!(!password_like(p, Sensitivity::Medium), "{p}");
        }
        assert!(!password_like("Tr0ub4dor&3x", Sensitivity::Off));
        assert!(password_like("xK9#mP2$vL7@", Sensitivity::Low));
        assert!(!password_like("Ab1!Cd2@", Sensitivity::Low), "short ones need High");
        assert!(password_like("Ab1!Cd2@", Sensitivity::High));
        assert!(!password_like("Hello123", Sensitivity::Medium));
    }

    #[test]
    fn custom_patterns() {
        let (d, bad) = Detector::new(
            Sensitivity::Off,
            &["CORP-[0-9]{6}".to_owned(), "(unclosed".to_owned(), "  ".to_owned()],
        );
        assert_eq!(bad, vec![1]);
        assert_eq!(d.detect("badge CORP-123456 issued"), Some(SecretKind::Custom));
        assert_eq!(d.detect("CORP-12"), None);
    }
}
