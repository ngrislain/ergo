//! Mechanical islands, not storage cells, authoring groups or individual bricks.
//! Discover edges without retaining a scene-wide contact list. Ground reactions
//! are prescribed boundary supports and do not join otherwise independent bodies.
use super::{StaticTolerances, ValidationInput, nominal_body_contact};
use crate::{Brick, JointSpec};
use anyhow::{Context, Result};

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
    crate::part_profiles::bearing_bounds(brick)
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

pub(super) fn partition_input(
    input: &dyn ValidationInput,
    joints: &[JointSpec],
    tol: StaticTolerances,
) -> Result<Vec<Component>> {
    let mut sets = Partition::new(input.len());
    for joint in joints {
        sets.join(
            input.body_index(joint.a)?.context("Joint body is absent")?,
            input.body_index(joint.b)?.context("Joint body is absent")?,
        );
    }
    // Include reviewed stepped/compound bearing faces, not just mass envelopes:
    // e.g. a 4444 wing can support another brick outside its main body box.
    if sets.count > 1 {
        for a in 0..input.len() {
            let brick = input.body(a)?;
            if !crate::interference::axis_aligned(&brick) {
                continue;
            }
            for candidate in
                input.query(bearing_envelope(&brick).expanded(tol.position_brick_units))?
            {
                let b = candidate?;
                anyhow::ensure!(b < input.len(), "Component candidate is absent");
                if b <= a || sets.root(a) == sets.root(b) {
                    continue;
                }
                let other = input.body(b)?;
                if !crate::interference::axis_aligned(&other) {
                    continue;
                }
                if nominal_body_contact(&brick, &other, a, b, tol).is_some()
                    || bearing_contact(&brick, &other, tol.position_brick_units)
                    || bearing_contact(&other, &brick, tol.position_brick_units)
                {
                    sets.join(a, b);
                }
            }
        }
    }
    let mut components: Vec<Component> = Vec::new();
    let mut group = vec![usize::MAX; input.len()];
    // Preserve source order within each component, including equal-height load
    // propagation ties. Original public piece IDs are never renumbered.
    for i in 0..input.len() {
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
        components[group[sets.root(input.body_index(joint.a)?.context("Joint body is absent")?)]]
            .joints
            .push(j);
    }
    Ok(components)
}
