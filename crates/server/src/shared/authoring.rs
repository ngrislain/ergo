//! Bounded authoring of one assembly in the authoritative shared world.
//! Replacements preserve creation identities and publish as one validated CAS.
use super::*;
use ergo_world::{SCHEMA_VERSION, WorldExport};

#[derive(Deserialize)]
pub struct ReplaceRequest {
    pub base: ContentId,
    pub scene: WorldExport,
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
        let mut ws = self.working(id, actor, revision).await?;
        let rules = ws.rules(assembly).await?;
        if rules.pieces.len() > ws.piece_limit() {
            return Err(SharedError::working_limit("authoring assembly"));
        }
        let mut bricks = Vec::new();
        let mut meshes = BTreeMap::new();
        for (&local, global) in &rules.pieces {
            let piece = ws.piece(global).await?.ok_or_else(SharedError::missing)?;
            ensure!(
                piece.assembly == *assembly && piece.local_id == local,
                "Assembly mapping mismatch"
            );
            let mesh = ws.mesh(&piece.mesh).await?;
            if let Some(previous) = meshes.insert(mesh.part.clone(), mesh.as_ref().clone()) {
                ensure!(
                    canonical_cbor(&previous)? == canonical_cbor(mesh.as_ref())?,
                    "Assembly contains incompatible meshes for one part ID"
                );
            }
            bricks.push(piece.geometry.brick(local, mesh.as_ref())?);
        }
        let ids = rules.pieces.values().cloned().collect::<BTreeSet<_>>();
        let domain = ws.discover(&ids).await?;
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
            model: source,
        };
        Ok(
            serde_json::json!({"world_id":id,"assembly_id":assembly,"base_revision":revision,
            "scene":scene,"external_dependencies":domain.iter().any(|cid|!ids.contains(cid))}),
        )
    }

    pub async fn replace_assembly(
        &self,
        id: &str,
        actor: &str,
        assembly: &ContentId,
        mut request: ReplaceRequest,
    ) -> SharedResult<Imported> {
        self.destination(id)?;
        ensure!(
            !request.scene.name.trim().is_empty() && request.scene.name.chars().count() <= 120,
            "Construction name must contain 1–120 characters"
        );
        let mut ws = self.working(id, actor, &request.base).await?;
        ensure!(
            canonical_cbor(&request.scene.geo)? == canonical_cbor(&ws.metadata.geo)?,
            "Wrong geographic origin"
        );
        if request.scene.bricks.is_empty() {
            ensure!(
                request.scene.schema == SCHEMA_VERSION
                    && request.scene.joints.is_empty()
                    && request.scene.fixed_bricks.is_empty(),
                "Empty assembly has dangling metadata"
            );
            ergo_world::groups::validate(&request.scene.bricks, &request.scene.groups)?;
        } else {
            crate::import::validate_export(&mut request.scene)?;
        }
        ws.model.measurements.normalize_scene(&mut request.scene)?;
        if request.scene.bricks.len() > ws.piece_limit()
            || canonical_cbor(&request.scene)?.len() > self.limits.bytes
        {
            return Err(SharedError::working_limit("assembly replacement"));
        }
        let original = ws.rules(assembly).await?.as_ref().clone();
        if original.pieces.len() > ws.piece_limit() {
            return Err(SharedError::working_limit("authoring assembly"));
        }
        let seeds = original.pieces.values().cloned().collect::<BTreeSet<_>>();
        let before = ws.discover(&seeds).await?;
        let mut old = BTreeMap::new();
        for (&local, global) in &original.pieces {
            let piece = ws.piece(global).await?.ok_or_else(SharedError::missing)?;
            ensure!(
                piece.assembly == *assembly && piece.local_id == local,
                "Assembly mapping mismatch"
            );
            ws.permit(actor, &piece)?;
            old.insert(local, piece);
        }
        let mut meshes = BTreeMap::new();
        for (name, mesh) in &request.scene.meshes {
            meshes.insert(name.clone(), ws.tx.stage_blob(mesh)?);
        }
        let mut pieces = BTreeMap::new();
        let mut changed = BTreeSet::new();
        let author_name = self.actor_name(actor).await?;
        for brick in &request.scene.bricks {
            ensure!(!pieces.contains_key(&brick.id), "Duplicate local piece ID");
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
                .stage_blob(&ergo_world::assembly::Model::for_scene(&request.scene))?,
        );
        let rules = Rules {
            id: assembly.clone(),
            name: request.scene.name.clone(),
            pieces: pieces.clone(),
            meshes,
            joints: request.scene.joints,
            fixed_bricks: request.scene.fixed_bricks,
            groups: request.scene.groups,
            source_model,
        };
        let rules_changed = canonical_cbor(&original)? != canonical_cbor(&rules)?;
        ensure!(!changed.is_empty() || rules_changed, "No shared changes");
        ws.stage_rules(rules)?;
        ws.flush_rules(&BTreeSet::from([assembly.clone()])).await?;
        ws.reset_dependencies();
        let mut validation_seeds = seeds;
        validation_seeds.extend(before);
        validation_seeds.extend(pieces.values().cloned());
        let affected = ws.discover(&validation_seeds).await?;
        let report = ws.validate(&affected).await?;
        self.advertise_blocks(id, ws.metadata.geo, &ws.new_blocks)
            .await?;
        let prepared = self
            .prepared(
                id,
                actor,
                request.base,
                "Edit shared construction".into(),
                changed,
                affected,
                report,
                ws,
            )
            .await?;
        let accepted = prepared.publish_with(self).await?;
        Ok(Imported {
            accepted,
            assembly: assembly.clone(),
            pieces,
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
) -> SharedResult<Json<serde_json::Value>> {
    let snapshot = snapshot(&s, &id, &h, None).await?;
    let after = q
        .after
        .as_deref()
        .map(str::parse::<OctreePath>)
        .transpose()?;
    let page = snapshot
        .scan_level(
            &root(4),
            87,
            after.as_ref(),
            q.limit.unwrap_or(32).clamp(1, 128),
        )
        .await?;
    let mut items = Vec::new();
    for (path, _) in page.leaves {
        if let Some(header) = snapshot
            .get_limited::<paged::RulesHeader>(&path.child(0)?, s.shared.limits.bytes)
            .await?
        {
            let members = snapshot.scan_page(&path.child(1)?, None, 1).await?;
            if !members.leaves.is_empty() {
                items.push(header);
            }
        }
    }
    Ok(Json(
        serde_json::json!({"items":items,"next":page.next.map(|p|p.to_string()),"revision":snapshot.id()}),
    ))
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
) -> SharedResult<Json<serde_json::Value>> {
    let snapshot = snapshot(&s, &id, &h, None).await?;
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
    Ok(Json(
        serde_json::json!({"world_id":id,"assembly_id":assembly,"revision":snapshot.id(),
        "metadata":metadata,"position_world":ergo_world::coordinates::public(piece.geometry.position).to_array()}),
    ))
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
