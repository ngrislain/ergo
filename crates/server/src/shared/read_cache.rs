//! Discardable compressed read packets. Authorization and accepted snapshot
//! selection always happen before lookup; commits never consume this cache.
use super::{Store, view::ViewPacket};
use anyhow::{Context, Result};
use bytes::Bytes;
use ergo_transactions::{ContentId, canonical_cbor};
use flate2::{Compression, GzBuilder, read::GzDecoder};
use object_store::{PutMode, UpdateVersion};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, HashMap},
    io::{Read, Write},
    sync::{Arc, Mutex, Weak},
};

const FORMAT: u32 = 1;
const MEMORY_BYTES: usize = 64 * 1024 * 1024;
const MAX_PACKET_BYTES: usize = 64 * 1024 * 1024;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub(super) struct ViewKey {
    pub world: String,
    pub revision: ContentId,
    pub coord: [i64; 3],
    pub pieces: usize,
    pub read_bytes: usize,
}

#[cfg(test)]
mod tests {
    use super::*;
    fn fixture() -> (Store, ViewKey, Arc<EncodedView>) {
        let store = Store {
            backend: Arc::new(object_store::memory::InMemory::new()),
            prefix: String::new(),
            kind: "test".into(),
            measurements: Arc::new(Default::default()),
            static_tolerances: Default::default(),
        };
        let mut demo = ergo_world::demo::Demo::new();
        demo.add("3003", 4, 0., 0., 0.);
        let scene = ergo_world::WorldExport {
            schema: ergo_world::SCHEMA_VERSION,
            name: "Cached scene".into(),
            geo: Default::default(),
            bricks: demo.bricks,
            meshes: demo.meshes,
            joints: vec![],
            fixed_bricks: vec![],
            groups: vec![],
            model: None,
            ground: vec![],
        };
        let key = ViewKey {
            world: "test-world".into(),
            revision: ContentId::of(b"accepted"),
            coord: [0; 3],
            pieces: 5000,
            read_bytes: 16 * 1024 * 1024,
        };
        let packet:ViewPacket=serde_json::from_value(serde_json::json!({"scene":scene,"revision":key.revision,"blocks":[],"assemblies":{},"view_kind":"bounded_read_only_region"})).unwrap();
        let encoded = Arc::new(EncodedView::encode(&packet, &key, 1024).unwrap());
        (store, key, encoded)
    }
    #[tokio::test]
    async fn persistent_packets_survive_restart_but_stale_or_corrupt_packets_are_misses() {
        let (store, key, value) = fixture();
        let first = ReadCache::default();
        let (missing, mode) = first.persistent(&store, &key).await;
        assert!(missing.is_none());
        ReadCache::persist(store.clone(), key.clone(), value.clone(), mode)
            .unwrap()
            .await
            .unwrap();
        let restarted = ReadCache::default();
        let (hit, _) = restarted.persistent(&store, &key).await;
        assert_eq!(hit.unwrap().cbor, value.cbor);
        let mut newer = key.clone();
        newer.revision = ContentId::of(b"new accepted");
        assert!(restarted.persistent(&store, &newer).await.0.is_none());
        store
            .backend
            .put(
                &key.location(&store).unwrap(),
                Bytes::from_static(b"corrupt packet").into(),
            )
            .await
            .unwrap();
        assert!(restarted.persistent(&store, &key).await.0.is_none());
    }
    #[test]
    fn memory_cache_is_byte_bounded_and_uses_lru_eviction() {
        let cache = ReadCache::default();
        let bytes = Bytes::from(vec![0; 20 * 1024 * 1024]);
        let make = || {
            Arc::new(EncodedView {
                cbor: bytes.clone(),
                gzip_cbor: bytes.clone(),
                blocks: Vec::new(),
                logical_read_bytes: 0,
                brick_count: 1,
            })
        };
        cache.insert("first".into(), make());
        assert!(cache.get("first").is_some());
        cache.insert("second".into(), make());
        assert!(cache.get("first").is_none());
        assert!(cache.get("second").is_some());
        assert!(cache.memory.lock().unwrap().bytes <= MEMORY_BYTES);
    }
    #[test]
    fn progress_lives_only_for_the_active_builder() {
        let cache = ReadCache::default();
        assert!(cache.progress("view").is_none());
        {
            let active = cache.start_progress("view");
            active.report("piece records: 1024/5000");
            assert_eq!(
                cache.progress("view").as_deref(),
                Some("piece records: 1024/5000")
            );
        }
        assert!(cache.progress("view").is_none());
    }
}
impl ViewKey {
    pub fn memory_key(&self) -> Result<String> {
        Ok(ContentId::of(&canonical_cbor(&(FORMAT, self))?).to_string())
    }
    fn location(&self, store: &Store) -> Result<object_store::path::Path> {
        // One latest packet per occupied region/configuration, not one large
        // persistent object for every historical edit. Revision is checked inside.
        let id = ContentId::of(&canonical_cbor(&(
            FORMAT,
            &self.world,
            self.coord,
            self.pieces,
            self.read_bytes,
        ))?);
        Ok(store.path(&format!("cache/read-views/v1/{id}.cbor")))
    }
}

