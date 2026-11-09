//! Disposable, fail-closed disk working state. Never an accepted-world store or
//! a durable authoring session. SQL cursors/locks never survive an await.
use crate::{ContentId, Error, OctreePath, Result, canonical_cbor, codec::decode};
use bytes::Bytes;
use rusqlite::{Connection, OptionalExtension, params};
use serde::{Serialize, de::DeserializeOwned};
use std::{
    collections::VecDeque,
    marker::PhantomData,
    sync::{Arc, Mutex},
};

const PAGE_BYTES: usize = 1024 * 1024;
const MAX_RECORD_BYTES: usize = 128 * 1024 * 1024;
const UPLOAD_BYTES: usize = 8 * 1024 * 1024;

/// Per local working database, not a whole-process RSS or filesystem quota.
#[derive(Clone, Copy, Debug)]
pub struct SpillLimits {
    pub database_bytes: usize,
    pub cache_kib: usize,
}
impl Default for SpillLimits {
    fn default() -> Self {
        Self {
            database_bytes: 512 * 1024 * 1024,
            cache_kib: 1024,
        }
    }
}

struct Database {
    connection: Connection,
    _directory: tempfile::TempDir,
}
struct State {
    database: Option<Database>,
    poisoned: Option<String>,
    next_collection: i64,
}
#[derive(Clone)]
pub struct SpillStore {
    state: Arc<Mutex<State>>,
    limits: SpillLimits,
}
impl Default for SpillStore {
    fn default() -> Self {
        Self::new(SpillLimits::default())
    }
}
fn failure(error: impl std::fmt::Display) -> Error {
    Error::Scratch(error.to_string())
}
impl SpillStore {
    pub(crate) fn abort(&self, message: impl Into<String>) -> Error {
        let message = message.into();
        if let Ok(mut state) = self.state.lock() {
            state.poisoned = Some(message.clone());
        }
        failure(message)
    }
    pub fn new(limits: SpillLimits) -> Self {
        Self {
            state: Arc::new(Mutex::new(State {
                database: None,
                poisoned: None,
                next_collection: 0,
            })),
            limits,
        }
    }
    fn initialized(&self) -> Result<bool> {
        let state = self
            .state
            .lock()
            .map_err(|_| failure("scratch mutex poisoned"))?;
        if let Some(error) = &state.poisoned {
            return Err(failure(error));
        }
        Ok(state.database.is_some())
    }
    fn open(&self) -> Result<Database> {
        if self.limits.database_bytes < 64 * 1024
            || self.limits.database_bytes / 4096 > i32::MAX as usize
            || !(4..=8192).contains(&self.limits.cache_kib)
        {
            return Err(failure("invalid scratch limits"));
        }
        let directory = tempfile::Builder::new()
            .prefix("ergo-working-")
            .tempdir()
            .map_err(failure)?;
        let connection =
            Connection::open(directory.path().join("working.sqlite")).map_err(failure)?;
        connection.execute_batch("PRAGMA page_size=4096; PRAGMA journal_mode=OFF; PRAGMA synchronous=OFF; PRAGMA mmap_size=0; PRAGMA temp_store=FILE; PRAGMA locking_mode=EXCLUSIVE;").map_err(failure)?;
        connection
            .pragma_update(None, "cache_size", -(self.limits.cache_kib as i64))
            .map_err(failure)?;
        connection
            .pragma_update(
                None,
                "max_page_count",
                (self.limits.database_bytes / 4096) as i64,
            )
            .map_err(failure)?;
        connection.execute_batch("CREATE TABLE records(collection INTEGER NOT NULL,key BLOB NOT NULL,value BLOB NOT NULL,PRIMARY KEY(collection,key)) WITHOUT ROWID;
            CREATE TABLE objects(id BLOB PRIMARY KEY,data BLOB NOT NULL,node INTEGER NOT NULL,uploaded INTEGER NOT NULL) WITHOUT ROWID;
            CREATE TABLE work(id BLOB PRIMARY KEY,done INTEGER NOT NULL) WITHOUT ROWID;
            CREATE INDEX work_ready ON work(done,id);").map_err(failure)?;
        Ok(Database {
            connection,
            _directory: directory,
        })
    }
    fn with<T>(&self, f: impl FnOnce(&Connection) -> Result<T>) -> Result<T> {
        let mut state = self
            .state
            .lock()
            .map_err(|_| failure("scratch mutex poisoned"))?;
        if let Some(error) = &state.poisoned {
            return Err(failure(error));
        }
        if state.database.is_none() {
            match self.open() {
                Ok(db) => state.database = Some(db),
                Err(error) => {
                    state.poisoned = Some(error.to_string());
                    return Err(error);
                }
            }
        }
        let result = f(&state.database.as_ref().unwrap().connection);
        if let Err(error) = &result
            && !matches!(error, Error::ObjectTooLarge { .. })
        {
            state.poisoned = Some(error.to_string());
        }
        result
    }
    pub fn map<K: SpillKey, V: Serialize + DeserializeOwned>(&self) -> Result<SpillMap<K, V>> {
        let mut state = self
            .state
            .lock()
            .map_err(|_| failure("scratch mutex poisoned"))?;
        if let Some(error) = &state.poisoned {
            return Err(failure(error));
        }
        state.next_collection = state
            .next_collection
            .checked_add(1)
            .ok_or_else(|| failure("scratch collection overflow"))?;
        Ok(SpillMap {
            store: self.clone(),
            collection: state.next_collection,
            len: 0,
            types: PhantomData,
        })
    }
    pub fn database_bytes(&self) -> Result<u64> {
        if !self.initialized()? {
            return Ok(0);
        }
        self.with(|db| {
            let pages: i64 = db
                .query_row("PRAGMA page_count", [], |r| r.get(0))
                .map_err(failure)?;
            Ok(u64::try_from(pages).map_err(failure)? * 4096)
        })
    }
    pub(crate) fn stage(&self, bytes: Vec<u8>, node: bool) -> Result<ContentId> {
        if bytes.len() > MAX_RECORD_BYTES {
            return Err(self.abort("staged record exceeds 128 MiB"));
        }
        let id = ContentId::of(&bytes);
        self.with(|db|{db.execute("INSERT INTO objects(id,data,node,uploaded) VALUES(?1,?2,?3,0) ON CONFLICT(id) DO UPDATE SET node=MAX(node,excluded.node)",params![id.as_str().as_bytes(),bytes,node]).map_err(failure)?;Ok(())})?;
        Ok(id)
    }
    pub(crate) fn contains_object(&self, id: &ContentId) -> Result<bool> {
        if !self.initialized()? {
            return Ok(false);
        }
        self.with(|db| {
            db.query_row(
                "SELECT EXISTS(SELECT 1 FROM objects WHERE id=?1)",
                [id.as_str().as_bytes()],
                |r| r.get(0),
            )
            .map_err(failure)
        })
    }
    pub(crate) fn object(&self, id: &ContentId, limit: usize) -> Result<Option<Bytes>> {
        if !self.initialized()? {
            return Ok(None);
        }
        self.with(|db| {
            let size: Option<i64> = db
                .query_row(
                    "SELECT length(data) FROM objects WHERE id=?1",
                    [id.as_str().as_bytes()],
                    |r| r.get(0),
                )
                .optional()
                .map_err(failure)?;
            let Some(size) = size else {
                return Ok(None);
            };
            let size = u64::try_from(size).map_err(failure)?;
            if size > limit as u64 {
                return Err(Error::ObjectTooLarge { size, limit });
            }
            let bytes: Vec<u8> = db
                .query_row(
                    "SELECT data FROM objects WHERE id=?1",
                    [id.as_str().as_bytes()],
                    |r| r.get(0),
                )
                .map_err(failure)?;
            if ContentId::of(&bytes) != *id {
                return Err(failure("staged object checksum mismatch"));
            }
            Ok(Some(bytes.into()))
        })
    }
    pub(crate) fn absorb(&self, other: &Self) -> Result<()> {
        if !other.initialized()? {
            return Ok(());
        }
        let mut after = Vec::new();
        loop {
            let page=other.with(|db|{
                let mut statement=db.prepare("SELECT id,data,node,uploaded FROM objects WHERE id>?1 ORDER BY id LIMIT 128").map_err(failure)?;
                let mut rows=statement.query([&after]).map_err(failure)?;let mut result=Vec::new();let mut bytes=0;
                while let Some(row)=rows.next().map_err(failure)?{
                    let value=row.get_ref(1).map_err(failure)?.as_blob().map_err(failure)?;
                    if !result.is_empty() && bytes+value.len()>PAGE_BYTES{break;}
                    bytes+=value.len();result.push((row.get::<_,Vec<u8>>(0).map_err(failure)?,value.to_vec(),row.get::<_,bool>(2).map_err(failure)?,row.get::<_,bool>(3).map_err(failure)?));
                }Ok(result)
            })?;
            if page.is_empty() {
                break;
            }
            for (key, bytes, node, uploaded) in page {
                let id = self.stage(bytes, node)?;
                if id.as_str().as_bytes() != key {
                    return Err(self.abort("absorbed object checksum mismatch"));
                }
                if uploaded {
                    self.mark_uploaded(&id)?;
                }
                after = key;
            }
        }
        Ok(())
    }
    pub(crate) fn start_work(
        &self,
        root: Option<&ContentId>,
        payloads: &[ContentId],
    ) -> Result<()> {
        self.with(|db| {
            db.execute("DELETE FROM work", []).map_err(failure)?;
            let mut insert = db
                .prepare("INSERT OR IGNORE INTO work(id,done) VALUES(?1,0)")
                .map_err(failure)?;
            for id in root.into_iter().chain(payloads) {
                insert.execute([id.as_str().as_bytes()]).map_err(failure)?;
            }
            Ok(())
        })
    }
    pub(crate) fn enqueue(&self, ids: impl IntoIterator<Item = ContentId>) -> Result<()> {
        self.with(|db| {
            let mut insert = db
                .prepare("INSERT OR IGNORE INTO work(id,done) VALUES(?1,0)")
                .map_err(failure)?;
            for id in ids {
                insert.execute([id.as_str().as_bytes()]).map_err(failure)?;
            }
            Ok(())
        })
    }
    pub(crate) fn next_work(&self, limit: usize) -> Result<Vec<Work>> {
        self.with(|db|{
        let mut statement=db.prepare("SELECT w.id,o.node,o.uploaded,length(o.data) FROM work w LEFT JOIN objects o ON o.id=w.id WHERE w.done=0 ORDER BY w.id LIMIT ?1").map_err(failure)?;
        let mut rows=statement.query([limit.min(128) as i64]).map_err(failure)?;let mut result=Vec::new();let mut bytes=0;
        while let Some(row)=rows.next().map_err(failure)?{
            let node:Option<bool>=row.get(1).map_err(failure)?;let uploaded:Option<bool>=row.get(2).map_err(failure)?;let size:Option<i64>=row.get(3).map_err(failure)?;let size=size.map(usize::try_from).transpose().map_err(failure)?;
            let size=if node==Some(true) || uploaded==Some(false){size.unwrap_or(0)}else{0};
            if !result.is_empty() && bytes+size>UPLOAD_BYTES{break;}
            bytes+=size;let raw:Vec<u8>=row.get(0).map_err(failure)?;let id=ContentId::from_key(&raw)?;
            result.push(Work{id,node:node.unwrap_or(false),upload:uploaded==Some(false),local:node.is_some()});
        }drop(rows);drop(statement);
        for item in &result{db.execute("UPDATE work SET done=1 WHERE id=?1",[item.id.as_str().as_bytes()]).map_err(failure)?;}Ok(result)
    })
    }
    pub(crate) fn mark_uploaded(&self, id: &ContentId) -> Result<()> {
        self.with(|db| {
            db.execute(
                "UPDATE objects SET uploaded=1 WHERE id=?1",
                [id.as_str().as_bytes()],
            )
            .map_err(failure)?;
            Ok(())
        })
    }
}
pub(crate) struct Work {
    pub id: ContentId,
    pub node: bool,
    pub upload: bool,
    pub local: bool,
}

/// Byte order must equal the key's authoritative logical order. No user values
/// are interpolated into SQL, and keys never become filesystem paths.
pub trait SpillKey: Sized {
    fn key(&self) -> Vec<u8>;
    fn from_key(bytes: &[u8]) -> Result<Self>;
}
impl SpillKey for Vec<u8> {
    fn key(&self) -> Vec<u8> {
        self.clone()
    }
    fn from_key(bytes: &[u8]) -> Result<Self> {
        Ok(bytes.to_vec())
    }
}
impl SpillKey for ContentId {
    fn key(&self) -> Vec<u8> {
        self.as_str().as_bytes().to_vec()
    }
    fn from_key(bytes: &[u8]) -> Result<Self> {
        std::str::from_utf8(bytes).map_err(failure)?.parse()
    }
}
impl SpillKey for OctreePath {
    fn key(&self) -> Vec<u8> {
        self.octants().to_vec()
    }
    fn from_key(bytes: &[u8]) -> Result<Self> {
        OctreePath::new(bytes.to_vec())
    }
}
impl SpillKey for u64 {
    fn key(&self) -> Vec<u8> {
        self.to_be_bytes().to_vec()
    }
    fn from_key(bytes: &[u8]) -> Result<Self> {
        Ok(u64::from_be_bytes(bytes.try_into().map_err(failure)?))
    }
}

pub struct SpillMap<K, V> {
    store: SpillStore,
    collection: i64,
    len: usize,
    types: PhantomData<fn() -> (K, V)>,
}
impl<K: SpillKey, V: Serialize + DeserializeOwned> SpillMap<K, V> {
    pub fn len(&self) -> usize {
        self.len
    }
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }
    pub fn contains_key(&self, key: &K) -> Result<bool> {
        if self.len == 0 {
            self.store.initialized()?;
            return Ok(false);
        }
        self.store.with(|db| {
            db.query_row(
                "SELECT EXISTS(SELECT 1 FROM records WHERE collection=?1 AND key=?2)",
                params![self.collection, key.key()],
                |r| r.get(0),
            )
            .map_err(failure)
        })
    }
    pub fn get(&self, key: &K) -> Result<Option<V>> {
        if self.len == 0 {
            self.store.initialized()?;
            return Ok(None);
        }
        self.store.with(|db| {
            let bytes: Option<Vec<u8>> = db
                .query_row(
                    "SELECT value FROM records WHERE collection=?1 AND key=?2",
                    params![self.collection, key.key()],
                    |r| r.get(0),
                )
                .optional()
                .map_err(failure)?;
            bytes.map(|v| decode(&v)).transpose()
        })
    }
    pub fn insert(&mut self, key: K, value: V) -> Result<()> {
        let bytes = canonical_cbor(&value)?;
        if bytes.len() > MAX_RECORD_BYTES {
            return Err(self.store.abort("scratch record exceeds 128 MiB"));
        }
        let present = self.contains_key(&key)?;
        self.store.with(|db|{db.execute("INSERT INTO records(collection,key,value) VALUES(?1,?2,?3) ON CONFLICT(collection,key) DO UPDATE SET value=excluded.value",params![self.collection,key.key(),bytes]).map_err(failure)?;Ok(())})?;
        if !present {
            self.len += 1;
        }
        Ok(())
    }
    pub fn remove(&mut self, key: &K) -> Result<Option<V>> {
        let before = self.get(key)?;
        if before.is_some() {
            self.store.with(|db| {
                db.execute(
                    "DELETE FROM records WHERE collection=?1 AND key=?2",
                    params![self.collection, key.key()],
                )
                .map_err(failure)?;
                Ok(())
            })?;
            self.len -= 1;
        }
        Ok(before)
    }
    pub fn pop_last(&mut self) -> Result<Option<(K, V)>> {
        if self.len == 0 {
            self.store.initialized()?;
            return Ok(None);
        }
        let key = self
            .store
            .with(|db| {
                let key: Option<Vec<u8>> = db
                    .query_row(
                        "SELECT key FROM records WHERE collection=?1 ORDER BY key DESC LIMIT 1",
                        [self.collection],
                        |r| r.get(0),
                    )
                    .optional()
                    .map_err(failure)?;
                key.map(|k| K::from_key(&k)).transpose()
            })?
            .ok_or_else(|| self.store.abort("scratch map lost its last row"))?;
        let value = self
            .remove(&key)?
            .ok_or_else(|| self.store.abort("scratch map lost a queued row"))?;
        Ok(Some((key, value)))
    }
    pub fn clear(&mut self) -> Result<()> {
        self.store.with(|db| {
            db.execute("DELETE FROM records WHERE collection=?1", [self.collection])
                .map_err(failure)?;
            Ok(())
        })?;
        self.len = 0;
        Ok(())
    }
    pub fn remove_prefix(&mut self, prefix: &[u8]) -> Result<()> {
        let mut upper = prefix.to_vec();
        while upper.last() == Some(&u8::MAX) {
            upper.pop();
        }
        let upper = if let Some(last) = upper.last_mut() {
            *last += 1;
            Some(upper)
        } else {
            None
        };
        let removed = self.store.with(|db| {
            db.execute(
                "DELETE FROM records WHERE collection=?1 AND key>=?2 AND (?3 IS NULL OR key<?3)",
                params![self.collection, prefix, upper],
            )
            .map_err(failure)
        })?;
        self.len = self
            .len
            .checked_sub(removed)
            .ok_or_else(|| self.store.abort("scratch map count mismatch"))?;
        Ok(())
    }
    pub fn page(&self, prefix: &[u8], after: Option<&K>, limit: usize) -> Result<Vec<(K, V)>> {
        if !(1..=128).contains(&limit) {
            return Err(failure("scratch page limit must be 1..=128"));
        }
        if self.len == 0 {
            self.store.initialized()?;
            return Ok(Vec::new());
        }
        self.store.with(|db|{
            let lower=after.map(|k|k.key());let mut statement=db.prepare("SELECT key,value FROM records WHERE collection=?1 AND key>=?2 AND (?3 IS NULL OR key>?3) ORDER BY key LIMIT ?4").map_err(failure)?;
            let mut rows=statement.query(params![self.collection,prefix,lower,limit as i64]).map_err(failure)?;let mut result=Vec::new();let mut bytes=0;
            while let Some(row)=rows.next().map_err(failure)?{
                let key=row.get_ref(0).map_err(failure)?.as_blob().map_err(failure)?;if !key.starts_with(prefix){break;}
                let value=row.get_ref(1).map_err(failure)?.as_blob().map_err(failure)?;
                if !result.is_empty() && bytes+key.len()+value.len()>PAGE_BYTES{break;}
                bytes+=key.len()+value.len();result.push((K::from_key(key)?,decode(value)?));
            }Ok(result)
        })
    }
    pub fn iter(&self) -> SpillIter<'_, K, V> {
        SpillIter {
            map: self,
            after: None,
            page: VecDeque::new(),
            done: false,
            remaining: self.len,
        }
    }
}
impl<K, V> Drop for SpillMap<K, V> {
    fn drop(&mut self) {
        if self.len > 0 {
            let _ = self.store.with(|db| {
                db.execute("DELETE FROM records WHERE collection=?1", [self.collection])
                    .map_err(failure)?;
                Ok(())
            });
        }
    }
}
pub struct SpillIter<'a, K, V> {
    map: &'a SpillMap<K, V>,
    after: Option<K>,
    page: VecDeque<(K, V)>,
    done: bool,
    remaining: usize,
}
impl<K: SpillKey, V: Serialize + DeserializeOwned> Iterator for SpillIter<'_, K, V> {
    type Item = Result<(K, V)>;
    fn next(&mut self) -> Option<Self::Item> {
        if self.page.is_empty() {
            if self.done {
                return None;
            }
            match self.map.page(&[], self.after.as_ref(), 128) {
                Ok(page) => {
                    if page.is_empty() {
                        self.done = true;
                        return if self.remaining == 0 {
                            None
                        } else {
                            Some(Err(self.map.store.abort("scratch iteration lost rows")))
                        };
                    }
                    self.page = page.into();
                }
                Err(error) => {
                    self.done = true;
                    return Some(Err(error));
                }
            }
        }
        let (key, value) = self.page.pop_front()?;
        let Some(remaining) = self.remaining.checked_sub(1) else {
            self.done = true;
            return Some(Err(self.map.store.abort("scratch iteration gained rows")));
        };
        self.remaining = remaining;
        match K::from_key(&key.key()) {
            Ok(after) => self.after = Some(after),
            Err(error) => {
                self.done = true;
                return Some(Err(error));
            }
        }
        Some(Ok((key, value)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn read_only_empty_maps_do_not_create_scratch_and_keys_keep_their_order() {
        fn thread_safe<T: Send + Sync>() {}
        thread_safe::<SpillStore>();
        thread_safe::<SpillMap<u64, u64>>();
        let store = SpillStore::default();
        let mut map = store.map::<u64, u64>().unwrap();
        assert_eq!(store.database_bytes().unwrap(), 0);
        assert_eq!(map.get(&7).unwrap(), None);
        assert!(!store.initialized().unwrap());
        for key in [u64::MAX, 256, 0, 65536, 255, 1] {
            map.insert(key, key).unwrap();
        }
        let rows = map.iter().collect::<Result<Vec<_>>>().unwrap();
        assert_eq!(
            rows.iter().map(|(key, _)| *key).collect::<Vec<_>>(),
            [0, 1, 255, 256, 65536, u64::MAX]
        );
        assert_eq!(map.pop_last().unwrap(), Some((u64::MAX, u64::MAX)));
        assert_eq!(map.len(), 5);
        map.clear().unwrap();
        assert!(map.iter().next().is_none());
    }
    #[test]
    fn maps_page_large_values_and_remove_exact_prefixes_without_losing_rows() {
        let store = SpillStore::default();
        let mut map = store.map::<Vec<u8>, Vec<u8>>().unwrap();
        for prefix in [1, 2] {
            for i in 0..300u16 {
                map.insert(
                    [vec![prefix], i.to_be_bytes().to_vec()].concat(),
                    vec![42; 4096],
                )
                .unwrap();
            }
        }
        assert!(store.database_bytes().unwrap() > 1024 * 1024);
        let first = map.page(&[1], None, 128).unwrap();
        assert!(
            first.len() < 128,
            "page byte budget must win over row count"
        );
        assert_eq!(map.iter().collect::<Result<Vec<_>>>().unwrap().len(), 600);
        map.remove_prefix(&[1]).unwrap();
        assert_eq!(map.len(), 300);
        assert!(map.page(&[1], None, 128).unwrap().is_empty());
        assert_eq!(map.page(&[2], None, 128).unwrap()[0].0, [2, 0, 0]);
        map.remove_prefix(&[]).unwrap();
        assert_eq!(map.len(), 0);
    }
    #[test]
    fn disk_full_is_sticky_and_never_falls_back_to_ram() {
        let store = SpillStore::new(SpillLimits {
            database_bytes: 64 * 1024,
            cache_kib: 4,
        });
        assert!(matches!(
            store.stage(vec![1; 256 * 1024], false),
            Err(Error::Scratch(_))
        ));
        assert!(matches!(
            store.stage(vec![2], false),
            Err(Error::Scratch(_))
        ));
        let path = store
            .state
            .lock()
            .unwrap()
            .database
            .as_ref()
            .unwrap()
            ._directory
            .path()
            .to_owned();
        drop(store);
        assert!(!path.exists());
    }
    #[test]
    fn staged_reads_enforce_limits_and_verify_the_content_hash() {
        let store = SpillStore::default();
        let id = store.stage(vec![4; 8192], false).unwrap();
        assert!(matches!(
            store.object(&id, 4096),
            Err(Error::ObjectTooLarge { .. })
        ));
        assert_eq!(store.object(&id, 8192).unwrap().unwrap().len(), 8192);
        store
            .with(|db| {
                db.execute(
                    "UPDATE objects SET data=?1 WHERE id=?2",
                    params![vec![9u8; 8192], id.key()],
                )
                .map_err(failure)?;
                Ok(())
            })
            .unwrap();
        assert!(matches!(store.object(&id, 8192), Err(Error::Scratch(_))));
        assert!(store.stage(vec![1], false).is_err());
    }

    #[test]
    fn missing_journal_rows_abort_instead_of_truncating_iteration() {
        let store = SpillStore::default();
        let mut map = store.map::<u64, u64>().unwrap();
        map.insert(1, 2).unwrap();
        store
            .with(|db| {
                db.execute("DELETE FROM records WHERE collection=?1", [map.collection])
                    .map_err(failure)?;
                Ok(())
            })
            .unwrap();
        assert!(matches!(map.iter().next(), Some(Err(Error::Scratch(_)))));
        assert!(store.stage(vec![1], false).is_err());
    }
}
