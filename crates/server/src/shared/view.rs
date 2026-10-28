//! Bounded read-only spatial view for browser navigation, never a whole-world
//! export or an editable/certified mechanical fragment.
use super::read_cache::{EncodedView, ReadCache, ViewKey};
use super::*;
use ergo_world::{BlockCoord, WorldExport, geometry::Mesh};
use futures::{StreamExt, TryStreamExt};
use std::sync::Arc;
const INDEX_PAGE_SIZE: usize = 512;

/// Read-only browser budgets are independent of authoring and dynamic bodies.
#[derive(Clone, Copy)]
pub(super) struct Limits {
    pub pieces: usize,
    pub bytes: usize,
}
impl Default for Limits {
    fn default() -> Self {
        Self {
            pieces: 50_000,
            bytes: 64 * 1024 * 1024,
        }
    }
}
impl Limits {
    pub fn configured(pieces: Option<&str>, bytes: Option<&str>) -> anyhow::Result<Self> {
        let defaults = Self::default();
        let limits = Self {
            pieces: pieces
                .map(str::parse)
                .transpose()?
                .unwrap_or(defaults.pieces),
            bytes: bytes.map(str::parse).transpose()?.unwrap_or(defaults.bytes),
        };
        anyhow::ensure!(
            (1..=100_000).contains(&limits.pieces)
                && (1..=64 * 1024 * 1024).contains(&limits.bytes),
            "Browser view budgets must be 1..100000 pieces and 1..67108864 bytes"
        );
        Ok(limits)
    }
}

#[derive(Clone, Copy, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
enum ViewFormat {
    #[default]
    Json,
    Cbor,
}

#[derive(Serialize, Deserialize)]
pub(super) struct ViewPacket {
    pub scene: WorldExport,
    pub revision: ContentId,
    pub(super) blocks: Vec<BlockCoord>,
    assemblies: BTreeMap<u32, ContentId>,
    #[serde(default)]
    fully_resident_assemblies: BTreeSet<ContentId>,
    pub(super) view_kind: String,
}

#[derive(Default, Deserialize)]
pub(super) struct ViewQuery {
    #[serde(default)]
    x: i64,
    #[serde(default)]
    y: i64,
    #[serde(default)]
    z: i64,
    revision: Option<ContentId>,
    #[serde(default)]
    format: ViewFormat,
    #[serde(default)]
    status: bool,
    from_x: Option<i64>,
    from_y: Option<i64>,
    from_z: Option<i64>,
}

