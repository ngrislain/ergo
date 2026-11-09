//! Revision-pinned authoring transport. A page is a working view, never a
//! complete mechanical domain. Sparse edits never delete unloaded pieces.
use crate::{Brick, JointSpec, WorldExport, geometry::Mesh, groups::Group};
use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

pub const FORMAT: &str = "ergo-edit-1";
pub const PAGE_PIECES: usize = 512;
pub const PAGE_BYTES: usize = 2 * 1024 * 1024;
pub const RESIDENT_PIECES: usize = 2048;

/// Transport precondition, distinct from the canonical-CBOR storage CID. JSON
/// object keys are ordered through Value before hashing on both client/server.
pub fn version<T: Serialize>(value: &T) -> Result<String> {
    use sha2::{Digest, Sha256};
    let value = serde_json::to_value(value)?;
    Ok(format!("{:x}", Sha256::digest(serde_json::to_vec(&value)?)))
}
/// Membership is a set: trie reconstruction orders members canonically even
/// when the authoring client originally submitted them in another order.
pub fn group_version(group: &Group) -> Result<String> {
    let mut group = group.clone();
    group.members.sort_by_key(|m| match m {
        crate::groups::Member::Piece { id } => (0, *id),
        crate::groups::Member::Group { id } => (1, *id),
    });
    version(&group)
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Delta {
    pub format: String,
    pub schema: u32,
    #[serde(default)]
    pub name: Option<String>,
    /// Preconditions for existing pieces. Values hash their canonical posed
    /// piece records, not mesh bytes or the stable birth identity alone.
    #[serde(default)]
    pub expected: BTreeMap<u32, String>,
    #[serde(default)]
    pub upsert: Vec<Brick>,
    #[serde(default)]
    pub remove: Vec<u32>,
    #[serde(default)]
    pub meshes: BTreeMap<String, Mesh>,
    #[serde(default)]
    pub joints: Vec<JointSpec>,
    #[serde(default)]
    pub remove_joints: Vec<[u32; 2]>,
    #[serde(default)]
    pub fixed_add: Vec<u32>,
    #[serde(default)]
    pub fixed_remove: Vec<u32>,
    #[serde(default)]
    pub groups: Vec<Group>,
    #[serde(default)]
    pub expected_groups: BTreeMap<u32, String>,
    #[serde(default)]
    pub remove_groups: Vec<u32>,
}

fn same<T: Serialize>(a: &T, b: &T) -> Result<bool> {
    Ok(serde_json::to_vec(a)? == serde_json::to_vec(b)?)
}

impl Delta {
    pub fn between(
        before: &WorldExport,
        after: &WorldExport,
        versions: &BTreeMap<u32, String>,
    ) -> Result<Self> {
        ensure!(
            same(&before.geo, &after.geo)?,
            "An edit cannot change the geographic origin"
        );
        let old: BTreeMap<_, _> = before.bricks.iter().map(|b| (b.id, b)).collect();
        let new: BTreeSet<_> = after.bricks.iter().map(|b| b.id).collect();
        ensure!(
            old.len() == before.bricks.len() && new.len() == after.bricks.len(),
            "Duplicate piece IDs"
        );
        let mut upsert = Vec::new();
        let mut expected = BTreeMap::new();
        let mut changed_parts = BTreeSet::new();
        for (name, mesh) in &after.meshes {
            if before
                .meshes
                .get(name)
                .is_none_or(|old| !same(old, mesh).unwrap_or(false))
            {
                changed_parts.insert(name.as_str());
            }
        }
        for b in &after.bricks {
            if !changed_parts.contains(b.part.as_str())
                && old.get(&b.id).is_some_and(|a| same(*a, b).unwrap_or(false))
            {
                continue;
            }
            if old.contains_key(&b.id) {
                expected.insert(
                    b.id,
                    versions.get(&b.id).cloned().ok_or_else(|| {
                        anyhow::anyhow!("Missing piece version; reload the authoring page")
                    })?,
                );
            }
            upsert.push(b.clone());
        }
        let remove: Vec<_> = old.keys().filter(|id| !new.contains(id)).copied().collect();
        for id in &remove {
            expected.insert(
                *id,
                versions.get(id).cloned().ok_or_else(|| {
                    anyhow::anyhow!("Missing piece version; reload the authoring page")
                })?,
            );
        }
        let parts: BTreeSet<_> = upsert.iter().map(|b| b.part.as_str()).collect();
        let meshes = after
            .meshes
            .iter()
            .filter(|(part, _)| {
                parts.contains(part.as_str()) && changed_parts.contains(part.as_str())
            })
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();
        let pair = |j: &JointSpec| [j.a.min(j.b), j.a.max(j.b)];
        let old_j: BTreeMap<_, _> = before.joints.iter().map(|j| (pair(j), j)).collect();
        let new_j: BTreeSet<_> = after.joints.iter().map(pair).collect();
        let joints = after
            .joints
            .iter()
            .filter(|j| {
                old_j
                    .get(&pair(j))
                    .is_none_or(|old| !same(*old, *j).unwrap_or(false))
            })
            .cloned()
            .collect();
        let old_g: BTreeMap<_, _> = before.groups.iter().map(|g| (g.id, g)).collect();
        let new_g: BTreeSet<_> = after.groups.iter().map(|g| g.id).collect();
        Ok(Self {
            format: FORMAT.into(),
            schema: crate::SCHEMA_VERSION,
            name: (before.name != after.name).then(|| after.name.clone()),
            expected,
            upsert,
            remove,
            meshes,
            joints,
            remove_joints: old_j
                .keys()
                .filter(|p| !new_j.contains(*p))
                .copied()
                .collect(),
            fixed_add: after
                .fixed_bricks
                .iter()
                .filter(|id| !before.fixed_bricks.contains(id))
                .copied()
                .collect(),
            fixed_remove: before
                .fixed_bricks
                .iter()
                .filter(|id| !after.fixed_bricks.contains(id))
                .copied()
                .collect(),
            groups: after
                .groups
                .iter()
                .filter(|g| {
                    old_g
                        .get(&g.id)
                        .is_none_or(|old| !same(*old, *g).unwrap_or(false))
                })
                .cloned()
                .collect(),
            expected_groups: before
                .groups
                .iter()
                .map(|g| Ok((g.id, group_version(g)?)))
                .collect::<Result<_>>()?,
            remove_groups: old_g
                .keys()
                .filter(|id| !new_g.contains(id))
                .copied()
                .collect(),
        })
    }

    pub fn validate(&self) -> Result<()> {
        ensure!(
            self.format == FORMAT && self.schema == crate::SCHEMA_VERSION,
            "Unsupported edit format/schema"
        );
        let mut ids = BTreeSet::new();
        ensure!(
            self.upsert.iter().all(|b| b.id > 0 && ids.insert(b.id)),
            "Duplicate/zero upsert ID"
        );
        ensure!(
            self.remove.iter().all(|id| *id > 0 && ids.insert(*id)),
            "Duplicate/conflicting removal"
        );
        ensure!(
            self.expected.keys().all(|id| ids.contains(id)),
            "Unused piece precondition"
        );
        let mut pairs = BTreeSet::new();
        ensure!(
            self.joints
                .iter()
                .all(|j| j.a != j.b && pairs.insert([j.a.min(j.b), j.a.max(j.b)])),
            "Duplicate/invalid joint edit"
        );
        let mut groups = BTreeSet::new();
        ensure!(
            self.groups.iter().all(|g| groups.insert(g.id))
                && self.remove_groups.iter().all(|id| groups.insert(*id)),
            "Duplicate/conflicting group edit"
        );
        ensure!(
            self.name
                .as_ref()
                .is_none_or(|n| !n.trim().is_empty() && n.chars().count() <= 120),
            "Invalid construction name"
        );
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn group_versions_ignore_member_order_and_id_allocation_uses_the_manifest_floor() {
        use crate::groups::{Group, Member};
        let a = Group {
            id: 7,
            name: "g".into(),
            members: vec![Member::Piece { id: 1 }, Member::Piece { id: 2 }],
            pivot_world: glam::DVec3::ZERO,
        };
        let mut b = a.clone();
        b.members.reverse();
        assert_eq!(group_version(&a).unwrap(), group_version(&b).unwrap());
        let mut scene = WorldExport {
            schema: crate::SCHEMA_VERSION,
            name: "empty".into(),
            geo: Default::default(),
            bricks: vec![],
            meshes: Default::default(),
            joints: vec![],
            fixed_bricks: vec![],
            ground: vec![],
            groups: vec![],
            model: None,
        };
        assert_eq!(
            crate::groups::create_at_least(&mut scene, "New".into(), vec![], None, 501).unwrap(),
            501
        );
    }
    #[test]
    fn a_partial_delta_never_removes_unloaded_ids_and_requires_versions() {
        let mut d = crate::demo::Demo::new();
        d.add("3003", 4, 0., 0., 0.);
        d.add("3003", 4, 10., 0., 0.);
        let scene = WorldExport {
            schema: crate::SCHEMA_VERSION,
            name: "test".into(),
            geo: Default::default(),
            bricks: d.bricks,
            meshes: d.meshes,
            joints: vec![],
            fixed_bricks: vec![],
            ground: vec![],
            groups: vec![],
            model: None,
        };
        let mut page = scene.clone();
        page.bricks.truncate(1);
        let mut edited = page.clone();
        edited.bricks[0].color = 1;
        assert!(Delta::between(&page, &edited, &BTreeMap::new()).is_err());
        let delta =
            Delta::between(&page, &edited, &BTreeMap::from([(1, "version".into())])).unwrap();
        assert!(delta.remove.is_empty());
        assert_eq!(delta.upsert.len(), 1);
        delta.validate().unwrap();
    }
}
