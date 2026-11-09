//! One sparse authoring mutation pipeline. Unloaded members are never inferred
//! to be deletions. The ordinary old/new closure and publication gate remain.
use super::*;
use ergo_world::{WorldExport, authoring::Delta};

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EditRequest {
    pub base: ContentId,
    pub delta: Delta,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AssemblyEdit {
    pub assembly_id: ContentId,
    pub delta: Delta,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BatchEditRequest {
    pub base: ContentId,
    pub assemblies: Vec<AssemblyEdit>,
}

impl SharedWorlds {
    pub async fn edit_assembly(
        &self,
        id: &str,
        actor: &str,
        assembly: &ContentId,
        request: EditRequest,
    ) -> SharedResult<Imported> {
        let result = self
            .edit_assemblies(
                id,
                actor,
                BatchEditRequest {
                    base: request.base,
                    assemblies: vec![AssemblyEdit {
                        assembly_id: assembly.clone(),
                        delta: request.delta,
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
                .context("Missing edit receipt")?
                .pieces,
        })
    }

    pub async fn edit_assemblies(
        &self,
        id: &str,
        actor: &str,
        request: BatchEditRequest,
    ) -> SharedResult<authoring::BatchReplaced> {
        self.destination(id)?;
        ensure!(
            !request.assemblies.is_empty() && request.assemblies.len() <= 32,
            "Edit 1–32 assemblies"
        );
        let mut ws = self.working(id, actor, &request.base).await?;
        let mut assemblies = BTreeSet::new();
        let mut originals = Vec::new();
        let mut seeds = BTreeSet::new();
        let mut input_bytes = 0usize;
        let mut count = 0usize;
        for edit in &request.assemblies {
            ensure!(
                assemblies.insert(edit.assembly_id.clone()),
                "Duplicate assembly edit"
            );
            edit.delta.validate()?;
            input_bytes = input_bytes
                .checked_add(canonical_cbor(&edit.delta)?.len())
                .context("Edit size overflow")?;
            count += edit.delta.upsert.len() + edit.delta.remove.len();
            if input_bytes > self.limits.bytes || count > ws.piece_limit() {
                return Err(SharedError::working_limit("edit batch"));
            }
            let original = ws.rules(&edit.assembly_id).await?;
            let metadata = edit.delta.name.is_some()
                || !edit.delta.groups.is_empty()
                || !edit.delta.remove_groups.is_empty()
                || !edit.delta.joints.is_empty()
                || !edit.delta.remove_joints.is_empty()
                || !edit.delta.fixed_add.is_empty()
                || !edit.delta.fixed_remove.is_empty();
            if metadata && !ws.policy.authenticated_editing && actor != ws.policy.owner {
                if original.pieces.is_empty() {
                    return Err(SharedError::forbidden());
                }
                for global in original.pieces.values() {
                    let piece = ws.piece(global).await?.ok_or_else(SharedError::missing)?;
                    ws.permit(actor, &piece)?;
                }
            }
            for group in edit
                .delta
                .groups
                .iter()
                .map(|g| g.id)
                .chain(edit.delta.remove_groups.iter().copied())
            {
                if let Some(old) = original.groups.iter().find(|g| g.id == group) {
                    ensure!(
                        edit.delta.expected_groups.get(&group)
                            == Some(&ergo_world::authoring::group_version(old)?),
                        "Group changed or was not loaded; inspect its complete membership first"
                    );
                } else {
                    ensure!(
                        !edit.delta.remove_groups.contains(&group)
                            && !edit.delta.expected_groups.contains_key(&group),
                        "Missing group"
                    );
                }
            }
            if original
                .pieces
                .len()
                .saturating_sub(edit.delta.remove.len())
                .saturating_add(
                    edit.delta
                        .upsert
                        .iter()
                        .filter(|b| !original.pieces.contains_key(&b.id))
                        .count(),
                )
                > ws.piece_limit()
            {
                return Err(SharedError::working_limit("authoring assembly"));
            }
            for local in edit
                .delta
                .upsert
                .iter()
                .map(|b| b.id)
                .chain(edit.delta.remove.iter().copied())
            {
                if let Some(global) = original.pieces.get(&local) {
                    let piece = ws.piece(global).await?.ok_or_else(SharedError::missing)?;
                    ws.permit(actor, &piece)?;
                    let version = ergo_world::authoring::version(&piece)?;
                    ensure!(
                        edit.delta.expected.get(&local) == Some(&version),
                        "Piece changed or missing precondition; reload authoring page"
                    );
                    seeds.insert(global.clone());
                } else {
                    ensure!(
                        !edit.delta.expected.contains_key(&local)
                            && !edit.delta.remove.contains(&local),
                        "Piece precondition failed"
                    );
                }
            }
            // Rule edits can affect members outside the loaded client page.
            if !edit.delta.joints.is_empty()
                || !edit.delta.remove_joints.is_empty()
                || !edit.delta.groups.is_empty()
                || !edit.delta.remove_groups.is_empty()
                || !edit.delta.fixed_add.is_empty()
                || !edit.delta.fixed_remove.is_empty()
            {
                seeds.extend(rule_references(&original)?);
            }
            if edit.delta.name.is_some() && seeds.is_empty() {
                if let Some(global) = original.pieces.values().next() {
                    let piece = ws.piece(global).await?.ok_or_else(SharedError::missing)?;
                    ws.permit(actor, &piece)?;
                } else {
                    ensure!(
                        actor == ws.policy.owner,
                        "Empty assembly rename requires its owner"
                    );
                }
            }
            originals.push(original);
        }
        for global in &seeds {
            if let Some(piece) = ws.piece(global).await? {
                ws.permit(actor, &piece)?;
            }
        }
        let before = ws.discover(&seeds).await?;
        let mut changed = BTreeSet::new();
        let mut results = Vec::new();
        let actor_name = self.actor_name(actor).await?;
        let mut any = false;
        for (edit, original) in request.assemblies.into_iter().zip(originals) {
            let assembly = edit.assembly_id;
            let delta = edit.delta;
            let mut rules = original.as_ref().clone();
            let mut scene = WorldExport {
                schema: ergo_world::SCHEMA_VERSION,
                name: rules.name.clone(),
                geo: ws.metadata.geo,
                bricks: delta.upsert,
                meshes: delta.meshes,
                joints: vec![],
                fixed_bricks: vec![],
                ground: vec![],
                groups: vec![],
                model: None,
            };
            for brick in &scene.bricks {
                if !scene.meshes.contains_key(&brick.part) {
                    let cid = rules.meshes.get(&brick.part).context("Missing part mesh")?;
                    scene
                        .meshes
                        .insert(brick.part.clone(), ws.mesh(cid).await?.as_ref().clone());
                }
            }
            if !scene.bricks.is_empty() {
                crate::import::validate_export(&mut scene)?;
                ws.model.measurements.normalize_scene(&mut scene)?;
            }
            for (name, mesh) in &scene.meshes {
                rules.meshes.insert(name.clone(), ws.tx.stage_blob(mesh)?);
            }
            let mut receipt = BTreeMap::new();
            for brick in &scene.bricks {
                let mesh = rules
                    .meshes
                    .get(&brick.part)
                    .context("Missing part mesh")?
                    .clone();
                let old = if let Some(global) = original.pieces.get(&brick.id) {
                    Some(ws.piece(global).await?.ok_or_else(SharedError::missing)?)
                } else {
                    None
                };
                let mut piece = if let Some(old) = &old {
                    old.clone()
                } else {
                    let birth = Birth {
                        nonce: uuid::Uuid::new_v4().to_string(),
                        created_by: actor.into(),
                        created_by_name: actor_name.clone(),
                        assembly: assembly.clone(),
                        initial: Geometry::of(brick),
                        mesh: mesh.clone(),
                    };
                    Piece {
                        id: ws.tx.stage_blob(&birth)?,
                        assembly: assembly.clone(),
                        local_id: brick.id,
                        geometry: Geometry::of(brick),
                        mesh: mesh.clone(),
                        replicas: BTreeSet::new(),
                    }
                };
                piece.geometry = Geometry::of(brick);
                piece.mesh = mesh;
                piece.replicas = ws.cells(&piece).await?;
                ws.permit(actor, &piece)?;
                if let Some(old) = &old {
                    ws.swept(old, &piece).await?;
                }
                rules.pieces.insert(brick.id, piece.id.clone());
                receipt.insert(brick.id, piece.id.clone());
                changed.insert(piece.id.clone());
                ws.replace(old.as_ref(), Some(piece)).await?;
                any = true;
            }
            for local in delta.remove {
                let global = rules
                    .pieces
                    .remove(&local)
                    .context("Missing removed member")?;
                let old = ws.piece(&global).await?.ok_or_else(SharedError::missing)?;
                ws.no_external(&global).await?;
                changed.insert(global);
                ws.replace(Some(&old), None).await?;
                rules.joints.retain(|j| j.a != local && j.b != local);
                rules.fixed_bricks.retain(|id| *id != local);
                for g in &mut rules.groups {
                    g.members.retain(
                        |m| !matches!(m,ergo_world::groups::Member::Piece{id} if *id==local),
                    );
                }
                any = true;
            }
            let pair = |j: &ergo_world::JointSpec| [j.a.min(j.b), j.a.max(j.b)];
            for key in delta.remove_joints {
                rules
                    .joints
                    .retain(|j| pair(j) != [key[0].min(key[1]), key[0].max(key[1])]);
                any = true;
            }
            for j in delta.joints {
                rules.joints.retain(|old| pair(old) != pair(&j));
                rules.joints.push(j);
                any = true;
            }
            for local in delta.fixed_remove {
                rules.fixed_bricks.retain(|id| *id != local);
                any = true;
            }
            for local in delta.fixed_add {
                if !rules.fixed_bricks.contains(&local) {
                    rules.fixed_bricks.push(local);
                }
                any = true;
            }
            for id in delta.remove_groups {
                let children = rules
                    .groups
                    .iter()
                    .find(|g| g.id == id)
                    .context("Missing removed group")?
                    .members
                    .clone();
                rules.groups.retain(|g| g.id != id);
                // Dissolve authoring structure without losing an unloaded
                // ancestor: splice children in the authoritative parent too.
                for parent in &mut rules.groups {
                    let mut members = Vec::new();
                    for member in &parent.members {
                        if *member == (ergo_world::groups::Member::Group { id }) {
                            members.extend(children.iter().copied());
                        } else {
                            members.push(*member);
                        }
                    }
                    parent.members = members;
                }
                any = true;
            }
            for g in delta.groups {
                rules.groups.retain(|old| old.id != g.id);
                rules.groups.push(g);
                any = true;
            }
            if let Some(name) = delta.name {
                any |= rules.name != name;
                rules.name = name;
            }
            rules.validate()?;
            let referenced = rule_references(&rules)?;
            for global in &referenced {
                let piece = ws.piece(global).await?.ok_or_else(SharedError::missing)?;
                ws.permit(actor, &piece)?;
            }
            seeds.extend(referenced);
            // Live records, not an optional stale full-model hint, are the
            // authoritative geometry after a sparse edit.
            rules.source_model = None;
            ws.stage_rules(rules)?;
            results.push(authoring::ReplacedAssembly {
                assembly_id: assembly,
                pieces: receipt,
            });
        }
        ensure!(any, "No shared changes");
        ws.flush_rules(&assemblies).await?;
        ws.reset_dependencies()?;
        seeds.extend(before);
        seeds.extend(changed.iter().cloned());
        let affected = ws.discover(&seeds).await?;
        let report = ws.validate(&affected).await?;
        self.advertise_blocks(id, ws.metadata.geo, &ws.new_blocks)
            .await?;
        let accepted = self
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
            .await?
            .publish_with(self)
            .await?;
        Ok(authoring::BatchReplaced {
            accepted,
            assemblies: results,
        })
    }
}

pub(super) async fn edit(
    State(s): State<AppState>,
    h: HeaderMap,
    Path((id, assembly)): Path<(String, ContentId)>,
    Json(body): Json<EditRequest>,
) -> SharedResult<Json<Imported>> {
    Ok(Json(
        s.shared
            .edit_assembly(&id, &editor(&s, &h)?, &assembly, body)
            .await?,
    ))
}
pub(super) async fn batch(
    State(s): State<AppState>,
    h: HeaderMap,
    Path(id): Path<String>,
    Json(body): Json<BatchEditRequest>,
) -> SharedResult<Json<authoring::BatchReplaced>> {
    Ok(Json(
        s.shared
            .edit_assemblies(&id, &editor(&s, &h)?, body)
            .await?,
    ))
}