pub(super) async fn view(
    State(s): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Query(query): Query<ViewQuery>,
) -> SharedResult<Response> {
    let started = std::time::Instant::now();
    ensure!(
        [query.x, query.y, query.z]
            .iter()
            .all(|n| n.unsigned_abs() < 1_000_000_000),
        "View block coordinate out of range"
    );
    let (repo, current, _) = s.shared.authorized(&id, &reader(&s, &headers)?).await?;
    let latest = query
        .revision
        .as_ref()
        .is_none_or(|revision| revision == current.id());
    let snapshot = if latest {
        current
    } else {
        repo.accepted_snapshot("main", query.revision.as_ref().unwrap())
            .await?
    };
    let key = ViewKey {
        world: id,
        revision: snapshot.id().clone(),
        coord: [query.x, query.y, query.z],
        pieces: s.shared.view_limits.pieces,
        read_bytes: s.shared.view_limits.bytes,
    };
    let memory_key = key.memory_key()?;
    if query.status {
        let (phase, progress) = s
            .shared
            .read_cache
            .progress_status(&memory_key)
            .map(|(phase, work)| (Some(phase), work))
            .unwrap_or_default();
        let mut response =
            Json(serde_json::json!({"revision":key.revision,"phase":phase,"progress":progress}))
                .into_response();
        response
            .headers_mut()
            .insert("cache-control", "private, no-store".parse().unwrap());
        return Ok(response);
    }
    let etag = format!(
        "W/\"{}\"",
        ContentId::of(&canonical_cbor(&(&memory_key, query.format))?)
    );
    // Current read permissions were checked above, even for 304/cache hits.
    let unchanged = headers
        .get("if-none-match")
        .and_then(|v| v.to_str().ok())
        .is_some_and(|value| {
            value.split(',').any(|v| {
                v.trim() == "*"
                    || v.trim().trim_start_matches("W/") == etag.trim_start_matches("W/")
            })
        });
    if unchanged {
        let mut response = StatusCode::NOT_MODIFIED.into_response();
        set_headers(&mut response, &etag, "revalidated", started);
        return Ok(response);
    }
    let cache = &s.shared.read_cache;
    if let Some(value) = cache.get(&memory_key) {
        return response(&value, query.format, &headers, &etag, "memory", started).await;
    }
    let gate = cache.gate(&memory_key);
    let _guard = gate.lock().await;
    if let Some(value) = cache.get(&memory_key) {
        return response(&value, query.format, &headers, &etag, "memory", started).await;
    }
    let progress = cache.start_progress(&memory_key);
    progress.report("persistent cache lookup");
    let (stored, write_mode) = cache.persistent(&s.shared.store, &key).await;
    if let Some(value) = stored {
        cache.insert(memory_key, value.clone());
        return Ok(with_work_timing(
            response(&value, query.format, &headers, &etag, "persistent", started).await?,
            &progress,
        ));
    }
    let previous = query
        .from_x
        .zip(query.from_y)
        .zip(query.from_z)
        .map(|((x, y), z)| [x, y, z]);
    progress.report("nearby cache lookup");
    let reused = if let Some(previous) = previous {
        reuse_neighbor(&s.shared, &snapshot, &key, previous).await
    } else {
        None
    };
    let source = if reused.is_some() { "neighbor" } else { "miss" };
    let value = if let Some(value) = reused {
        value
    } else {
        let (packet, read_bytes) = build(&snapshot, &query, &key, &progress).await?;
        progress.report("encoding CBOR and gzip representations");
        let encoding_key = key.clone();
        Arc::new(
            tokio::task::spawn_blocking(move || {
                EncodedView::encode(&packet, &encoding_key, read_bytes)
            })
            .await
            .map_err(anyhow::Error::from)??,
        )
    };
    cache.insert(memory_key, value.clone());
    if latest {
        ReadCache::persist(s.shared.store.clone(), key, value.clone(), write_mode);
    }
    Ok(with_work_timing(
        response(&value, query.format, &headers, &etag, source, started).await?,
        &progress,
    ))
}

fn with_work_timing(
    mut response: Response,
    progress: &super::read_cache::ReadProgress<'_>,
) -> Response {
    let total = response
        .headers()
        .get("server-timing")
        .and_then(|header| header.to_str().ok())
        .unwrap_or_default();
    let stages = progress
        .timings_ms()
        .into_iter()
        .map(|(stage, ms)| format!("view_{stage};dur={ms:.2}"))
        .collect::<Vec<_>>()
        .join(", ");
    if let Ok(header) = format!("{total}, {stages}").parse() {
        response.headers_mut().insert("server-timing", header);
    }
    response
}

fn in_region(block: [i64; 3], centre: [i64; 3]) -> bool {
    (0..3).all(|axis| (i128::from(block[axis]) - i128::from(centre[axis])).abs() <= 1)
}

