//! The clipboard history: an in-memory index (rebuilt from the vault at start) in front of the encrypted log.

use std::collections::HashMap;
use std::path::Path;

use crate::classify::classify_text;
use crate::dedup::content_hash;
use crate::error::{Error, Result};
use crate::model::{Added, BlobKind, Item, Kind, NewItem, Reject};
use crate::search::fuzzy_score;
use crate::snippets::Snippet;
use crate::vault::{CONFLICT, Delta, Entry, Event, Vault};

const DAY: u64 = 86_400;
/// Text searched per entry; the rest of a very long text is stored but not matched.
const SEARCH_CHARS: usize = 4000;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Limits {
    pub max_item_bytes: u64,
    /// 0 = keep forever.
    pub max_age_days: u32,
    /// 0 = unlimited. Pinned entries are never evicted.
    pub max_items: u32,
}

impl Default for Limits {
    fn default() -> Self {
        Limits {
            max_item_bytes: 16 << 20,
            max_age_days: 0,
            max_items: 0,
        }
    }
}

#[derive(Debug)]
pub enum AddError {
    Rejected(Reject),
    Store(Error),
}

impl From<Error> for AddError {
    fn from(e: Error) -> Self {
        AddError::Store(e)
    }
}

#[derive(Clone, Debug, Default)]
pub struct Query {
    pub text: String,
    pub kinds: Vec<Kind>,
    pub app: Option<String>,
    /// Only entries used at or after this Unix time.
    pub since: Option<u64>,
    pub pinned_only: bool,
}

struct Slot {
    item: Item,
    lower: String,
    /// Position in the order of changes: breaks ties between entries used in the same second.
    order: u64,
}

pub struct History {
    vault: Option<Vault>,
    slots: HashMap<u64, Slot>,
    by_hash: HashMap<String, u64>,
    /// Blobs of entries that have no vault behind them (in-memory entries, the demo).
    mem_blobs: HashMap<u64, Vec<u8>>,
    next_id: u64,
    rev: u64,
    snippets: HashMap<u64, Snippet>,
}

fn lower_of(item: &Item) -> String {
    let mut s: String = item.text.chars().take(SEARCH_CHARS).collect::<String>().to_lowercase();
    if !item.source_app.is_empty() {
        s.push('\n');
        s.push_str(&item.source_app.to_lowercase());
    }
    s
}

impl History {
    pub fn memory() -> History {
        History {
            vault: None,
            slots: HashMap::new(),
            by_hash: HashMap::new(),
            mem_blobs: HashMap::new(),
            next_id: 1,
            rev: 0,
            snippets: HashMap::new(),
        }
    }

    pub fn open(dir: &Path, key: &crate::crypto::MasterKey) -> Result<History> {
        let (vault, events) = Vault::open(dir, key)?;
        let mut h = History::memory();
        h.vault = Some(vault);
        for e in events {
            h.apply(e);
        }
        Ok(h)
    }

    /// Grows on every change; lets the UI cache query results.
    pub fn revision(&self) -> u64 {
        self.rev
    }

    pub fn is_persistent(&self) -> bool {
        self.vault.is_some()
    }

    fn apply(&mut self, e: Event) {
        self.rev += 1;
        match e {
            Event::Put { item } => {
                self.next_id = self.next_id.max(item.id + 1);
                if let Some(old) = self.slots.remove(&item.id) {
                    self.by_hash.remove(&old.item.hash);
                }
                self.by_hash.insert(item.hash.clone(), item.id);
                let lower = lower_of(&item);
                self.slots.insert(
                    item.id,
                    Slot {
                        item,
                        lower,
                        order: self.rev,
                    },
                );
            }
            Event::Touch { id, at } => {
                if let Some(s) = self.slots.get_mut(&id) {
                    s.item.count += 1;
                    s.item.last_used = s.item.last_used.max(at);
                    s.order = self.rev;
                }
            }
            Event::Pin { id, pinned } => {
                if let Some(s) = self.slots.get_mut(&id) {
                    s.item.pinned = pinned;
                }
            }
            Event::Delete { id } => self.remove(id),
            Event::Clear { keep_pinned } => {
                let doomed: Vec<u64> = self
                    .slots
                    .values()
                    .filter(|s| !s.item.volatile && !(keep_pinned && s.item.pinned))
                    .map(|s| s.item.id)
                    .collect();
                for id in doomed {
                    self.remove(id);
                }
            }
            Event::SnipPut { snippet } => {
                self.next_id = self.next_id.max(snippet.id + 1);
                self.snippets.insert(snippet.id, snippet);
            }
            Event::SnipDelete { id } => {
                self.snippets.remove(&id);
            }
            Event::Unknown => {}
        }
    }

