//! Disposable disk index for complete packed cells. Never used to validate writes.
use super::*;
use ergo_world::Aabb;
use std::{
    fs::File,
    io::{Read, Seek, Write},
    path::PathBuf,
    sync::{Arc, Mutex},
};

const MAX_BYTES: u64 = 256 * 1024 * 1024;
const MAX_ENTRY: usize = 2 * 1024 * 1024;
const ENTRY_OVERHEAD: u64 = 8192;
const FREE_RESERVE: u64 = 1024 * 1024 * 1024;

#[derive(Serialize, Deserialize)]
pub(super) struct Row {
    pub piece: ContentId,
    pub assembly: ContentId,
    pub body: Aabb,
}

#[derive(Default)]
struct State {
    lru: std::collections::VecDeque<(ContentId, File, u64)>,
    charged: u64,
}

#[derive(Default)]
pub(super) struct Cache {
    directory: Option<PathBuf>,
    limit: u64,
    reserve: u64,
    state: Mutex<State>,
}

impl Cache {
    pub fn production() -> Arc<Self> {
        Arc::new(Self {
            directory: Some(std::env::temp_dir()),
            limit: MAX_BYTES,
            reserve: FREE_RESERVE,
            ..Self::default()
        })
    }

    pub fn enabled(&self) -> bool {
        self.directory.is_some()
    }

    pub async fn get(self: &Arc<Self>, key: ContentId) -> Option<Vec<Row>> {
        let cache = self.clone();
        tokio::task::spawn_blocking(move || {
            let mut state = cache.state.try_lock().ok()?;
            let i = state.lru.iter().position(|(cid, _, _)| cid == &key)?;
            let (key, mut file, charge) = state.lru.remove(i)?;
            state.charged -= charge;
            file.rewind().ok()?;
            let mut bytes = Vec::new();
            (&mut file)
                .take(MAX_ENTRY as u64 + 1)
                .read_to_end(&mut bytes)
                .ok()?;
            if bytes.len() > MAX_ENTRY {
                return None;
            }
            let rows = ciborium::from_reader(bytes.as_slice()).ok()?;
            state.lru.push_back((key, file, charge));
            state.charged += charge;
            Some(rows)
        })
        .await
        .ok()
        .flatten()
    }

    pub async fn put(self: &Arc<Self>, key: ContentId, rows: Vec<Row>) -> Vec<Row> {
        let cache = self.clone();
        // Keep the authoritative S3 result even if the write is skipped.
        let rows = Arc::new(rows);
        let input = rows.clone();
        let _ = tokio::task::spawn_blocking(move || cache.store(key, &input)).await;
        Arc::try_unwrap(rows).unwrap_or_else(|_| unreachable!("cache worker completed"))
    }

    fn store(&self, key: ContentId, rows: &[Row]) {
        let Some(dir) = &self.directory else {
            return;
        };
        let Ok(mut state) = self.state.try_lock() else {
            return;
        };
        if state.lru.iter().any(|(cid, _, _)| cid == &key) {
            return;
        }
        let mut bytes = Vec::new();
        if ciborium::into_writer(rows, &mut bytes).is_err() || bytes.len() > MAX_ENTRY {
            return;
        }
        let charge = (bytes.len() as u64).div_ceil(4096) * 4096 + ENTRY_OVERHEAD;
        if charge > self.limit {
            return;
        }
        while state.charged + charge > self.limit
            || !fs2::available_space(dir).is_ok_and(|free| free >= self.reserve + charge)
            || state.lru.len() >= 512
        {
            let Some((_, file, bytes)) = state.lru.pop_front() else {
                return;
            };
            drop(file);
            state.charged -= bytes;
        }
        // Create only after reserving space, under the same lock. Anonymous
        // temp files disappear on close, eviction, or process exit (even SIGKILL).
        let Ok(mut file) = tempfile::tempfile_in(dir) else {
            return;
        };
        if file.write_all(&bytes).is_err() {
            return;
        }
        state.lru.push_back((key, file, charge));
        state.charged += charge;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn cell_cache_evicts_least_recently_used_and_skips_low_disk() {
        let dir = tempfile::tempdir().unwrap();
        let cache = Arc::new(Cache {
            directory: Some(dir.path().into()),
            limit: 2 * (4096 + ENTRY_OVERHEAD),
            ..Cache::default()
        });
        let [a, b, c] = [b'a', b'b', b'c'].map(|x| ContentId::of(&[x]));
        let row = || {
            vec![Row {
                piece: a.clone(),
                assembly: b.clone(),
                body: Aabb::new(glam::DVec3::ZERO, glam::DVec3::ONE),
            }]
        };
        cache.put(a.clone(), row()).await;
        cache.put(b.clone(), row()).await;
        assert!(cache.get(a.clone()).await.is_some());
        cache.put(c.clone(), row()).await;
        assert!(cache.get(b.clone()).await.is_none());
        assert_eq!(cache.get(c).await.unwrap()[0].piece, a);
        assert!(cache.state.lock().unwrap().charged <= cache.limit);
        let blocked = Arc::new(Cache {
            directory: Some(dir.path().into()),
            limit: MAX_BYTES,
            reserve: u64::MAX - MAX_BYTES,
            ..Cache::default()
        });
        blocked.put(b.clone(), row()).await;
        assert!(blocked.get(b).await.is_none());
    }
}
