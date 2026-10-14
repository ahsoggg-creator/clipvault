//! Clipvault core: everything except the user interface and the Windows clipboard glue.
//!
//! The history lives in an encrypted append-only vault (`vault`), indexed in memory (`history`); entries are
//! classified (`classify`), deduplicated by BLAKE3 (`dedup`) and searched fuzzily (`search`).

pub mod classify;
pub mod config;
pub mod crypto;
pub mod dedup;
pub mod error;
pub mod history;
pub mod model;
pub mod search;
pub mod vault;

pub use error::{Error, Result};
pub use history::{AddError, History, Limits, Query};
pub use model::{Added, BlobKind, Item, Kind, NewItem, Reject};

pub fn now_unix() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}
