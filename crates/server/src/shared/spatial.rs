//! Adaptive block-local octrees. Own references live under 0; child cells under
//! 1/q. One reference per piece/block avoids replicating a large mesh into every
//! fine cell. A separate allocated-block directory changes only on first use.
use super::*;
use ergo_world::{Aabb, BLOCK_SIZE, BlockCoord};

#[derive(Clone, Serialize, Deserialize)]
pub struct SpatialRef {
    pub coord: BlockCoord,
    pub piece: ContentId,
    pub bounds: Aabb,
}
#[derive(Clone, Serialize, Deserialize)]
pub struct BlockHeader {
    pub coord: BlockCoord,
    #[serde(deserialize_with = "super::paged::current_paged")]
    pub paged: bool,
}
pub fn directory(coord: BlockCoord) -> OctreePath {
    let mut p = block_path(coord).octants().to_vec();
    p[0] = 5;
    OctreePath::new(p).unwrap()
}
pub fn coordinate(path: &OctreePath) -> SharedResult<BlockCoord> {
    ensure!(path.octants().len() == 65, "Invalid block directory path");
    let mut p = [0u64; 3];
    for q in &path.octants()[1..] {
        for (a, v) in p.iter_mut().enumerate() {
            *v = (*v << 1) | u64::from((q >> a) & 1);
        }
    }
    Ok(p.map(|v| (v ^ (1 << 63)) as i64).into())
}
fn public(bounds: Aabb) -> ([f64; 3], [f64; 3]) {
    let a = ergo_world::coordinates::public(bounds.min);
    let b = ergo_world::coordinates::public(bounds.max);
    (a.min(b).to_array(), a.max(b).to_array())
}
fn reference(piece: &ContentId, coord: BlockCoord, bounds: Aabb) -> OctreePath {
    let (lo, hi) = public(bounds);
    let c: [i64; 3] = coord.into();
    let mut origin = c.map(|v| v as f64 * BLOCK_SIZE);
    let mut size = BLOCK_SIZE;
    let mut path = block_path(coord);
    let lo = std::array::from_fn::<_, 3, _>(|a| lo[a].max(origin[a]));
    let hi = std::array::from_fn::<_, 3, _>(|a| (hi[a] - 1e-8).min(origin[a] + BLOCK_SIZE));
    for _ in 0..10 {
        let half = size / 2.;
        let mut q = 0u8;
        let mut split = false;
        for a in 0..3 {
            let mid = origin[a] + half;
            if lo[a] >= mid {
                q |= 1 << a;
            } else if hi[a] >= mid {
                split = true;
                break;
            }
        }
        if split {
            break;
        }
        path = path.child(1).unwrap().child(q).unwrap();
        for (a, v) in origin.iter_mut().enumerate() {
            if q & (1 << a) != 0 {
                *v += half;
            }
        }
        size = half;
    }
    paged::append(
        &path.child(0).unwrap(),
        &entity_path(0, piece).octants()[1..],
    )
}
pub fn record(piece: &Piece, coord: BlockCoord, bounds: Aabb) -> (OctreePath, SpatialRef) {
    (
        reference(&piece.id, coord, bounds),
        SpatialRef {
            coord,
            piece: piece.id.clone(),
            bounds,
        },
    )
}

/// Layout-2 spatial keys already contain the full birth ID. A bounded visual
/// enumeration needs that ID, not the separate bounding-box payload.
pub(super) fn reference_piece(path: &OctreePath, coord: BlockCoord) -> SharedResult<ContentId> {
    let block = block_path(coord);
    let suffix = path
        .octants()
        .strip_prefix(block.octants())
        .context("Spatial block mismatch")?;
    ensure!(suffix.len() >= 87, "Invalid spatial reference path");
    let (cell, identity) = suffix.split_at(suffix.len() - 86);
    ensure!(
        cell.len() <= 21
            && cell.len() % 2 == 1
            && cell.last() == Some(&0)
            && cell[..cell.len() - 1]
                .chunks_exact(2)
                .all(|pair| pair[0] == 1)
            && identity[0] <= 1,
        "Invalid spatial reference path"
    );
    let mut bytes = [0u8; 32];
    for bit in 0..256 {
        let offset = bit + 2;
        bytes[bit / 8] |= ((identity[offset / 3] >> (2 - offset % 3)) & 1) << (7 - bit % 8);
    }
    Ok(hex::encode(bytes).parse()?)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn spatial_keys_round_trip_complete_birth_ids_at_every_depth() {
        for id in [
            "00".repeat(32),
            "ff".repeat(32),
            hex::encode((0..32).collect::<Vec<u8>>()),
        ] {
            let id: ContentId = id.parse().unwrap();
            for coord in [[0, 0, 0], [-1, 19, 0], [19, -1, -1]] {
                let coord: BlockCoord = coord.into();
                let block = block_path(coord);
                let mut cell = block.clone();
                for depth in 0..=10 {
                    let key =
                        paged::append(&cell.child(0).unwrap(), &entity_path(0, &id).octants()[1..]);
                    assert_eq!(reference_piece(&key, coord).unwrap(), id);
                    if depth < 10 {
                        cell = cell.child(1).unwrap().child((depth % 8) as u8).unwrap();
                    }
                }
                assert!(reference_piece(&block, coord).is_err());
                let invalid = paged::append(
                    &block.child(2).unwrap(),
                    &entity_path(0, &id).octants()[1..],
                );
                assert!(reference_piece(&invalid, coord).is_err());
            }
        }
    }
}

