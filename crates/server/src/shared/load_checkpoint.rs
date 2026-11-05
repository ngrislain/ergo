//! Bounded, conditional, discardable read-only checkpoints. Never world records.
use super::{Store, read_cache::ViewKey};
use anyhow::{Context, Result};
use ergo_transactions::{ContentId, canonical_cbor};
use flate2::{Compression, GzBuilder, read::GzDecoder};
use object_store::{PutMode, UpdateVersion, path::Path};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use std::{
    io::{Read, Write},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

const FORMAT: u32 = 1;
const SLOTS: usize = 64;
const MAX_STORED: usize = 32 * 1024 * 1024;
const MAX_DECODED: usize = 128 * 1024 * 1024;
pub(super) const RETENTION_SECONDS: u64 = 7 * 86400;
const INTERVAL: Duration = Duration::from_secs(30);
fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs()
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub(super) struct Mark {
    pub steps: u64,
    pub cells: usize,
    pub pieces: usize,
    #[serde(default)]
    pub stage: String,
    #[serde(default)]
    pub rows: usize,
}
#[derive(Clone, Debug, Serialize)]
pub(super) struct Status {
    pub resumed: bool,
    pub attempted: bool,
    pub available: bool,
    pub saved: Mark,
}
#[derive(Serialize, Deserialize)]
struct Record {
    format: u32,
    key: ViewKey,
    updated: u64,
    mark: Mark,
    decoded: usize,
    checksum: ContentId,
    #[serde(with = "serde_bytes")]
    gzip: Vec<u8>,
}
struct Target {
    path: Path,
}
pub(super) struct Journal {
    store: Store,
    key: ViewKey,
    target: Option<Target>,
    last: Instant,
    loaded_checksum: Option<ContentId>,
    invalid_checksum: Option<ContentId>,
    pub status: Status,
}
impl Journal {
    fn paths(store: &Store, key: &ViewKey) -> Result<Vec<Path>> {
        let digest = ContentId::of(&canonical_cbor(&(FORMAT, key))?).to_string();
        let world = ContentId::of(key.world.as_bytes());
        let mut slots = std::collections::BTreeSet::new();
        for index in 0..4 {
            slots.insert(usize::from_str_radix(&digest[index * 2..index * 2 + 2], 16)? % SLOTS);
        }
        Ok(slots
            .into_iter()
            .map(|slot| store.path(&format!("cache/read-loads/cell128-v1/{world}/{slot}.cbor")))
            .collect())
    }
    async fn read(store: &Store, path: &Path) -> Result<(Option<Record>, PutMode)> {
        let result = match store.backend.get(path).await {
            Ok(result) => result,
            Err(object_store::Error::NotFound { .. }) => return Ok((None, PutMode::Create)),
            Err(error) => return Err(error.into()),
        };
        let version = UpdateVersion {
            e_tag: result.meta.e_tag.clone(),
            version: result.meta.version.clone(),
        };
        anyhow::ensure!(
            version.e_tag.is_some() || version.version.is_some(),
            "Checkpoint store needs conditional writes"
        );
        let mode = PutMode::Update(version);
        if result.meta.size > MAX_STORED as u64 {
            return Ok((None, mode));
        }
        let bytes = result.bytes().await?;
        let record = ciborium::from_reader::<Record, _>(bytes.as_ref())
            .ok()
            .filter(|record| {
                record.format == FORMAT
                    && record.updated <= now() + 300
                    && now().saturating_sub(record.updated) < RETENTION_SECONDS
                    && record.decoded <= MAX_DECODED
                    && ContentId::of(&record.gzip) == record.checksum
            });
        Ok((record, mode))
    }
    pub async fn open<T: DeserializeOwned + Send + 'static>(
        store: Store,
        key: ViewKey,
    ) -> (Self, Option<T>) {
        let mut journal = Self {
            store,
            key,
            target: None,
            last: Instant::now(),
            loaded_checksum: None,
            invalid_checksum: None,
            status: Status {
                resumed: false,
                attempted: false,
                available: false,
                saved: Mark::default(),
            },
        };
        let mut restored = None;
        if let Ok(paths) = Self::paths(&journal.store, &journal.key) {
            for path in paths {
                let Ok((record, _mode)) = Self::read(&journal.store, &path).await else {
                    continue;
                };
                match record {
                    Some(record) if record.key == journal.key => {
                        let mark = record.mark.clone();
                        let checksum = record.checksum.clone();
                        let decoded = tokio::task::spawn_blocking(move || -> Result<T> {
                            let mut raw = Vec::new();
                            GzDecoder::new(record.gzip.as_slice())
                                .take(MAX_DECODED as u64 + 1)
                                .read_to_end(&mut raw)?;
                            anyhow::ensure!(
                                raw.len() == record.decoded,
                                "Invalid checkpoint length"
                            );
                            Ok(ciborium::from_reader(raw.as_slice())
                                .context("Invalid checkpoint state")?)
                        })
                        .await;
                        if let Ok(Ok(value)) = decoded {
                            if restored.is_none() || mark.steps > journal.status.saved.steps {
                                restored = Some(value);
                                journal.target = Some(Target { path });
                                journal.loaded_checksum = Some(checksum);
                                journal.status.saved = mark;
                                journal.status.resumed = true;
                                journal.status.attempted = true;
                                journal.status.available = true;
                            }
                        } else if restored.is_none() {
                            journal.target = Some(Target { path });
                            journal.invalid_checksum = Some(checksum);
                        }
                    }
                    None if journal.target.is_none() => journal.target = Some(Target { path }),
                    // Fresh checkpoints belonging to another key are never evicted.
                    _ => {}
                }
            }
        }
        (journal, restored)
    }
    pub fn due(&self) -> bool {
        self.last.elapsed() >= INTERVAL
    }
    pub fn reject_loaded(&mut self) {
        self.invalid_checksum = self.loaded_checksum.take();
        self.status.resumed = false;
        self.status.attempted = false;
        self.status.available = false;
        self.status.saved = Mark::default();
    }
    pub async fn retire_empty(&self) {
        let Some(target) = &self.target else {
            return;
        };
        if let Ok((Some(mut record), mode)) = Self::read(&self.store, &target.path).await
            && record.key == self.key
        {
            record.format = 0;
            record.gzip.clear();
            record.decoded = 0;
            if let Ok(bytes) = canonical_cbor(&record) {
                let _ = self
                    .store
                    .backend
                    .put_opts(&target.path, bytes.into(), mode.into())
                    .await;
            }
        }
    }
    pub async fn save<T: Serialize + Send + 'static>(&mut self, state: T, mark: Mark) -> bool {
        self.status.attempted = true;
        self.last = Instant::now();
        let Some(target) = &self.target else {
            self.status.available = false;
            return false;
        };
        let key = self.key.clone();
        let encoded = tokio::task::spawn_blocking(move || -> Result<Vec<u8>> {
            let mut raw = Vec::new();
            ciborium::into_writer(&state, &mut raw)?;
            anyhow::ensure!(raw.len() <= MAX_DECODED, "Checkpoint state exceeds bound");
            let mut encoder = GzBuilder::new()
                .mtime(0)
                .write(Vec::new(), Compression::fast());
            encoder.write_all(&raw)?;
            let gzip = encoder.finish()?;
            let record = Record {
                format: FORMAT,
                key,
                updated: now(),
                mark,
                decoded: raw.len(),
                checksum: ContentId::of(&gzip),
                gzip,
            };
            let bytes = canonical_cbor(&record)?;
            anyhow::ensure!(
                bytes.len() <= MAX_STORED,
                "Checkpoint exceeds stored byte bound"
            );
            Ok(bytes)
        })
        .await;
        let Ok(Ok(bytes)) = encoded else {
            self.status.available = false;
            return false;
        };
        // Check the slot again: another process may have saved newer progress.
        let path = target.path.clone();
        let Ok((previous, mode)) = Self::read(&self.store, &path).await else {
            self.status.available = false;
            return false;
        };
        if let Some(record) = &previous {
            if record.key != self.key {
                self.status.available = false;
                return false;
            }
            let candidate: Record = ciborium::from_reader(bytes.as_slice()).unwrap();
            if record.mark.steps >= candidate.mark.steps
                && self.invalid_checksum.as_ref() != Some(&record.checksum)
            {
                self.status.saved = record.mark.clone();
                self.status.available = true;
                self.last = Instant::now();
                return true;
            }
        }
        let result = self
            .store
            .backend
            .put_opts(&path, bytes.clone().into(), mode.into())
            .await;
        // A lost acknowledgement is resolved by an exact persisted read, never
        // by replaying an unverified write or treating memory as durability.
        let confirmed = match result {
            Ok(_) => ciborium::from_reader::<Record, _>(bytes.as_slice()).ok(),
            Err(_) => Self::read(&self.store, &path)
                .await
                .ok()
                .and_then(|(record, _)| record)
                .filter(|record| record.key == self.key),
        };
        if let Some(record) = confirmed {
            let candidate: Record = ciborium::from_reader(bytes.as_slice()).unwrap();
            if record.mark.steps >= candidate.mark.steps {
                self.status.saved = record.mark;
                self.status.available = true;
                self.last = Instant::now();
                return true;
            }
        }
        self.status.available = false;
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    fn fixture() -> (Store, ViewKey) {
        (
            Store {
                backend: Arc::new(object_store::memory::InMemory::new()),
                prefix: String::new(),
                kind: "checkpoint test".into(),
                measurements: Arc::new(Default::default()),
                static_tolerances: Default::default(),
            },
            ViewKey {
                world: "test-world".into(),
                revision: ContentId::of(b"accepted"),
                coord: [0; 3],
                radius: 1,
                pieces: 1000,
                read_bytes: 1024 * 1024,
            },
        )
    }
    #[tokio::test]
    async fn durable_restart_retains_the_newest_progress_and_checks_the_exact_key() {
        let (store, key) = fixture();
        let (mut first, empty) = Journal::open::<Vec<u32>>(store.clone(), key.clone()).await;
        assert!(empty.is_none());
        assert!(
            first
                .save(
                    vec![1u32, 2, 3],
                    Mark {
                        steps: 7,
                        pieces: 64,
                        cells: 2,
                        ..Default::default()
                    }
                )
                .await
        );
        let (mut restarted, data) = Journal::open::<Vec<u32>>(store.clone(), key.clone()).await;
        assert_eq!(data, Some(vec![1, 2, 3]));
        assert!(restarted.status.resumed);
        assert!(
            restarted
                .save(
                    vec![9u32],
                    Mark {
                        steps: 3,
                        pieces: 1,
                        cells: 1,
                        ..Default::default()
                    }
                )
                .await
        );
        let (_, data) = Journal::open::<Vec<u32>>(store.clone(), key.clone()).await;
        assert_eq!(
            data,
            Some(vec![1, 2, 3]),
            "A delayed writer cannot roll progress back"
        );
        let mut different = key.clone();
        different.revision = ContentId::of(b"another accepted snapshot");
        assert!(
            Journal::open::<Vec<u32>>(store.clone(), different)
                .await
                .1
                .is_none()
        );
        let mut different = key;
        different.read_bytes -= 1;
        assert!(
            Journal::open::<Vec<u32>>(store, different)
                .await
                .1
                .is_none()
        );
    }
    #[tokio::test]
    async fn corruption_is_a_miss_and_cannot_poison_future_checkpoints() {
        let (store, key) = fixture();
        let (mut first, _) = Journal::open::<Vec<u32>>(store.clone(), key.clone()).await;
        assert!(
            first
                .save(
                    vec![1u32],
                    Mark {
                        steps: 8,
                        ..Default::default()
                    }
                )
                .await
        );
        let path = first.target.as_ref().unwrap().path.clone();
        store
            .backend
            .put(&path, b"corrupt checkpoint".to_vec().into())
            .await
            .unwrap();
        let (mut next, data) = Journal::open::<Vec<u32>>(store.clone(), key.clone()).await;
        assert!(data.is_none());
        assert!(
            next.save(
                vec![2u32],
                Mark {
                    steps: 1,
                    ..Default::default()
                }
            )
            .await
        );
        assert_eq!(
            Journal::open::<Vec<u32>>(store, key).await.1,
            Some(vec![2u32])
        );
    }
    #[tokio::test]
    async fn a_full_probe_set_never_overwrites_another_fresh_load() {
        let (store, key) = fixture();
        let mut encoder = GzBuilder::new()
            .mtime(0)
            .write(Vec::new(), Compression::fast());
        encoder.write_all(&[0x80]).unwrap();
        let gzip = encoder.finish().unwrap();
        let paths = Journal::paths(&store, &key).unwrap();
        for (index, path) in paths.iter().enumerate() {
            let mut foreign = key.clone();
            foreign.coord[0] = index as i64 + 1;
            let record = Record {
                format: FORMAT,
                key: foreign,
                updated: now(),
                mark: Mark {
                    steps: 4,
                    ..Default::default()
                },
                decoded: 1,
                checksum: ContentId::of(&gzip),
                gzip: gzip.clone(),
            };
            store
                .backend
                .put(path, canonical_cbor(&record).unwrap().into())
                .await
                .unwrap();
        }
        let (mut blocked, data) = Journal::open::<Vec<u32>>(store.clone(), key.clone()).await;
        assert!(data.is_none());
        assert!(
            !blocked
                .save(
                    vec![1u32],
                    Mark {
                        steps: 9,
                        ..Default::default()
                    }
                )
                .await
        );
        for path in paths {
            assert_ne!(
                Journal::read(&store, &path).await.unwrap().0.unwrap().key,
                key
            );
        }
    }
}