    fn remove(&mut self, id: u64) {
        if let Some(s) = self.slots.remove(&id) {
            self.by_hash.remove(&s.item.hash);
        }
        self.mem_blobs.remove(&id);
        self.rev += 1;
    }

    /// Picks up changes made by another process (the CLI).
    pub fn refresh(&mut self) -> Result<bool> {
        let Some(v) = self.vault.as_mut() else { return Ok(false) };
        match v.sync()? {
            Delta::Unchanged => Ok(false),
            Delta::Appended(events) => {
                for e in events {
                    self.apply(e);
                }
                Ok(true)
            }
            Delta::Reloaded(events) => {
                self.slots.retain(|_, s| s.item.volatile);
                self.by_hash.retain(|_, id| self.slots.contains_key(id));
                self.snippets.clear();
                for e in events {
                    self.apply(e);
                }
                Ok(true)
            }
        }
    }

    /// Writes an event (retrying after a lost race with another process), then applies it.
    fn commit(&mut self, mut ev: Event, blob: Option<&[u8]>) -> Result<()> {
        if self.vault.is_none() {
            if let (Event::Put { item }, Some(b)) = (&ev, blob) {
                self.mem_blobs.insert(item.id, b.to_vec());
            }
            self.apply(ev);
            return Ok(());
        }
        for _ in 0..8 {
            let v = self.vault.as_mut().expect("checked above");
            match v.append(&ev, blob) {
                Ok(off) => {
                    if let (Some(off), Event::Put { item }) = (off, &mut ev) {
                        item.blob_off = off;
                    }
                    self.apply(ev);
                    return Ok(());
                }
                Err(Error::Other(m)) if m == CONFLICT => {
                    self.refresh()?;
                    if let Event::Put { item } = &mut ev {
                        item.id = item.id.max(self.next_id);
                    }
                }
                Err(e) => return Err(e),
            }
        }
        Err(Error::other("хранилище занято другим процессом"))
    }

    fn prepare(new: NewItem, now: u64, limits: &Limits) -> std::result::Result<(Item, Vec<u8>), Reject> {
        let kind = new.kind.unwrap_or_else(|| classify_text(&new.text));
        if new.text.trim().is_empty() && new.blob.is_empty() {
            return Err(Reject::Empty);
        }
        if (new.text.len() + new.blob.len()) as u64 > limits.max_item_bytes {
            return Err(Reject::TooLarge);
        }
        let hash = content_hash(kind, &new.text, &new.blob);
        let item = Item {
            id: 0,
            kind,
            hash,
            text: new.text,
            blob_kind: if new.blob.is_empty() {
                BlobKind::None
            } else {
                new.blob_kind
            },
            blob_off: 0,
            blob_len: new.blob.len() as u64,
            img: new.img,
            source_app: new.source_app,
            source_title: new.source_title,
            created: now,
            last_used: now,
            count: 1,
            pinned: false,
            masked: new.masked,
            secret: new.secret,
            volatile: false,
        };
        Ok((item, new.blob))
    }

    /// Adds an entry, or — if the same content is already there — moves it to the top and counts the repeat.
    pub fn add(&mut self, new: NewItem, now: u64, limits: &Limits) -> std::result::Result<Added, AddError> {
        let (mut item, blob) = Self::prepare(new, now, limits).map_err(AddError::Rejected)?;
        if let Some(&id) = self.by_hash.get(&item.hash) {
            if self.slots.get(&id).is_some_and(|s| s.item.volatile) {
                self.apply(Event::Touch { id, at: now });
            } else {
                self.commit(Event::Touch { id, at: now }, None)?;
            }
            return Ok(Added::Bumped(id));
        }
        item.id = self.next_id;
        let id = item.id;
        let hash = item.hash.clone();
        let has_blob = !blob.is_empty();
        self.commit(Event::Put { item }, has_blob.then_some(&blob[..]))?;
        self.enforce(limits, now)?;
        Ok(Added::New(self.by_hash.get(&hash).copied().unwrap_or(id)))
    }

