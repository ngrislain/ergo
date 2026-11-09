//! Bounded authoring of assemblies in the authoritative shared world.
//! Replacements preserve creation identities and publish as one validated CAS.
use super::*;
use ergo_world::{SCHEMA_VERSION, WorldExport};
use std::sync::Arc;

#[cfg(test)]
#[derive(Deserialize)]
pub struct ReplaceRequest {
    pub base: ContentId,
    pub scene: WorldExport,
}

#[cfg(test)]
#[derive(Deserialize)]
pub struct AssemblyReplacement {
    pub assembly_id: ContentId,
    pub scene: WorldExport,
}
#[cfg(test)]
#[derive(Deserialize)]
pub struct BatchReplaceRequest {
    pub base: ContentId,
    pub assemblies: Vec<AssemblyReplacement>,
}
#[derive(Serialize)]
pub struct ReplacedAssembly {
    pub assembly_id: ContentId,
    pub pieces: BTreeMap<u32, ContentId>,
}
#[derive(Serialize)]
pub struct BatchReplaced {
    pub accepted: Accepted,
    pub assemblies: Vec<ReplacedAssembly>,
}

impl SharedWorlds {
    pub(super) fn destination(&self, id: &str) -> SharedResult<()> {
        if self
            .common_world
            .as_ref()
            .is_some_and(|common| common != id)
        {
            return Err(SharedError::status(
                StatusCode::CONFLICT,
                "Production edits must target the configured common world",
            ));
        }
        Ok(())
    }

    pub async fn authoring_scene(
        &self,
        id: &str,
        actor: &str,
        assembly: &ContentId,
        revision: &ContentId,
    ) -> SharedResult<serde_json::Value> {
        self.read_assembly(id, actor, assembly, revision, false)
            .await
    }

