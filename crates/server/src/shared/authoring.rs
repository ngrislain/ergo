//! Bounded authoring of assemblies in the authoritative shared world.
//! Replacements preserve creation identities and publish as one validated CAS.
use super::*;
use ergo_world::{SCHEMA_VERSION, WorldExport};
use futures::{StreamExt, TryStreamExt};
use std::sync::Arc;

#[derive(Deserialize)]
pub struct ReplaceRequest {
    pub base: ContentId,
    pub scene: WorldExport,
}

#[derive(Deserialize)]
pub struct AssemblyReplacement {
    pub assembly_id: ContentId,
    pub scene: WorldExport,
}
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
        // Reopening a cold assembly must not serialize one object-store round
        // trip per member and then another per piece. Warm only this accepted
        // assembly's bounded records; Working still performs and charges every
        // ordinary read below, with its original dependency/identity checks.
        let snapshot = self.repo(id)?.accepted_snapshot("main", revision).await?;
        let header: paged::RulesHeader = snapshot
            .get_limited(&paged::rule_path(assembly, 0), ws.remaining())
            .await?
            .context("Missing paged assembly header")?;
        if header
            .piece_count
            .is_some_and(|count| count > ws.piece_limit())
        {
            return Err(SharedError::working_limit("authoring assembly"));
        }
        let mut after = None;
        let mut warm_bytes = ws.remaining();
        let mut count = 0;
        loop {
            let page = snapshot
                .scan_page_prefetched(&paged::rule_path(assembly, 1), after.as_ref(), 512)
                .await?;
            let limit = warm_bytes;
            let mut records = futures::stream::iter(page.leaves)
                .map(|(_, cid)| {
                    let snapshot = &snapshot;
                    async move {
                        let member: paged::MemberRow = snapshot.blob_limited(&cid, limit).await?;
                        let piece: Piece = snapshot
                            .get_limited(&entity_path(3, &member.piece), limit)
                            .await?
                            .context("Missing assembly piece")?;
                        Ok::<_, SharedError>(
                            canonical_cbor(&member)?.len() + canonical_cbor(&piece)?.len(),
                        )
                    }
                })
                .buffered(READ_CONCURRENCY);
            while let Some(bytes) = records.try_next().await? {
                count += 1;
                if count > ws.piece_limit() {
                    return Err(SharedError::working_limit("authoring assembly"));
                }
                warm_bytes = warm_bytes
                    .checked_sub(bytes)
                    .ok_or_else(|| SharedError::working_limit("CBOR byte budget"))?;
            }
            after = page.next;
            if after.is_none() {
                break;
            }
        }
        let rules = ws.rules(assembly).await?;
        if rules.pieces.len() > ws.piece_limit() {
            return Err(SharedError::working_limit("authoring assembly"));
        }
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
        let ids = rules.pieces.values().cloned().collect::<BTreeSet<_>>();
        let external = if export_only {
            None
        } else {
            Some(
                ws.has_external_dependencies(&ids, ergo_world::bounds(&bricks))
                    .await?,
            )
        };
        let source = if let Some(cid) = &rules.source_model {
            let model: ergo_world::assembly::Model =
                ws.tx.blob_limited(cid, ws.remaining()).await?;
            Some(model)
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
            "scene":scene,"external_dependencies":external}),
        )
    }

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

    pub async fn replace_assemblies(
        &self,
        id: &str,
        actor: &str,
        mut request: BatchReplaceRequest,
    ) -> SharedResult<BatchReplaced> {
        self.destination(id)?;
        ensure!(
            !request.assemblies.is_empty() && request.assemblies.len() <= 32,
            "Replace 1–32 assemblies in one request"
        );
        let assemblies = request
            .assemblies
            .iter()
            .map(|a| a.assembly_id.clone())
            .collect::<BTreeSet<_>>();
        ensure!(
            assemblies.len() == request.assemblies.len(),
            "Duplicate assembly replacement"
        );
        let mut ws = self.working(id, actor, &request.base).await?;
        let mut input_bytes = 0usize;
        let mut input_pieces = 0usize;
        let mut old_pieces = 0usize;
        let mut originals = Vec::new();
        let mut seeds = BTreeSet::new();
        for replacement in &mut request.assemblies {
            let scene = &mut replacement.scene;
            ensure!(
                !scene.name.trim().is_empty() && scene.name.chars().count() <= 120,
                "Construction name must contain 1–120 characters"
            );
            ensure!(
                canonical_cbor(&scene.geo)? == canonical_cbor(&ws.metadata.geo)?,
                "Wrong geographic origin"
            );
            if scene.bricks.is_empty() {
                ensure!(
                    scene.schema == SCHEMA_VERSION
                        && scene.joints.is_empty()
                        && scene.fixed_bricks.is_empty(),
                    "Empty assembly has dangling metadata"
                );
                ergo_world::groups::validate(&scene.bricks, &scene.groups)?;
            } else {
                crate::import::validate_export(scene)?;
            }
            ws.model.measurements.normalize_scene(scene)?;
            if let Some(model) = &scene.model {
                model.validate_isometries()?;
            }
            input_bytes += canonical_cbor(scene)?.len();
            input_pieces += scene.bricks.len();
            let original = ws.rules(&replacement.assembly_id).await?.as_ref().clone();
            old_pieces += original.pieces.len();
            if input_bytes > self.limits.bytes
                || input_pieces > ws.piece_limit()
                || old_pieces > ws.piece_limit()
            {
                return Err(SharedError::working_limit("assembly replacement batch"));
            }
            seeds.extend(original.pieces.values().cloned());
            originals.push(original);
        }
        // Discover every old dependency before staging any replacement. Only
        // the final multi-assembly state is validated and published.
        let before = ws.discover(&seeds).await?;
        let mut changed = BTreeSet::new();
        let mut results = Vec::new();
        let mut rules_changed = false;
        let author_name = self.actor_name(actor).await?;
        for (replacement, original) in request.assemblies.into_iter().zip(originals) {
            let AssemblyReplacement {
                assembly_id: assembly,
                scene,
            } = replacement;
            let mut old = BTreeMap::new();
            for (&local, global) in &original.pieces {
                let piece = ws.piece(global).await?.ok_or_else(SharedError::missing)?;
                ensure!(
                    piece.assembly == assembly && piece.local_id == local,
                    "Assembly mapping mismatch"
                );
                ws.permit(actor, &piece)?;
                old.insert(local, piece);
            }
            let mut meshes = BTreeMap::new();
            for (name, mesh) in &scene.meshes {
                meshes.insert(name.clone(), ws.tx.stage_blob(mesh)?);
            }
            let mut pieces = BTreeMap::new();
            for brick in &scene.bricks {
                ensure!(
                    brick.id > 0 && !pieces.contains_key(&brick.id),
                    "Duplicate or zero local piece ID"
                );
                let mesh = meshes
                    .get(&brick.part)
                    .context("Missing part mesh")?
                    .clone();
                let geometry = Geometry::of(brick);
                let previous = old.remove(&brick.id);
                let mut piece = if let Some(previous) = &previous {
                    previous.clone()
                } else {
                    let birth = Birth {
                        nonce: uuid::Uuid::new_v4().to_string(),
                        created_by: actor.into(),
                        created_by_name: author_name.clone(),
                        assembly: assembly.clone(),
                        initial: geometry.clone(),
                        mesh: mesh.clone(),
                    };
                    Piece {
                        id: ws.tx.stage_blob(&birth)?,
                        assembly: assembly.clone(),
                        local_id: brick.id,
                        geometry: geometry.clone(),
                        mesh: mesh.clone(),
                        replicas: BTreeSet::new(),
                    }
                };
                piece.geometry = geometry;
                piece.mesh = mesh;
                pieces.insert(brick.id, piece.id.clone());
                if let Some(old) = &previous {
                    if canonical_cbor(&old.geometry)? == canonical_cbor(&piece.geometry)?
                        && old.mesh == piece.mesh
                    {
                        continue;
                    }
                }
                piece.replicas = ws.cells(&piece).await?;
                ws.permit(actor, &piece)?;
                if let Some(previous) = &previous {
                    ws.swept(previous, &piece).await?;
                }
                changed.insert(piece.id.clone());
                ws.replace(previous.as_ref(), Some(piece)).await?;
            }
            for previous in old.values() {
                ws.no_external(&previous.id).await?;
                changed.insert(previous.id.clone());
                ws.replace(Some(previous), None).await?;
            }
            let source_model = Some(
                ws.tx
                    .stage_blob(&ergo_world::assembly::Model::for_scene(&scene))?,
            );
            let rules = Rules {
                id: assembly.clone(),
                name: scene.name,
                creation: original.creation.clone(),
                pieces: pieces.clone(),
                meshes,
                joints: scene.joints,
                fixed_bricks: scene.fixed_bricks,
                groups: scene.groups,
                source_model,
            };
            rules_changed |= canonical_cbor(&original)? != canonical_cbor(&rules)?;
            ws.stage_rules(rules)?;
            seeds.extend(pieces.values().cloned());
            results.push(ReplacedAssembly {
                assembly_id: assembly,
                pieces,
            });
        }
        ensure!(!changed.is_empty() || rules_changed, "No shared changes");
        ws.flush_rules(&assemblies).await?;
        ws.reset_dependencies();
        let mut validation_seeds = seeds;
        validation_seeds.extend(before);
        let affected = ws.discover(&validation_seeds).await?;
        let report = ws.validate(&affected).await?;
        self.advertise_blocks(id, ws.metadata.geo, &ws.new_blocks)
            .await?;
        let prepared = self
            .prepared(
                id,
                actor,
                request.base,
                if results.len() == 1 {
                    "Edit shared construction"
                } else {
                    "Replace shared constructions atomically"
                }
                .into(),
                changed,
                affected,
                report,
                ws,
            )
            .await?;
        let accepted = prepared.publish_with(self).await?;
        Ok(BatchReplaced {
            accepted,
            assemblies: results,
        })
    }
}