pub(super) struct EncodedView {
    pub cbor: Bytes,
    pub gzip_cbor: Bytes,
    pub logical_read_bytes: usize,
    pub brick_count: usize,
    pub blocks: Vec<ergo_world::BlockCoord>,
}
impl EncodedView {
    pub fn encode(packet: &ViewPacket, key: &ViewKey, logical_read_bytes: usize) -> Result<Self> {
        Self::check(packet, key, logical_read_bytes)?;
        let mut cbor = Vec::new();
        ciborium::into_writer(packet, &mut cbor)?;
        ensure!(cbor.len() <= MAX_PACKET_BYTES, "Read packet too large");
        let gzip_cbor = Self::gzip(&cbor)?;
        Ok(Self {
            cbor: cbor.into(),
            gzip_cbor,
            logical_read_bytes,
            brick_count: packet.scene.bricks.len(),
            blocks: packet.blocks.clone(),
        })
    }
    fn check(packet: &ViewPacket, key: &ViewKey, logical_read_bytes: usize) -> Result<()> {
        ensure!(
            packet.revision == key.revision && packet.scene.bricks.len() <= key.pieces,
            "Invalid cached region"
        );
        ensure!(
            packet.scene.schema == ergo_world::SCHEMA_VERSION
                && packet.view_kind == "bounded_read_only_region",
            "Unsupported cached view schema"
        );
        ensure!(
            logical_read_bytes <= key.read_bytes,
            "Cached region exceeds read budget"
        );
        Ok(())
    }
    pub fn gzip(data: &[u8]) -> Result<Bytes> {
        let mut encoder = GzBuilder::new()
            .mtime(0)
            .write(Vec::new(), Compression::default());
        encoder.write_all(data)?;
        Ok(encoder.finish()?.into())
    }
    fn size(&self) -> usize {
        self.cbor.len() + self.gzip_cbor.len()
    }
}

