//! The encrypted store: an append-only log of sealed frames.
//!
//! ```text
//! file   = "CLIPVLT1" generation:u64 frame*
//! frame  = len:u32 kind:u8 sealed[len]          kind 0 = event (JSON), 1 = blob (raw bytes)
//! sealed = nonce[24] || XChaCha20-Poly1305(payload), AAD = generation || sequence number
//! ```
//!
//! Nothing readable is ever written: metadata, text and binary payloads all live inside sealed frames, and
//! the search corpus is rebuilt in memory at start-up. There is no WAL and no temporary plaintext;
//! compaction writes a new sealed file next to the old one and renames it over. All access is serialised by
//! an advisory lock on `vault.lock`, so the GUI and `clipvault-cli` can work on the same vault.

use std::collections::HashMap;
use std::fs::{File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::crypto::{Cipher, MasterKey};
use crate::error::{Error, Result};
use crate::model::Item;

const MAGIC: &[u8; 8] = b"CLIPVLT1";
const HEADER: u64 = 16;
const KIND_EVENT: u8 = 0;
const KIND_BLOB: u8 = 1;
const MAX_FRAME: u32 = 512 << 20;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "t")]
pub enum Event {
    Put {
        item: Item,
    },
    Touch {
        id: u64,
        at: u64,
    },
    Pin {
        id: u64,
        pinned: bool,
    },
    Delete {
        id: u64,
    },
    Clear {
        keep_pinned: bool,
    },
    /// Written by a newer version; skipped.
    #[serde(other)]
    Unknown,
}

pub struct Vault {
    dir: PathBuf,
    cipher: Cipher,
    generation: u64,
    /// End of the last valid frame we know about.
    pos: u64,
    next_seq: u64,
    /// Blob frame offset -> its sequence number (the AAD needs it).
    blobs: HashMap<u64, u64>,
}

/// What `sync` found.
pub enum Delta {
    Unchanged,
    /// New events appended by someone else.
    Appended(Vec<Event>),
    /// The file was compacted or replaced: rebuild everything from these events.
    Reloaded(Vec<Event>),
}

/// One event of a rewrite, with the blob it refers to.
pub struct Entry {
    pub event: Event,
    pub blob: Option<Vec<u8>>,
}

/// `append` lost a race with another process: `sync`, then retry.
pub const CONFLICT: &str = "conflict";

fn aad(generation: u64, seq: u64) -> [u8; 16] {
    let mut a = [0u8; 16];
    a[..8].copy_from_slice(&generation.to_le_bytes());
    a[8..].copy_from_slice(&seq.to_le_bytes());
    a
}

fn push_frame(cipher: &Cipher, generation: u64, out: &mut Vec<u8>, kind: u8, plain: &[u8], seq: u64) {
    let sealed = cipher.seal(plain, &aad(generation, seq));
    out.extend_from_slice(&(sealed.len() as u32).to_le_bytes());
    out.push(kind);
    out.extend_from_slice(&sealed);
}

struct LockGuard(File);

impl LockGuard {
    fn acquire(dir: &Path, exclusive: bool) -> Result<LockGuard> {
        let f = OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(dir.join("vault.lock"))?;
        if exclusive {
            f.lock()?;
        } else {
            f.lock_shared()?;
        }
        Ok(LockGuard(f))
    }
}

impl Drop for LockGuard {
    fn drop(&mut self) {
        let _ = self.0.unlock();
    }
}

struct Scan {
    events: Vec<Event>,
    blobs: Vec<(u64, u64)>,
    end: u64,
    next_seq: u64,
}

fn read_header(f: &mut File) -> Result<u64> {
    let mut h = [0u8; HEADER as usize];
    f.seek(SeekFrom::Start(0))?;
    f.read_exact(&mut h).map_err(|_| Error::Corrupt)?;
    if &h[..8] != MAGIC {
        return Err(Error::Corrupt);
    }
    Ok(u64::from_le_bytes(h[8..].try_into().unwrap()))
}

impl Vault {
    fn path(&self) -> PathBuf {
        self.dir.join("vault.bin")
    }