pub(super) async fn workspace(
    State(s): State<AppState>,
    h: HeaderMap,
    Path((id, assembly)): Path<(String, ContentId)>,
    Query(q): Query<RevisionQuery>,
) -> SharedResult<Json<serde_json::Value>> {
    let actor = reader(&s, &h)?.required()?;
    let head = s
        .shared
        .head(
            &id,
            &Principal {
                id: Some(actor.clone()),
            },
            q.revision.as_ref(),
        )
        .await?;
    Ok(Json(
        s.shared
            .authoring_scene(&id, &actor, &assembly, &head.revision)
            .await?,
    ))
}
pub(super) async fn replace(
    State(s): State<AppState>,
    h: HeaderMap,
    Path((id, assembly)): Path<(String, ContentId)>,
    Json(body): Json<ReplaceRequest>,
) -> SharedResult<Json<Imported>> {
    Ok(Json(
        s.shared
            .replace_assembly(&id, &editor(&s, &h)?, &assembly, body)
            .await?,
    ))
}

pub(super) async fn replace_batch(
    State(s): State<AppState>,
    h: HeaderMap,
    Path(id): Path<String>,
    Json(body): Json<BatchReplaceRequest>,
) -> SharedResult<Json<BatchReplaced>> {
    Ok(Json(
        s.shared
            .replace_assemblies(&id, &editor(&s, &h)?, body)
            .await?,
    ))
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
    // Snapshot identity invalidates geometry/membership metadata immediately;
    // legacy profile-name lookups are revalidated at most 60 seconds later.
    let minute = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
        / 60;
    let etag = format!(
        "W/\"catalog-{}\"",
        ContentId::of(&canonical_cbor(&(
            "assembly-list-v3",
            &id,
            snapshot.id(),
            &after,
            limit,
            minute
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
