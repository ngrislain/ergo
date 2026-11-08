//! Mechanical islands, not storage cells, authoring groups or individual bricks.
//! Discover edges without retaining a scene-wide contact list. Ground reactions
//! are prescribed boundary supports and do not join otherwise independent bodies.
use super::{StaticTolerances, nominal_contact};
use crate::{Brick, JointSpec, spatial::Octree};
use anyhow::{Context, Result};
use std::collections::HashMap;

pub(super) struct Partition {
    parents: Vec<usize>,
    sizes: Vec<usize>,
    count: usize,
}

impl Partition {
    pub fn new(n: usize) -> Self {
        Self {
            parents: (0..n).collect(),
            sizes: vec![1; n],
            count: n,
        }
    }

    pub fn root(&mut self, mut i: usize) -> usize {
        while self.parents[i] != i {
            self.parents[i] = self.parents[self.parents[i]];
            i = self.parents[i];
        }
        i
    }

    pub fn join(&mut self, a: usize, b: usize) {
        let (mut a, mut b) = (self.root(a), self.root(b));
        if a == b {
            return;
        }
        if self.sizes[a] < self.sizes[b] {
            std::mem::swap(&mut a, &mut b);
        }
        self.parents[b] = a;
        self.sizes[a] += self.sizes[b];
        self.count -= 1;
    }
}

pub(super) struct Component {
    pub bodies: Vec<usize>,
    pub joints: Vec<usize>,
}

fn bearing_envelope(brick: &Brick) -> crate::Aabb {
    crate::part_profiles::lower_bearing_regions(brick)
        .into_iter()
        .chain(crate::part_profiles::upper_bearing_regions(brick))
        .fold(brick.bounds, crate::Aabb::union)
}

fn bearing_contact(upper: &Brick, lower: &Brick, tolerance: f64) -> bool {
    if lower.bounds.min.y >= upper.bounds.min.y {
        return false;
    }
    crate::part_profiles::lower_bearing_regions(upper)
        .iter()
        .any(|foot| {
            crate::part_profiles::upper_bearing_regions(lower)
                .iter()
                .any(|face| {
                    let overlap = foot.max.min(face.max) - foot.min.max(face.min);
                    (face.max.y - foot.min.y).abs() <= tolerance
                        && overlap.x > tolerance
                        && overlap.z > tolerance
                })
        })
}

pub(super) fn partition(
    bricks: &[Brick],
    joints: &[JointSpec],
    tol: StaticTolerances,
) -> Result<Vec<Component>> {
    let index: HashMap<_, _> = bricks.iter().enumerate().map(|(i, b)| (b.id, i)).collect();
    let mut sets = Partition::new(bricks.len());
    for joint in joints {
        sets.join(
            *index.get(&joint.a).context("Joint body is absent")?,
            *index.get(&joint.b).context("Joint body is absent")?,
        );
    }
    // Include reviewed stepped/compound bearing faces, not just mass envelopes:
    // e.g. a 4444 wing can support another brick outside its main body box.
    if sets.count > 1 {
        let tree = Octree::new(bricks.iter().map(bearing_envelope).collect());
        for (a, brick) in bricks.iter().enumerate() {
            if !crate::interference::axis_aligned(brick) {
                continue;
            }
            for b in tree.query(bearing_envelope(brick).expanded(tol.position_brick_units)) {
                if b <= a
                    || sets.root(a) == sets.root(b)
                    || !crate::interference::axis_aligned(&bricks[b])
                {
                    continue;
                }
                if nominal_contact(crate::equilibrium::Bodies::all(bricks), a, b, tol).is_some()
                    || bearing_contact(brick, &bricks[b], tol.position_brick_units)
                    || bearing_contact(&bricks[b], brick, tol.position_brick_units)
                {
                    sets.join(a, b);
                }
            }
        }
    }
    let mut components: Vec<Component> = Vec::new();
    let mut group = vec![usize::MAX; bricks.len()];
    // Preserve source order within each component, including equal-height load
    // propagation ties. Original public piece IDs are never renumbered.
    for i in 0..bricks.len() {
        let root = sets.root(i);
        if group[root] == usize::MAX {
            group[root] = components.len();
            components.push(Component {
                bodies: Vec::new(),
                joints: Vec::new(),
            });
        }
        components[group[root]].bodies.push(i);
    }
    for (j, joint) in joints.iter().enumerate() {
        components[group[sets.root(index[&joint.a])]].joints.push(j);
    }
    Ok(components)
}
