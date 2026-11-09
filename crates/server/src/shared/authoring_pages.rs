//! Bounded, revision-pinned posed-piece pages. Spatial queries enumerate the
//! existing canonical reference octree, not an alternative geometry store.
use super::*;
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use ergo_world::{
    Aabb, BlockCoord,
    authoring::{PAGE_BYTES, PAGE_PIECES},
};

/// Decode only the small summary fields. The assembly's potentially large
/// mesh map is skipped rather than copied into every manifest and page.
#[derive(Deserialize)]
struct HeaderSummary {
    id: ContentId,
    name: String,
    #[serde(default)]
    piece_count: Option<usize>,
    #[serde(rename = "paged", deserialize_with = "paged::current_paged")]
    _paged: bool,
}

#[derive(Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct PageQuery {
    revision: Option<ContentId>,
    cursor: Option<String>,
    limit: Option<usize>,
    /// JSON public-coordinate [minimum, maximum].
    bounds: Option<String>,
    ids: Option<String>,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Cursor {
    world: String,
    assembly: ContentId,
    revision: ContentId,
    actor: String,
    bounds: Option<[[f64; 3]; 2]>,
    after: Option<OctreePath>,
    region: usize,
    directory_after: Option<OctreePath>,
    coord: Option<BlockCoord>,
    cells: Vec<SpatialTask>,
    selected: Option<Vec<u32>>,
    selected_after: usize,
}

#[derive(Clone, Serialize, Deserialize)]
enum SpatialTask {
    Visit(Vec<u8>),
    Scan {
        suffix: Vec<u8>,
        after: Option<OctreePath>,
    },
}
fn cell_bounds(coord: BlockCoord, suffix: &[u8]) -> SharedResult<([f64; 3], f64)> {
    ensure!(
        suffix.len() <= 20 && suffix.len() % 2 == 0,
        "Invalid spatial cursor depth"
    );
    let c: [i64; 3] = coord.into();
    let mut origin = c.map(|v| v as f64 * ergo_world::BLOCK_SIZE);
    let mut size = ergo_world::BLOCK_SIZE;
    for pair in suffix.chunks_exact(2) {
        ensure!(pair[0] == 1 && pair[1] < 8, "Invalid spatial cursor path");
        size *= 0.5;
        for axis in 0..3 {
            if pair[1] & (1 << axis) != 0 {
                origin[axis] += size;
            }
        }
    }
    Ok((origin, size))
}
fn cursor_encode(c: &Cursor) -> SharedResult<String> {
    Ok(URL_SAFE_NO_PAD.encode(serde_json::to_vec(c).map_err(anyhow::Error::from)?))
}
fn cursor_decode(value: &str) -> SharedResult<Cursor> {
    ensure!(value.len() <= 16384, "Authoring cursor is too large");
    Ok(
        serde_json::from_slice(&URL_SAFE_NO_PAD.decode(value).map_err(anyhow::Error::from)?)
            .map_err(anyhow::Error::from)?,
    )
}
fn bounds(raw: [[f64; 3]; 2]) -> SharedResult<Aabb> {
    ensure!(
        raw.iter()
            .flatten()
            .all(|x| x.is_finite() && x.abs() <= ergo_world::MAX_WORLD_COORD),
        "Invalid authoring bounds"
    );
    ensure!(
        (0..3).all(|i| raw[0][i] <= raw[1][i]),
        "Reversed authoring bounds"
    );
    let a = ergo_world::coordinates::internal(glam::DVec3::from(raw[0]));
    let b = ergo_world::coordinates::internal(glam::DVec3::from(raw[1]));
    Ok(Aabb::new(a.min(b), a.max(b)))
}

