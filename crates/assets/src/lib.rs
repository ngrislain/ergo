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
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PartAsset {
    pub mesh: Mesh,
    pub credits: Vec<Credit>,
    pub cache_hit: bool,
    pub warning: Option<String>,
}
#[derive(Serialize, Deserialize)]
struct Index {
    schema: u32,
    part: String,
    mesh_hash: String,
    credits: Vec<Credit>,
}
#[derive(Serialize, Deserialize)]
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
}
impl PartCache {
    pub fn new(store: Arc<dyn ObjectStore>, prefix: String) -> Result<Self> {
        let local = std::env::var_os("LDRAW_DIR")
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
            base: std::env::var("LDRAW_BASE_URL")
                .unwrap_or_else(|_| "https://library.ldraw.org/library/official".into())
                .trim_end_matches('/')
                .to_owned(),
            local,
            fetch: std::env::var("ERGO_ASSET_FETCH").is_ok_and(|s| s != "0" && s != "false")
                || std::env::var("ERGO_ASSET_FETCH").is_err(),
            gate: Arc::new(tokio::sync::Mutex::new(())),
            failures: Arc::new(std::sync::Mutex::new(HashMap::new())),
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
            ergo_world::catalog::PARTS.iter().any(|(p, _)| *p == part),
            "Unsupported part {part}"
        );
        let _guard = self.gate.lock().await;
        let key = format!("assets/v{}/parts/{part}.json", ergo_world::SCHEMA_VERSION);
        if let Some(bytes) = self.get(&key).await? {
            let record: Index = serde_json::from_slice(&bytes)?;
            if record.schema == ergo_world::SCHEMA_VERSION
                && let Some(mesh) = self
                    .get(&format!("objects/{}.cbor", record.mesh_hash))
                    .await?
            {
                let mesh: Mesh = ciborium::from_reader(mesh.as_slice())?;
                return Ok(PartAsset {
                    mesh,
                    credits: record.credits,
                    cache_hit: true,
                    warning: None,
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
            });
        }
        match self.build(&part).await {
            Ok((mesh, credits)) => {
                let mut data = Vec::new();
                ciborium::into_writer(&mesh, &mut data)?;
                let hash = hex::encode(Sha256::digest(&data));
                self.put(&format!("objects/{hash}.cbor"), data).await?;
                self.put(
                    &key,
                    serde_json::to_vec(&Index {
                        schema: ergo_world::SCHEMA_VERSION,
                        part: part.clone(),
                        mesh_hash: hash,
                        credits: credits.clone(),
                    })?,
                )
                .await?;
                Ok(PartAsset {
                    mesh,
                    credits,
                    cache_hit: false,
                    warning: None,
                })
            }
            Err(e) => {
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
        let nominal = fallback_mesh(part);
        ensure!(
            nominal.bounds.expanded(0.05).contains(mesh.bounds),
            "Downloaded geometry does not match nominal catalog dimensions"
        );
        mesh.body_bounds = nominal.body_bounds;
        mesh.credits = credits.clone();
        Ok((mesh, credits))
    }
    async fn source(&self, name: &str, top: bool) -> Result<(String, String)> {
        let key = format!("assets/ldraw/source/{name}.json");
        if let Some(bytes) = self.get(&key).await? {
            let source: Source = serde_json::from_slice(&bytes)?;
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
                    self.put(
                        &key,
                        serde_json::to_vec(&Source {
                            text: text.clone(),
                            url: url.clone(),
                        })?,
                    )
                    .await?;
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
            self.put(
                &key,
                serde_json::to_vec(&Source {
                    text: text.clone(),
                    url: url.clone(),
                })?,
            )
            .await?;
            return Ok((text, url));
        }
        anyhow::bail!("LDraw dependency {name} was not found")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
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
        }
    }
    #[tokio::test]
    async fn baked_part_is_persisted_and_reused_by_a_fresh_cache_instance() {
        let store: Arc<dyn ObjectStore> = Arc::new(object_store::memory::InMemory::new());
        let first = cache(store.clone());
        let source=Source{text:"0 Test cube\n0 Author: Test fixture\n0 !LICENSE Test fixture\n4 16 -20 0 -20 20 0 -20 20 0 20 -20 0 20\n4 16 -20 24 -20 -20 24 20 20 24 20 20 24 -20\n4 16 -20 0 -20 -20 24 -20 20 24 -20 20 0 -20\n".into(),url:"http://127.0.0.1:9/parts/3003.dat".into()};
        first
            .put(
                "assets/ldraw/source/3003.dat.json",
                serde_json::to_vec(&source).unwrap(),
            )
            .await
            .unwrap();
        let a = first.resolve("3003.dat").await.unwrap();
        assert_eq!(a.mesh.quality, "ldraw");
        assert!(!a.cache_hit);
        assert!(!a.mesh.credits.is_empty());
        let second = cache(store);
        let b = second.resolve("3003.dat").await.unwrap();
        assert!(b.cache_hit);
        assert_eq!(a.mesh.vertices, b.mesh.vertices);
    }
}
