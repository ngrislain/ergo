//! Mechanical islands, not storage cells, authoring groups or individual bricks.
//! Discover edges without retaining a scene-wide contact list. Ground reactions
//! are prescribed boundary supports and do not join otherwise independent bodies.
use super::ValidationConstraints;
use super::{
    StaticTolerances, ValidationInput, ValidationRecord, ValidationState, nominal_body_contact,
    working_state::{key, required, unavailable},
};
use crate::Brick;
use anyhow::{Context, Result};
use std::borrow::Cow;

const NODE: u8 = 0;
const BODY: u8 = 1;
const JOINT: u8 = 2;
const COMPONENT: u8 = 3;

#[derive(Clone, Copy)]
struct Node {
    parent: usize,
    size: usize,
    component: Option<usize>,
    ordinal: Option<usize>,
    anchored: bool,
}
fn node(state: &dyn ValidationState, i: usize, n: usize) -> Result<Node> {
    match required(state, &key(NODE, &[i]))? {
        ValidationRecord::Node {
            parent,
            size,
            component,
            ordinal,
            anchored,
        } if i < n
            && parent < n
            && size > 0
            && size <= n
            && component.is_none_or(|c| c < n)
            && ordinal.is_none_or(|o| o < n) =>
        {
            Ok(Node {
                parent,
                size,
                component,
                ordinal,
                anchored,
            })
        }
        _ => Err(unavailable("invalid component union-find record")),
    }
}
fn put_node(state: &mut dyn ValidationState, i: usize, node: Node) -> Result<()> {
    state.put(
        key(NODE, &[i]),
        ValidationRecord::Node {
            parent: node.parent,
            size: node.size,
            component: node.component,
            ordinal: node.ordinal,
            anchored: node.anchored,
        },
    )
}
fn root(state: &mut dyn ValidationState, mut i: usize, n: usize) -> Result<usize> {
    for _ in 0..n {
        let mut row = node(state, i, n)?;
        if row.parent == i {
            return Ok(i);
        }
        row.parent = node(state, row.parent, n)?.parent;
        put_node(state, i, row)?;
        i = row.parent;
    }
    Err(unavailable("component union-find cycle"))
}
fn join(state: &mut dyn ValidationState, a: usize, b: usize, n: usize) -> Result<bool> {
    let (mut a, mut b) = (root(state, a, n)?, root(state, b, n)?);
    if a == b {
        return Ok(false);
    }
    let (mut left, mut right) = (node(state, a, n)?, node(state, b, n)?);
    if left.size < right.size {
        std::mem::swap(&mut a, &mut b);
        std::mem::swap(&mut left, &mut right);
    }
    left.size = left
        .size
        .checked_add(right.size)
        .filter(|&size| size <= n)
        .ok_or_else(|| unavailable("component size overflow"))?;
    right.parent = a;
    put_node(state, a, left)?;
    put_node(state, b, right)?;
    Ok(true)
}