impl SharedWorlds {
    pub async fn authoring_manifest(
        &self,
        id: &str,
        actor: &str,
        assembly: &ContentId,
        revision: Option<&ContentId>,
    ) -> SharedResult<serde_json::Value> {
        let head = self
            .head(
                id,
                &Principal {
                    id: Some(actor.into()),
                },
                revision,
            )
            .await?;
        let snapshot = self
            .repo(id)?
            .accepted_snapshot("main", &head.revision)
            .await?;
        let header: HeaderSummary = snapshot
            .get_limited(&paged::rule_path(assembly, 0), self.limits.bytes)
            .await?
            .ok_or_else(SharedError::missing)?;
        ensure!(header.id == *assembly, "Assembly identity mismatch");
        let highest =
            if let Some((path, cid)) = snapshot.last(&paged::rule_path(assembly, 1)).await? {
                let member = snapshot
                    .blob_limited::<paged::MemberRow>(&cid, 4096)
                    .await?;
                ensure!(
                    path == paged::row(assembly, 1, u64::from(member.local_id)),
                    "Member key mismatch"
                );
                member.local_id
            } else {
                0
            };
        let group_prefix = paged::rule_path(assembly, 5);
        let highest_group = if let Some((path, _)) = snapshot.last(&group_prefix).await? {
            let start = group_prefix.octants().len();
            ensure!(path.octants().len() >= start + 11, "Invalid group path");
            let n = path.octants()[start..start + 11]
                .iter()
                .fold(0u64, |n, q| (n << 3) | u64::from(*q));
            u32::try_from(n).context("Invalid group ID")?
        } else {
            0
        };
        let result = serde_json::json!({"format":"ergo-workspace-2","world_id":id,"assembly_id":assembly,"base_revision":head.revision,"name":header.name,"geo":head.metadata.geo,"piece_count":header.piece_count,"next_local_id":u64::from(highest)+1,"next_group_id":u64::from(highest_group)+1,"page_piece_limit":PAGE_PIECES,"page_byte_limit":PAGE_BYTES,"resident_piece_limit":ergo_world::authoring::RESIDENT_PIECES,"external_dependencies":true,"partial":true});
        snapshot.close().await?;
        Ok(result)
    }

