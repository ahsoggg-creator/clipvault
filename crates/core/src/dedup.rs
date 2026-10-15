//! Duplicate detection: BLAKE3 over the normalized content.

use crate::model::Kind;

/// Line endings unified, outer whitespace trimmed: the same text copied from two editors is one entry.
pub fn normalize_text(s: &str) -> String {
    s.replace("\r\n", "\n").replace('\r', "\n").trim().to_owned()
}

/// Hex BLAKE3 of `kind` + the normalized text, or of the raw bytes for images.
pub fn content_hash(kind: Kind, text: &str, blob: &[u8]) -> String {
    let mut h = blake3::Hasher::new();
    h.update(kind.as_str().as_bytes());
    h.update(&[0]);
    match kind {
        Kind::Image => {
            h.update(blob);
        }
        Kind::Files => {
            // case-insensitive file systems: C:\A and c:\a are the same list
            h.update(normalize_text(text).to_lowercase().replace('/', "\\").as_bytes());
        }
        // rich text counts as its plain text: the same words in another font are the same entry
        _ => {
            h.update(normalize_text(text).as_bytes());
        }
    }
    h.finalize().to_hex().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn same_content_same_hash() {
        let a = content_hash(Kind::Text, "hello\r\nworld  ", &[]);
        let b = content_hash(Kind::Text, "  hello\nworld", &[]);
        assert_eq!(a, b);
        assert_eq!(a.len(), 64);
        assert_ne!(a, content_hash(Kind::Text, "hello world", &[]));
        assert_ne!(a, content_hash(Kind::Link, "hello\nworld", &[]));
    }

    #[test]
    fn files_ignore_case_and_slashes() {
        assert_eq!(
            content_hash(Kind::Files, "C:\\A\\b.txt", &[]),
            content_hash(Kind::Files, "c:/a/B.TXT", &[])
        );
    }

    #[test]
    fn images_hash_their_bytes() {
        assert_ne!(
            content_hash(Kind::Image, "", &[1, 2]),
            content_hash(Kind::Image, "", &[1, 3])
        );
    }
}
