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
                            for (_, cid) in refs.leaves {
                                let r: SpatialRef =
                                    self.tx.blob_limited(&cid, self.remaining()).await?;
                                self.charge(&r)?;
                                if r.bounds.intersects(bounds)
                                    && let Some(piece) = self.piece(&r.piece).await?
                                    && self.brick(&piece, 1).await?.bounds.intersects(bounds)
                                {
                                    result.insert(r.piece);
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