    async fn authoring_page(
        &self,
        id: &str,
        actor: &str,
        assembly: &ContentId,
        q: PageQuery,
    ) -> SharedResult<serde_json::Value> {
        let limit = q.limit.unwrap_or(128);
        ensure!(
            (1..=PAGE_PIECES).contains(&limit),
            "Page piece limit must be 1..512"
        );
        let query_bounds: Option<[[f64; 3]; 2]> = q
            .bounds
            .as_deref()
            .map(serde_json::from_str)
            .transpose()
            .map_err(anyhow::Error::from)?;
        let selected: Option<Vec<u32>> = q
            .ids
            .as_deref()
            .map(serde_json::from_str)
            .transpose()
            .map_err(anyhow::Error::from)?;
        if let Some(ids) = &selected {
            ensure!(
                !ids.is_empty()
                    && ids.len() <= 128
                    && ids.iter().all(|id| *id > 0)
                    && ids.iter().collect::<BTreeSet<_>>().len() == ids.len(),
                "Select 1–128 distinct piece IDs per request"
            );
        }
        ensure!(
            selected.is_none() || query_bounds.is_none(),
            "Choose IDs or spatial bounds, not both"
        );
        let mut c = if let Some(cursor) = &q.cursor {
            let c = cursor_decode(cursor)?;
            ensure!(
                c.world == id
                    && c.assembly == *assembly
                    && c.actor == actor
                    && q.revision.as_ref().is_none_or(|r| *r == c.revision)
                    && query_bounds.is_none_or(|b| Some(b) == c.bounds),
                "Cursor belongs to another actor, revision or query"
            );
            ensure!(
                selected
                    .as_ref()
                    .is_none_or(|ids| Some(ids) == c.selected.as_ref()),
                "Cursor belongs to another selection"
            );
            c
        } else {
            let head = self
                .head(
                    id,
                    &Principal {
                        id: Some(actor.into()),
                    },
                    q.revision.as_ref(),
                )
                .await?;
            Cursor {
                world: id.into(),
                assembly: assembly.clone(),
                revision: head.revision,
                actor: actor.into(),
                bounds: query_bounds,
                after: None,
                region: 0,
                directory_after: None,
                coord: None,
                cells: Vec::new(),
                selected,
                selected_after: 0,
            }
        };
        // Rechecks current authorization and accepted-history membership for
        // every page, including requests that carry a cursor.
        let head = self
            .head(
                id,
                &Principal {
                    id: Some(actor.into()),
                },
                Some(&c.revision),
            )
            .await?;
        let snapshot = self
            .repo(id)?
            .accepted_snapshot("main", &c.revision)
            .await?;
        let header: HeaderSummary = snapshot
            .get_limited(&paged::rule_path(assembly, 0), self.limits.bytes)
            .await?
            .ok_or_else(SharedError::missing)?;
        ensure!(header.id == *assembly, "Assembly identity mismatch");
        let query = c.bounds.map(bounds).transpose()?;
        let regions = if let Some(query) = query {
            address::cover(query)?
        } else {
            vec![]
        };
        let mut items = Vec::new();
        let mut versions = BTreeMap::new();
        let mut mesh_ids = BTreeMap::new();
        let mut mesh_cache: Option<(ContentId, ergo_world::geometry::Mesh)> = None;
        let mut bytes = 0usize;
        let mut scanned = 0;
        let mut done = false;
        'read: while items.len() < limit && scanned < 2048 {
            let prior = c.clone();
            let global = if let Some(ids) = &c.selected {
                let Some(local) = ids.get(c.selected_after) else {
                    done = true;
                    break;
                };
                c.selected_after += 1;
                scanned += 1;
                let member = snapshot
                    .get_limited::<paged::MemberRow>(
                        &paged::row(assembly, 1, u64::from(*local)),
                        4096,
                    )
                    .await?
                    .ok_or_else(SharedError::missing)?;
                ensure!(member.local_id == *local, "Member key mismatch");
                member.piece
            } else if let Some(query) = query {
                if c.region >= regions.len() {
                    done = true;
                    break;
                }
                if c.coord.is_none() {
                    let mut p = regions[c.region].octants().to_vec();
                    p[0] = 5;
                    let directory = OctreePath::new(p)?;
                    let page = snapshot
                        .scan_page(&directory, c.directory_after.as_ref(), 1)
                        .await?;
                    if let Some((path, cid)) = page.leaves.into_iter().next() {
                        let block: spatial::BlockHeader = snapshot.blob_limited(&cid, 4096).await?;
                        ensure!(
                            spatial::coordinate(&path)? == block.coord,
                            "Block directory mismatch"
                        );
                        c.coord = Some(block.coord);
                        c.directory_after = Some(path);
                        c.cells = vec![SpatialTask::Visit(vec![])];
                        scanned += 1;
                    } else {
                        c.region += 1;
                        c.directory_after = None;
                        continue;
                    }
                }
                let coord = c.coord.unwrap();
                let a = ergo_world::coordinates::public(query.min);
                let b = ergo_world::coordinates::public(query.max);
                let lo = a.min(b).to_array();
                let hi = a.max(b).to_array();
                let next = loop {
                    if scanned >= 2048 {
                        break 'read;
                    }
                    let Some(task) = c.cells.pop() else {
                        c.coord = None;
                        break None;
                    };
                    scanned += 1;
                    match task {
                        SpatialTask::Visit(suffix) => {
                            let (origin, size) = cell_bounds(coord, &suffix)?;
                            if (0..3).any(|i| origin[i] > hi[i] || origin[i] + size < lo[i]) {
                                continue;
                            }
                            if !snapshot
                                .directory_exists(&paged::append(&block_path(coord), &suffix))
                                .await?
                            {
                                continue;
                            }
                            let full =
                                (0..3).all(|i| origin[i] >= lo[i] && origin[i] + size <= hi[i]);
                            if full {
                                c.cells.push(SpatialTask::Scan {
                                    suffix,
                                    after: None,
                                });
                            } else {
                                if suffix.len() < 20 {
                                    for q in (0..8u8).rev() {
                                        let mut child = suffix.clone();
                                        child.extend([1, q]);
                                        c.cells.push(SpatialTask::Visit(child));
                                    }
                                }
                                let mut own = suffix;
                                own.push(0);
                                c.cells.push(SpatialTask::Scan {
                                    suffix: own,
                                    after: None,
                                });
                            }
                        }
                        SpatialTask::Scan { suffix, after } => {
                            let base = if suffix.last() == Some(&0) && suffix.len() % 2 == 1 {
                                &suffix[..suffix.len() - 1]
                            } else {
                                suffix.as_slice()
                            };
                            cell_bounds(coord, base)?;
                            let prefix = paged::append(&block_path(coord), &suffix);
                            let page = snapshot.scan_page(&prefix, after.as_ref(), 1).await?;
                            if let Some((path, cid)) = page.leaves.into_iter().next() {
                                c.cells.push(SpatialTask::Scan {
                                    suffix,
                                    after: Some(path.clone()),
                                });
                                break Some((path, cid));
                            }
                        }
                    }
                };
                let Some((path, cid)) = next else {
                    continue;
                };
                let r: spatial::SpatialRef = snapshot.blob_limited(&cid, 4096).await?;
                ensure!(
                    r.coord == coord && spatial::reference_piece(&path, coord)? == r.piece,
                    "Spatial reference identity mismatch"
                );
                if !r.bounds.intersects(query) {
                    continue;
                }
                r.piece
            } else {
                let page = snapshot
                    .scan_page(&paged::rule_path(assembly, 1), c.after.as_ref(), 1)
                    .await?;
                let Some((path, cid)) = page.leaves.into_iter().next() else {
                    done = true;
                    break;
                };
                c.after = Some(path.clone());
                scanned += 1;
                let member = snapshot
                    .blob_limited::<paged::MemberRow>(&cid, 4096)
                    .await?;
                ensure!(
                    path == paged::row(assembly, 1, u64::from(member.local_id)),
                    "Member key mismatch"
                );
                member.piece
            };
            let piece: Piece = snapshot
                .get_limited(&entity_path(3, &global), 64 * 1024)
                .await?
                .ok_or_else(SharedError::missing)?;
            ensure!(piece.id == global, "Piece identity mismatch");
            if piece.assembly != *assembly {
                continue;
            }
            let member: paged::MemberRow = snapshot
                .get_limited(&paged::row(assembly, 1, u64::from(piece.local_id)), 4096)
                .await?
                .ok_or_else(SharedError::missing)?;
            ensure!(
                member.local_id == piece.local_id && member.piece == global,
                "Assembly member mismatch"
            );
            if let Some(query) = query {
                // A crossing piece has one reference per intersected block.
                // Only the first query-intersecting replica owns this result.
                let a = ergo_world::coordinates::public(query.min);
                let b = ergo_world::coordinates::public(query.max);
                let lo = a
                    .min(b)
                    .to_array()
                    .map(|x| (x / ergo_world::BLOCK_SIZE).floor() as i64);
                let hi = a
                    .max(b)
                    .to_array()
                    .map(|x| (x / ergo_world::BLOCK_SIZE).floor() as i64);
                let owner = piece.replicas.iter().find(|&&coord| {
                    let x: [i64; 3] = coord.into();
                    (0..3).all(|i| x[i] >= lo[i] && x[i] <= hi[i])
                });
                if owner.copied() != c.coord {
                    continue;
                }
            }
            if mesh_cache.as_ref().is_none_or(|(id, _)| *id != piece.mesh) {
                mesh_cache = Some((
                    piece.mesh.clone(),
                    snapshot
                        .blob_limited(&piece.mesh, self.limits.bytes)
                        .await?,
                ));
            }
            let mesh = &mesh_cache.as_ref().unwrap().1;
            let brick = piece.geometry.brick(piece.local_id, mesh)?;
            if query.is_some_and(|q| !brick.render_bounds.intersects(q)) {
                continue;
            }
            let size = serde_json::to_vec(&brick)
                .map_err(anyhow::Error::from)?
                .len()
                + 256;
            if bytes + size > PAGE_BYTES - 4096 {
                c = prior;
                break;
            }
            bytes += size;
            ensure!(
                mesh_ids
                    .get(&brick.part)
                    .is_none_or(|old| *old == piece.mesh),
                "Page contains incompatible meshes for one part name; select the variants separately"
            );
            versions.insert(piece.local_id, ergo_world::authoring::version(&piece)?);
            mesh_ids.insert(brick.part.clone(), piece.mesh);
            items.push(brick);
        }
        if c.selected
            .as_ref()
            .is_some_and(|ids| c.selected_after >= ids.len())
        {
            done = true;
        }
        let next = if done { None } else { Some(cursor_encode(&c)?) };
        let result = serde_json::json!({"format":"ergo-authoring-page-1","world_id":id,"assembly_id":assembly,"base_revision":c.revision,"name":header.name,"geo":head.metadata.geo,"piece_count":header.piece_count,"bricks":items,"versions":versions,"mesh_ids":mesh_ids,"next":next,"partial":true,"external_dependencies":true,"scanned_records":scanned});
        ensure!(
            serde_json::to_vec(&result)
                .map_err(anyhow::Error::from)?
                .len()
                <= PAGE_BYTES,
            "Authoring page exceeds byte limit"
        );
        snapshot.close().await?;
        Ok(result)
    }
}

