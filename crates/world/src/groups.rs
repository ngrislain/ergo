//! Persistent authoring groups. Piece poses remain world-space and authoritative
//! for indexing/physics. A group transform is baked into each descendant once;
//! grouping does not create physical bonds or make an unstable assembly rigid.
use crate::{Aabb, Brick, WorldExport};
use anyhow::{Context, Result, ensure};
use glam::{DMat3, DVec3};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Member {
    #[serde(alias = "brick")]
    Piece {
        id: u32,
    },
    Group {
        id: u32,
    },
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Group {
    pub id: u32,
    pub name: String,
    pub members: Vec<Member>,
    #[serde(with = "crate::coordinates::vec3")]
    pub pivot_world: DVec3,
}

pub fn validate(bricks: &[Brick], groups: &[Group]) -> Result<()> {
    validate_ids(bricks.iter().map(|b| b.id), groups)
}

/// Validate authoring references without materializing unrelated geometry.
pub fn validate_ids(ids: impl IntoIterator<Item = u32>, groups: &[Group]) -> Result<()> {
    ensure!(groups.len() <= 200_000, "Too many authoring groups");
    let pieces: HashSet<_> = ids.into_iter().collect();
    let index: HashMap<_, _> = groups.iter().map(|g| (g.id, g)).collect();
    ensure!(index.len() == groups.len(), "Duplicate group IDs");
    let mut parents = HashMap::new();
    for group in groups {
        ensure!(
            group.id > 0
                && !group.name.trim().is_empty()
                && group.name.chars().count() <= 120
                && group.pivot_world.is_finite()
                && group.pivot_world.abs().max_element() <= crate::MAX_WORLD_COORD,
            "Invalid group identity/name/pivot"
        );
        for member in &group.members {
            match member {
                Member::Piece { id } => ensure!(
                    pieces.contains(id),
                    "Group {} references missing piece {}",
                    group.id,
                    id
                ),
                Member::Group { id } => ensure!(
                    index.contains_key(id),
                    "Group {} references missing group {}",
                    group.id,
                    id
                ),
            }
            ensure!(
                parents.insert(*member, group.id).is_none(),
                "A piece/group may have only one direct parent; duplicate/overlapping membership in group {}",
                group.id
            );
        }
    }
    fn visit(
        id: u32,
        depth: usize,
        index: &HashMap<u32, &Group>,
        state: &mut HashMap<u32, u8>,
    ) -> Result<u8> {
        ensure!(depth < 64, "Group nesting exceeds 64 levels");
        if let Some(&height) = state.get(&id) {
            ensure!(height != 255, "Cyclic group membership");
            ensure!(
                depth + height as usize <= 64,
                "Group nesting exceeds 64 levels"
            );
            return Ok(height);
        }
        state.insert(id, 255);
        let mut height = 1;
        for member in &index[&id].members {
            if let Member::Group { id } = member {
                height = height.max(1 + visit(*id, depth + 1, index, state)?);
            }
        }
        ensure!(height <= 64, "Group nesting exceeds 64 levels");
        state.insert(id, height);
        Ok(height)
    }
    let mut state = HashMap::new();
    for g in groups {
        visit(g.id, 0, &index, &mut state)?;
    }
    Ok(())
}

pub fn descendants(groups: &[Group], id: u32) -> Result<(HashSet<u32>, HashSet<u32>)> {
    let index: HashMap<_, _> = groups.iter().map(|g| (g.id, g)).collect();
    ensure!(index.contains_key(&id), "Group {} is absent", id);
    let mut pieces = HashSet::new();
    let mut nodes = HashSet::new();
    let mut stack = vec![(id, 0)];
    while let Some((id, depth)) = stack.pop() {
        ensure!(
            depth < 64 && nodes.insert(id),
            "Cyclic or duplicate nested groups"
        );
        for m in &index.get(&id).context("Missing nested group")?.members {
            match m {
                Member::Piece { id } => {
                    pieces.insert(*id);
                }
                Member::Group { id } => stack.push((*id, depth + 1)),
            }
        }
    }
    Ok((pieces, nodes))
}
pub fn bounds(scene: &WorldExport, id: u32) -> Result<Option<Aabb>> {
    let (pieces, _) = descendants(&scene.groups, id)?;
    Ok(scene
        .bricks
        .iter()
        .filter(|b| pieces.contains(&b.id))
        .map(|b| b.render_bounds)
        .reduce(Aabb::union))
}

pub fn create(
    scene: &mut WorldExport,
    name: String,
    members: Vec<Member>,
    pivot: Option<DVec3>,
) -> Result<u32> {
    let id = scene
        .groups
        .iter()
        .map(|g| g.id)
        .max()
        .unwrap_or(0)
        .checked_add(1)
        .context("Group ID overflow")?;
    let mut groups = scene.groups.clone();
    groups.push(Group {
        id,
        name,
        members,
        pivot_world: pivot.unwrap_or(DVec3::ZERO),
    });
    validate(&scene.bricks, &groups)?;
    if pivot.is_none() {
        let (pieces, _) = descendants(&groups, id)?;
        let p = scene
            .bricks
            .iter()
            .filter(|b| pieces.contains(&b.id))
            .map(|b| b.bounds)
            .reduce(Aabb::union)
            .map(|b| b.center())
            .unwrap_or(DVec3::ZERO);
        groups.iter_mut().find(|g| g.id == id).unwrap().pivot_world = p;
    }
    scene.groups = groups;
    Ok(id)
}
pub fn set_members(
    scene: &mut WorldExport,
    id: u32,
    members: Vec<Member>,
    reparent: bool,
) -> Result<()> {
    ensure!(scene.groups.iter().any(|g| g.id == id), "Group is absent");
    let mut groups = scene.groups.clone();
    if reparent {
        let moved: HashSet<_> = members.iter().copied().collect();
        for g in &mut groups {
            if g.id != id {
                g.members.retain(|m| !moved.contains(m));
            }
        }
    }
    groups.iter_mut().find(|g| g.id == id).unwrap().members = members;
    validate(&scene.bricks, &groups)?;
    scene.groups = groups;
    Ok(())
}
pub fn ungroup(scene: &mut WorldExport, id: u32) -> Result<()> {
    let children = scene
        .groups
        .iter()
        .find(|g| g.id == id)
        .context("Group is absent")?
        .members
        .clone();
    let mut groups = scene.groups.clone();
    groups.retain(|g| g.id != id);
    for group in &mut groups {
        let mut members = Vec::new();
        for m in &group.members {
            if *m == (Member::Group { id }) {
                members.extend(children.iter().copied());
            } else {
                members.push(*m);
            }
        }
        group.members = members;
    }
    validate(&scene.bricks, &groups)?;
    scene.groups = groups;
    Ok(())
}
pub fn remove_piece_references(groups: &mut [Group], ids: &HashSet<u32>) {
    for group in groups {
        group
            .members
            .retain(|m| !matches!(m,Member::Piece{id} if ids.contains(id)));
    }
}

/// Rigid SI rotation + construction-unit translation. Destination-only rotation
/// validation is a CAD edit, not a continuous collision/physics trajectory.
pub fn transform(
    scene: &mut WorldExport,
    id: u32,
    translation: DVec3,
    axis_metric: DVec3,
    angle: f64,
    pivot: Option<DVec3>,
) -> Result<usize> {
    validate(&scene.bricks, &scene.groups)?;
    ensure!(
        translation.is_finite()
            && axis_metric.is_finite()
            && angle.is_finite()
            && axis_metric.length() > 1e-10,
        "Invalid rigid group transform"
    );
    let (pieces, nodes) = descendants(&scene.groups, id)?;
    let pivot = pivot.unwrap_or_else(|| {
        scene
            .groups
            .iter()
            .find(|g| g.id == id)
            .unwrap()
            .pivot_world
    });
    ensure!(pivot.is_finite(), "Invalid group pivot");
    ensure!(
        !scene.fixed_bricks.iter().any(|id| pieces.contains(id)),
        "Group contains an anchored piece"
    );
    ensure!(
        !scene
            .joints
            .iter()
            .any(|j| pieces.contains(&j.a) != pieces.contains(&j.b)),
        "Group has a mechanical constraint to an outside piece; include it or use the client constraint solver"
    );
    let rotation_metric = DMat3::from_axis_angle(axis_metric.normalize(), angle);
    let rotation = crate::physical::world_transform(rotation_metric);
    let point = |p: DVec3| pivot + rotation * (p - pivot) + translation;
    let mut bricks = scene.bricks.clone();
    for b in &mut bricks {
        if pieces.contains(&b.id) {
            b.position = point(b.position);
            b.transform = rotation * b.transform;
            let mesh = scene
                .meshes
                .get(&b.part)
                .context("Missing group geometry")?;
            b.bounds = mesh.body_bounds.transformed(b.transform, b.position);
            b.render_bounds = mesh.bounds.transformed(b.transform, b.position);
            crate::physical::validate_pose(b)?;
            ensure!(
                b.bounds.min.y >= -crate::validation::OVERLAP_TOLERANCE,
                "Group transform crosses the Z=0 floor"
            );
        }
    }
    let tree = crate::spatial::Octree::new(bricks.iter().map(|b| b.bounds).collect());
    for (i, b) in bricks
        .iter()
        .enumerate()
        .filter(|(_, b)| pieces.contains(&b.id))
    {
        for j in tree.query(b.bounds) {
            if i == j {
                continue;
            }
            let other = &bricks[j];
            let overlap = b.bounds.max.min(other.bounds.max) - b.bounds.min.max(other.bounds.min);
            ensure!(
                overlap.min_element() <= crate::validation::OVERLAP_TOLERANCE,
                "Group transform overlaps piece {} with {}",
                b.id,
                other.id
            );
        }
    }
    crate::partition(&bricks)?;
    let mut groups = scene.groups.clone();
    for group in &mut groups {
        if nodes.contains(&group.id) {
            group.pivot_world = point(group.pivot_world);
        }
    }
    validate(&bricks, &groups)?;
    for joint in &mut scene.joints {
        if pieces.contains(&joint.a) {
            joint.anchor = point(joint.anchor);
            if let crate::JointKind::Revolute { axis, .. } = &mut joint.kind {
                *axis = (rotation_metric * *axis).normalize();
            }
        }
    }
    scene.groups = groups;
    scene.bricks = bricks;
    Ok(pieces.len())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn scene() -> WorldExport {
        let mut d = crate::demo::Demo::new();
        d.add("3003", 4, 2.0, 0.0, 2.0);
        d.add("3003", 1, 8.0, 0.0, 2.0);
        WorldExport {
            schema: crate::SCHEMA_VERSION,
            name: "groups".into(),
            geo: Default::default(),
            bricks: d.bricks,
            meshes: d.meshes,
            joints: Vec::new(),
            fixed_bricks: Vec::new(),
            ground: Vec::new(),
            groups: Vec::new(),
            model: None,
        }
    }
    #[test]
    fn nested_group_transforms_each_piece_once() {
        let mut s = scene();
        let a = create(&mut s, "child".into(), vec![Member::Piece { id: 1 }], None).unwrap();
        let b = create(
            &mut s,
            "parent".into(),
            vec![Member::Group { id: a }, Member::Piece { id: 2 }],
            None,
        )
        .unwrap();
        let before = s.bricks.iter().map(|b| b.position).collect::<Vec<_>>();
        assert_eq!(
            transform(&mut s, b, DVec3::Z, DVec3::Y, 0.0, None).unwrap(),
            2
        );
        for (i, p) in before.into_iter().enumerate() {
            assert_eq!(s.bricks[i].position, p + DVec3::Z);
        }
        assert_eq!(descendants(&s.groups, b).unwrap().0.len(), 2);
    }
    #[test]
    fn cycles_and_duplicate_parents_are_rejected() {
        let mut s = scene();
        let a = create(&mut s, "a".into(), vec![Member::Piece { id: 1 }], None).unwrap();
        let b = create(&mut s, "b".into(), vec![Member::Group { id: a }], None).unwrap();
        let mut copy = s.clone();
        assert!(set_members(&mut copy, a, vec![Member::Group { id: b }], false).is_err());
        assert!(
            create(
                &mut s,
                "duplicate".into(),
                vec![Member::Piece { id: 1 }],
                None
            )
            .is_err()
        );
    }
}
