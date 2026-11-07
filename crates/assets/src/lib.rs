//! Content-addressed part geometry from the synthesized part library, plus
//! ground imagery. No physics engine or renderer runs here.
pub mod ground;
pub mod library;
mod simple_map;
use anyhow::{Context, Result, ensure};
use ergo_world::{geometry::Mesh, part_library::LibraryPart};
use object_store::{ObjectStore, path::Path};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, VecDeque},
    sync::Arc,
};

pub use ergo_world::geometry::GeometryCredit as Credit;

/// One resolved library part: its verified mesh and catalogue entry.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PartAsset {
    pub mesh: Mesh,
    pub credits: Vec<Credit>,
    pub cache_hit: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub warning: Option<String>,
    pub entry: LibraryPart,
}

/// Reads only the immutable `objects/<mesh_hash>.cbor` named by the installed
/// library. Geometry is produced exclusively by the library synthesis.
#[derive(Clone)]
pub struct PartCache {
    store: Arc<dyn ObjectStore>,
    prefix: String,
    resolved: Arc<std::sync::Mutex<ResolvedCache>>,
    /// MCP mirrors: fetch missing immutable objects from a hosted Ergo origin.
    origin: Option<(reqwest::Client, String)>,
}

/// Bounded LRU of decoded meshes keyed by immutable mesh hash.
#[derive(Default)]
struct ResolvedCache {
    values: BTreeMap<String, (Arc<Mesh>, usize)>,
    order: VecDeque<String>,
    bytes: usize,
}
const RESOLVED_BUDGET: usize = 64 * 1024 * 1024;
impl ResolvedCache {
    fn get(&mut self, hash: &str) -> Option<Arc<Mesh>> {
        let mesh = self.values.get(hash)?.0.clone();
        self.order.retain(|key| key != hash);
        self.order.push_back(hash.into());
        Some(mesh)
    }
    fn insert(&mut self, hash: &str, mesh: Arc<Mesh>, bytes: usize) {
        if bytes > RESOLVED_BUDGET || self.values.contains_key(hash) {
            return;
        }
        while self.bytes + bytes > RESOLVED_BUDGET {
            let Some(old) = self.order.pop_front() else {
                break;
            };
            if let Some((_, size)) = self.values.remove(&old) {
                self.bytes -= size;
            }
        }
        self.bytes += bytes;
        self.order.push_back(hash.into());
        self.values.insert(hash.into(), (mesh, bytes));
    }
}

impl PartCache {
    pub fn new(store: Arc<dyn ObjectStore>, prefix: String) -> Result<Self> {
        Ok(Self {
            store,
            prefix,
            resolved: Default::default(),
            origin: None,
        })
    }
    /// Mirror missing objects from `origin`'s `/api/part-objects/{hash}`.
    pub fn with_origin(mut self, http: reqwest::Client, origin: &str) -> Self {
        self.origin = Some((http, origin.trim_end_matches('/').to_owned()));
        self
    }
    /// Verified CBOR bytes of one library geometry object.
    pub async fn object(&self, hash: &str) -> Result<Vec<u8>> {
        let entry = ergo_world::part_library::by_mesh_hash(hash)
            .context("Not a geometry object of the installed part library")?;
        let location = self.path(&library::object_key(hash));
        let bytes = match self.store.get(&location).await {
            Ok(result) => result.bytes().await?.to_vec(),
            Err(object_store::Error::NotFound { .. }) => {
                let Some((http, origin)) = &self.origin else {
                    anyhow::bail!(
                        "Geometry object for {} is missing; run the part-library synthesis",
                        entry.catalogue.id
                    );
                };
                let bytes = http
                    .get(format!("{origin}/api/part-objects/{hash}"))
                    .send()
                    .await?
                    .error_for_status()?
                    .bytes()
                    .await?
                    .to_vec();
                ensure!(
                    bytes.len() <= 64 * 1024 * 1024 && hex::encode(Sha256::digest(&bytes)) == *hash,
                    "Mirrored geometry object checksum mismatch for {}",
                    entry.catalogue.id
                );
                self.store.put(&location, bytes.clone().into()).await?;
                bytes
            }
            Err(error) => return Err(error.into()),
        };
        ensure!(
            hex::encode(Sha256::digest(&bytes)) == *hash,
            "Geometry object checksum mismatch for {}",
            entry.catalogue.id
        );
        Ok(bytes)
    }
    pub fn store(&self) -> &Arc<dyn ObjectStore> {
        &self.store
    }
    pub fn prefix(&self) -> &str {
        &self.prefix
    }
    fn path(&self, key: &str) -> Path {
        Path::from(if self.prefix.is_empty() {
            key.into()
        } else {
            format!("{}/{key}", self.prefix)
        })
    }
    /// Resolve a part or alias of the installed library to verified geometry.
    pub async fn resolve(&self, name: &str) -> Result<PartAsset> {
        let name = ergo_world::ldraw::normalize_name(name)?;
        let entry = ergo_world::part_library::get(&name)
            .with_context(|| format!("Part {name} is not in the Ergo part library"))?;
        let hash = &entry.mesh_hash;
        let cached = self.resolved.lock().unwrap().get(hash);
        let cache_hit = cached.is_some();
        let mesh = match cached {
            Some(mesh) => mesh,
            None => {
                let bytes = self.object(hash).await?;
                let mesh: Mesh = ciborium::from_reader(bytes.as_slice())?;
                let mut vertices = Sha256::new();
                for value in &mesh.vertices {
                    vertices.update(ergo_world::geometry::vertex_bytes(*value));
                }
                ensure!(
                    mesh.schema == ergo_world::SCHEMA_VERSION
                        && mesh.part == entry.catalogue.id
                        && mesh.quality == "source"
                        && hex::encode(vertices.finalize()) == entry.vertex_hash,
                    "Geometry object identity mismatch for {}",
                    entry.catalogue.id
                );
                let mesh = Arc::new(mesh);
                self.resolved
                    .lock()
                    .unwrap()
                    .insert(hash, mesh.clone(), bytes.len() * 4);
                mesh
            }
        };
        let mesh = Mesh::clone(&mesh);
        Ok(PartAsset {
            credits: mesh.credits.clone(),
            mesh,
            cache_hit,
            warning: None,
            entry: entry.clone(),
        })
    }
}

