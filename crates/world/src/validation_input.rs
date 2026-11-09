//! Physical body access for the one validation gate. Native inputs can page
//! poses and conservative spatial candidates; local/WASM inputs borrow a slice.
use crate::{Aabb, Brick, spatial::Octree};
use anyhow::Result;
use std::{borrow::Cow, collections::HashMap};

/// Immutable for the duration of a validation. Every ordinal must resolve to
/// exactly one body, and body IDs must resolve back to that same ordinal.
pub trait ValidationInput {
    fn len(&self) -> usize;
    fn is_empty(&self) -> bool {
        self.len() == 0
    }
    fn body(&self, index: usize) -> Result<Cow<'_, Brick>>;
    fn body_index(&self, id: u32) -> Result<Option<usize>>;
    /// Enumerate all intersecting conservative candidates exactly once, in
    /// strictly increasing ordinal order, without truncation.
    /// Invalid/missing storage records must fail, not become empty space.
    fn query(&self, bounds: Aabb) -> Result<Box<dyn Iterator<Item = Result<usize>> + '_>>;
    /// Refuse missing records or an index box that does not contain `expected`.
    fn check_indexed_bounds(&self, index: usize, expected: Aabb) -> Result<()>;
}
/// One envelope covers all candidate queries used by geometry and island
/// discovery. Exact pair predicates still decide collisions and physical edges.
pub fn validation_envelope(brick: &Brick) -> Aabb {
    crate::part_profiles::bearing_bounds(brick)
        .union(crate::attachments::bounds(brick))
        .union(
            crate::part_profiles::collision_boxes(&brick.part)
                .into_iter()
                .map(|b| b.transformed(brick.transform, brick.position))
                .fold(brick.bounds, Aabb::union),
        )
}
pub(crate) struct SliceInput<'a> {
    bodies: &'a [Brick],
    index: HashMap<u32, usize>,
    tree: Octree,
}
impl<'a> SliceInput<'a> {
    pub fn new(bodies: &'a [Brick]) -> Self {
        Self {
            bodies,
            index: bodies.iter().enumerate().map(|(i, b)| (b.id, i)).collect(),
            tree: Octree::new(bodies.iter().map(validation_envelope).collect()),
        }
    }
}
impl ValidationInput for SliceInput<'_> {
    fn len(&self) -> usize {
        self.bodies.len()
    }
    fn body(&self, index: usize) -> Result<Cow<'_, Brick>> {
        Ok(Cow::Borrowed(self.bodies.get(index).ok_or_else(|| {
            anyhow::anyhow!("Physical body is absent")
        })?))
    }
    fn body_index(&self, id: u32) -> Result<Option<usize>> {
        Ok(self.index.get(&id).copied())
    }
    fn query(&self, bounds: Aabb) -> Result<Box<dyn Iterator<Item = Result<usize>> + '_>> {
        let mut ids = self.tree.query(bounds);
        ids.sort_unstable();
        Ok(Box::new(ids.into_iter().map(Ok)))
    }
    fn check_indexed_bounds(&self, _: usize, _: Aabb) -> Result<()> {
        Ok(())
    } // immutable slice/index constructed together
}
