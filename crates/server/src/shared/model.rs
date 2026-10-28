use anyhow::Result;
use ergo_transactions::{ContentId, OctreePath};
use ergo_world::{
    BlockCoord, Brick, GeoReference, JointSpec, SCHEMA_VERSION,
    geometry::Mesh,
    groups::Group,
    physical::ConnectionMeasurements,
    validation::{StaticTolerances, ValidationReport},
};
use glam::{DMat3, DVec3};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Grant {
    pub user: String,
    pub regions: Vec<OctreePath>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Policy {
    pub owner: String,
    pub grants: Vec<Grant>,
    #[serde(default)]
    pub keep_last: Option<u64>,
    #[serde(default)]
    pub authenticated_editing: bool,
}
impl Policy {
    pub fn can_read(&self, actor: &str) -> bool {
        self.authenticated_editing
            || self.owner == actor
            || self.grants.iter().any(|g| g.user == actor)
    }
    pub fn can_write(&self, actor: &str, coord: BlockCoord) -> bool {
        self.authenticated_editing
            || self.owner == actor
            || self.grants.iter().filter(|g| g.user == actor).any(|g| {
                g.regions
                    .iter()
                    .any(|p| p.is_prefix_of(&OctreePath::for_block(coord.into())))
            })
    }
    pub fn validate(&self) -> Result<()> {
        ensure!(
            self.keep_last != Some(0),
            "Retention must keep at least the current revision"
        );
        ensure!(self.grants.len() <= 1024, "Too many grants");
        let mut users = BTreeSet::new();
        for g in &self.grants {
            ensure!(
                !g.user.is_empty() && g.user.len() <= 128 && users.insert(&g.user),
                "Invalid or duplicate grant user"
            );
            ensure!(
                !g.regions.is_empty()
                    && g.regions.len() <= 256
                    && g.regions.iter().all(|p| p.octants().len() <= 64),
                "Grants must cover block-coordinate prefixes"
            );
        }
        Ok(())
    }
}

/// World metadata has no piece list, global counts, or world-sized graph.
#[derive(Clone, Serialize, Deserialize)]
pub struct Metadata {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    pub name: String,
    pub geo: GeoReference,
    pub model: ContentId,
    #[serde(deserialize_with = "current_layout")]
    pub layout: u32,
}
fn current_layout<'de, D: serde::Deserializer<'de>>(deserializer: D) -> Result<u32, D::Error> {
    let layout = u32::deserialize(deserializer)?;
    if ![2, 3].contains(&layout) {
        return Err(serde::de::Error::custom(
            "Only shared spatial layouts 2 and 3 are supported",
        ));
    }
    Ok(layout)
}
#[derive(Clone, Serialize, Deserialize)]
pub struct PhysicalModel {
    pub algorithm: u32,
    pub measurements: ConnectionMeasurements,
    pub tolerances: StaticTolerances,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Geometry {
    pub part: String,
    pub color: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rgb: Option<u32>,
    #[serde(with = "ergo_world::coordinates::vec3")]
    pub position: DVec3,
    #[serde(with = "ergo_world::coordinates::mat3")]
    pub transform: DMat3,
    #[serde(default)]
    pub step: u32,
}
impl Geometry {
    pub fn of(brick: &Brick) -> Self {
        Self {
            part: brick.part.clone(),
            color: brick.color,
            rgb: Some(brick.rgb),
            position: brick.position,
            transform: brick.transform,
            step: brick.step,
        }
    }
    pub fn brick(&self, id: u32, mesh: &Mesh) -> Result<Brick> {
        ensure!(
            mesh.schema == SCHEMA_VERSION && mesh.part == self.part,
            "Geometry/mesh identity mismatch"
        );
        // Uploaded shared meshes are server-normalized by validate_export.
        let body = mesh.body_bounds.transformed(self.transform, self.position);
        let render = mesh
            .bounds
            .transformed(self.transform, self.position)
            .union(body);
        let brick = Brick {
            id,
            part: self.part.clone(),
            color: self.color,
            rgb: self
                .rgb
                .unwrap_or_else(|| ergo_world::color_rgb(self.color)),
            position: self.position,
            transform: self.transform,
            bounds: body,
            render_bounds: render,
            step: self.step,
        };
        // Historical/imported geometry remains inspectable. Physical invariants
        // are enforced by the authoritative pre-commit domain validator.
        ergo_world::physical::validate_affine_pose(&brick)?;
        ergo_world::catalog::validate(std::slice::from_ref(&brick))?;
        Ok(brick)
    }
}

#[derive(Clone, Serialize, Deserialize)]
pub struct Birth {
    pub nonce: String,
    pub created_by: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub created_by_name: Option<String>,
    pub assembly: ContentId,
    pub initial: Geometry,
    pub mesh: ContentId,
}
#[derive(Clone, Serialize, Deserialize)]
pub struct Piece {
    /// SHA-256 of Birth; stable for the lifetime of this instance.
    pub id: ContentId,
    pub assembly: ContentId,
    pub local_id: u32,
    pub geometry: Geometry,
    pub mesh: ContentId,
    pub replicas: BTreeSet<BlockCoord>,
}
/// Server-owned creation attribution, retained across later edits.
#[derive(Clone, Serialize, Deserialize)]
pub struct Creation {
    pub created_by: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub created_by_name: Option<String>,
    /// Server-recorded Unix milliseconds. Absent on historical construction births.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub created_at: Option<u64>,
}
/// A bounded assembly's sparse rules; scoped local IDs map to global births.
#[derive(Clone, Serialize, Deserialize)]
pub struct Rules {
    pub id: ContentId,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub creation: Option<Creation>,
    pub pieces: BTreeMap<u32, ContentId>,
    pub meshes: BTreeMap<String, ContentId>,
    pub joints: Vec<JointSpec>,
    pub fixed_bricks: Vec<u32>,
    pub groups: Vec<Group>,
    /// Immutable authoring/provenance blob for this accepted assembly state;
    /// materialized piece records remain the live geometry authority.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_model: Option<ContentId>,
}
impl Rules {
    pub fn validate(&self) -> Result<()> {
        ensure!(
            self.pieces.keys().all(|id| *id > 0)
                && self.pieces.values().collect::<BTreeSet<_>>().len() == self.pieces.len(),
            "Invalid assembly identity mapping"
        );
        ergo_world::constraints::validate_joint_ids(
            self.pieces.keys().copied(),
            &self.joints,
            &self.fixed_bricks,
        )?;
        ergo_world::groups::validate_ids(self.pieces.keys().copied(), &self.groups)
    }
    pub fn adjacency(&self) -> Result<BTreeMap<ContentId, BTreeSet<ContentId>>> {
        let mut graph: BTreeMap<ContentId, BTreeSet<ContentId>> = BTreeMap::new();
        let mut edge = |a: u32, b: u32| {
            let a = &self.pieces[&a];
            let b = &self.pieces[&b];
            graph.entry(a.clone()).or_default().insert(b.clone());
            graph.entry(b.clone()).or_default().insert(a.clone());
        };
        for j in &self.joints {
            edge(j.a, j.b);
        }
        for group in &self.groups {
            let (ids, _) = ergo_world::groups::descendants(&self.groups, group.id)?;
            if let Some(&first) = ids.iter().next() {
                for id in ids {
                    edge(first, id);
                }
            }
        }
        Ok(graph)
    }
}

#[derive(Clone, Serialize, Deserialize)]
pub struct Audit {
    pub actor: String,
    /// Server-resolved profile name at acceptance; permissions use the stable ID.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub actor_name: Option<String>,
    pub base: ContentId,
    pub policy_hash: ContentId,
    pub model: ContentId,
    pub changed_pieces: BTreeSet<ContentId>,
    pub affected_pieces: BTreeSet<ContentId>,
    pub watched_regions: BTreeSet<OctreePath>,
    pub validation: Option<ValidationReport>,
    pub loaded_pieces: usize,
    pub loaded_cbor_bytes: usize,
}
#[derive(Clone, Copy)]
pub struct Limits {
    pub pieces: usize,
    pub bytes: usize,
}
impl Default for Limits {
    fn default() -> Self {
        Self {
            pieces: 5000,
            bytes: 128 * 1024 * 1024,
        }
    }
}

#[derive(Serialize, Deserialize)]
pub struct NewWorld {
    pub name: String,
    pub geo: GeoReference,
    #[serde(default)]
    pub authenticated_editing: bool,
}
#[derive(Serialize, Deserialize)]
pub struct Head {
    pub id: String,
    pub revision: ContentId,
    pub policy: Policy,
    pub metadata: Metadata,
}
#[derive(Clone, Serialize, Deserialize)]
pub struct PieceEdit {
    pub id: ContentId,
    pub geometry: Option<Geometry>,
    pub mesh: Option<ContentId>,
}
#[derive(Clone, Serialize, Deserialize)]
pub struct RulesEdit {
    pub assembly: ContentId,
    pub joints: Vec<JointSpec>,
    pub fixed_bricks: Vec<u32>,
    pub groups: Vec<Group>,
}
#[derive(Clone, Serialize, Deserialize)]
pub struct CommitRequest {
    pub base: ContentId,
    pub pieces: Vec<PieceEdit>,
    #[serde(default)]
    pub rules: Vec<RulesEdit>,
    #[serde(default)]
    pub joints: Vec<super::joints::JointEdit>,
    #[serde(default)]
    pub message: String,
}
#[derive(Serialize, Deserialize)]
pub struct ImportRequest {
    pub base: ContentId,
    pub scene: ergo_world::WorldExport,
}
#[derive(Serialize, Deserialize)]
pub struct Imported {
    pub accepted: Accepted,
    pub assembly: ContentId,
    pub pieces: BTreeMap<u32, ContentId>,
}
#[derive(Serialize, Deserialize)]
pub struct Accepted {
    pub id: String,
    pub revision: ContentId,
    pub attempts: usize,
    pub rebases: usize,
    pub audit: Audit,
    #[serde(default)]
    pub joint_ids: Vec<ContentId>,
}
#[derive(Serialize, Deserialize)]
pub struct GrantRequest {
    pub base: ContentId,
    pub grants: Vec<Grant>,
    #[serde(default)]
    pub authenticated_editing: Option<bool>,
}