/// Test support: an in-memory store holding the committed fixture library's mesh
/// objects (reviewed tier plus derived samples), matching `test-library`.
#[cfg(any(test, feature = "test-library"))]
pub fn fixture_store() -> Arc<dyn ObjectStore> {
    use std::io::Read;
    let mut cbor = Vec::new();
    flate2::read::GzDecoder::new(&include_bytes!("../tests/fixtures/library-objects.cbor.gz")[..])
        .read_to_end(&mut cbor)
        .expect("Reviewed object fixture");
    let objects: BTreeMap<String, serde_bytes::ByteBuf> =
        ciborium::from_reader(cbor.as_slice()).expect("Reviewed object fixture");
    let store = object_store::memory::InMemory::new();
    futures::executor::block_on(async {
        for (hash, bytes) in objects {
            store
                .put(
                    &Path::from(library::object_key(&hash)),
                    bytes.into_vec().into(),
                )
                .await
                .expect("Seed reviewed object");
        }
    });
    Arc::new(store)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn cache() -> (Arc<dyn ObjectStore>, PartCache) {
        let store: Arc<dyn ObjectStore> = Arc::new(object_store::memory::InMemory::new());
        (store.clone(), PartCache::new(store, String::new()).unwrap())
    }
    #[tokio::test]
    async fn only_library_parts_with_their_exact_objects_resolve() {
        let (store, cache) = cache();
        let error = cache.resolve("invented-part.dat").await.unwrap_err();
        assert!(error.to_string().contains("not in the Ergo part library"));
        let error = cache.resolve("3001.dat").await.unwrap_err();
        assert!(error.to_string().contains("missing"), "{error:#}");
        // A corrupt object under the expected name is rejected.
        let entry = ergo_world::part_library::get("3001.dat").unwrap();
        store
            .put(
                &Path::from(library::object_key(&entry.mesh_hash)),
                b"not cbor".to_vec().into(),
            )
            .await
            .unwrap();
        let error = cache.resolve("3001.dat").await.unwrap_err();
        assert!(error.to_string().contains("checksum"), "{error:#}");
    }
    #[tokio::test]
    async fn every_reviewed_part_and_alias_resolves_from_the_fixture_objects() {
        let cache = PartCache::new(fixture_store(), String::new()).unwrap();
        for entry in ergo_world::part_library::reviewed_parts() {
            let asset = cache.resolve(&entry.catalogue.id).await.unwrap();
            assert_eq!(asset.mesh.part, entry.catalogue.id);
            assert!(!asset.credits.is_empty());
            assert!(asset.warning.is_none());
            for alias in &entry.catalogue.aliases {
                assert_eq!(
                    cache.resolve(alias).await.unwrap().entry.catalogue.id,
                    entry.catalogue.id
                );
            }
        }
        cache.resolve("3001.dat").await.unwrap();
        assert!(cache.resolve("3001.dat").await.unwrap().cache_hit);
    }
}