    /// Opens (creating if needed) the vault in `dir` and returns every event in order.
    pub fn open(dir: &Path, key: &MasterKey) -> Result<(Vault, Vec<Event>)> {
        std::fs::create_dir_all(dir)?;
        let _lock = LockGuard::acquire(dir, true)?;
        let path = dir.join("vault.bin");
        if !path.exists() || std::fs::metadata(&path)?.len() < HEADER {
            let mut f = File::create(&path)?;
            f.write_all(MAGIC)?;
            f.write_all(&1u64.to_le_bytes())?;
            f.sync_all()?;
        }
        let mut v = Vault {
            dir: dir.to_owned(),
            cipher: Cipher::new(key),
            generation: 0,
            pos: HEADER,
            next_seq: 0,
            blobs: HashMap::new(),
        };
        let events = v.reload()?;
        Ok((v, events))
    }

    fn scan(&self, f: &mut File, generation: u64, from: u64, mut seq: u64) -> Result<Scan> {
        let len = f.metadata()?.len();
        let mut pos = from;
        let mut events = Vec::new();
        let mut blobs = Vec::new();
        f.seek(SeekFrom::Start(pos))?;
        let mut r = std::io::BufReader::new(f);
        while pos + 5 <= len {
            let mut head = [0u8; 5];
            r.read_exact(&mut head)?;
            let flen = u32::from_le_bytes(head[..4].try_into().unwrap());
            if flen > MAX_FRAME || pos + 5 + u64::from(flen) > len {
                break; // torn tail: ignored, overwritten by the next append
            }
            if head[4] == KIND_EVENT {
                let mut sealed = vec![0u8; flen as usize];
                r.read_exact(&mut sealed)?;
                let plain = self.cipher.open(&sealed, &aad(generation, seq))?;
                events.push(serde_json::from_slice::<Event>(&plain).unwrap_or(Event::Unknown));
            } else {
                r.seek_relative(i64::from(flen))?;
                blobs.push((pos, seq));
            }
            pos += 5 + u64::from(flen);
            seq += 1;
        }
        Ok(Scan {
            events,
            blobs,
            end: pos,
            next_seq: seq,
        })
    }

    /// Re-reads the whole file; the caller holds the lock.
    fn reload(&mut self) -> Result<Vec<Event>> {
        let mut f = File::open(self.path())?;
        let generation = read_header(&mut f)?;
        let s = self.scan(&mut f, generation, HEADER, 0)?;
        self.generation = generation;
        self.pos = s.end;
        self.next_seq = s.next_seq;
        self.blobs = s.blobs.into_iter().collect();
        Ok(s.events)
    }

    /// Picks up what other processes did since we last looked.
    pub fn sync(&mut self) -> Result<Delta> {
        let _lock = LockGuard::acquire(&self.dir, false)?;
        let mut f = File::open(self.path())?;
        let generation = read_header(&mut f)?;
        if generation != self.generation {
            return Ok(Delta::Reloaded(self.reload()?));
        }
        if f.metadata()?.len() <= self.pos {
            return Ok(Delta::Unchanged);
        }
        let s = self.scan(&mut f, generation, self.pos, self.next_seq)?;
        self.pos = s.end;
        self.next_seq = s.next_seq;
        self.blobs.extend(s.blobs);
        Ok(if s.events.is_empty() {
            Delta::Unchanged
        } else {
            Delta::Appended(s.events)
        })
    }

