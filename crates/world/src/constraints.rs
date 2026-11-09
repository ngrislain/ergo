//! Lightweight graph and body-box operations shared by REST commit validation
//! and the client. This module has no rigid-body engine dependency.
use crate::{Aabb, Brick, JointKind, JointSpec, spatial::Octree};
use anyhow::{Result, ensure};
use glam::DVec3;
use std::collections::{HashSet, VecDeque};

pub const INFERRED_JOINT_HALO: f64 = 0.003;

pub fn inferred_joints(bricks: &[Brick]) -> Vec<JointSpec> {
    inferred_joints_with_tolerance(bricks, 0.002)
}

pub fn inferred_joints_with_tolerance(bricks: &[Brick], tolerance: f64) -> Vec<JointSpec> {
    let tree = Octree::new(bricks.iter().map(crate::attachments::bounds).collect());
    let mut out = Vec::new();
    for (i, brick) in bricks.iter().enumerate() {
        for j in tree
            .query(crate::attachments::bounds(brick).expanded(INFERRED_JOINT_HALO.max(tolerance)))
        {
            if j <= i {
                continue;
            }
            let other = &bricks[j];
            if let Some(joint) = inferred_pair(brick, other, tolerance) {
                out.push(joint);
            }
        }
    }
    out
}
pub fn inferred_pair(a: &Brick, b: &Brick, tolerance: f64) -> Option<JointSpec> {
    crate::spatial::relation_with_tolerance(a, b, tolerance)
        .is_some_and(|r| r.certainty == "inferred")
        .then(|| crate::attachments::joint_with_tolerance(a, b, tolerance))
}
pub fn validate_joints(bricks: &[Brick], joints: &[JointSpec], fixed: &[u32]) -> Result<()> {
    validate_joint_ids(bricks.iter().map(|b| b.id), joints, fixed)
}

pub fn validate_joint_ids(
    ids: impl IntoIterator<Item = u32>,
    joints: &[JointSpec],
    fixed: &[u32],
) -> Result<()> {
    let input: Vec<_> = ids.into_iter().collect();
    let ids: HashSet<_> = input.iter().copied().collect();
    ensure!(ids.len() == input.len(), "Duplicate brick IDs");
    ensure!(
        joints.len() <= ids.len().saturating_mul(12),
        "Too many constraints"
    );
    let mut pairs = HashSet::new();
    for j in joints {
        ensure!(
            pairs.insert((j.a.min(j.b), j.a.max(j.b))),
            "Duplicate attachment joint for pieces {} and {}",
            j.a,
            j.b
        );
        ensure!(
            j.a != j.b && ids.contains(&j.a) && ids.contains(&j.b),
            "Joint references an absent or identical body"
        );
        validate_joint_spec(j)?;
    }
    ensure!(
        fixed.iter().all(|id| ids.contains(id)),
        "Fixed body is absent"
    );
    Ok(())
}
pub fn validate_joint_spec(j: &JointSpec) -> Result<()> {
    ensure!(j.anchor.is_finite(), "Invalid joint anchor");
    ensure!(
        j.break_force.is_none_or(|f| f.is_finite() && f > 0.0)
            && j.break_torque.is_none_or(|f| f.is_finite() && f > 0.0),
        "Connection capacities must be positive and finite"
    );
    if let JointKind::Revolute { axis, limits } = &j.kind {
        ensure!(
            axis.is_finite() && axis.length() > 1e-8,
            "Hinge axis must be nonzero"
        );
        if let Some([lo, hi]) = limits {
            ensure!(
                lo.is_finite() && hi.is_finite() && lo <= hi,
                "Invalid hinge limits"
            );
        }
    }
    Ok(())
}
pub fn connected_group(id: u32, joints: &[JointSpec]) -> HashSet<u32> {
    let mut adjacency = std::collections::HashMap::<u32, Vec<u32>>::new();
    for j in joints.iter().filter(|j| matches!(j.kind, JointKind::Fixed)) {
        adjacency.entry(j.a).or_default().push(j.b);
        adjacency.entry(j.b).or_default().push(j.a);
    }
    let mut group = HashSet::from([id]);
    let mut queue = VecDeque::from([id]);
    while let Some(id) = queue.pop_front() {
        if let Some(neighbors) = adjacency.get(&id) {
            for &n in neighbors {
                if group.insert(n) {
                    queue.push_back(n);
                }
            }
        }
    }
    group
}
pub fn can_translate(bricks: &[Brick], ids: &HashSet<u32>, delta: DVec3) -> Result<()> {
    ensure!(
        delta.is_finite() && delta.length() <= 32.0,
        "Edit moves are limited to 32 brick units"
    );
    let steps = (delta.length() / 0.05).ceil().max(1.0) as usize;
    let tree = Octree::new(bricks.iter().map(|b| b.bounds).collect());
    for brick in bricks.iter().filter(|b| ids.contains(&b.id)) {
        for step in 1..=steps {
            let offset = delta * (step as f64 / steps as f64);
            let bounds = Aabb::new(brick.bounds.min + offset, brick.bounds.max + offset);
            ensure!(bounds.min.y >= -0.002, "Move intersects the Z=0 baseplate");
            for i in tree.query(bounds) {
                let other = &bricks[i];
                if ids.contains(&other.id) {
                    continue;
                }
                let mut translated = brick.clone();
                translated.translated(offset);
                ensure!(
                    !crate::interference::bodies_penetrate(&translated, other, 0.002),
                    "Move blocked by brick {} (body bounds)",
                    other.id
                );
            }
        }
    }
    Ok(())
}