    async fn read_assembly(
        &self,
        id: &str,
        actor: &str,
        assembly: &ContentId,
        revision: &ContentId,
        export_only: bool,
    ) -> SharedResult<serde_json::Value> {
        let started = std::time::Instant::now();
        let mut ws = if export_only {
            // Geometry interchange is read-only, not an authoring or dynamics
            // operation. Retain authorization/byte limits without a body cap or
            // unnecessary traversal into neighboring mechanical dependencies.
            let (repo, _, _) = self
                .authorized(
                    id,
                    &Principal {
                        id: Some(actor.into()),
                    },
                )
                .await?;
            Working::new(
                repo.begin_at("main", revision).await?,
                Limits {
                    pieces: usize::MAX,
                    bytes: self.limits.bytes,
                },
            )
            .await?
        } else {
            self.working(id, actor, revision).await?
        };
        // Reconstruct once, under the ordinary transaction's byte/identity
        // checks. A separate snapshot scan used to walk every member first,
        // then scan the same tree again during rules reconstruction. In a cold
        // 14k-piece assembly that doubles hundreds of thousands of index reads.
        let rules = ws.rules(assembly).await?;
        tracing::info!(assembly = %assembly, pieces = rules.pieces.len(), elapsed_ms = started.elapsed().as_millis(), "Authoring rules reconstructed");
        if rules.pieces.len() > ws.piece_limit() {
            return Err(SharedError::working_limit("authoring assembly"));
        }
        // Speculate only immutable piece records after their authoritative IDs
        // have been checked. Ordinary ws.piece() still charges and validates
        // each record, and failures here cannot mask a missing/corrupt piece.
        let mut piece_ids = Vec::with_capacity(rules.pieces.len().min(1024));
        for id in rules.pieces.values() {
            piece_ids.push(id.clone());
            if piece_ids.len() == 512 {
                ws.prefetch_pieces(std::mem::take(&mut piece_ids)).await;
            }
        }
        ws.prefetch_pieces(piece_ids).await;
        tracing::info!(assembly = %assembly, pieces = rules.pieces.len(), elapsed_ms = started.elapsed().as_millis(), "Authoring pieces prefetched");
        let mut bricks = Vec::new();
        let mut meshes = BTreeMap::new();
        let mut mesh_names = BTreeMap::<ContentId, String>::new();
        let mut export_aliases = false;
        for (&local, global) in &rules.pieces {
            let piece = ws.piece(global).await?.ok_or_else(SharedError::missing)?;
            ensure!(
                piece.assembly == *assembly && piece.local_id == local,
                "Assembly mapping mismatch"
            );
            let mesh = ws.mesh(&piece.mesh).await?;
            let mut brick = piece.geometry.brick(local, mesh.as_ref())?;
            if let Some(name) = mesh_names.get(&piece.mesh) {
                brick.part = name.clone();
            } else if let Some(previous) = meshes.get(&mesh.part) {
                if canonical_cbor(previous)? != canonical_cbor(mesh.as_ref())? {
                    ensure!(
                        export_only,
                        "Assembly contains incompatible meshes for one part ID"
                    );
                    let alias = format!(
                        "{}-ergo-{}.dat",
                        mesh.part.trim_end_matches(".dat"),
                        piece.mesh
                    );
                    let mut variant = mesh.as_ref().clone();
                    variant.part = alias.clone();
                    variant.quality = format!(
                        "{}; export geometry variant of {}",
                        variant.quality, mesh.part
                    );
                    meshes.insert(alias.clone(), variant);
                    brick.part = alias;
                    export_aliases = true;
                }
                mesh_names.insert(piece.mesh.clone(), brick.part.clone());
            } else {
                meshes.insert(mesh.part.clone(), mesh.as_ref().clone());
                mesh_names.insert(piece.mesh.clone(), mesh.part.clone());
            }
            bricks.push(brick);
        }
        tracing::info!(assembly = %assembly, pieces = bricks.len(), elapsed_ms = started.elapsed().as_millis(), "Authoring pieces materialized");
        let ids = rules.pieces.values().cloned().collect::<BTreeSet<_>>();
        let bounds = ergo_world::bounds(&bricks);
        let conservative_dependencies = !export_only
            && bounds.is_some_and(|bounds| bounds.size().max_element() > ergo_world::BLOCK_SIZE);
        let external = if export_only {
            None
        } else if conservative_dependencies {
            // An isolated simulation of a multi-cell assembly cannot prove
            // completeness cheaply: per-member spatial closure can exceed the
            // request lifetime. Treat it as *possibly* dependent, disabling only
            // local simulations. Authoritative writes still validate the full
            // affected domain and are not weakened by this read-only shortcut.
            Some(true)
        } else {
            Some(ws.has_external_dependencies(&ids, bounds).await?)
        };
        tracing::info!(assembly = %assembly, pieces = bricks.len(), external_dependencies = ?external, elapsed_ms = started.elapsed().as_millis(), "Authoring geometry and dependencies loaded");
        let source = if let Some(cid) = &rules.source_model {
            // The hierarchy is an optional editing hint, not the authoritative
            // piece inventory. Read/hash/size failures still propagate, but an
            // unusable hint must not prevent exporting already-canonical pieces.
            // Only the current Model parser is tried: retired constructs are
            // never resolved or expanded. Native export rebuilds from the pieces.
            let value: serde_json::Value = ws.tx.blob_limited(cid, ws.remaining()).await?;
            serde_json::from_value::<ergo_world::assembly::Model>(value)
                .ok()
                .filter(|model| model.matches(&bricks))
        } else {
            None
        };
        let scene = WorldExport {
            schema: SCHEMA_VERSION,
            name: rules.name.clone(),
            geo: ws.metadata.geo,
            bricks,
            meshes,
            joints: rules.joints.clone(),
            fixed_bricks: rules.fixed_bricks.clone(),
            groups: rules.groups.clone(),
            ground: Vec::new(),
            model: if export_aliases { None } else { source },
        };
        Ok(
            serde_json::json!({"world_id":id,"assembly_id":assembly,"base_revision":revision,
            "scene":scene,"external_dependencies":external,"external_dependency_check":if export_only {"not_checked_export"} else if conservative_dependencies {"conservative_large_span"} else {"verified"}}),
        )
    }

    #[cfg(test)]
    pub async fn replace_assembly(
        &self,
        id: &str,
        actor: &str,
        assembly: &ContentId,
        request: ReplaceRequest,
    ) -> SharedResult<Imported> {
        let result = self
            .replace_assemblies(
                id,
                actor,
                BatchReplaceRequest {
                    base: request.base,
                    assemblies: vec![AssemblyReplacement {
                        assembly_id: assembly.clone(),
                        scene: request.scene,
                    }],
                },
            )
            .await?;
        Ok(Imported {
            accepted: result.accepted,
            assembly: assembly.clone(),
            pieces: result
                .assemblies
                .into_iter()
                .next()
                .context("Missing replacement result")?
                .pieces,
        })
    }