/// Reuse geometry only when the removed cells were empty and the added cells
/// have never been allocated in this exact accepted snapshot. An allocated but
/// empty cell conservatively falls back to normal materialization.
async fn reuse_neighbor(
    shared: &SharedWorlds,
    snapshot: &Snapshot,
    key: &ViewKey,
    previous: [i64; 3],
) -> Option<Arc<EncodedView>> {
    if previous == key.coord
        || !in_region(previous, key.coord)
        || previous.iter().any(|n| n.unsigned_abs() >= 1_000_000_000)
    {
        return None;
    }
    let old_key = ViewKey {
        coord: previous,
        ..key.clone()
    };
    let cached = if let Some(value) = shared.read_cache.get(&old_key.memory_key().ok()?) {
        Some(value)
    } else {
        shared
            .read_cache
            .persistent(&shared.store, &old_key)
            .await
            .0
    }?;
    if cached
        .blocks
        .iter()
        .any(|block| !in_region((*block).into(), key.coord))
    {
        return None;
    }
    for dx in -1..=1 {
        for dy in -1..=1 {
            for dz in -1..=1 {
                let coord = [key.coord[0] + dx, key.coord[1] + dy, key.coord[2] + dz];
                if !in_region(coord, previous) {
                    match snapshot
                        .get_limited::<spatial::BlockHeader>(
                            &spatial::directory(coord.into()),
                            key.read_bytes,
                        )
                        .await
                    {
                        Ok(None) => {}
                        _ => return None,
                    }
                }
            }
        }
    }
    // The packet contains only occupied blocks, not the requested centre. The
    // proof above establishes identical geometry, so even the encoded bytes can
    // be reused under the new checked region key.
    Some(cached)
}