/// An indexed view, not an independently supported spatial cut. Every member
/// comes from the completed joint/bearing closure, in original source order.
pub(super) struct ComponentInput<'a> {
    input: &'a dyn ValidationInput,
    state: &'a dyn ValidationState,
    component: usize,
    pub bodies: usize,
    pub joints: usize,
    pub anchored: bool,
}
impl<'a> ComponentInput<'a> {
    pub fn new(
        input: &'a dyn ValidationInput,
        state: &'a dyn ValidationState,
        component: usize,
    ) -> Result<Self> {
        match required(state, &key(COMPONENT, &[component]))? {
            ValidationRecord::Component {
                bodies,
                joints,
                anchored,
            } if bodies > 0 && bodies <= input.len() => Ok(Self {
                input,
                state,
                component,
                bodies,
                joints,
                anchored,
            }),
            _ => Err(unavailable("component metadata is absent or invalid")),
        }
    }
    pub fn source_index(&self, ordinal: usize) -> Result<usize> {
        let ValidationRecord::Index(index) =
            required(self.state, &key(BODY, &[self.component, ordinal]))?
        else {
            return Err(unavailable("invalid component member"));
        };
        let row = node(self.state, index, self.input.len())?;
        if ordinal >= self.bodies
            || row.component != Some(self.component)
            || row.ordinal != Some(ordinal)
        {
            return Err(unavailable("component membership identity changed"));
        }
        Ok(index)
    }
    pub fn joint_index(&self, ordinal: usize) -> Result<usize> {
        match required(self.state, &key(JOINT, &[self.component, ordinal]))? {
            ValidationRecord::Index(index) if ordinal < self.joints => Ok(index),
            _ => Err(unavailable("invalid component joint member")),
        }
    }
    pub fn is_anchored(&self, ordinal: usize) -> Result<bool> {
        Ok(node(self.state, self.source_index(ordinal)?, self.input.len())?.anchored)
    }
    fn local_index(&self, source: usize) -> Result<Option<usize>> {
        let row = node(self.state, source, self.input.len())?;
        if row.component != Some(self.component) {
            return Ok(None);
        }
        let ordinal = row
            .ordinal
            .ok_or_else(|| unavailable("component ordinal is absent"))?;
        if self.source_index(ordinal)? != source {
            return Err(unavailable("component reverse membership changed"));
        }
        Ok(Some(ordinal))
    }
}
impl ValidationInput for ComponentInput<'_> {
    fn len(&self) -> usize {
        self.bodies
    }
    fn body(&self, index: usize) -> Result<Cow<'_, Brick>> {
        self.input.body(self.source_index(index)?)
    }
    fn body_index(&self, id: u32) -> Result<Option<usize>> {
        self.input
            .body_index(id)?
            .map(|i| self.local_index(i))
            .transpose()
            .map(Option::flatten)
    }
    fn query(&self, bounds: crate::Aabb) -> Result<Box<dyn Iterator<Item = Result<usize>> + '_>> {
        Ok(Box::new(self.input.query(bounds)?.filter_map(
            move |candidate| match candidate.and_then(|i| self.local_index(i)) {
                Ok(Some(i)) => Some(Ok(i)),
                Ok(None) => None,
                Err(error) => Some(Err(error)),
            },
        )))
    }
    fn check_indexed_bounds(&self, index: usize, expected: crate::Aabb) -> Result<()> {
        self.input
            .check_indexed_bounds(self.source_index(index)?, expected)
    }
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
    joints: &ValidationConstraints,
    tol: StaticTolerances,
    state: &mut dyn ValidationState,
) -> Result<usize> {
    state.clear()?;
    let n = input.len();
    let mut count = n;
    for i in 0..n {
        put_node(
            state,
            i,
            Node {
                parent: i,
                size: 1,
                component: None,
                ordinal: None,
                anchored: false,
            },
        )?;
    }
    for i in 0..joints.anchor_len() {
        let id = joints.anchor(i)?;
        let i = input.body_index(id)?.context("Anchored brick is absent")?;
        let mut row = node(state, i, n)?;
        row.anchored = true;
        put_node(state, i, row)?;
    }
    for i in 0..joints.len() {
        let joint = joints.joint(i)?;
        if join(
            state,
            input.body_index(joint.a)?.context("Joint body is absent")?,
            input.body_index(joint.b)?.context("Joint body is absent")?,
            n,
        )? {
            count -= 1;
        }
    }
    // Include reviewed stepped/compound bearing faces, not just mass envelopes:
    // e.g. a 4444 wing can support another brick outside its main body box.
    if count > 1 {
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
                if b <= a || root(state, a, n)? == root(state, b, n)? {
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
                    if join(state, a, b, n)? {
                        count -= 1;
                    }
                }
            }
        }
    }
    let mut components = 0;
    // Preserve source order within each component, including equal-height load
    // propagation ties. Original public piece IDs are never renumbered.
    for i in 0..input.len() {
        let r = root(state, i, n)?;
        let mut root_row = node(state, r, n)?;
        let component = if let Some(component) = root_row.component {
            component
        } else {
            let c = components;
            components += 1;
            root_row.component = Some(c);
            put_node(state, r, root_row)?;
            state.put(
                key(COMPONENT, &[c]),
                ValidationRecord::Component {
                    bodies: 0,
                    joints: 0,
                    anchored: false,
                },
            )?;
            c
        };
        let ValidationRecord::Component {
            mut bodies,
            joints,
            mut anchored,
        } = required(state, &key(COMPONENT, &[component]))?
        else {
            return Err(unavailable("invalid component metadata"));
        };
        let mut row = node(state, i, n)?;
        row.component = Some(component);
        row.ordinal = Some(bodies);
        anchored |= row.anchored;
        put_node(state, i, row)?;
        state.put(key(BODY, &[component, bodies]), ValidationRecord::Index(i))?;
        bodies += 1;
        state.put(
            key(COMPONENT, &[component]),
            ValidationRecord::Component {
                bodies,
                joints,
                anchored,
            },
        )?;
    }
    for j in 0..joints.len() {
        let joint = joints.joint(j)?;
        let a = node(
            state,
            input.body_index(joint.a)?.context("Joint body is absent")?,
            n,
        )?;
        let b = node(
            state,
            input.body_index(joint.b)?.context("Joint body is absent")?,
            n,
        )?;
        if a.component != b.component {
            return Err(unavailable("joint was split across mechanical components"));
        }
        let c = a
            .component
            .ok_or_else(|| unavailable("joint component is absent"))?;
        let ValidationRecord::Component {
            bodies,
            mut joints,
            anchored,
        } = required(state, &key(COMPONENT, &[c]))?
        else {
            return Err(unavailable("invalid component metadata"));
        };
        state.put(key(JOINT, &[c, joints]), ValidationRecord::Index(j))?;
        joints += 1;
        state.put(
            key(COMPONENT, &[c]),
            ValidationRecord::Component {
                bodies,
                joints,
                anchored,
            },
        )?;
    }
    if components != count {
        return Err(unavailable("component count changed"));
    }
    Ok(components)
}