    #[cfg(test)]
    pub async fn replace_assemblies(
        &self,
        id: &str,
        actor: &str,
        request: BatchReplaceRequest,
    ) -> SharedResult<BatchReplaced> {
        // Fixture convenience only: all mutations, including whole-model test
        // expectations, run through the production sparse-edit implementation.
        let mut edits = Vec::new();
        for replacement in request.assemblies {
            if let Some(model) = &replacement.scene.model {
                model.validate_isometries()?;
            }
            let old = self
                .authoring_scene(id, actor, &replacement.assembly_id, &request.base)
                .await?;
            let old: WorldExport =
                serde_json::from_value(old["scene"].clone()).map_err(anyhow::Error::from)?;
            let mut ws = self.working(id, actor, &request.base).await?;
            let rules = ws.rules(&replacement.assembly_id).await?;
            let mut versions = BTreeMap::new();
            for (&local, global) in &rules.pieces {
                versions.insert(
                    local,
                    ergo_world::authoring::version(
                        &ws.piece(global).await?.ok_or_else(SharedError::missing)?,
                    )?,
                );
            }
            ws.tx.close().await?;
            edits.push(super::edits::AssemblyEdit {
                assembly_id: replacement.assembly_id,
                delta: ergo_world::authoring::Delta::between(&old, &replacement.scene, &versions)?,
            });
        }
        let mut result = self
            .edit_assemblies(
                id,
                actor,
                super::edits::BatchEditRequest {
                    base: request.base,
                    assemblies: edits,
                },
            )
            .await?;
        for receipt in &mut result.assemblies {
            let mut ws = self.working(id, actor, &result.accepted.revision).await?;
            receipt.pieces = ws.rules(&receipt.assembly_id).await?.pieces.clone();
            ws.tx.close().await?;
        }
        Ok(result)
    }
}

#[derive(Default, Deserialize)]
#[serde(rename_all = "lowercase")]
enum DownloadFormat {
    Ergo,
    #[default]
    Ldraw,
}
#[derive(Default, Deserialize)]
pub(super) struct DownloadQuery {
    #[serde(default)]
    format: DownloadFormat,
    revision: Option<ContentId>,
}
pub(super) async fn download(
    State(s): State<AppState>,
    h: HeaderMap,
    Path((id, assembly)): Path<(String, ContentId)>,
    Query(query): Query<DownloadQuery>,
) -> SharedResult<Response> {
    let actor = reader(&s, &h)?.required()?;
    let head = s
        .shared
        .head(
            &id,
            &Principal {
                id: Some(actor.clone()),
            },
            query.revision.as_ref(),
        )
        .await?;
    let workspace = s
        .shared
        .read_assembly(&id, &actor, &assembly, &head.revision, true)
        .await?;
    let scene: WorldExport =
        serde_json::from_value(workspace["scene"].clone()).context("Invalid authoring scene")?;
    let (bytes, mime, extension) = match query.format {
        DownloadFormat::Ergo => (
            serde_json::to_vec(&ergo_world::storage::Construction::from(&scene))
                .context("Cannot encode construction")?,
            "application/json",
            "ergo.json",
        ),
        DownloadFormat::Ldraw => (
            tokio::task::spawn_blocking(move || ergo_world::exchange::mpd(&scene))
                .await
                .map_err(anyhow::Error::from)??
                .into_bytes(),
            "application/x-ldraw",
            "mpd",
        ),
    };
    let mut response = bytes.into_response();
    response
        .headers_mut()
        .insert("content-type", mime.parse().unwrap());
    response.headers_mut().insert(
        "content-disposition",
        format!("attachment; filename=\"{assembly}.{extension}\"")
            .parse()
            .unwrap(),
    );
    response
        .headers_mut()
        .insert("cache-control", "private, no-store".parse().unwrap());
    response.headers_mut().insert(
        "x-ergo-revision",
        head.revision.to_string().parse().unwrap(),
    );
    Ok(response)
}