impl SharedWorlds {
    /// Storage-operator cache warm-up, never exposed as a network write route.
    /// Materializes the same bounded accepted snapshot as the HTTP reader.
    pub async fn warm_view(&self, id: &str, coord: [i64; 3]) -> anyhow::Result<serde_json::Value> {
        ensure!(
            coord.iter().all(|n| n.unsigned_abs() < 1_000_000_000),
            "View block coordinate out of range"
        );
        let started = std::time::Instant::now();
        let repo = self.repo(id)?;
        let snapshot = repo.checkout("main").await?;
        let metadata: Metadata = snapshot
            .get_limited(&root(2), self.view_limits.bytes)
            .await?
            .context("Missing world metadata")?;
        anyhow::ensure!(
            metadata.id.as_deref().is_none_or(|stored| stored == id),
            "Configured sandbox ID does not match the stored namespace"
        );
        anyhow::ensure!(
            [2, 3].contains(&metadata.layout),
            "Only current paged shared records are supported"
        );
        let key = ViewKey {
            world: id.into(),
            revision: snapshot.id().clone(),
            coord,
            pieces: self.view_limits.pieces,
            read_bytes: self.view_limits.bytes,
        };
        let memory_key = key.memory_key()?;
        let gate = self.read_cache.gate(&memory_key);
        let _guard = gate.lock().await;
        let progress = self.read_cache.start_progress(&memory_key);
        progress.report("persistent cache lookup");
        let (stored, mode) = self.read_cache.persistent(&self.store, &key).await;
        let value = if let Some(value) = stored {
            value
        } else {
            let query = ViewQuery {
                x: coord[0],
                y: coord[1],
                z: coord[2],
                ..Default::default()
            };
            let work = build(&snapshot, &query, &key, &progress);
            tokio::pin!(work);
            let mut interval = tokio::time::interval(std::time::Duration::from_secs(10));
            let (packet, read_bytes) = loop {
                tokio::select! {
                    result=&mut work=>break result?,
                    _=interval.tick()=>tracing::info!(phase=?self.read_cache.progress(&memory_key),elapsed_s=started.elapsed().as_secs(),"Warming accepted read view"),
                }
            };
            let encoding_key = key.clone();
            progress.report("encoding CBOR and gzip representations");
            let value = Arc::new(
                tokio::task::spawn_blocking(move || {
                    EncodedView::encode(&packet, &encoding_key, read_bytes)
                })
                .await??,
            );
            anyhow::ensure!(
                repo.checkout("main").await?.id() == &key.revision,
                "World changed during warm-up; no packet persisted"
            );
            progress.report("persisting read-view cache");
            if value.brick_count > 0 {
                ReadCache::persist(self.store.clone(), key.clone(), value.clone(), mode)
                    .context("No safe conditional cache-write mode")?
                    .await?;
            }
            value
        };
        if value.brick_count > 0 {
            progress.report("verifying saved cache packet");
            let (verified, _) = self.read_cache.persistent(&self.store, &key).await;
            anyhow::ensure!(
                verified.is_some_and(|stored| stored.cbor == value.cbor),
                "Read-view cache write was not confirmed"
            );
        }
        self.read_cache.insert(memory_key, value.clone());
        let revision = key.revision.clone();
        snapshot.close().await?;
        Ok(
            serde_json::json!({"kind":"discardable accepted read-view cache; not a world commit","world_id":id,"revision":revision,
            "coord":coord,"pieces":value.brick_count,"logical_read_bytes":value.logical_read_bytes,"cbor_bytes":value.cbor.len(),"gzip_cbor_bytes":value.gzip_cbor.len(),"elapsed_s":started.elapsed().as_secs_f64(),"stage_ms":progress.timings_ms()}),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::Limits;

    #[test]
    fn view_configuration_has_explicit_bounds_and_rejects_invalid_values() {
        let defaults = Limits::configured(None, None).unwrap();
        assert_eq!(defaults.pieces, 50_000);
        assert_eq!(defaults.bytes, 64 * 1024 * 1024);
        assert_eq!(
            Limits::configured(Some("6000"), Some("1048576"))
                .unwrap()
                .pieces,
            6000
        );
        for (pieces, bytes) in [
            (Some("0"), None),
            (Some("100001"), None),
            (Some("invalid"), None),
            (None, Some("0")),
            (None, Some("67108865")),
        ] {
            assert!(Limits::configured(pieces, bytes).is_err());
        }
    }
}

fn set_headers(response: &mut Response, etag: &str, cache: &str, started: std::time::Instant) {
    let headers = response.headers_mut();
    headers.insert("etag", etag.parse().unwrap());
    headers.insert("cache-control", "private, no-cache".parse().unwrap());
    headers.insert("vary", "Accept-Encoding".parse().unwrap());
    headers.insert("x-ergo-read-cache", cache.parse().unwrap());
    headers.insert(
        "server-timing",
        format!("view;dur={:.2}", started.elapsed().as_secs_f64() * 1000.)
            .parse()
            .unwrap(),
    );
}
async fn response(
    value: &EncodedView,
    format: ViewFormat,
    request: &HeaderMap,
    etag: &str,
    cache: &str,
    started: std::time::Instant,
) -> SharedResult<Response> {
    let gzip = accepts_gzip(request);
    let bytes = match (format, gzip) {
        (ViewFormat::Json, _) => {
            let cbor = value.cbor.clone();
            tokio::task::spawn_blocking(move || -> anyhow::Result<bytes::Bytes> {
                let packet: ViewPacket = ciborium::from_reader(cbor.as_ref())?;
                let json = serde_json::to_vec(&packet)?;
                if gzip {
                    EncodedView::gzip(&json)
                } else {
                    Ok(json.into())
                }
            })
            .await
            .map_err(anyhow::Error::from)??
        }
        (ViewFormat::Cbor, false) => value.cbor.clone(),
        (ViewFormat::Cbor, true) => value.gzip_cbor.clone(),
    };
    let mut response = bytes.into_response();
    set_headers(&mut response, etag, cache, started);
    response.headers_mut().insert(
        "content-type",
        match format {
            ViewFormat::Json => "application/json",
            ViewFormat::Cbor => "application/cbor",
        }
        .parse()
        .unwrap(),
    );
    if gzip {
        response
            .headers_mut()
            .insert("content-encoding", "gzip".parse().unwrap());
    }
    Ok(response)
}
fn accepts_gzip(headers: &HeaderMap) -> bool {
    let mut explicit = None;
    let mut wildcard = false;
    for item in headers
        .get("accept-encoding")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .split(',')
    {
        let mut pieces = item.trim().split(';');
        let encoding = pieces.next().unwrap_or("").trim();
        let allowed = pieces
            .find_map(|p| {
                p.trim()
                    .strip_prefix("q=")
                    .map(|v| v.parse::<f64>().unwrap_or(0.))
            })
            .unwrap_or(1.)
            > 0.;
        if encoding.eq_ignore_ascii_case("gzip") {
            explicit = Some(allowed);
        } else if encoding == "*" {
            wildcard = allowed;
        }
    }
    explicit.unwrap_or(wildcard)
}

async fn build(
    snapshot: &Snapshot,
    query: &ViewQuery,
    key: &ViewKey,
    progress: &super::read_cache::ReadProgress<'_>,
) -> SharedResult<(ViewPacket, usize)> {
    let metadata: Metadata = snapshot
        .get_limited(&root(2), key.read_bytes)
        .await?
        .context("Missing world metadata")?;
    let mut ids = BTreeSet::new();
    let mut preloaded = BTreeMap::<ContentId, Piece>::new();
    let mut remaining = key
        .read_bytes
        .checked_sub(canonical_cbor(&metadata)?.len())
        .ok_or_else(|| SharedError::working_limit("browser view bytes"))?;
    let mut blocks = Vec::new();
    let mut checked_cells = 0;
    for dx in -1..=1 {
        for dy in -1..=1 {
            for dz in -1..=1 {
                let coord: BlockCoord = [query.x + dx, query.y + dy, query.z + dz].into();
                progress.scanning(checked_cells, ids.len(), Some(coord.into()));
                let mut packed_count = 0usize;
                let packed = if metadata.layout >= 3 {
                    bulk::read_block(snapshot, coord, |page| {
                        packed_count += page.pieces.len();
                        for (id, piece) in page.pieces {
                            if let Some(previous) = preloaded.get(&id) {
                                ensure!(
                                    canonical_cbor(previous)? == canonical_cbor(&piece)?,
                                    "Inconsistent packed block replicas"
                                );
                            } else {
                                ids.insert(id.clone());
                                if ids.len() > key.pieces {
                                    return Err(SharedError::working_limit("browser view pieces"));
                                }
                                preloaded.insert(id, piece);
                            }
                        }
                        progress.scanning(checked_cells, ids.len(), Some(coord.into()));
                        Ok(())
                    })
                    .await?
                } else {
                    false
                };
                if packed {
                    if packed_count > 0 {
                        blocks.push(coord);
                    }
                    checked_cells += 1;
                    progress.scanning(checked_cells, ids.len(), Some(coord.into()));
                    continue;
                }
                let mut after = None;
                loop {
                    let page = snapshot
                        .scan_page_prefetched(&block_path(coord), after.as_ref(), INDEX_PAGE_SIZE)
                        .await?;
                    if !page.leaves.is_empty() && !blocks.contains(&coord) {
                        blocks.push(coord);
                    }
                    for (path, _) in page.leaves {
                        ids.insert(spatial::reference_piece(&path, coord)?);
                        if ids.len() % 2048 == 0 {
                            progress.scanning(checked_cells, ids.len(), None);
                        }
                        if ids.len() > key.pieces {
                            return Err(SharedError::working_limit("browser view pieces"));
                        }
                    }
                    after = page.next;
                    if after.is_none() {
                        break;
                    }
                }
                if ids.len() > key.pieces {
                    return Err(SharedError::working_limit("browser view pieces"));
                }
                checked_cells += 1;
                progress.scanning(checked_cells, ids.len(), Some(coord.into()));
            }
        }
    }
    let mut bricks = Vec::new();
    let total = ids.len();
    progress.pieces(0, total, 0);
    let mut meshes = BTreeMap::new();
    let mut mesh_ids = BTreeMap::<ContentId, String>::new();
    let mut assemblies = BTreeMap::new();
    let limit = remaining;
    let mut pieces = futures::stream::iter(ids)
        .map(|piece_id| {
            let packed = preloaded.remove(&piece_id);
            async move {
                if let Some(piece) = packed {
                    return Ok(piece);
                }
                snapshot
                    .get_limited::<Piece>(&entity_path(3, &piece_id), limit)
                    .await?
                    .context("Missing viewed piece")
            }
        })
        .buffered(READ_CONCURRENCY);
    let mut index = 0;
    while let Some(piece) = pieces.try_next().await? {
        remaining = remaining
            .checked_sub(canonical_cbor(&piece)?.len())
            .ok_or_else(|| SharedError::working_limit("browser view bytes"))?;
        let part = if let Some(key) = mesh_ids.get(&piece.mesh) {
            key.clone()
        } else {
            let mut mesh: Mesh = snapshot.blob_limited(&piece.mesh, remaining).await?;
            remaining = remaining
                .checked_sub(canonical_cbor(&mesh)?.len())
                .ok_or_else(|| SharedError::working_limit("browser view bytes"))?;
            let key = if meshes.contains_key(&piece.geometry.part) {
                format!("view/{}/{}", piece.mesh, piece.geometry.part)
            } else {
                piece.geometry.part.clone()
            };
            mesh.part = key.clone();
            mesh_ids.insert(piece.mesh.clone(), key.clone());
            meshes.insert(key.clone(), mesh);
            key
        };
        let local = index as u32 + 1;
        let mut geometry = piece.geometry.clone();
        geometry.part = part.clone();
        bricks.push(geometry.brick(local, &meshes[&part])?);
        assemblies.insert(local, piece.assembly);
        index += 1;
        if index % 1024 == 0 {
            progress.pieces(index, total, meshes.len());
        }
    }
    progress.pieces(index, total, meshes.len());
    let scene = WorldExport {
        schema: ergo_world::SCHEMA_VERSION,
        name: metadata.name,
        geo: metadata.geo,
        bricks,
        meshes,
        joints: Vec::new(),
        fixed_bricks: Vec::new(),
        groups: Vec::new(),
        model: None,
        ground: Vec::new(),
    };
    let mut visible = BTreeMap::<ContentId, usize>::new();
    for assembly in assemblies.values() {
        *visible.entry(assembly.clone()).or_default() += 1;
    }
    let mut fully_resident_assemblies = BTreeSet::new();
    let assembly_total = visible.len();
    progress.membership(0, assembly_total);
    let mut checked_assemblies = 0;
    for (assembly, count) in visible {
        let header: paged::RulesHeader = snapshot
            .get_limited(&paged::rule_path(&assembly, 0), remaining)
            .await?
            .context("Missing viewed assembly header")?;
        remaining = remaining
            .checked_sub(canonical_cbor(&header)?.len())
            .ok_or_else(|| SharedError::working_limit("browser view bytes"))?;
        let complete = if let Some(total) = header.piece_count {
            total == count
        } else {
            let mut total = 0;
            let mut after = None;
            loop {
                let page = snapshot
                    .scan_page(&paged::rule_path(&assembly, 1), after.as_ref(), 128)
                    .await?;
                total += page.leaves.len();
                if total > count {
                    break false;
                }
                after = page.next;
                if after.is_none() {
                    break total == count;
                }
            }
        };
        if complete {
            fully_resident_assemblies.insert(assembly);
        }
        checked_assemblies += 1;
        progress.membership(checked_assemblies, assembly_total);
    }
    Ok((
        ViewPacket {
            scene,
            revision: snapshot.id().clone(),
            blocks,
            assemblies,
            fully_resident_assemblies,
            view_kind: "bounded_read_only_region".into(),
        },
        key.read_bytes - remaining,
    ))
}