    /// An entry that never reaches the disk (a secret kept by the "memory only" policy).
    pub fn add_volatile(&mut self, new: NewItem, now: u64, limits: &Limits) -> std::result::Result<Added, AddError> {
        let (mut item, blob) = Self::prepare(new, now, limits).map_err(AddError::Rejected)?;
        if let Some(&id) = self.by_hash.get(&item.hash) {
            self.apply(Event::Touch { id, at: now });
            return Ok(Added::Bumped(id));
        }
        item.id = self.next_id;
        item.volatile = true;
        let id = item.id;
        self.next_id += 1;
        self.rev += 1;
        self.mem_blobs.insert(id, blob);
        self.by_hash.insert(item.hash.clone(), id);
        let lower = lower_of(&item);
        self.slots.insert(
            id,
            Slot {
                item,
                lower,
                order: self.rev,
            },
        );
        Ok(Added::New(id))
    }

    pub fn get(&self, id: u64) -> Option<&Item> {
        self.slots.get(&id).map(|s| &s.item)
    }

    pub fn len(&self) -> usize {
        self.slots.len()
    }

    pub fn is_empty(&self) -> bool {
        self.slots.is_empty()
    }

    /// The binary payload (HTML / RTF / PNG) of an entry.
    pub fn blob(&self, id: u64) -> Result<Option<Vec<u8>>> {
        let Some(s) = self.slots.get(&id) else { return Ok(None) };
        if let Some(b) = self.mem_blobs.get(&id) {
            return Ok(Some(b.clone()));
        }
        if s.item.blob_kind == BlobKind::None {
            return Ok(None);
        }
        match &self.vault {
            Some(v) => v.read_blob(s.item.blob_off).map(Some),
            None => Ok(None),
        }
    }

    /// Entries matching the query: pinned first, then the best match / the most recent.
    pub fn query(&self, q: &Query) -> Vec<&Item> {
        let needle = q.text.trim().to_lowercase();
        let mut hits: Vec<(i32, u64, &Item)> = Vec::new();
        for s in self.slots.values() {
            let it = &s.item;
            if (!q.kinds.is_empty() && !q.kinds.contains(&it.kind))
                || (q.pinned_only && !it.pinned)
                || q.since.is_some_and(|t| it.last_used < t)
                || q.app.as_ref().is_some_and(|a| !it.source_app.eq_ignore_ascii_case(a))
            {
                continue;
            }
            let score = if needle.is_empty() {
                0
            } else {
                match fuzzy_score(&needle, &s.lower) {
                    Some(sc) => sc,
                    None => continue,
                }
            };
            hits.push((score, s.order, it));
        }
        hits.sort_by(|a, b| {
            b.2.pinned
                .cmp(&a.2.pinned)
                .then(b.0.cmp(&a.0))
                .then(b.2.last_used.cmp(&a.2.last_used))
                .then(b.1.cmp(&a.1))
        });
        hits.into_iter().map(|(_, _, it)| it).collect()
    }

