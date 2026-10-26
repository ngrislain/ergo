//! Bounded read-only spatial view for browser navigation, never a whole-world
//! export or an editable/certified mechanical fragment.
use super::*;
use ergo_world::{BlockCoord, WorldExport, geometry::Mesh};

#[derive(Default, Deserialize)]
pub(super) struct ViewQuery {
    #[serde(default)]
    x: i64,
    #[serde(default)]
    y: i64,
    #[serde(default)]
    z: i64,
    revision: Option<ContentId>,
}

pub(super) async fn view(
    State(s): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Query(query): Query<ViewQuery>,
) -> SharedResult<Json<serde_json::Value>> {
    ensure!(
        [query.x, query.y, query.z]
            .iter()
            .all(|n| n.unsigned_abs() < 1_000_000_000),
        "View block coordinate out of range"
    );
    let snapshot = snapshot(&s, &id, &headers, query.revision.as_ref()).await?;
    let metadata: Metadata = snapshot
        .get_limited(&root(2), s.shared.limits.bytes)
        .await?
        .context("Missing world metadata")?;
    let mut ids = BTreeSet::new();
    let mut remaining = s.shared.limits.bytes.min(16 * 1024 * 1024);
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
                    for (_, cid) in page.leaves {
                        let row: spatial::SpatialRef =
                            snapshot.blob_limited(&cid, remaining).await?;
                        remaining = remaining.saturating_sub(canonical_cbor(&row)?.len());
                        ids.insert(row.piece);
                        if ids.len() > s.shared.limits.pieces.min(5000) {
                            return Err(SharedError::working_limit("browser view pieces"));
                        }
                    }
                    after = page.next;
                    if after.is_none() {
                        break;
                    }
                }
                if ids.len() > s.shared.limits.pieces.min(5000) {
                    return Err(SharedError::working_limit("browser view pieces"));
                }
            }
        }
    }
    let mut bricks = Vec::new();
    let mut meshes = BTreeMap::new();
    let mut mesh_ids = BTreeMap::<ContentId, String>::new();
    let mut assemblies = BTreeMap::new();
    for (index, piece_id) in ids.into_iter().enumerate() {
        let piece: Piece = snapshot
            .get_limited(&entity_path(3, &piece_id), remaining)
            .await?
            .context("Missing viewed piece")?;
        remaining = remaining.saturating_sub(canonical_cbor(&piece)?.len());
        let part = if let Some(key) = mesh_ids.get(&piece.mesh) {
            key.clone()
        } else {
            let mut mesh: Mesh = snapshot.blob_limited(&piece.mesh, remaining).await?;
            remaining = remaining.saturating_sub(canonical_cbor(&mesh)?.len());
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
    Ok(Json(
        serde_json::json!({"scene":scene,"revision":snapshot.id(),"blocks":blocks,"assemblies":assemblies,"view_kind":"bounded_read_only_region"}),
    ))
}
