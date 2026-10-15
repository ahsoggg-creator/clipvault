use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    Text,
    Rich,
    Image,
    Files,
    Color,
    Link,
    Path,
}

impl Kind {
    pub const ALL: [Kind; 7] = [
        Kind::Text,
        Kind::Rich,
        Kind::Image,
        Kind::Files,
        Kind::Color,
        Kind::Link,
        Kind::Path,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            Kind::Text => "text",
            Kind::Rich => "rich",
            Kind::Image => "image",
            Kind::Files => "files",
            Kind::Color => "color",
            Kind::Link => "link",
            Kind::Path => "path",
        }
    }

    pub fn parse(s: &str) -> Option<Kind> {
        Kind::ALL.into_iter().find(|k| k.as_str().eq_ignore_ascii_case(s))
    }
}

/// What kind of secret the filter recognised.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SecretKind {
    ApiKey,
    Token,
    PrivateKey,
    ConnectionString,
    SeedPhrase,
    Card,
    Password,
    Custom,
}

/// What the attached binary payload is.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum BlobKind {
    #[default]
    None,
    Html,
    Rtf,
    Png,
}

/// A history entry. `text` is the plain-text form (searchable, always in memory); the binary payload
/// (HTML / RTF / PNG) stays encrypted in the vault and is read on demand through `blob_off`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Item {
    pub id: u64,
    pub kind: Kind,
    /// BLAKE3 of the normalized content, hex.
    pub hash: String,
    pub text: String,
    #[serde(default)]
    pub blob_kind: BlobKind,
    /// Offset of the blob frame in the vault file (0 = none). Rewritten by compaction.
    #[serde(default)]
    pub blob_off: u64,
    #[serde(default)]
    pub blob_len: u64,
    #[serde(default)]
    pub img: Option<(u32, u32)>,
    #[serde(default)]
    pub source_app: String,
    #[serde(default)]
    pub source_title: String,
    pub created: u64,
    pub last_used: u64,
    pub count: u32,
    #[serde(default)]
    pub pinned: bool,
    /// The content was replaced by a mask because it looked like a secret.
    #[serde(default)]
    pub masked: bool,
    /// What the secret filter recognised (set for masked entries and for entries kept in memory only).
    #[serde(default)]
    pub secret: Option<SecretKind>,
    /// Lives in memory only (a secret kept by the "memory only" policy): never written to disk.
    #[serde(skip)]
    pub volatile: bool,
}

/// What the capture code hands over; the history adds id, time and the hash.
#[derive(Clone, Debug, Default)]
pub struct NewItem {
    pub kind: Option<Kind>,
    pub text: String,
    pub blob: Vec<u8>,
    pub blob_kind: BlobKind,
    pub img: Option<(u32, u32)>,
    pub source_app: String,
    pub source_title: String,
    pub masked: bool,
    pub secret: Option<SecretKind>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Reject {
    TooLarge,
    Empty,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Added {
    New(u64),
    /// An identical entry already existed: it moved to the top and its counter grew.
    Bumped(u64),
}

impl Added {
    pub fn id(self) -> u64 {
        match self {
            Added::New(i) | Added::Bumped(i) => i,
        }
    }
}
