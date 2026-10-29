//! Small, revision-keyed discovery responses; authorization always precedes hits.
use super::*;
use bytes::Bytes;
use std::collections::VecDeque;

const MAX_ENTRY: usize = 512 * 1024;
const MAX_STORED: usize = MAX_ENTRY * 2 + 4096;
#[derive(Serialize, Deserialize)]
struct StoredMetadata {
    format: u32,
    etag: String,
    revision: ContentId,
    checksum: ContentId,
    text: String,
}

fn slot(store: &Store, world: &str, etag: &str) -> object_store::path::Path {
    let hash = ContentId::of(etag.as_bytes()).to_string();
    // Sixteen bounded discardable slots per world, not one permanent object
    // per historical revision/query. A collision is only a cache miss.
    let bucket = u8::from_str_radix(&hash[..2], 16).unwrap() % 16;
    store.path(&format!("read-cache/metadata-v1/{world}/{bucket:02}.json"))
}

pub(super) async fn load(
    store: &Store,
    world: &str,
    revision: &ContentId,
    etag: &str,
) -> Option<Bytes> {
    tokio::time::timeout(std::time::Duration::from_millis(750), async {
        let response = store.backend.get(&slot(store, world, etag)).await.ok()?;
        if response.meta.size > MAX_STORED as u64 {
            return None;
        }
        let stored: StoredMetadata = serde_json::from_slice(&response.bytes().await.ok()?).ok()?;
        if stored.format != 1
            || stored.etag != etag
            || stored.revision != *revision
            || stored.text.len() > MAX_ENTRY
            || ContentId::of(stored.text.as_bytes()) != stored.checksum
        {
            return None;
        }
        let body: serde_json::Value = serde_json::from_str(&stored.text).ok()?;
        if body["revision"].as_str() != Some(revision.to_string().as_str()) {
            return None;
        }
        Some(Bytes::from(stored.text))
    })
    .await
    .ok()
    .flatten()
}

pub(super) async fn persist(
    store: &Store,
    world: &str,
    revision: &ContentId,
    etag: &str,
    bytes: &Bytes,
) {
    if bytes.len() > MAX_ENTRY {
        return;
    }
    let Ok(text) = std::str::from_utf8(bytes) else {
        return;
    };
    let record = StoredMetadata {
        format: 1,
        etag: etag.into(),
        revision: revision.clone(),
        checksum: ContentId::of(bytes),
        text: text.into(),
    };
    let Ok(encoded) = serde_json::to_vec(&record) else {
        return;
    };
    if encoded.len() > MAX_STORED {
        return;
    }
    // This is a discardable projection, never an accepted world commit. An
    // older concurrent slot write cannot pass another revision's ETag check.
    let _ = tokio::time::timeout(
        std::time::Duration::from_secs(1),
        store.backend.put(&slot(store, world, etag), encoded.into()),
    )
    .await;
}

#[derive(Default)]
pub(super) struct MetadataCache {
    entries: VecDeque<(String, Bytes)>,
    bytes: usize,
}
impl MetadataCache {
    pub fn get(&mut self, key: &str) -> Option<Bytes> {
        let index = self
            .entries
            .iter()
            .position(|(candidate, _)| candidate == key)?;
        let entry = self.entries.remove(index)?;
        let bytes = entry.1.clone();
        self.entries.push_back(entry);
        Some(bytes)
    }
    pub fn insert(&mut self, key: String, bytes: Bytes) {
        if bytes.len() > MAX_ENTRY {
            return;
        }
        if let Some(index) = self
            .entries
            .iter()
            .position(|(candidate, _)| *candidate == key)
        {
            self.bytes -= self.entries.remove(index).unwrap().1.len();
        }
        while self.bytes + bytes.len() > 4 * 1024 * 1024 {
            if let Some((_, old)) = self.entries.pop_front() {
                self.bytes -= old.len();
            } else {
                break;
            }
        }
        self.bytes += bytes.len();
        self.entries.push_back((key, bytes));
    }
}
pub(super) fn unchanged(headers: &HeaderMap, etag: &str) -> bool {
    headers
        .get("if-none-match")
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| {
            value.split(',').any(|value| {
                value.trim() == "*"
                    || value.trim().trim_start_matches("W/") == etag.trim_start_matches("W/")
            })
        })
}
pub(super) fn response(bytes: Option<Bytes>, etag: &str, source: &str) -> Response {
    let mut response = bytes.map_or_else(
        || StatusCode::NOT_MODIFIED.into_response(),
        IntoResponse::into_response,
    );
    response.headers_mut().insert("etag", etag.parse().unwrap());
    response
        .headers_mut()
        .insert("cache-control", "private, no-cache".parse().unwrap());
    response
        .headers_mut()
        .insert("content-type", "application/json".parse().unwrap());
    response
        .headers_mut()
        .insert("x-ergo-metadata-cache", source.parse().unwrap());
    response
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn persistent_metadata_rejects_corruption_revision_mismatch_and_oversize() {
        let store = Store {
            backend: std::sync::Arc::new(object_store::memory::InMemory::new()),
            prefix: String::new(),
            kind: "test".into(),
            measurements: std::sync::Arc::new(Default::default()),
            static_tolerances: Default::default(),
        };
        let revision = ContentId::of(b"accepted revision");
        let etag = "W/\"catalog-test\"";
        let bytes = Bytes::from(
            serde_json::to_vec(&serde_json::json!({"revision":revision,"items":[],"next":null}))
                .unwrap(),
        );
        persist(&store, "world", &revision, etag, &bytes).await;
        assert_eq!(load(&store, "world", &revision, etag).await.unwrap(), bytes);
        assert!(
            load(&store, "world", &ContentId::of(b"other revision"), etag)
                .await
                .is_none()
        );
        let object = store
            .backend
            .get(&slot(&store, "world", etag))
            .await
            .unwrap()
            .bytes()
            .await
            .unwrap();
        let mut record: StoredMetadata = serde_json::from_slice(&object).unwrap();
        record.text.push(' ');
        store
            .backend
            .put(
                &slot(&store, "world", etag),
                serde_json::to_vec(&record).unwrap().into(),
            )
            .await
            .unwrap();
        assert!(load(&store, "world", &revision, etag).await.is_none());
        store
            .backend
            .put(&slot(&store, "world", etag), vec![0; MAX_STORED + 1].into())
            .await
            .unwrap();
        assert!(load(&store, "world", &revision, etag).await.is_none());
    }
}