    /// Appends one event, optionally preceded by a blob frame; returns that frame's offset.
    /// An error whose text is [`CONFLICT`] means somebody else wrote first: `sync`, then retry.
    pub fn append(&mut self, ev: &Event, blob: Option<&[u8]>) -> Result<Option<u64>> {
        let _lock = LockGuard::acquire(&self.dir, true)?;
        let mut f = OpenOptions::new().read(true).write(true).open(self.path())?;
        let generation = read_header(&mut f)?;
        let len = f.metadata()?.len();
        if generation != self.generation || len < self.pos {
            return Err(Error::Other(CONFLICT.into()));
        }
        if len > self.pos {
            // Either someone appended complete frames (conflict) or this is a torn tail that may be cut.
            let s = self.scan(&mut f, generation, self.pos, self.next_seq)?;
            if !s.events.is_empty() || s.end != self.pos {
                return Err(Error::Other(CONFLICT.into()));
            }
            f.set_len(self.pos)?;
        }
        f.seek(SeekFrom::Start(self.pos))?;
        let mut buf = Vec::new();
        let mut seq = self.next_seq;
        let mut blob_at = None;
        if let Some(b) = blob {
            blob_at = Some(self.pos);
            self.blobs.insert(self.pos, seq);
            push_frame(&self.cipher, generation, &mut buf, KIND_BLOB, b, seq);
            seq += 1;
        }
        let json = serde_json::to_vec(ev).map_err(Error::other)?;
        push_frame(&self.cipher, generation, &mut buf, KIND_EVENT, &json, seq);
        seq += 1;
        f.write_all(&buf)?;
        f.sync_data()?;
        self.pos += buf.len() as u64;
        self.next_seq = seq;
        Ok(blob_at)
    }

    /// Reads and decrypts the blob frame that starts at `off`.
    pub fn read_blob(&self, off: u64) -> Result<Vec<u8>> {
        let seq = *self.blobs.get(&off).ok_or(Error::Corrupt)?;
        let _lock = LockGuard::acquire(&self.dir, false)?;
        let mut f = File::open(self.path())?;
        if read_header(&mut f)? != self.generation {
            return Err(Error::Other(CONFLICT.into()));
        }
        f.seek(SeekFrom::Start(off))?;
        let mut head = [0u8; 5];
        f.read_exact(&mut head).map_err(|_| Error::Corrupt)?;
        let flen = u32::from_le_bytes(head[..4].try_into().unwrap());
        if head[4] != KIND_BLOB || flen > MAX_FRAME {
            return Err(Error::Corrupt);
        }
        let mut sealed = vec![0u8; flen as usize];
        f.read_exact(&mut sealed).map_err(|_| Error::Corrupt)?;
        self.cipher.open(&sealed, &aad(self.generation, seq))
    }