#[derive(Default, Deserialize)]
pub(super) struct AssemblyQuery {
    limit: Option<usize>,
    after: Option<String>,
}
pub(super) async fn list(
    State(s): State<AppState>,
    h: HeaderMap,
    Path(id): Path<String>,
    Query(q): Query<AssemblyQuery>,
) -> SharedResult<Response> {
    let snapshot = snapshot(&s, &id, &h, None).await?;
    let after = q
        .after
        .as_deref()
        .map(str::parse::<OctreePath>)
        .transpose()?;
    let limit = q.limit.unwrap_or(32).clamp(1, 128);
    // Catalogue labels belong to this accepted snapshot. Do not expire an
    // unchanged catalogue every minute and force a cold object-store traversal.
    let etag = format!(
        "W/\"catalog-{}\"",
        ContentId::of(&canonical_cbor(&(
            "assembly-list-v4",
            &id,
            snapshot.id(),
            &after,
            limit
        ))?)
    );
    if super::metadata_cache::unchanged(&h, &etag) {
        return Ok(super::metadata_cache::response(None, &etag, "revalidated"));
    }
    if let Some(bytes) = s.shared.read_cache.metadata.lock().unwrap().get(&etag) {
        return Ok(super::metadata_cache::response(
            Some(bytes),
            &etag,
            "memory",
        ));
    }
    let gate = s.shared.read_cache.gate(&etag);
    let _guard = gate.lock().await;
    if let Some(bytes) = s.shared.read_cache.metadata.lock().unwrap().get(&etag) {
        return Ok(super::metadata_cache::response(
            Some(bytes),
            &etag,
            "memory",
        ));
    }
    if let Some(bytes) =
        super::metadata_cache::load(&s.shared.store, &id, snapshot.id(), &etag).await
    {
        s.shared
            .read_cache
            .metadata
            .lock()
            .unwrap()
            .insert(etag.clone(), bytes.clone());
        return Ok(super::metadata_cache::response(
            Some(bytes),
            &etag,
            "persistent",
        ));
    }
    let page = snapshot
        .scan_level_prefetched(&root(4), 87, after.as_ref(), limit)
        .await?;
    use futures::{StreamExt, TryStreamExt};
    let names = tokio::sync::Mutex::new(BTreeMap::<
        String,
        Arc<tokio::sync::OnceCell<Option<String>>>,
    >::new());
    let items: Vec<Option<serde_json::Value>> = futures::stream::iter(page.leaves)
        .map(|(path, _)| {
            let s = &s;
            let snapshot = &snapshot;
            let names = &names;
            async move {
                if let Some(header) = snapshot
                    .get_limited::<paged::RulesHeader>(&path.child(0)?, s.shared.limits.bytes)
                    .await?
                {
                    // Rule-header counts are summaries; individual deletions can
                    // leave the header in place. The live membership probe remains
                    // authoritative for whether this construction is still placed.
                    let populated = !snapshot
                        .scan_page_prefetched(&path.child(1)?, None, 1)
                        .await?
                        .leaves
                        .is_empty();
                    if populated {
                        let creation = if let Some(creation) = &header.creation {
                            Some(creation.clone())
                        } else {
                            // The immutable assembly birth identifies its real creator;
                            // historical births did not record a creation timestamp.
                            let birth: Vec<serde_json::Value> =
                                snapshot.blob_limited(&header.id, 64 * 1024).await?;
                            match birth.first().and_then(|v| v.as_str()) {
                                Some("assembly-birth-v1") => {
                                    let actor = birth
                                        .get(2)
                                        .and_then(|v| v.as_str())
                                        .context("Invalid assembly birth")?;
                                    let name =
                                        names.lock().await.entry(actor.into()).or_default().clone();
                                    Some(Creation {
                                        created_by: actor.into(),
                                        created_by_name: name
                                            .get_or_try_init(|| s.shared.actor_name(actor))
                                            .await?
                                            .clone(),
                                        created_at: None,
                                    })
                                }
                                Some("assembly-birth-v2") => Some(
                                    serde_json::from_value(
                                        birth.get(4).context("Invalid assembly birth")?.clone(),
                                    )
                                    .context("Invalid creation metadata")?,
                                ),
                                _ => None,
                            }
                        };
                        // Discovery must not walk thousands of legacy members
                        // merely to display a size. Missing summaries stay
                        // explicitly unknown until a normal edit records one.
                        let count = header.piece_count;
                        let mut item = serde_json::to_value(header)
                            .context("Cannot encode assembly header")?;
                        item["piece_count"] = serde_json::json!(count);
                        item["created_by"] =
                            serde_json::json!(creation.as_ref().map(|c| &c.created_by));
                        item["created_by_name"] = serde_json::json!(
                            creation.as_ref().and_then(|c| c.created_by_name.as_deref())
                        );
                        item["created_at"] =
                            serde_json::json!(creation.as_ref().and_then(|c| c.created_at));
                        return Ok(Some(item));
                    }
                }
                Ok::<_, SharedError>(None)
            }
        })
        .buffered(16)
        .try_collect()
        .await?;
    let items: Vec<_> = items.into_iter().flatten().collect();
    let bytes = bytes::Bytes::from(serde_json::to_vec(
        &serde_json::json!({"items":items,"next":page.next.map(|p|p.to_string()),"revision":snapshot.id()}),
    ).context("Cannot encode construction list")?);
    s.shared
        .read_cache
        .metadata
        .lock()
        .unwrap()
        .insert(etag.clone(), bytes.clone());
    super::metadata_cache::persist(&s.shared.store, &id, snapshot.id(), &etag, &bytes).await;
    Ok(super::metadata_cache::response(Some(bytes), &etag, "miss"))
}