pub(super) async fn manifest(
    State(s): State<AppState>,
    method: axum::http::Method,
    h: HeaderMap,
    Path((id, assembly)): Path<(String, ContentId)>,
    Query(q): Query<RevisionQuery>,
) -> SharedResult<Response> {
    let actor = reader(&s, &h)?.required()?;
    let value = s
        .shared
        .authoring_manifest(&id, &actor, &assembly, q.revision.as_ref())
        .await?;
    let etag = format!(
        "W/\"{}\"",
        ContentId::of(&canonical_cbor(&(
            &value,
            &actor,
            s.shared.limits.pieces,
            s.shared.limits.bytes,
            env!("CARGO_PKG_VERSION")
        ))?)
    );
    let unchanged = h
        .get("if-none-match")
        .and_then(|v| v.to_str().ok())
        .is_some_and(|v| v.split(',').any(|s| s.trim() == etag));
    let mut response = if unchanged {
        StatusCode::NOT_MODIFIED.into_response()
    } else if method == axum::http::Method::HEAD {
        StatusCode::OK.into_response()
    } else {
        Json(value.clone()).into_response()
    };
    response.headers_mut().insert("etag", etag.parse().unwrap());
    response
        .headers_mut()
        .insert("cache-control", "private, no-cache".parse().unwrap());
    response.headers_mut().insert(
        "x-ergo-revision",
        value["base_revision"].as_str().unwrap().parse().unwrap(),
    );
    Ok(response)
}
pub(super) async fn page(
    State(s): State<AppState>,
    h: HeaderMap,
    Path((id, assembly)): Path<(String, ContentId)>,
    Query(q): Query<PageQuery>,
) -> SharedResult<Response> {
    let actor = reader(&s, &h)?.required()?;
    let mut response =
        Json(s.shared.authoring_page(&id, &actor, &assembly, q).await?).into_response();
    response
        .headers_mut()
        .insert("cache-control", "private, no-store".parse().unwrap());
    Ok(response)
}

