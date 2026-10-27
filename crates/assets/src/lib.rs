//! Cache-first, on-demand catalog assets. No physics engine or renderer runs here.
pub mod ground;
use anyhow::{Context, Result, ensure};
use ergo_world::{
    geometry::{Mesh, fallback_mesh},
    ldraw,
};
use futures::StreamExt;
use object_store::{ObjectStore, path::Path};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, HashMap, VecDeque},
    path::PathBuf,
    sync::Arc,
    time::{Duration, Instant},
};

pub use ergo_world::geometry::GeometryCredit as Credit;
pub const PART_FORMAT: &str = "ergo-part-1";
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PartAsset {
    pub mesh: Mesh,
    pub credits: Vec<Credit>,
    pub cache_hit: bool,
    pub warning: Option<String>,
    #[serde(default)]
    pub description: Option<PartDescription>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PartDescription {
    pub format: String,
    schema: u32,
    part: String,
    mesh_hash: String,
    bounds: ergo_world::Aabb,
    body_bounds: ergo_world::Aabb,
    catalog_mass: Option<CatalogMass>,
    credits: Vec<Credit>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
struct CatalogMass {
    kg: f64,
    source: String,
}
impl PartDescription {
    pub fn from_mesh(mesh: &Mesh, mesh_hash: String) -> Self {
        Self {
            format: PART_FORMAT.into(),
            schema: ergo_world::SCHEMA_VERSION,
            part: mesh.part.clone(),
            mesh_hash,
            bounds: mesh.bounds,
            body_bounds: mesh.body_bounds,
            catalog_mass: ergo_world::physical::mass_kg(&mesh.part)
                .ok()
                .map(|kg| CatalogMass {
                    kg,
                    source: ergo_world::physical::mass_source(&mesh.part),
                }),
            credits: mesh.credits.clone(),
        }
    }
}
#[derive(Clone)]
struct Source {
    text: String,
    url: String,
}

#[derive(Clone)]
pub struct PartCache {
    store: Arc<dyn ObjectStore>,
    prefix: String,
    http: reqwest::Client,
    base: String,
    local: Option<PathBuf>,
    fetch: bool,
    gate: Arc<tokio::sync::Mutex<()>>,
    failures: Arc<std::sync::Mutex<HashMap<String, (Instant, String)>>>,
    sources: Arc<std::sync::Mutex<(usize, BTreeMap<String, Source>)>>,
}
impl PartCache {
    pub fn new(store: Arc<dyn ObjectStore>, prefix: String) -> Result<Self> {
        let local = std::env::var_os("ERGO_ASSET_LIBRARY_DIR")
            .map(PathBuf::from)
            .map(|p| p.canonicalize())
            .transpose()?;
        Ok(Self {
            store,
            prefix,
            http: reqwest::Client::builder()
                .user_agent("Ergo/0.1 LDraw catalog asset resolver")
                .timeout(Duration::from_secs(12))
                .build()?,
            base: std::env::var("ERGO_ASSET_SOURCE_URL")
                .unwrap_or_else(|_| "https://library.ldraw.org/library/official".into())
                .trim_end_matches('/')
                .to_owned(),
            local,
            fetch: std::env::var("ERGO_ASSET_FETCH").is_ok_and(|s| s != "0" && s != "false")
                || std::env::var("ERGO_ASSET_FETCH").is_err(),
            gate: Arc::new(tokio::sync::Mutex::new(())),
            failures: Arc::new(std::sync::Mutex::new(HashMap::new())),
            sources: Default::default(),
        })
    }
    fn path(&self, key: &str) -> Path {
        Path::from(if self.prefix.is_empty() {
            key.into()
        } else {
            format!("{}/{key}", self.prefix)
        })
    }
    async fn get(&self, key: &str) -> Result<Option<Vec<u8>>> {
        match self.store.get(&self.path(key)).await {
            Ok(r) => Ok(Some(r.bytes().await?.to_vec())),
            Err(object_store::Error::NotFound { .. }) => Ok(None),
            Err(e) => Err(e.into()),
        }
    }
    async fn put(&self, key: &str, bytes: Vec<u8>) -> Result<()> {
        self.store.put(&self.path(key), bytes.into()).await?;
        Ok(())
    }
    pub async fn resolve(&self, name: &str) -> Result<PartAsset> {
        let part = ergo_world::catalog::canonical_part(&ldraw::normalize_name(name)?);
        ensure!(
            ergo_world::catalog::valid_part(&part),
            "Invalid part {part}"
        );
        let _guard = self.gate.lock().await;
        let key = format!("assets/parts/{part}.json");
        if let Some(bytes) = self.get(&key).await? {
            let mut record: PartDescription = serde_json::from_slice(&bytes)?;
            ensure!(
                record.format == PART_FORMAT
                    && record.schema == ergo_world::SCHEMA_VERSION
                    && record.part == part,
                "Unsupported Ergo part description"
            );
            if let Some(mesh) = self
                .get(&format!("objects/{}.cbor", record.mesh_hash))
                .await?
            {
                ensure!(
                    hex::encode(Sha256::digest(&mesh)) == record.mesh_hash,
                    "Ergo geometry hash mismatch"
                );
                let mut mesh: Mesh = ciborium::from_reader(mesh.as_slice())?;
                ensure!(
                    mesh.schema == ergo_world::SCHEMA_VERSION && mesh.part == record.part,
                    "Ergo geometry identity mismatch"
                );
                // Upgrade derived descriptions created before a catalogue
                // profile existed. Keep genuine vertices/credits unchanged;
                // publish new native geometry bytes before its description.
                if let Some(profile) = ergo_world::part_profiles::profile(&part) {
                    if mesh.body_bounds.min != profile.bounds.min
                        || mesh.body_bounds.max != profile.bounds.max
                        || record.catalog_mass.is_none()
                    {
                        mesh.body_bounds = profile.bounds;
                        let mut data = Vec::new();
                        ciborium::into_writer(&mesh, &mut data)?;
                        let hash = hex::encode(Sha256::digest(&data));
                        self.put(&format!("objects/{hash}.cbor"), data).await?;
                        record = PartDescription::from_mesh(&mesh, hash);
                        self.put(&key, serde_json::to_vec(&record)?).await?;
                    }
                }
                return Ok(PartAsset {
                    mesh,
                    credits: record.credits.clone(),
                    cache_hit: true,
                    warning: None,
                    description: Some(record),
                });
            }
        }
        if let Some((when, message)) = self
            .failures
            .lock()
            .unwrap()
            .get(&part)
            .cloned()
            .filter(|(when, _)| when.elapsed() < Duration::from_secs(30))
        {
            let _ = when;
            return Ok(PartAsset {
                mesh: fallback_mesh(&part),
                credits: Vec::new(),
                cache_hit: false,
                warning: Some(message),
                description: None,
            });
        }
        match self.build(&part).await {
            Ok((mesh, credits)) => {
                let mut data = Vec::new();
                ciborium::into_writer(&mesh, &mut data)?;
                let hash = hex::encode(Sha256::digest(&data));
                self.put(&format!("objects/{hash}.cbor"), data).await?;
                let description = PartDescription::from_mesh(&mesh, hash);
                self.put(&key, serde_json::to_vec(&description)?).await?;
                Ok(PartAsset {
                    mesh,
                    credits,
                    cache_hit: false,
                    warning: None,
                    description: Some(description),
                })
            }
            Err(e) => {
                ensure!(
                    ergo_world::geometry::part_spec(&part).is_some(),
                    "No geometry for {part}: {e:#}. Supply an asset library with ERGO_ASSET_LIBRARY_DIR; non-standard parts never use placeholder geometry"
                );
                let message =
                    format!("{e:#}; using parametric geometry (upstream fetch will be retried)");
                self.failures
                    .lock()
                    .unwrap()
                    .insert(part.clone(), (Instant::now(), message.clone()));
                Ok(PartAsset {
                    mesh: fallback_mesh(&part),
                    credits: Vec::new(),
                    cache_hit: false,
                    warning: Some(message),
                    description: None,
                })
            }
        }
    }
    async fn build(&self, part: &str) -> Result<(Mesh, Vec<Credit>)> {
        let mut queue = VecDeque::from([part.to_owned()]);
        let mut files = BTreeMap::new();
        let mut credits = Vec::new();
        let mut total = 0;
        while let Some(name) = queue.pop_front() {
            if files.contains_key(&name) {
                continue;
            }
            ensure!(files.len() < 4096, "Too many geometry dependencies");
            let (text, url) = self.source(&name, name == part).await?;
            total += text.len();
            ensure!(total <= 32 * 1024 * 1024, "Part dependencies exceed 32 MiB");
            let author = text
                .lines()
                .find_map(|l| l.strip_prefix("0 Author:"))
                .unwrap_or("LDraw contributors")
                .trim()
                .to_owned();
            let license = text
                .lines()
                .find_map(|l| l.strip_prefix("0 !LICENSE"))
                .unwrap_or("See upstream part license")
                .trim()
                .to_owned();
            credits.push(Credit {
                file: name.clone(),
                source_url: url,
                author,
                license,
            });
            for line in text.lines() {
                let tokens: Vec<_> = line.split_whitespace().collect();
                if tokens.first() == Some(&"1") {
                    queue.push_back(ldraw::reference(&tokens)?.name);
                }
            }
            files.insert(name, text);
        }
        let mut mesh = ldraw::bake_mesh(part, &|name| files.get(name).cloned(), &BTreeMap::new())?;
        if ergo_world::geometry::part_spec(part).is_some() {
            let nominal = fallback_mesh(part);
            ensure!(
                nominal.bounds.expanded(0.05).contains(mesh.bounds),
                "Downloaded geometry does not match nominal catalog dimensions"
            );
            mesh.body_bounds = nominal.body_bounds;
        } else {
            mesh.body_bounds =
                ergo_world::geometry::nominal_body_bounds(part).unwrap_or(mesh.bounds);
        }
        mesh.credits = credits.clone();
        Ok((mesh, credits))
    }
    async fn source(&self, name: &str, top: bool) -> Result<(String, String)> {
        if let Some(source) = self.sources.lock().unwrap().1.get(name).cloned() {
            return Ok((source.text, source.url));
        }
        if let Some(root) = &self.local {
            for dir in ["parts", "p", "unofficial/parts", "unofficial/p"] {
                let Ok(path) = root.join(dir).join(name).canonicalize() else {
                    continue;
                };
                if !path.starts_with(root) || path.metadata()?.len() > 1024 * 1024 {
                    continue;
                }
                if let Ok(text) = std::fs::read_to_string(&path) {
                    let url = format!("LDraw local library: {dir}/{name}");
                    self.remember(
                        name,
                        Source {
                            text: text.clone(),
                            url: url.clone(),
                        },
                    );
                    return Ok((text, url));
                }
            }
        }
        ensure!(
            self.fetch,
            "Automatic LDraw fetching is disabled and {name} is not cached"
        );
        let candidates = if name.starts_with("s/") || top {
            vec![format!("parts/{name}")]
        } else {
            vec![format!("p/{name}"), format!("parts/{name}")]
        };
        for path in candidates {
            let url = format!("{}/{path}", self.base);
            let response = self.http.get(&url).send().await?;
            if response.status() == reqwest::StatusCode::NOT_FOUND {
                continue;
            }
            let response = response.error_for_status()?;
            ensure!(
                response.content_length().is_none_or(|n| n <= 1024 * 1024),
                "Geometry source is too large"
            );
            let mut stream = response.bytes_stream();
            let mut bytes = Vec::new();
            while let Some(chunk) = stream.next().await {
                let chunk = chunk?;
                ensure!(
                    bytes.len() + chunk.len() <= 1024 * 1024,
                    "Geometry source is too large"
                );
                bytes.extend_from_slice(&chunk);
            }
            let text = String::from_utf8(bytes).context("LDraw source is not UTF-8")?;
            ensure!(
                !text.contains("<html"),
                "Upstream returned HTML instead of LDraw"
            );
            self.remember(
                name,
                Source {
                    text: text.clone(),
                    url: url.clone(),
                },
            );
            return Ok((text, url));
        }
        anyhow::bail!("LDraw dependency {name} was not found")
    }
    fn remember(&self, name: &str, source: Source) {
        let mut cache = self.sources.lock().unwrap();
        let bytes = source.text.len() + source.url.len();
        if cache.0 + bytes > 32 * 1024 * 1024 {
            cache.0 = 0;
            cache.1.clear();
        }
        cache.0 += bytes;
        cache.1.insert(name.into(), source);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn legacy_specialty_cache_upgrades_nominal_bounds_and_mass_without_changing_vertices() {
        let store: Arc<dyn ObjectStore> = Arc::new(object_store::memory::InMemory::new());
        let resolver = cache(store.clone());
        let raw_bounds = ergo_world::Aabb::new(
            glam::DVec3::new(-2., -3., -0.5),
            glam::DVec3::new(2., 1. / 6., 0.5),
        );
        let mut old = ergo_world::geometry::body_mesh("60594.dat", raw_bounds);
        old.quality = "synthetic source fixture".into();
        let mut bytes = Vec::new();
        ciborium::into_writer(&old, &mut bytes).unwrap();
        let hash = hex::encode(Sha256::digest(&bytes));
        let mut description = PartDescription::from_mesh(&old, hash.clone());
        description.catalog_mass = None;
        store
            .put(&Path::from(format!("objects/{hash}.cbor")), bytes.into())
            .await
            .unwrap();
        store
            .put(
                &Path::from("assets/parts/60594.dat.json"),
                serde_json::to_vec(&description).unwrap().into(),
            )
            .await
            .unwrap();
        let upgraded = resolver.resolve("60594.dat").await.unwrap();
        assert!(upgraded.cache_hit);
        assert_eq!(upgraded.mesh.vertices, old.vertices);
        assert_eq!(upgraded.mesh.body_bounds.max.y, 0.);
        let updated = upgraded.description.unwrap();
        assert!((updated.catalog_mass.unwrap().kg - 0.00185).abs() < 1e-12);
        assert_ne!(updated.mesh_hash, hash);
        assert!(
            resolver
                .get(&format!("objects/{}.cbor", updated.mesh_hash))
                .await
                .unwrap()
                .is_some()
        );
        assert!(
            resolver.resolve("3037.dat").await.is_err(),
            "A missing source slope must not become a rectangular substitute"
        );
    }
    fn cache(store: Arc<dyn ObjectStore>) -> PartCache {
        PartCache {
            store,
            prefix: String::new(),
            http: reqwest::Client::new(),
            base: "http://127.0.0.1:9".into(),
            local: None,
            fetch: false,
            gate: Arc::new(tokio::sync::Mutex::new(())),
            failures: Arc::new(std::sync::Mutex::new(HashMap::new())),
            sources: Default::default(),
        }
    }
    #[tokio::test]
    async fn baked_part_is_persisted_and_reused_by_a_fresh_cache_instance() {
        let store: Arc<dyn ObjectStore> = Arc::new(object_store::memory::InMemory::new());
        let first = cache(store.clone());
        let source=Source{text:"0 Test cube\n0 Author: Test fixture\n0 !LICENSE Test fixture\n4 16 -20 0 -20 20 0 -20 20 0 20 -20 0 20\n4 16 -20 24 -20 -20 24 20 20 24 20 20 24 -20\n4 16 -20 0 -20 -20 24 -20 20 24 -20 20 0 -20\n".into(),url:"http://127.0.0.1:9/parts/3003.dat".into()};
        first.remember("3003.dat", source);
        let a = first.resolve("3003.dat").await.unwrap();
        assert_eq!(a.mesh.quality, "source");
        assert!(!a.cache_hit);
        assert!(!a.mesh.credits.is_empty());
        let second = cache(store);
        let b = second.resolve("3003.dat").await.unwrap();
        assert!(b.cache_hit);
        assert_eq!(a.mesh.vertices, b.mesh.vertices);
    }
}