    /// Replaces the vault with a new sealed file holding exactly `entries`, under a new generation.
    /// For `Put` entries with a blob the item's `blob_off` is set to the new location (also in the
    /// returned events, which the caller uses to refresh its index).
    pub fn rewrite(&mut self, entries: Vec<Entry>) -> Result<Vec<Event>> {
        let _lock = LockGuard::acquire(&self.dir, true)?;
        let mut cur = File::open(self.path())?;
        let generation = read_header(&mut cur)?.max(self.generation) + 1;
        drop(cur);
        let mut out = Vec::new();
        out.extend_from_slice(MAGIC);
        out.extend_from_slice(&generation.to_le_bytes());
        let mut seq = 0u64;
        let mut events = Vec::with_capacity(entries.len());
        let mut blobs = HashMap::new();
        for e in entries {
            let mut ev = e.event;
            if let (Some(b), Event::Put { item }) = (&e.blob, &mut ev) {
                item.blob_off = out.len() as u64;
                blobs.insert(item.blob_off, seq);
                push_frame(&self.cipher, generation, &mut out, KIND_BLOB, b, seq);
                seq += 1;
            }
            let json = serde_json::to_vec(&ev).map_err(Error::other)?;
            push_frame(&self.cipher, generation, &mut out, KIND_EVENT, &json, seq);
            seq += 1;
            events.push(ev);
        }
        let tmp = self.dir.join("vault.bin.tmp");
        {
            let mut f = File::create(&tmp)?;
            f.write_all(&out)?;
            f.sync_all()?;
        }
        std::fs::rename(&tmp, self.path())?;
        self.generation = generation;
        self.pos = out.len() as u64;
        self.next_seq = seq;
        self.blobs = blobs;
        Ok(events)
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::crypto::random_bytes;
    use crate::model::{BlobKind, Kind};

    fn item(id: u64, text: &str) -> Item {
        Item {
            id,
            kind: Kind::Text,
            hash: format!("{id:064x}"),
            text: text.into(),
            blob_kind: BlobKind::None,
            blob_off: 0,
            blob_len: 0,
            img: None,
            source_app: String::new(),
            source_title: String::new(),
            created: 1,
            last_used: 1,
            count: 1,
            pinned: false,
            masked: false,
            volatile: false,
        }
    }

    #[test]
    fn append_reopen_and_blob() {
        let dir = tempfile::tempdir().unwrap();
        let key = random_bytes();
        let (mut v, ev) = Vault::open(dir.path(), &key).unwrap();
        assert!(ev.is_empty());
        let off = v
            .append(&Event::Put { item: item(1, "alpha") }, Some(b"BLOBDATA"))
            .unwrap();
        v.append(&Event::Touch { id: 1, at: 5 }, None).unwrap();
        assert_eq!(v.read_blob(off.unwrap()).unwrap(), b"BLOBDATA");
        let (v2, ev) = Vault::open(dir.path(), &key).unwrap();
        assert_eq!(ev.len(), 2);
        assert_eq!(v2.read_blob(off.unwrap()).unwrap(), b"BLOBDATA");
    }

    #[test]
    fn wrong_key_is_rejected() {
        let dir = tempfile::tempdir().unwrap();
        let (mut v, _) = Vault::open(dir.path(), &random_bytes()).unwrap();
        v.append(&Event::Put { item: item(1, "x") }, None).unwrap();
        assert!(Vault::open(dir.path(), &random_bytes()).is_err());
    }

    #[test]
    fn nothing_readable_on_disk() {
        let dir = tempfile::tempdir().unwrap();
        let (mut v, _) = Vault::open(dir.path(), &random_bytes()).unwrap();
        v.append(
            &Event::Put {
                item: item(1, "MARKER-needle-12345"),
            },
            Some(b"MARKER-blob-67890"),
        )
        .unwrap();
        v.rewrite(vec![Entry {
            event: Event::Put {
                item: item(1, "MARKER-needle-12345"),
            },
            blob: Some(b"MARKER-blob-67890".to_vec()),
        }])
        .unwrap();
        for f in std::fs::read_dir(dir.path()).unwrap() {
            let bytes = std::fs::read(f.unwrap().path()).unwrap();
            assert!(!bytes.windows(6).any(|w| w == b"MARKER"));
        }
    }

    #[test]
    fn two_handles_stay_in_sync_and_conflicts_are_detected() {
        let dir = tempfile::tempdir().unwrap();
        let key = random_bytes();
        let (mut a, _) = Vault::open(dir.path(), &key).unwrap();
        let (mut b, _) = Vault::open(dir.path(), &key).unwrap();
        a.append(
            &Event::Put {
                item: item(1, "from a"),
            },
            None,
        )
        .unwrap();
        let err = b
            .append(
                &Event::Put {
                    item: item(2, "from b"),
                },
                None,
            )
            .unwrap_err();
        assert!(err.to_string().contains(CONFLICT));
        assert!(matches!(b.sync().unwrap(), Delta::Appended(e) if e.len() == 1));
        b.append(
            &Event::Put {
                item: item(2, "from b"),
            },
            None,
        )
        .unwrap();
        a.rewrite(vec![]).unwrap();
        assert!(matches!(b.sync().unwrap(), Delta::Reloaded(e) if e.is_empty()));
    }

    #[test]
    fn torn_tail_is_ignored_and_cut() {
        let dir = tempfile::tempdir().unwrap();
        let key = random_bytes();
        let (mut v, _) = Vault::open(dir.path(), &key).unwrap();
        v.append(&Event::Put { item: item(1, "keep") }, None).unwrap();
        let p = dir.path().join("vault.bin");
        let mut f = OpenOptions::new().append(true).open(&p).unwrap();
        f.write_all(&[200, 0, 0, 0, 0, 1, 2, 3]).unwrap();
        drop(f);
        let (mut v2, ev) = Vault::open(dir.path(), &key).unwrap();
        assert_eq!(ev.len(), 1);
        v2.append(&Event::Touch { id: 1, at: 2 }, None).unwrap();
        let (_, ev) = Vault::open(dir.path(), &key).unwrap();
        assert_eq!(ev.len(), 2);
    }
}
