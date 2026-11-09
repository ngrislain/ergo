use super::{READ_CONCURRENCY, SharedError, SharedResult, address::entity_path, model::*};
use anyhow::Context;
use ergo_transactions::spill::SpillMap;
use ergo_transactions::{ContentId, OctreePath, Transaction, canonical_cbor};
use ergo_world::{Aabb, BlockCoord, constraints::INFERRED_JOINT_HALO, geometry::Mesh};
use serde::{Serialize, de::DeserializeOwned};
#[path = "working_graph.rs"]
mod graph;
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
    pub pieces: SpillMap<ContentId, Option<Piece>>,
    pub rules: BTreeMap<ContentId, Arc<Rules>>,
    /// Rules as read from the base revision; `flush_rules` writes only the
    /// records that differ from them.
    stored_rules: BTreeMap<ContentId, Arc<Rules>>,
    graphs: SpillMap<Vec<u8>, graph::GraphRow>,
    headers: BTreeMap<ContentId, Arc<super::paged::RulesHeader>>,
    joints: SpillMap<ContentId, super::paged::BoundJoint>,
    fixed: SpillMap<ContentId, ()>,
    meshes: BTreeMap<ContentId, Arc<Mesh>>,
    spatial_refs: BTreeMap<ContentId, Arc<super::spatial::SpatialRef>>,
    bulk: super::bulk::Updates,
    pub regions: BTreeSet<OctreePath>,
    pub new_blocks: BTreeSet<BlockCoord>,
    pub loaded_bytes: usize,
    limits: Limits,
    discovery: Option<DiscoveryProgress>,
}