#[derive(Serialize, Deserialize)]
struct StoredView {
    format: u32,
    key: ViewKey,
    logical_read_bytes: usize,
    decoded_bytes: usize,
    checksum: ContentId,
    #[serde(with = "serde_bytes")]
    gzip: Vec<u8>,
}
#[derive(Default)]
struct Memory {
    values: HashMap<String, (Arc<EncodedView>, u64)>,
    order: BTreeMap<u64, String>,
    clock: u64,
    bytes: usize,
}
#[derive(Default)]
pub(super) struct ReadCache {
    memory: Mutex<Memory>,
    gates: Mutex<HashMap<String, Weak<tokio::sync::Mutex<()>>>>,
    progress: Mutex<HashMap<String, ProgressState>>,
}
#[derive(Clone, Serialize)]
pub(super) struct ReadWork {
    stage: &'static str,
    completed: usize,
    total: usize,
    found_pieces: Option<usize>,
}
#[derive(Clone)]
struct ProgressState {
    phase: String,
    work: Option<ReadWork>,
}
pub(super) struct ReadProgress<'a> {
    cache: &'a ReadCache,
    key: String,
    timer: Mutex<StageTimer>,
}
struct StageTimer {
    stage: &'static str,
    since: std::time::Instant,
    totals: BTreeMap<&'static str, f64>,
}
impl ReadProgress<'_> {
    pub fn report(&self, phase: impl Into<String>) {
        self.set(phase.into(), None);
    }
    fn set(&self, phase: String, work: Option<ReadWork>) {
        let stage = work.as_ref().map(|work| work.stage).unwrap_or_else(|| {
            if phase.contains("cache lookup") {
                "cache"
            } else if phase.starts_with("encoding") {
                "encoding"
            } else if phase.starts_with("persisting") {
                "persisting"
            } else if phase.starts_with("verifying") {
                "cache_verify"
            } else {
                "preparing"
            }
        });
        let mut timer = self.timer.lock().unwrap();
        if timer.stage != stage {
            let previous = timer.stage;
            let elapsed = timer.since.elapsed().as_secs_f64() * 1000.;
            *timer.totals.entry(previous).or_default() += elapsed;
            timer.stage = stage;
            timer.since = std::time::Instant::now();
        }
        drop(timer);
        self.cache
            .progress
            .lock()
            .unwrap()
            .insert(self.key.clone(), ProgressState { phase, work });
    }
    pub fn scanning(&self, completed: usize, found: usize, coord: Option<[i64; 3]>) {
        let phase = coord
            .map(|coord| format!("spatial index: block {coord:?}, {found} unique pieces"))
            .unwrap_or_else(|| format!("spatial references: {found} unique pieces"));
        self.set(
            phase,
            Some(ReadWork {
                stage: "discovering",
                completed,
                total: 27,
                found_pieces: Some(found),
            }),
        );
    }
    pub fn pieces(&self, completed: usize, total: usize, meshes: usize) {
        self.set(
            format!("piece records: {completed}/{total}, {meshes} meshes"),
            Some(ReadWork {
                stage: "pieces",
                completed,
                total,
                found_pieces: None,
            }),
        );
    }
    pub fn membership(&self, completed: usize, total: usize) {
        self.set(
            format!("checking complete visual membership: {completed}/{total}"),
            Some(ReadWork {
                stage: "membership",
                completed,
                total,
                found_pieces: None,
            }),
        );
    }
    pub fn timings_ms(&self) -> BTreeMap<&'static str, f64> {
        let timer = self.timer.lock().unwrap();
        let mut totals = timer.totals.clone();
        *totals.entry(timer.stage).or_default() += timer.since.elapsed().as_secs_f64() * 1000.;
        totals
    }
}
impl Drop for ReadProgress<'_> {
    fn drop(&mut self) {
        self.cache.progress.lock().unwrap().remove(&self.key);
    }
}
impl ReadCache {
    pub fn progress(&self, key: &str) -> Option<String> {
        self.progress
            .lock()
            .unwrap()
            .get(key)
            .map(|state| state.phase.clone())
    }
    pub fn progress_status(&self, key: &str) -> Option<(String, Option<ReadWork>)> {
        self.progress
            .lock()
            .unwrap()
            .get(key)
            .map(|state| (state.phase.clone(), state.work.clone()))
    }
    pub fn start_progress(&self, key: &str) -> ReadProgress<'_> {
        let progress = ReadProgress {
            cache: self,
            key: key.into(),
            timer: Mutex::new(StageTimer {
                stage: "preparing",
                since: std::time::Instant::now(),
                totals: BTreeMap::new(),
            }),
        };
        progress.report("starting");
        progress
    }
    pub fn get(&self, key: &str) -> Option<Arc<EncodedView>> {
        let mut memory = self.memory.lock().unwrap();
        let (value, old) = memory.values.get(key)?.clone();
        memory.order.remove(&old);
        memory.clock += 1;
        let stamp = memory.clock;
        memory.values.insert(key.into(), (value.clone(), stamp));
        memory.order.insert(stamp, key.into());
        Some(value)
    }
    pub fn insert(&self, key: String, value: Arc<EncodedView>) {
        let size = value.size();
        if size > MEMORY_BYTES {
            return;
        }
        let mut memory = self.memory.lock().unwrap();
        if memory.values.contains_key(&key) {
            return;
        }
        while memory.bytes + size > MEMORY_BYTES {
            let Some((_, old)) = memory.order.pop_first() else {
                break;
            };
            if let Some((value, _)) = memory.values.remove(&old) {
                memory.bytes -= value.size();
            }
        }
        memory.clock += 1;
        let stamp = memory.clock;
        memory.bytes += size;
        memory.values.insert(key.clone(), (value, stamp));
        memory.order.insert(stamp, key);
    }
    pub fn gate(&self, key: &str) -> Arc<tokio::sync::Mutex<()>> {
        let mut gates = self.gates.lock().unwrap();
        if let Some(gate) = gates.get(key).and_then(Weak::upgrade) {
            return gate;
        }
        if gates.len() >= 128 {
            gates.retain(|_, gate| gate.strong_count() > 0);
        }
        let gate = Arc::new(tokio::sync::Mutex::new(()));
        gates.insert(key.into(), Arc::downgrade(&gate));
        gate
    }
    pub async fn persistent(
        &self,
        store: &Store,
        key: &ViewKey,
    ) -> (Option<Arc<EncodedView>>, Option<PutMode>) {
        let Ok(location) = key.location(store) else {
            return (None, None);
        };
        let result = match store.backend.get(&location).await {
            Ok(result) => result,
            Err(object_store::Error::NotFound { .. }) => return (None, Some(PutMode::Create)),
            Err(_) => return (None, None),
        };
        let version = UpdateVersion {
            e_tag: result.meta.e_tag.clone(),
            version: result.meta.version.clone(),
        };
        let mode = if version.e_tag.is_some() || version.version.is_some() {
            Some(PutMode::Update(version))
        } else {
            None
        };
        if result.meta.size > MAX_PACKET_BYTES as u64 {
            return (None, mode);
        }
        let Ok(bytes) = result.bytes().await else {
            return (None, mode);
        };
        let key = key.clone();
        let decoded = tokio::task::spawn_blocking(move || -> Result<Arc<EncodedView>> {
            let stored: StoredView =
                ciborium::from_reader(bytes.as_ref()).context("Invalid read cache")?;
            ensure!(
                stored.format == FORMAT && stored.key == key,
                "Stale read cache"
            );
            ensure!(
                stored.decoded_bytes <= MAX_PACKET_BYTES
                    && stored.logical_read_bytes <= key.read_bytes,
                "Oversized read cache"
            );
            ensure!(
                ContentId::of(&stored.gzip) == stored.checksum,
                "Corrupt read cache"
            );
            let mut raw = Vec::new();
            GzDecoder::new(stored.gzip.as_slice())
                .take(MAX_PACKET_BYTES as u64 + 1)
                .read_to_end(&mut raw)?;
            ensure!(
                raw.len() == stored.decoded_bytes,
                "Invalid read cache length"
            );
            let packet: ViewPacket = ciborium::from_reader(raw.as_slice())?;
            EncodedView::check(&packet, &key, stored.logical_read_bytes)?;
            Ok(Arc::new(EncodedView {
                cbor: raw.into(),
                gzip_cbor: stored.gzip.into(),
                logical_read_bytes: stored.logical_read_bytes,
                brick_count: packet.scene.bricks.len(),
                blocks: packet.blocks.clone(),
            }))
        })
        .await;
        (decoded.ok().and_then(Result::ok), mode)
    }
    pub fn persist(
        store: Store,
        key: ViewKey,
        value: Arc<EncodedView>,
        mode: Option<PutMode>,
    ) -> Option<tokio::task::JoinHandle<()>> {
        if value.brick_count == 0 {
            return None;
        }
        let mode = mode?;
        Some(tokio::spawn(async move {
            let result = async {
                let record = StoredView {
                    format: FORMAT,
                    key: key.clone(),
                    logical_read_bytes: value.logical_read_bytes,
                    decoded_bytes: value.cbor.len(),
                    checksum: ContentId::of(&value.gzip_cbor),
                    gzip: value.gzip_cbor.to_vec(),
                };
                let bytes = canonical_cbor(&record)?;
                store
                    .backend
                    .put_opts(&key.location(&store)?, bytes.into(), mode.into())
                    .await?;
                Ok::<_, anyhow::Error>(())
            }
            .await;
            if result.is_err() {
                tracing::debug!(
                    "Derived view cache write skipped; authoritative data is unchanged"
                );
            }
        }))
    }
}