impl Working {
    pub async fn replace_spatial(
        &mut self,
        old: Option<&Piece>,
        new: Option<&Piece>,
    ) -> SharedResult<()> {
        let old_box = if let Some(p) = old {
            Some(self.brick(p, 1).await?.render_bounds)
        } else {
            None
        };
        let new_box = if let Some(p) = new {
            Some(self.brick(p, 1).await?.render_bounds)
        } else {
            None
        };
        if let Some(p) = old {
            for &coord in &p.replicas {
                let path = reference(&p.id, coord, old_box.unwrap());
                if new.is_none_or(|n| {
                    !n.replicas.contains(&coord)
                        || reference(&n.id, coord, new_box.unwrap()) != path
                }) {
                    self.tx.remove(&path).await?;
                }
            }
        }
        if let Some(p) = new {
            for &coord in &p.replicas {
                if self
                    .tx
                    .get::<BlockHeader>(&directory(coord))
                    .await?
                    .is_none()
                {
                    self.new_blocks.insert(coord);
                    self.tx
                        .put(&directory(coord), &BlockHeader { coord, paged: true })
                        .await?;
                }
                let (path, row) = record(p, coord, new_box.unwrap());
                let unchanged = old.is_some_and(|o| {
                    o.replicas.contains(&coord)
                        && reference(&o.id, coord, old_box.unwrap()) == path
                        && old_box.map(|b| (b.min, b.max)) == new_box.map(|b| (b.min, b.max))
                });
                if !unchanged {
                    self.charge(&row)?;
                    self.tx.put(&path, &row).await?;
                }
            }
        }
        Ok(())
    }
    pub async fn spatial_neighbors(&mut self, bounds: Aabb) -> SharedResult<BTreeSet<ContentId>> {
        self.spatial_neighbors_excluding(bounds, &BTreeSet::new())
            .await
    }
    pub(super) async fn spatial_neighbors_excluding(
        &mut self,
        bounds: Aabb,
        excluded: &BTreeSet<ContentId>,
    ) -> SharedResult<BTreeSet<ContentId>> {
        let mut result = BTreeSet::new();
        let (query_lo, query_hi) = public(bounds);
        for region in address::cover(bounds)? {
            let mut p = region.octants().to_vec();
            p[0] = 5;
            let directory = OctreePath::new(p)?;
            self.regions.insert(region.clone());
            let mut after = None;
            loop {
                let page = self.tx.scan_page(&directory, after.as_ref(), 128).await?;
                for (path, cid) in page.leaves {
                    let h: BlockHeader = self.tx.blob_limited(&cid, self.remaining()).await?;
                    self.charge(&h)?;
                    ensure!(coordinate(&path)? == h.coord, "Block directory mismatch");
                    let c: [i64; 3] = h.coord.into();
                    let mut stack = vec![(
                        block_path(h.coord),
                        c.map(|v| v as f64 * BLOCK_SIZE),
                        BLOCK_SIZE,
                        0,
                    )];
                    while let Some((prefix, origin, size, depth)) = stack.pop() {
                        if (0..3).any(|a| origin[a] > query_hi[a] || origin[a] + size < query_lo[a])
                        {
                            continue;
                        }
                        let full = (0..3)
                            .all(|a| origin[a] >= query_lo[a] && origin[a] + size <= query_hi[a]);
                        let own = if full {
                            prefix.clone()
                        } else {
                            prefix.child(0)?
                        };
                        let mut cursor = None;
                        loop {
                            let refs = self.tx.scan_page(&own, cursor.as_ref(), 128).await?;
                            for (path, cid) in refs.leaves {
                                if !excluded.is_empty()
                                    && excluded.contains(&reference_piece(&path, h.coord)?)
                                {
                                    continue;
                                }
                                let r = self.spatial_ref(&cid).await?;
                                if r.bounds.intersects(bounds)
                                    && let Some(piece) = self.piece(&r.piece).await?
                                    && self.brick(&piece, 1).await?.bounds.intersects(bounds)
                                {
                                    result.insert(r.piece.clone());
                                }
                            }
                            cursor = refs.next;
                            if cursor.is_none() {
                                break;
                            }
                        }
                        if !full && depth < 10 {
                            let half = size / 2.;
                            for q in 0..8u8 {
                                let origin = std::array::from_fn(|a| {
                                    origin[a] + if q & (1 << a) != 0 { half } else { 0. }
                                });
                                if (0..3).any(|a| {
                                    origin[a] > query_hi[a] || origin[a] + half < query_lo[a]
                                }) {
                                    continue;
                                }
                                let child = prefix.child(1)?.child(q)?;
                                if self.tx.directory_exists(&child).await? {
                                    stack.push((child, origin, half, depth + 1));
                                }
                            }
                        }
                    }
                }
                after = page.next;
                if after.is_none() {
                    break;
                }
            }
        }
        Ok(result)
    }
}
