use super::{SharedError, SharedResult, address::entity_path, model::*};
use anyhow::Context;
use ergo_transactions::{ContentId, OctreePath, Transaction, canonical_cbor};
use ergo_world::{Aabb, BlockCoord, WorldExport, constraints::INFERRED_JOINT_HALO, geometry::Mesh};
use serde::{Serialize, de::DeserializeOwned};
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
};

pub struct Working {
    pub tx: Transaction,
    pub metadata: Metadata,
    pub policy: Policy,
    pub policy_cid: ContentId,
    pub model: PhysicalModel,
    pub pieces: BTreeMap<ContentId, Option<Piece>>,
    pub rules: BTreeMap<ContentId, Arc<Rules>>,
    graphs: BTreeMap<ContentId, BTreeMap<ContentId, BTreeSet<ContentId>>>,
    headers: BTreeMap<ContentId, Arc<super::paged::RulesHeader>>,
    joints: BTreeMap<ContentId, super::paged::BoundJoint>,
    fixed: BTreeSet<ContentId>,
    meshes: BTreeMap<ContentId, Arc<Mesh>>,
    spatial_refs: BTreeMap<ContentId, Arc<super::spatial::SpatialRef>>,
    pub regions: BTreeSet<OctreePath>,
    pub new_blocks: BTreeSet<BlockCoord>,
    pub loaded_bytes: usize,
    limits: Limits,
}
impl Working {
    pub(super) fn add_joint(&mut self, id: ContentId, joint: super::paged::BoundJoint) {
        self.joints.insert(id, joint);
    }
    pub fn reset_dependencies(&mut self) {
        self.joints.clear();
        self.fixed.clear();
    }
    pub async fn new(mut tx: Transaction, limits: Limits) -> SharedResult<Self> {
        let policy: Policy = tx
            .get_limited(&super::address::root(0), limits.bytes)
            .await?
            .context("Missing shared policy")?;
        let policy_cid = tx
            .value_id(&super::address::root(0))
            .await?
            .context("Missing policy CID")?;
        let metadata: Metadata = tx
            .get_limited(&super::address::root(2), limits.bytes)
            .await?
            .context("Missing shared metadata")?;
        let model: PhysicalModel = tx.blob_limited(&metadata.model, limits.bytes).await?;
        ensure!(
            model.algorithm == 1,
            "Unsupported frozen physics model version"
        );
        model.measurements.validate()?;
        model.tolerances.validate()?;
        let mut result = Self {
            tx,
            metadata,
            policy,
            policy_cid,
            model,
            pieces: BTreeMap::new(),
            rules: BTreeMap::new(),
            graphs: BTreeMap::new(),
            headers: BTreeMap::new(),
            joints: BTreeMap::new(),
            fixed: BTreeSet::new(),
            meshes: BTreeMap::new(),
            spatial_refs: BTreeMap::new(),
            regions: BTreeSet::new(),
            new_blocks: BTreeSet::new(),
            loaded_bytes: 0,
            limits,
        };
        result.charge(&result.policy.clone())?;
        result.charge(&result.metadata.clone())?;
        result.charge(&result.model.clone())?;
        Ok(result)
    }
    pub(super) fn remaining(&self) -> usize {
        self.limits.bytes.saturating_sub(self.loaded_bytes)
    }
    pub(super) fn piece_limit(&self) -> usize {
        self.limits.pieces
    }
    pub(super) async fn spatial_ref(
        &mut self,
        id: &ContentId,
    ) -> SharedResult<Arc<super::spatial::SpatialRef>> {
        if let Some(value) = self.spatial_refs.get(id) {
            return Ok(value.clone());
        }
        let value: super::spatial::SpatialRef = self.tx.blob_limited(id, self.remaining()).await?;
        self.charge(&value)?;
        let value = Arc::new(value);
        self.spatial_refs.insert(id.clone(), value.clone());
        Ok(value)
    }
    pub fn charge<T: Serialize>(&mut self, value: &T) -> SharedResult<()> {
        let bytes = canonical_cbor(value)?.len();
        if bytes > self.remaining() {
            return Err(SharedError::working_limit("CBOR byte budget"));
        }
        self.loaded_bytes += bytes;
        Ok(())
    }
    pub(super) async fn read<T: DeserializeOwned + Serialize>(
        &mut self,
        path: &OctreePath,
    ) -> SharedResult<Option<T>> {
        let value: Option<T> = self.tx.get_limited(path, self.remaining()).await?;
        if let Some(value) = &value {
            self.charge(value)?;
        }
        Ok(value)
    }
    pub async fn piece(&mut self, id: &ContentId) -> SharedResult<Option<Piece>> {
        if let Some(piece) = self.pieces.get(id) {
            return Ok(piece.clone());
        }
        if self.pieces.len() >= self.limits.pieces {
            return Err(SharedError::working_limit("piece/dependency budget"));
        }
        let piece: Option<Piece> = self.read(&entity_path(3, id)).await?;
        if let Some(piece) = &piece {
            ensure!(piece.id == *id, "Stored piece identity mismatch");
        }
        self.pieces.insert(id.clone(), piece.clone());
        Ok(piece)
    }
    pub async fn rules(&mut self, id: &ContentId) -> SharedResult<Arc<Rules>> {
        if let Some(rules) = self.rules.get(id) {
            return Ok(rules.clone());
        }
        let header = self
            .tx
            .get_limited::<super::paged::RulesHeader>(
                &super::paged::rule_path(id, 0),
                self.remaining(),
            )
            .await?
            .context("Missing paged assembly header")?;
        self.charge(&header)?;
        let rules = super::paged::reconstruct(self, &header).await?;
        ensure!(rules.id == *id, "Stored assembly identity mismatch");
        rules.validate()?;
        self.graphs.insert(id.clone(), rules.adjacency()?);
        let rules = Arc::new(rules);
        self.rules.insert(id.clone(), rules.clone());
        Ok(rules)
    }
    pub fn stage_rules(&mut self, rules: Rules) -> SharedResult<()> {
        self.charge(&rules)?;
        rules.validate()?;
        self.graphs.insert(rules.id.clone(), rules.adjacency()?);
        self.headers.insert(
            rules.id.clone(),
            Arc::new(super::paged::RulesHeader::of(&rules)),
        );
        self.rules.insert(rules.id.clone(), Arc::new(rules));
        Ok(())
    }
    pub async fn assembly(
        &mut self,
        id: &ContentId,
    ) -> SharedResult<Arc<super::paged::RulesHeader>> {
        if let Some(h) = self.headers.get(id) {
            return Ok(h.clone());
        }
        let header = self
            .tx
            .get_limited::<super::paged::RulesHeader>(
                &super::paged::rule_path(id, 0),
                self.remaining(),
            )
            .await?
            .context("Missing paged assembly header")?;
        self.charge(&header)?;
        let header = Arc::new(header);
        self.headers.insert(id.clone(), header.clone());
        Ok(header)
    }
    async fn piece_rules(&mut self, piece: &Piece) -> SharedResult<BTreeSet<ContentId>> {
        if let Some(rules) = self.rules.get(&piece.assembly).cloned() {
            ensure!(
                rules.pieces.get(&piece.local_id) == Some(&piece.id),
                "Piece mapping mismatch"
            );
            for spec in &rules.joints {
                if spec.a == piece.local_id || spec.b == piece.local_id {
                    let joint = super::paged::BoundJoint {
                        a: rules.pieces[&spec.a].clone(),
                        b: rules.pieces[&spec.b].clone(),
                        spec: spec.clone(),
                    };
                    self.joints
                        .insert(ContentId::of(&canonical_cbor(&joint)?), joint);
                }
            }
            if rules.fixed_bricks.contains(&piece.local_id) {
                self.fixed.insert(piece.id.clone());
            }
            return Ok(self.graphs[&piece.assembly]
                .get(&piece.id)
                .cloned()
                .unwrap_or_default());
        }
        self.assembly(&piece.assembly).await?;
        let member: super::paged::MemberRow = self
            .read(&super::paged::row(
                &piece.assembly,
                1,
                u64::from(piece.local_id),
            ))
            .await?
            .context("Missing member")?;
        ensure!(
            member.piece == piece.id && member.local_id == piece.local_id,
            "Piece mapping mismatch"
        );
        let peers: Vec<ContentId> = super::paged::rows(
            self,
            &super::paged::row(&piece.assembly, 2, u64::from(piece.local_id)),
        )
        .await?;
        let joints: Vec<super::paged::BoundJoint> = super::paged::rows(
            self,
            &super::paged::row(&piece.assembly, 3, u64::from(piece.local_id)),
        )
        .await?;
        for joint in joints {
            self.joints
                .insert(ContentId::of(&canonical_cbor(&joint)?), joint);
        }
        if self
            .read::<bool>(&super::paged::row(
                &piece.assembly,
                4,
                u64::from(piece.local_id),
            ))
            .await?
            .unwrap_or(false)
        {
            self.fixed.insert(piece.id.clone());
        }
        Ok(peers.into_iter().collect())
    }
    pub async fn remove_member(&mut self, piece: &Piece) -> SharedResult<()> {
        let peers = self.piece_rules(piece).await?;
        ensure!(
            peers.is_empty() && !self.fixed.contains(&piece.id),
            "Remove group/constraint/anchor references before deleting this piece"
        );
        self.tx
            .remove(&super::paged::row(
                &piece.assembly,
                1,
                u64::from(piece.local_id),
            ))
            .await?;
        Ok(())
    }
    pub async fn mesh(&mut self, id: &ContentId) -> SharedResult<Arc<Mesh>> {
        if let Some(mesh) = self.meshes.get(id) {
            return Ok(mesh.clone());
        }
        let mesh: Mesh = self.tx.blob_limited(id, self.remaining()).await?;
        self.charge(&mesh)?;
        let mesh = Arc::new(mesh);
        self.meshes.insert(id.clone(), mesh.clone());
        Ok(mesh)
    }
    pub async fn brick(&mut self, piece: &Piece, local_id: u32) -> SharedResult<ergo_world::Brick> {
        let mesh = self.mesh(&piece.mesh).await?;
        Ok(piece.geometry.brick(local_id, mesh.as_ref())?)
    }
    pub async fn cells(&mut self, piece: &Piece) -> SharedResult<BTreeSet<BlockCoord>> {
        let brick = self.brick(piece, 1).await?;
        Ok(ergo_world::partition(&[brick])?.into_keys().collect())
    }
    pub fn permit(&self, actor: &str, piece: &Piece) -> SharedResult<()> {
        if piece
            .replicas
            .iter()
            .any(|c| !self.policy.can_write(actor, *c))
        {
            return Err(SharedError::forbidden());
        }
        Ok(())
    }
    pub async fn replace(
        &mut self,
        old: Option<&Piece>,
        mut new: Option<Piece>,
    ) -> SharedResult<()> {
        let id = new
            .as_ref()
            .map(|p| p.id.clone())
            .or_else(|| old.map(|p| p.id.clone()))
            .context("Empty piece mutation")?;
        if let Some(piece) = &mut new {
            piece.replicas = self.cells(piece).await?;
        }
        self.replace_spatial(old, new.as_ref()).await?;
        if let Some(piece) = &new {
            self.tx.put(&entity_path(3, &id), piece).await?;
        } else {
            self.tx.remove(&entity_path(3, &id)).await?;
        }
        if !self.pieces.contains_key(&id) && self.pieces.len() >= self.limits.pieces {
            return Err(SharedError::working_limit("piece/dependency budget"));
        }
        self.pieces.insert(id, new);
        Ok(())
    }
    pub async fn neighbors(&mut self, bounds: Aabb) -> SharedResult<BTreeSet<ContentId>> {
        self.spatial_neighbors(bounds).await
    }
    pub async fn discover(
        &mut self,
        seeds: &BTreeSet<ContentId>,
    ) -> SharedResult<BTreeSet<ContentId>> {
        let mut visited = BTreeSet::new();
        let mut stack: Vec<_> = seeds.iter().cloned().collect();
        let halo = self
            .model
            .tolerances
            .position_brick_units
            .max(INFERRED_JOINT_HALO);
        while let Some(id) = stack.pop() {
            if !visited.insert(id.clone()) {
                continue;
            }
            let Some(piece) = self.piece(&id).await? else {
                continue;
            };
            stack.extend(self.piece_rules(&piece).await?);
            stack.extend(self.external(&piece.id).await?);
            let bounds = self.brick(&piece, 1).await?.bounds.expanded(halo);
            stack.extend(self.neighbors(bounds).await?);
        }
        Ok(visited)
    }
    pub async fn swept(&mut self, old: &Piece, new: &Piece) -> SharedResult<()> {
        let a = self.brick(old, 1).await?;
        let b = self.brick(new, 1).await?;
        let radius = [&a, &b]
            .into_iter()
            .map(|b| {
                (b.bounds.min - b.position)
                    .abs()
                    .max((b.bounds.max - b.position).abs())
                    .length()
            })
            .fold(0.0, f64::max);
        let halo = self
            .model
            .tolerances
            .position_brick_units
            .max(INFERRED_JOINT_HALO);
        self.neighbors(
            Aabb::new(a.position.min(b.position), a.position.max(b.position))
                .expanded(radius + halo),
        )
        .await?;
        Ok(())
    }
    pub async fn validate(
        &mut self,
        affected: &BTreeSet<ContentId>,
    ) -> SharedResult<Option<ergo_world::validation::ValidationReport>> {
        let sources = self
            .rules
            .values()
            .filter_map(|r| r.source_model.clone())
            .collect::<BTreeSet<_>>();
        for source in sources {
            let model: ergo_world::assembly::Model =
                self.tx.blob_limited(&source, self.remaining()).await?;
            self.charge(&model)?;
            model.validate_isometries()?;
        }
        let mut local = BTreeMap::new();
        let mut bricks = Vec::new();
        for id in affected {
            if let Some(piece) = self.piece(id).await? {
                let number = u32::try_from(bricks.len() + 1)
                    .context("Working-set integer index overflow")?;
                bricks.push(self.brick(&piece, number).await?);
                local.insert(id.clone(), number);
            }
        }
        if bricks.is_empty() {
            return Ok(None);
        }
        let mut joints = Vec::new();
        let mut fixed = Vec::new();
        for joint in self.joints.values() {
            let a = &joint.a;
            let b = &joint.b;
            let mut joint = joint.spec.clone();
            if local.contains_key(a) || local.contains_key(b) {
                joint.a = *local
                    .get(a)
                    .context("Incomplete constraint dependency closure")?;
                joint.b = *local
                    .get(b)
                    .context("Incomplete constraint dependency closure")?;
                joints.push(joint);
            }
        }
        for id in &self.fixed {
            if let Some(id) = local.get(id) {
                fixed.push(*id);
            }
        }
        let mut scene = WorldExport {
            schema: ergo_world::SCHEMA_VERSION,
            name: self.metadata.name.clone(),
            geo: self.metadata.geo,
            bricks,
            meshes: BTreeMap::new(),
            joints,
            fixed_bricks: fixed,
            groups: Vec::new(),
            model: None,
            ground: Vec::new(),
        };
        let model = self.model.clone();
        Ok(Some(tokio::task::spawn_blocking(move || -> anyhow::Result<_> {
            let pairs: BTreeSet<_> = scene.joints.iter().map(|j| (j.a.min(j.b), j.a.max(j.b))).collect();
            scene.joints.extend(ergo_world::constraints::inferred_joints(&scene.bricks).into_iter().filter(|j| !pairs.contains(&(j.a.min(j.b), j.a.max(j.b)))));
            for joint in &mut scene.joints {
                joint.break_force=None; joint.break_torque=None; joint.strength_source=None;
            }
            let mut report = ergo_world::validation::validate_static_with_measurements(&mut scene, model.tolerances,&model.measurements)?;
            if report.joint_strength_required != Some(false) {
                model.measurements.apply(&scene.bricks, &mut scene.joints)?;
                report = ergo_world::validation::validate_static_with_measurements(&mut scene, model.tolerances,&model.measurements)?;
            }
            report.physical_calibration = if report.joint_strength_required == Some(false) {
                "Frozen model, sourced masses and nominal bounds; complete gravity-contact proof with zero joint wrench; no clutch capacity assumed; no simulation"
            } else { "Frozen server-owned measurement/model version; complete affected dependency domain; measured connector capacities; no simulation" }.into();
            Ok(report)
        }).await.map_err(anyhow::Error::from)??))
    }
    pub async fn flush_rules(&mut self, assemblies: &BTreeSet<ContentId>) -> SharedResult<()> {
        for id in assemblies {
            super::paged::write(&mut self.tx, self.rules[id].as_ref()).await?;
        }
        Ok(())
    }
}
