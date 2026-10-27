//! Bounded read-only spatial view for browser navigation, never a whole-world
//! export or an editable/certified mechanical fragment.
use super::read_cache::{EncodedView, ReadCache, ViewKey};
use super::*;
use ergo_world::{BlockCoord, WorldExport, geometry::Mesh};
use futures::{StreamExt, TryStreamExt};
use std::sync::Arc;

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
    blocks: Vec<BlockCoord>,
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
    let snapshot = snapshot(&s, &id, &headers, query.revision.as_ref()).await?;
    let key = ViewKey {
        world: id,
        revision: snapshot.id().clone(),
        coord: [query.x, query.y, query.z],
        pieces: s.shared.view_limits.pieces,
        read_bytes: s.shared.view_limits.bytes,
    };
    let memory_key = key.memory_key()?;
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
        return Ok(response(
            &value,
            query.format,
            &headers,
            &etag,
            "memory",
            started,
        ));
    }
    let gate = cache.gate(&memory_key);
    let _guard = gate.lock().await;
    if let Some(value) = cache.get(&memory_key) {
        return Ok(response(
            &value,
            query.format,
            &headers,
            &etag,
            "memory",
            started,
        ));
    }
    let (stored, write_mode) = cache.persistent(&s.shared.store, &key).await;
    if let Some(value) = stored {
        cache.insert(memory_key, value.clone());
        return Ok(response(
            &value,
            query.format,
            &headers,
            &etag,
            "persistent",
            started,
        ));
    }
    let (packet, read_bytes) = build(&snapshot, &query, &key).await?;
    let encoding_key = key.clone();
    let value = Arc::new(
        tokio::task::spawn_blocking(move || {
            EncodedView::encode(&packet, &encoding_key, read_bytes)
        })
        .await
        .map_err(anyhow::Error::from)??,
    );
    cache.insert(memory_key, value.clone());
    if query.revision.is_none() {
        ReadCache::persist(s.shared.store.clone(), key, value.clone(), write_mode);
    }
    Ok(response(
        &value,
        query.format,
        &headers,
        &etag,
        "miss",
        started,
    ))
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
fn response(
    value: &EncodedView,
    format: ViewFormat,
    request: &HeaderMap,
    etag: &str,
    cache: &str,
    started: std::time::Instant,
) -> Response {
    let gzip = accepts_gzip(request);
    let bytes = match (format, gzip) {
        (ViewFormat::Json, false) => &value.json,
        (ViewFormat::Json, true) => &value.gzip_json,
        (ViewFormat::Cbor, false) => &value.cbor,
        (ViewFormat::Cbor, true) => &value.gzip_cbor,
    };
    let mut response = bytes.clone().into_response();
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
    response
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
) -> SharedResult<(ViewPacket, usize)> {
    let metadata: Metadata = snapshot
        .get_limited(&root(2), key.read_bytes)
        .await?
        .context("Missing world metadata")?;
    let mut ids = BTreeSet::new();
    let mut remaining = key
        .read_bytes
        .checked_sub(canonical_cbor(&metadata)?.len())
        .ok_or_else(|| SharedError::working_limit("browser view bytes"))?;
    let mut blocks = Vec::new();
    for dx in -1..=1 {
        for dy in -1..=1 {
            for dz in -1..=1 {
                let coord: BlockCoord = [query.x + dx, query.y + dy, query.z + dz].into();
                let mut after = None;
                loop {
                    let page = snapshot
                        .scan_page(&block_path(coord), after.as_ref(), 128)
                        .await?;
                    if !page.leaves.is_empty() && !blocks.contains(&coord) {
                        blocks.push(coord);
                    }
                    let limit = remaining;
                    let mut rows = futures::stream::iter(page.leaves)
                        .map(|(_, cid)| async move {
                            snapshot
                                .blob_limited::<spatial::SpatialRef>(&cid, limit)
                                .await
                        })
                        .buffered(8);
                    while let Some(row) = rows.try_next().await? {
                        remaining = remaining
                            .checked_sub(canonical_cbor(&row)?.len())
                            .ok_or_else(|| SharedError::working_limit("browser view bytes"))?;
                        ids.insert(row.piece);
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
            }
        }
    }
    let mut bricks = Vec::new();
    let mut meshes = BTreeMap::new();
    let mut mesh_ids = BTreeMap::<ContentId, String>::new();
    let mut assemblies = BTreeMap::new();
    let limit = remaining;
    let mut pieces = futures::stream::iter(ids)
        .map(|piece_id| async move {
            snapshot
                .get_limited::<Piece>(&entity_path(3, &piece_id), limit)
                .await?
                .context("Missing viewed piece")
        })
        .buffered(8);
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
    }
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
