//! The authoring dependency index lives with the transaction's other disk state.
//! Authoring groups add conservative discovery edges; they are not physical welds.
use super::*;
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize)]
pub(super) enum GraphRow {
    Peer(ContentId),
    Joint(super::super::paged::BoundJoint),
    Fixed,
}
fn prefix(assembly: &ContentId, piece: &ContentId) -> Vec<u8> {
    [assembly.as_str().as_bytes(), piece.as_str().as_bytes()].concat()
}
fn key(assembly: &ContentId, piece: &ContentId, kind: u8, other: Option<&ContentId>) -> Vec<u8> {
    let mut key = prefix(assembly, piece);
    key.push(kind);
    if let Some(other) = other {
        key.extend_from_slice(other.as_str().as_bytes());
    }
    key
}

impl Working {
    pub(super) fn stage_graph(&mut self, rules: &Rules) -> SharedResult<()> {
        self.graphs.remove_prefix(rules.id.as_str().as_bytes())?;
        for edge in rules.dependency_edges() {
            let (a, b) = edge?;
            let (a, b) = (&rules.pieces[&a], &rules.pieces[&b]);
            self.graphs
                .insert(key(&rules.id, a, 0, Some(b)), GraphRow::Peer(b.clone()))?;
        }
        for spec in &rules.joints {
            let joint = super::super::paged::BoundJoint {
                a: rules.pieces[&spec.a].clone(),
                b: rules.pieces[&spec.b].clone(),
                spec: spec.clone(),
            };
            let id = ContentId::of(&canonical_cbor(&joint)?);
            for endpoint in [&joint.a, &joint.b] {
                self.graphs.insert(
                    key(&rules.id, endpoint, 1, Some(&id)),
                    GraphRow::Joint(joint.clone()),
                )?;
            }
        }
        for local in &rules.fixed_bricks {
            self.graphs.insert(
                key(&rules.id, &rules.pieces[local], 2, None),
                GraphRow::Fixed,
            )?;
        }
        Ok(())
    }
    pub(super) fn graph_peers(&mut self, piece: &Piece) -> SharedResult<BTreeSet<ContentId>> {
        let prefix = prefix(&piece.assembly, &piece.id);
        let mut after = None;
        let mut peers = BTreeSet::new();
        loop {
            let page = self.graphs.page(&prefix, after.as_ref(), 128)?;
            if page.is_empty() {
                break;
            }
            for (key, row) in page {
                match row {
                    GraphRow::Peer(id) => {
                        peers.insert(id);
                    }
                    GraphRow::Joint(joint) => {
                        self.add_joint(ContentId::of(&canonical_cbor(&joint)?), joint)?
                    }
                    GraphRow::Fixed => self.fixed.insert(piece.id.clone(), ())?,
                }
                after = Some(key);
            }
        }
        Ok(peers)
    }
}