#[derive(Default, Deserialize)]
pub(super) struct LocateQuery {
    #[serde(default)]
    x: f64,
    #[serde(default)]
    y: f64,
    #[serde(default)]
    z: f64,
}

/// A bounded location hint from one current member, not an isolated model export.
pub(super) async fn location(
    State(s): State<AppState>,
    h: HeaderMap,
    Path((id, assembly)): Path<(String, ContentId)>,
) -> SharedResult<Response> {
    let snapshot = snapshot(&s, &id, &h, None).await?;
    let etag = format!(
        "W/\"location-{}\"",
        ContentId::of(&canonical_cbor(&(
            "assembly-location-v1",
            &id,
            &assembly,
            snapshot.id()
        ))?)
    );
    if super::metadata_cache::unchanged(&h, &etag) {
        return Ok(super::metadata_cache::response(None, &etag, "revalidated"));
    }
    if let Some(bytes) = s.shared.read_cache.metadata.lock().unwrap().get(&etag) {
        return Ok(super::metadata_cache::response(
            Some(bytes),
            &etag,
            "memory",
        ));
    }
    let metadata: Metadata = snapshot
        .get_limited(&root(2), 64 * 1024)
        .await?
        .context("Missing world metadata")?;
    let member = {
        let page = snapshot
            .scan_page(&paged::rule_path(&assembly, 1), None, 1)
            .await?;
        let (_, cid) = page
            .leaves
            .first()
            .context("Construction contains no placed pieces")?;
        let row: paged::MemberRow = snapshot.blob_limited(cid, 64 * 1024).await?;
        row.piece
    };
    let piece: Piece = snapshot
        .get_limited(&entity_path(3, &member), 64 * 1024)
        .await?
        .context("Missing construction member")?;
    ensure!(piece.assembly == assembly, "Construction member mismatch");
    let bytes = bytes::Bytes::from(serde_json::to_vec(
        &serde_json::json!({"world_id":id,"assembly_id":assembly,"revision":snapshot.id(),
        "metadata":metadata,"position_world":ergo_world::coordinates::public(piece.geometry.position).to_array()}),
    ).context("Cannot encode construction location")?);
    s.shared
        .read_cache
        .metadata
        .lock()
        .unwrap()
        .insert(etag.clone(), bytes.clone());
    Ok(super::metadata_cache::response(Some(bytes), &etag, "miss"))
}
pub(super) async fn locate(
    State(s): State<AppState>,
    h: HeaderMap,
    Query(q): Query<LocateQuery>,
) -> SharedResult<Json<serde_json::Value>> {
    let id = s
        .shared
        .common_world
        .as_deref()
        .context("No common world is configured")?;
    let actor = reader(&s, &h)?.required()?;
    let head = s
        .shared
        .head(
            id,
            &Principal {
                id: Some(actor.clone()),
            },
            None,
        )
        .await?;
    let position = ergo_world::coordinates::internal(glam::DVec3::new(q.x, q.y, q.z));
    ensure!(
        position.is_finite() && position.abs().max_element() <= ergo_world::MAX_WORLD_COORD,
        "Invalid world position"
    );
    let mut ws = s.shared.working(id, &actor, &head.revision).await?;
    let bounds = ergo_world::Aabb::new(
        position - glam::DVec3::splat(1536.),
        position + glam::DVec3::splat(1536.),
    );
    let mut nearest: Option<(f64, ContentId)> = None;
    for cid in ws.neighbors(bounds).await? {
        if let Some(piece) = ws.piece(&cid).await? {
            let brick = ws.brick(&piece, 1).await?;
            let point = position.clamp(brick.bounds.min, brick.bounds.max);
            let distance = ergo_world::physical::to_meters(position - point).length_squared();
            if nearest.as_ref().is_none_or(|(old, _)| distance < *old) {
                nearest = Some((distance, piece.assembly));
            }
        }
    }
    Ok(Json(
        serde_json::json!({"world_id":id,"base_revision":head.revision,"geo":head.metadata.geo,
        "assembly_id":nearest.map(|(_,id)|id),"position_world":[q.x,q.y,q.z],"search_radius_world":1536}),
    ))
}