#[derive(Deserialize)]
pub(super) struct MeshQuery {
    revision: ContentId,
    piece: u32,
}
pub(super) async fn mesh(
    State(s): State<AppState>,
    h: HeaderMap,
    Path((id, assembly, mesh)): Path<(String, ContentId, ContentId)>,
    Query(q): Query<MeshQuery>,
) -> SharedResult<Response> {
    let actor = reader(&s, &h)?.required()?;
    let revision = q.revision;
    s.shared
        .head(&id, &Principal { id: Some(actor) }, Some(&revision))
        .await?;
    let snapshot = s
        .shared
        .repo(&id)?
        .accepted_snapshot("main", &revision)
        .await?;
    let local = q.piece;
    let member: paged::MemberRow = snapshot
        .get_limited(&paged::row(&assembly, 1, u64::from(local)), 4096)
        .await?
        .ok_or_else(SharedError::missing)?;
    let piece: Piece = snapshot
        .get_limited(&entity_path(3, &member.piece), 64 * 1024)
        .await?
        .ok_or_else(SharedError::missing)?;
    let belongs = piece.assembly == assembly
        && piece.local_id == local
        && member.local_id == local
        && piece.id == member.piece
        && piece.mesh == mesh;
    ensure!(belongs, "Mesh is not in this assembly");
    let result: ergo_world::geometry::Mesh =
        snapshot.blob_limited(&mesh, s.shared.limits.bytes).await?;
    snapshot.close().await?;
    let mut response = Json(result).into_response();
    response
        .headers_mut()
        .insert("cache-control", "private, no-store".parse().unwrap());
    Ok(response)
}