struct DiscoveryProgress {
    visited: usize,
    pending: usize,
    phase: &'static str,
    since: std::time::Instant,
    milliseconds: BTreeMap<&'static str, u64>,
}
impl DiscoveryProgress {
    fn phase(&mut self, next: &'static str) {
        *self.milliseconds.entry(self.phase).or_default() +=
            self.since.elapsed().as_millis() as u64;
        self.phase = next;
        self.since = std::time::Instant::now();
    }
    fn report(&self) -> serde_json::Value {
        let mut milliseconds = self.milliseconds.clone();
        *milliseconds.entry(self.phase).or_default() += self.since.elapsed().as_millis() as u64;
        serde_json::json!({"visited":self.visited,"pending":self.pending,"active_phase":self.phase,"phase_milliseconds":milliseconds})
    }
}
impl Working {
    pub(super) fn discovery_report(&self) -> serde_json::Value {
        self.discovery
            .as_ref()
            .map_or(serde_json::Value::Null, DiscoveryProgress::report)
    }
    pub(super) fn discovery_phase(&mut self, phase: &'static str) {
        if let Some(progress) = self.discovery.as_mut() {
            progress.phase(phase);
        }
    }
    pub(super) fn add_joint(
        &mut self,
        id: ContentId,
        joint: super::paged::BoundJoint,
    ) -> SharedResult<()> {
        self.joints.insert(id, joint)?;
        Ok(())
    }
    pub fn reset_dependencies(&mut self) -> SharedResult<()> {
        self.joints.clear()?;
        self.fixed.clear()?;
        Ok(())
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
        metadata.require_current_cells()?;
        let model: PhysicalModel = tx.blob_limited(&metadata.model, limits.bytes).await?;
        ensure!(
            model.algorithm == 1,
            "Unsupported frozen physics model version"
        );
        model.measurements.validate()?;
        model.tolerances.validate()?;
        let graphs = tx.local_map()?;
        let joints = tx.local_map()?;
        let fixed = tx.local_map()?;
        let pieces = tx.local_map()?;
        let mut result = Self {
            tx,
            metadata,
            policy,
            policy_cid,
            model,
            pieces,
            rules: BTreeMap::new(),
            stored_rules: BTreeMap::new(),
            graphs,
            headers: BTreeMap::new(),
            joints,
            fixed,
            meshes: BTreeMap::new(),
            spatial_refs: BTreeMap::new(),
            bulk: Default::default(),
            regions: BTreeSet::new(),
            new_blocks: BTreeSet::new(),
            loaded_bytes: 0,
            limits,
            discovery: None,
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
    pub(super) async fn load_spatial_refs(&mut self, ids: BTreeSet<ContentId>) -> SharedResult<()> {
        use futures::{StreamExt, TryStreamExt};
        let tx = &self.tx;
        let limit = self.remaining();
        let records: Vec<_> = futures::stream::iter(
            ids.into_iter()
                .filter(|id| !self.spatial_refs.contains_key(id)),
        )
        .map(|id| async move {
            let value: super::spatial::SpatialRef = tx.blob_limited(&id, limit).await?;
            Ok::<_, SharedError>((id, value))
        })
        .buffered(READ_CONCURRENCY)
        .try_collect()
        .await?;
        for (id, value) in records {
            self.charge(&value)?;
            self.spatial_refs.insert(id, Arc::new(value));
        }
        Ok(())
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
        if let Some(piece) = self.pieces.get(id)? {
            return Ok(piece);
        }
        if self.pieces.len() >= self.limits.pieces {
            return Err(SharedError::working_limit("piece/dependency budget"));
        }
        let piece: Option<Piece> = self.read(&entity_path(3, id)).await?;
        if let Some(piece) = &piece {
            ensure!(piece.id == *id, "Stored piece identity mismatch");
        }
        self.pieces.insert(id.clone(), piece.clone())?;
        Ok(piece)
    }
    pub(super) async fn prefetch_pieces(&self, ids: Vec<ContentId>) {
        let paths = ids
            .into_iter()
            .filter(|id| !self.pieces.contains_key(id).unwrap_or(true))
            .map(|id| entity_path(3, &id))
            .collect();
        self.tx.prefetch_read_records(paths, self.remaining()).await;
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
        if header
            .piece_count
            .is_some_and(|count| count > self.piece_limit())
        {
            return Err(SharedError::working_limit("authoring assembly"));
        }
        if let Some(count) = header
            .piece_count
            .filter(|&count| (512..=20_000).contains(&count))
        {
            // Current imports assign consecutive local IDs. Concurrent bounded
            // immutable lookahead avoids a cold, one-object-at-a-time walk of
            // the member trie. This is only speculation: sparse/legacy IDs,
            // failed probes and changed records still use the ordinary scan
            // below, which observes the full prefix and charges every row.
            let mut paths = Vec::with_capacity(512);
            for local in 1..=count {
                paths.push(super::paged::row(id, 1, local as u64));
                if paths.len() == 512 {
                    self.tx
                        .prefetch_read_records(std::mem::take(&mut paths), self.remaining())
                        .await;
                }
            }
            self.tx.prefetch_read_records(paths, self.remaining()).await;
        }
        let rules = super::paged::reconstruct(self, &header).await?;
        ensure!(rules.id == *id, "Stored assembly identity mismatch");
        rules.validate()?;
        self.stage_graph(&rules)?;
        let rules = Arc::new(rules);
        self.rules.insert(id.clone(), rules.clone());
        self.stored_rules.insert(id.clone(), rules.clone());
        Ok(rules)
    }
    pub fn stage_rules(&mut self, rules: Rules) -> SharedResult<()> {
        self.charge(&rules)?;
        rules.validate()?;
        self.stage_graph(&rules)?;
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
            return self.graph_peers(piece);
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
                .insert(ContentId::of(&canonical_cbor(&joint)?), joint)?;
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
            self.fixed.insert(piece.id.clone(), ())?;
        }
        Ok(peers.into_iter().collect())
    }
    pub async fn remove_member(&mut self, piece: &Piece) -> SharedResult<()> {
        let peers = self.piece_rules(piece).await?;
        ensure!(
            peers.is_empty() && !self.fixed.contains_key(&piece.id)?,
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
        crate::import::validate_catalogue_mesh(&mesh)?;
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
        self.bulk.replace(&mut self.tx, old, new.as_ref()).await?;
        if let Some(piece) = &new {
            self.tx.put(&entity_path(3, &id), piece).await?;
        } else {
            self.tx.remove(&entity_path(3, &id)).await?;
        }
        if !self.pieces.contains_key(&id)? && self.pieces.len() >= self.limits.pieces {
            return Err(SharedError::working_limit("piece/dependency budget"));
        }
        self.pieces.insert(id, new)?;
        Ok(())
    }
    pub async fn neighbors(&mut self, bounds: Aabb) -> SharedResult<BTreeSet<ContentId>> {
        self.spatial_neighbors(bounds).await
    }
    pub(super) async fn flush_bulk(&mut self) -> SharedResult<()> {
        self.bulk.flush(&mut self.tx).await
    }
    pub async fn discover(
        &mut self,
        seeds: &BTreeSet<ContentId>,
    ) -> SharedResult<BTreeSet<ContentId>> {
        self.discover_from(seeds, BTreeSet::new()).await
    }
    /// Dependency closure of every member of whole assemblies, plus `known`
    /// pieces already in the closure (whose own neighbours are already in it).
    /// Equivalent to `discover(members ∪ known)` when only the members may have
    /// changed, but the members' outside neighbours come from one spatial query
    /// over their combined bounds instead of one store round trip per member,
    /// so a one-brick edit of a large assembly does not walk the store piece by
    /// piece. Pieces found outside the members continue the ordinary discovery.
    pub async fn discover_members(
        &mut self,
        members: &BTreeSet<ContentId>,
        known: BTreeSet<ContentId>,
    ) -> SharedResult<BTreeSet<ContentId>> {
        let halo = self
            .model
            .tolerances
            .position_brick_units
            .max(INFERRED_JOINT_HALO);
        let mut visited = known;
        visited.extend(members.iter().cloned());
        let mut frontier = BTreeSet::new();
        let mut boxes = Vec::new();
        for id in members {
            let Some(piece) = self.piece(id).await? else {
                continue;
            };
            frontier.extend(self.piece_rules(&piece).await?);
            frontier.extend(self.external(&piece.id).await?);
            boxes.push(
                ergo_world::attachments::bounds(&self.brick(&piece, 1).await?)
                    .expanded(halo + ergo_world::attachments::PORT_OVERHANG),
            );
        }
        if let Some(region) = boxes.iter().copied().reduce(|a, b| a.union(b)) {
            let index = ergo_world::spatial::Octree::new(boxes);
            for id in self.spatial_neighbors_excluding(region, members).await? {
                if visited.contains(&id) {
                    continue;
                }
                let Some(piece) = self.piece(&id).await? else {
                    continue;
                };
                // The region is a union; keep only pieces that touch a member.
                let bounds = self.brick(&piece, 1).await?.bounds;
                if !index.query(bounds).is_empty() {
                    frontier.insert(id);
                }
            }
        }
        frontier.retain(|id| !visited.contains(id));
        self.discover_from(&frontier, visited).await
    }
    async fn discover_from(
        &mut self,
        seeds: &BTreeSet<ContentId>,
        known: BTreeSet<ContentId>,
    ) -> SharedResult<BTreeSet<ContentId>> {
        let mut visited = self.tx.local_map::<ContentId, ()>()?;
        let mut stack = self.tx.local_map::<ContentId, ()>()?;
        for id in known {
            visited.insert(id, ())?;
        }
        for id in seeds {
            stack.insert(id.clone(), ())?;
        }
        let started = std::time::Instant::now();
        self.discovery = Some(DiscoveryProgress {
            visited: visited.len(),
            pending: stack.len(),
            phase: "warming",
            since: started,
            milliseconds: BTreeMap::new(),
        });
        let mut next_progress = 1024;
        let halo = self
            .model
            .tolerances
            .position_brick_units
            .max(INFERRED_JOINT_HALO);
        while !stack.is_empty() {
            let mut wave = Vec::with_capacity(READ_CONCURRENCY);
            for _ in 0..READ_CONCURRENCY {
                let Some((id, _)) = stack.pop_last()? else {
                    break;
                };
                wave.push(id);
            }
            let mut warm = Vec::new();
            for id in &wave {
                if !visited.contains_key(id)? && !self.pieces.contains_key(id)? {
                    warm.push(entity_path(3, id));
                }
            }
            self.discovery_phase("warming");
            self.tx.prefetch_read_records(warm, self.remaining()).await;
            // Broad optional lookahead pays off on large closures; small edits
            // keep their narrow required reads without an extra page warm-up.
            if wave.len() >= 16 {
                self.discovery_phase("spatial_warming");
                let mut neighbor_bounds = Vec::new();
                for id in &wave {
                    if !visited.contains_key(id)?
                        && let Some(piece) = self.piece(id).await?
                    {
                        neighbor_bounds.push(
                            ergo_world::attachments::bounds(&self.brick(&piece, 1).await?)
                                .expanded(halo + ergo_world::attachments::PORT_OVERHANG),
                        );
                    }
                }
                let paths = super::spatial::neighbor_read_paths(&neighbor_bounds)?;
                self.tx.prefetch_read_paths(paths.clone()).await;
                self.discovery_phase("spatial_pages_warming");
                let hints = self
                    .tx
                    .prefetch_read_pages(
                        paths
                            .into_iter()
                            .filter(|p| p.octants().first() == Some(&1))
                            .collect(),
                        self.remaining(),
                    )
                    .await;
                self.discovery_phase("spatial_hint_pieces");
                let pieces = hints
                    .into_iter()
                    .filter_map(|(path, _)| {
                        let coord = super::spatial::coordinate(
                            &OctreePath::new(path.octants().get(..65)?.to_vec()).ok()?,
                        )
                        .ok()?;
                        super::spatial::reference_piece(&path, coord).ok()
                    })
                    .collect::<BTreeSet<_>>();
                self.prefetch_pieces(pieces.into_iter().collect()).await;
            }
            for id in wave {
                if visited.contains_key(&id)? {
                    continue;
                }
                visited.insert(id.clone(), ())?;
                self.discovery.as_mut().unwrap().visited = visited.len();
                self.discovery_phase("pieces");
                let Some(piece) = self.piece(&id).await? else {
                    continue;
                };
                self.discovery_phase("rules");
                for peer in self.piece_rules(&piece).await? {
                    if !visited.contains_key(&peer)? {
                        stack.insert(peer, ())?;
                    }
                }
                self.discovery_phase("external_joints");
                for peer in self.external(&piece.id).await? {
                    if !visited.contains_key(&peer)? {
                        stack.insert(peer, ())?;
                    }
                }
                self.discovery_phase("meshes");
                let bounds = ergo_world::attachments::bounds(&self.brick(&piece, 1).await?)
                    .expanded(halo + ergo_world::attachments::PORT_OVERHANG);
                self.discovery_phase("spatial_neighbors");
                for peer in self.neighbors(bounds).await? {
                    if !visited.contains_key(&peer)? {
                        stack.insert(peer, ())?;
                    }
                }
                self.discovery.as_mut().unwrap().pending = stack.len();
            }
            if visited.len() >= next_progress {
                tracing::info!(
                    visited = visited.len(),
                    pending = stack.len(),
                    elapsed_s = started.elapsed().as_secs(),
                    "Discovering bounded dependency closure"
                );
                next_progress = visited.len() + 1024;
            }
        }
        self.discovery_phase("complete");
        // The public validation/audit boundary still consumes the full ID set;
        // only active traversal state is externalized in this phase.
        Ok(visited
            .iter()
            .map(|row| row.map(|(id, _)| id))
            .collect::<ergo_transactions::Result<_>>()?)
    }
    /// Conservative read-only dependency guard. Complete assembly reads already
    /// have every member, so one compact spatial query replaces one per brick.
    /// Explicit peers/joints are still checked, including remote endpoints.
    pub async fn has_external_dependencies(
        &mut self,
        ids: &BTreeSet<ContentId>,
        bounds: Option<Aabb>,
    ) -> SharedResult<bool> {
        let Some(bounds) = bounds else {
            return Ok(false);
        };
        ensure!(
            bounds.size().max_element() <= ergo_world::BLOCK_SIZE,
            "Exact external-dependency check requires a compact assembly"
        );
        let halo = self
            .model
            .tolerances
            .position_brick_units
            .max(INFERRED_JOINT_HALO);
        if self
            .has_spatial_neighbor_excluding(
                bounds.expanded(halo + 2. * ergo_world::attachments::PORT_OVERHANG),
                ids,
            )
            .await?
        {
            return Ok(true);
        }
        for id in ids {
            if let Some(piece) = self.piece(id).await? {
                if self
                    .piece_rules(&piece)
                    .await?
                    .iter()
                    .any(|peer| !ids.contains(peer))
                    || self
                        .external(id)
                        .await?
                        .iter()
                        .any(|peer| !ids.contains(peer))
                {
                    return Ok(true);
                }
            }
        }
        Ok(false)
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
                .expanded(radius + halo + 2. * ergo_world::attachments::PORT_OVERHANG),
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
            let value: serde_json::Value = self.tx.blob_limited(&source, self.remaining()).await?;
            self.charge(&value)?;
            // Already-accepted canonical pieces remain authoritative when an
            // optional editing hierarchy is no longer usable. Incoming models
            // are checked separately before staging; current hints retain the
            // strict nested-isometry check. No retired construct is resolved.
            if let Ok(model) = serde_json::from_value::<ergo_world::assembly::Model>(value) {
                model.validate_isometries()?;
            }
        }
        use ergo_world::validation::ValidationInput;
        let mut input = super::validation_input::NativeValidationInput::new(&self.tx)?;
        for id in affected {
            if let Some(piece) = self.piece(id).await? {
                let number =
                    u32::try_from(input.len() + 1).context("Working-set integer index overflow")?;
                let brick = self.brick(&piece, number).await?;
                input.push(id.clone(), brick, &self.model.measurements)?;
            }
        }
        let input = input.finish()?;
        if input.is_empty() {
            return Ok(None);
        }
        let mut joints = Vec::new();
        let mut fixed = Vec::new();
        for row in self.joints.iter() {
            let (_, joint) = row?;
            let a = &joint.a;
            let b = &joint.b;
            let mut joint = joint.spec.clone();
            let a = input.local_id(a)?;
            let b = input.local_id(b)?;
            if a.is_some() || b.is_some() {
                joint.a = a.context("Incomplete constraint dependency closure")?;
                joint.b = b.context("Incomplete constraint dependency closure")?;
                joints.push(joint);
            }
        }
        for row in self.fixed.iter() {
            let (id, _) = row?;
            if let Some(id) = input.local_id(&id)? {
                fixed.push(id);
            }
        }
        let model = self.model.clone();
        let mut workspace = super::equilibrium_workspace::NativeSolverWorkspace::new(&self.tx)?;
        Ok(Some(tokio::task::spawn_blocking(move || -> anyhow::Result<_> {
            // Validation assigns every capacity from the frozen measurement
            // book and the reviewed connector estimates; uploads claim none.
            let mut report = ergo_world::validation::validate_input_with_workspace(&input, &mut joints, &fixed, model.tolerances,&model.measurements,&mut workspace)?;
            report.physical_calibration = if report.joint_strength_required == Some(false) {
                "Frozen model, sourced masses and nominal bounds; complete gravity-contact proof with zero joint wrench; no clutch capacity assumed; no simulation"
            } else { "Frozen server-owned measurement/model version; complete affected dependency domain; equilibrium with measured or reviewed-estimate connector capacities; no simulation" }.into();
            Ok(report)
        }).await.map_err(anyhow::Error::from)??))
    }
    pub async fn flush_rules(&mut self, assemblies: &BTreeSet<ContentId>) -> SharedResult<()> {
        for id in assemblies {
            let rules = self.rules[id].clone();
            super::paged::write_changes(
                &mut self.tx,
                self.stored_rules.get(id).map(Arc::as_ref),
                &rules,
            )
            .await?;
            self.stored_rules.insert(id.clone(), rules);
        }
        Ok(())
    }
}