    /// Distinct source applications, most used first.
    pub fn apps(&self) -> Vec<String> {
        let mut counts: HashMap<&str, usize> = HashMap::new();
        for s in self.slots.values() {
            if !s.item.source_app.is_empty() {
                *counts.entry(s.item.source_app.as_str()).or_default() += 1;
            }
        }
        let mut v: Vec<(&str, usize)> = counts.into_iter().collect();
        v.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(b.0)));
        v.into_iter().map(|(a, _)| a.to_owned()).collect()
    }

    /// Counts per kind: all the diagnostics ever need (never any content).
    pub fn counts(&self) -> Vec<(Kind, usize)> {
        Kind::ALL
            .into_iter()
            .map(|k| (k, self.slots.values().filter(|s| s.item.kind == k).count()))
            .collect()
    }

    /// All snippets, by folder then name.
    pub fn snippets(&self) -> Vec<&Snippet> {
        let mut v: Vec<&Snippet> = self.snippets.values().collect();
        v.sort_by_key(|s| (s.folder.to_lowercase(), s.name.to_lowercase(), s.id));
        v
    }

    pub fn snippet(&self, id: u64) -> Option<&Snippet> {
        self.snippets.get(&id)
    }

    /// The snippet whose keyword is exactly `word` (case-insensitive).
    pub fn snippet_by_keyword(&self, word: &str) -> Option<&Snippet> {
        let w = word.trim().to_lowercase();
        (!w.is_empty())
            .then(|| {
                self.snippets
                    .values()
                    .find(|s| !s.keyword.is_empty() && s.keyword.to_lowercase() == w)
            })
            .flatten()
    }

    /// Distinct folders, sorted.
    pub fn snippet_folders(&self) -> Vec<String> {
        let mut f: Vec<String> = self
            .snippets
            .values()
            .map(|s| s.folder.clone())
            .filter(|f| !f.is_empty())
            .collect();
        f.sort_by_key(|s| s.to_lowercase());
        f.dedup();
        f
    }

    /// Saves a snippet (id 0 = a new one) and returns its id.
    pub fn put_snippet(&mut self, mut snippet: Snippet, now: u64) -> Result<u64> {
        if snippet.id == 0 {
            snippet.id = self.next_id;
        }
        snippet.updated = now;
        let id = snippet.id;
        self.commit(Event::SnipPut { snippet }, None)?;
        Ok(id)
    }

    pub fn delete_snippet(&mut self, id: u64) -> Result<()> {
        self.commit(Event::SnipDelete { id }, None)?;
        self.compact()
    }

    pub fn pin(&mut self, id: u64, pinned: bool) -> Result<()> {
        match self.slots.get(&id) {
            Some(s) if !s.item.volatile => self.commit(Event::Pin { id, pinned }, None),
            Some(_) => {
                self.apply(Event::Pin { id, pinned });
                Ok(())
            }
            None => Ok(()),
        }
    }

    /// Deletes an entry and rewrites the vault so its ciphertext does not linger in the file.
    pub fn delete(&mut self, id: u64) -> Result<()> {
        let volatile = self.slots.get(&id).is_some_and(|s| s.item.volatile);
        if volatile {
            self.remove(id);
            return Ok(());
        }
        self.commit(Event::Delete { id }, None)?;
        self.compact()
    }

    pub fn clear(&mut self, keep_pinned: bool) -> Result<()> {
        self.commit(Event::Clear { keep_pinned }, None)?;
        // the vault event leaves volatile entries alone (they are not in it); "clear all" must not
        self.slots.retain(|_, s| keep_pinned && s.item.pinned);
        self.by_hash.retain(|_, id| self.slots.contains_key(id));
        self.mem_blobs.retain(|id, _| self.slots.contains_key(id));
        self.rev += 1;
        self.compact()
    }

    /// Removes entries beyond the age / count limits (pinned ones stay).
    pub fn enforce(&mut self, limits: &Limits, now: u64) -> Result<()> {
        let mut doomed: Vec<u64> = Vec::new();
        if limits.max_age_days > 0 {
            let cutoff = now.saturating_sub(u64::from(limits.max_age_days) * DAY);
            doomed.extend(
                self.slots
                    .values()
                    .filter(|s| !s.item.pinned && !s.item.volatile && s.item.last_used < cutoff)
                    .map(|s| s.item.id),
            );
        }
        if limits.max_items > 0 {
            let mut live: Vec<&Item> = self
                .slots
                .values()
                .map(|s| &s.item)
                .filter(|i| !i.pinned && !i.volatile && !doomed.contains(&i.id))
                .collect();
            if live.len() > limits.max_items as usize {
                live.sort_by_key(|i| (std::cmp::Reverse(i.last_used), std::cmp::Reverse(i.id)));
                doomed.extend(live[limits.max_items as usize..].iter().map(|i| i.id));
            }
        }
        if doomed.is_empty() {
            return Ok(());
        }
        for id in doomed {
            self.commit(Event::Delete { id }, None)?;
        }
        self.compact()
    }

    /// Rewrites the vault with only the live entries.
    pub fn compact(&mut self) -> Result<()> {
        if self.vault.is_none() {
            return Ok(());
        }
        let mut ids: Vec<u64> = self
            .slots
            .values()
            .filter(|s| !s.item.volatile)
            .map(|s| s.item.id)
            .collect();
        ids.sort_unstable();
        let mut entries = Vec::with_capacity(ids.len());
        for id in ids {
            let item = self.slots[&id].item.clone();
            let blob = if item.blob_kind == BlobKind::None {
                None
            } else {
                self.blob(id)?
            };
            entries.push(Entry {
                event: Event::Put { item },
                blob,
            });
        }
        let mut snips: Vec<&Snippet> = self.snippets.values().collect();
        snips.sort_by_key(|s| s.id);
        entries.extend(snips.into_iter().map(|s| Entry {
            event: Event::SnipPut { snippet: s.clone() },
            blob: None,
        }));
        let events = self.vault.as_mut().expect("checked above").rewrite(entries)?;
        for e in events {
            if let Event::Put { item } = e
                && let Some(s) = self.slots.get_mut(&item.id)
            {
                s.item.blob_off = item.blob_off;
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::crypto::random_bytes;

    fn text(t: &str) -> NewItem {
        NewItem {
            text: t.into(),
            source_app: "notepad.exe".into(),
            ..Default::default()
        }
    }

    fn lim() -> Limits {
        Limits::default()
    }

    #[test]
    fn duplicates_bump_and_move_to_top() {
        let mut h = History::memory();
        let a = h.add(text("alpha"), 10, &lim()).unwrap();
        let b = h.add(text("beta"), 20, &lim()).unwrap();
        assert!(matches!((a, b), (Added::New(_), Added::New(_))));
        let again = h.add(text("  alpha\r\n"), 30, &lim()).unwrap();
        assert_eq!(again, Added::Bumped(a.id()));
        assert_eq!(h.len(), 2);
        let it = h.get(a.id()).unwrap();
        assert_eq!((it.count, it.last_used), (2, 30));
        let order: Vec<u64> = h.query(&Query::default()).iter().map(|i| i.id).collect();
        assert_eq!(order, vec![a.id(), b.id()]);
    }

    #[test]
    fn persistence_dedup_pin_delete_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let key = random_bytes();
        let id = {
            let mut h = History::open(dir.path(), &key).unwrap();
            let a = h.add(text("keep me"), 1, &lim()).unwrap().id();
            h.add(text("keep me"), 2, &lim()).unwrap();
            let b = h.add(text("drop me"), 3, &lim()).unwrap().id();
            h.pin(a, true).unwrap();
            h.delete(b).unwrap();
            a
        };
        let h = History::open(dir.path(), &key).unwrap();
        assert_eq!(h.len(), 1);
        let it = h.get(id).unwrap();
        assert_eq!((it.count, it.pinned, it.text.as_str()), (2, true, "keep me"));
    }

    #[test]
    fn blobs_survive_compaction() {
        let dir = tempfile::tempdir().unwrap();
        let key = random_bytes();
        let mut h = History::open(dir.path(), &key).unwrap();
        let img = NewItem {
            kind: Some(Kind::Image),
            blob: vec![7; 5000],
            blob_kind: BlobKind::Png,
            img: Some((10, 10)),
            ..Default::default()
        };
        let a = h.add(img, 1, &lim()).unwrap().id();
        let junk = h.add(text("junk"), 2, &lim()).unwrap().id();
        h.delete(junk).unwrap();
        assert_eq!(h.blob(a).unwrap().unwrap(), vec![7; 5000]);
        let h2 = History::open(dir.path(), &key).unwrap();
        assert_eq!(h2.blob(a).unwrap().unwrap(), vec![7; 5000]);
    }

    #[test]
    fn query_filters_and_fuzzy() {
        let mut h = History::memory();
        h.add(text("hello world"), 1, &lim()).unwrap();
        h.add(text("https://example.com"), 2, &lim()).unwrap();
        h.add(text("#ff0000"), 3, &lim()).unwrap();
        let q = |t: &str| Query {
            text: t.into(),
            ..Default::default()
        };
        assert_eq!(h.query(&q("hlwrd")).len(), 1);
        assert_eq!(h.query(&q("zzz")).len(), 0);
        let links = Query {
            kinds: vec![Kind::Link],
            ..Default::default()
        };
        assert_eq!(h.query(&links)[0].text, "https://example.com");
        let recent = Query {
            since: Some(3),
            ..Default::default()
        };
        assert_eq!(h.query(&recent).len(), 1);
        let app = Query {
            app: Some("NOTEPAD.exe".into()),
            ..Default::default()
        };
        assert_eq!(h.query(&app).len(), 3);
        assert_eq!(h.apps(), vec!["notepad.exe".to_string()]);
    }

    #[test]
    fn limits() {
        let mut h = History::memory();
        let tiny = Limits {
            max_item_bytes: 10,
            ..lim()
        };
        assert!(matches!(
            h.add(text("this is far too long"), 1, &tiny),
            Err(AddError::Rejected(Reject::TooLarge))
        ));
        assert!(matches!(
            h.add(text("   "), 1, &lim()),
            Err(AddError::Rejected(Reject::Empty))
        ));

        let capped = Limits { max_items: 2, ..lim() };
        let a = h.add(text("one"), 1, &capped).unwrap().id();
        h.pin(a, true).unwrap();
        h.add(text("two"), 2, &capped).unwrap();
        h.add(text("three"), 3, &capped).unwrap();
        h.add(text("four"), 4, &capped).unwrap();
        let texts: Vec<&str> = h.query(&Query::default()).iter().map(|i| i.text.as_str()).collect();
        assert_eq!(
            texts,
            vec!["one", "four", "three"],
            "pinned stays; the oldest unpinned go"
        );

        let aged = Limits {
            max_age_days: 1,
            ..lim()
        };
        h.enforce(&aged, 3 * DAY).unwrap();
        let texts: Vec<&str> = h.query(&Query::default()).iter().map(|i| i.text.as_str()).collect();
        assert_eq!(texts, vec!["one"]);
    }

    #[test]
    fn clear_keeps_pinned_on_request() {
        let mut h = History::memory();
        let a = h.add(text("pinned"), 1, &lim()).unwrap().id();
        h.add(text("loose"), 2, &lim()).unwrap();
        h.pin(a, true).unwrap();
        h.clear(true).unwrap();
        assert_eq!(h.len(), 1);
        h.clear(false).unwrap();
        assert!(h.is_empty());
    }

    #[test]
    fn two_processes_see_each_other() {
        let dir = tempfile::tempdir().unwrap();
        let key = random_bytes();
        let mut app = History::open(dir.path(), &key).unwrap();
        let mut cli = History::open(dir.path(), &key).unwrap();
        app.add(text("from app"), 1, &lim()).unwrap();
        let id = app.add(text("second"), 2, &lim()).unwrap().id();
        assert!(cli.refresh().unwrap());
        assert_eq!(cli.len(), 2);
        cli.delete(id).unwrap();
        assert!(app.refresh().unwrap());
        assert_eq!(app.len(), 1);
        app.add(text("after purge"), 3, &lim()).unwrap();
        assert_eq!(app.len(), 2);
    }

    #[test]
    fn volatile_entries_never_reach_the_file() {
        let dir = tempfile::tempdir().unwrap();
        let key = random_bytes();
        let mut h = History::open(dir.path(), &key).unwrap();
        h.add_volatile(text("VOLATILE-SECRET-123"), 1, &lim()).unwrap();
        let keep = h.add(text("normal"), 2, &lim()).unwrap().id();
        h.pin(keep, true).unwrap();
        h.compact().unwrap();
        assert_eq!(h.len(), 2);
        let h2 = History::open(dir.path(), &key).unwrap();
        assert_eq!(h2.len(), 1);
    }

    #[test]
    fn snippets_survive_restart_compaction_and_clear() {
        let dir = tempfile::tempdir().unwrap();
        let key = random_bytes();
        let id = {
            let mut h = History::open(dir.path(), &key).unwrap();
            let s = Snippet {
                name: "Sig".into(),
                text: "Best, {date}".into(),
                folder: "Mail".into(),
                keyword: "sig".into(),
                ..Default::default()
            };
            let id = h.put_snippet(s, 10).unwrap();
            let gone = h
                .put_snippet(
                    Snippet {
                        name: "tmp".into(),
                        ..Default::default()
                    },
                    11,
                )
                .unwrap();
            h.delete_snippet(gone).unwrap();
            h.add(text("an entry"), 12, &lim()).unwrap();
            h.clear(false).unwrap(); // clearing the history leaves snippets alone
            id
        };
        let h = History::open(dir.path(), &key).unwrap();
        assert!(h.is_empty());
        assert_eq!(h.snippets().len(), 1);
        let s = h.snippet(id).unwrap();
        assert_eq!((s.name.as_str(), s.folder.as_str(), s.updated), ("Sig", "Mail", 10));
        assert_eq!(h.snippet_by_keyword(" SIG ").map(|s| s.id), Some(id));
        assert_eq!(h.snippet_by_keyword("other"), None);
        assert_eq!(h.snippet_folders(), vec!["Mail".to_string()]);
    }
}
