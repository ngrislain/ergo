use anyhow::Result;
use ergo_transactions::{ContentId, OctreePath};
use ergo_world::{Aabb, BLOCK_SIZE, BlockCoord};

pub fn root(q: u8) -> OctreePath {
    OctreePath::new(vec![q]).unwrap()
}
pub fn block_path(coord: BlockCoord) -> OctreePath {
    let mut p = vec![1];
    p.extend(OctreePath::for_block(coord.into()).octants());
    OctreePath::new(p).unwrap()
}
pub fn entity_path(kind: u8, id: &ContentId) -> OctreePath {
    let bytes = hex::decode(id.as_str()).expect("validated content ID");
    let bits: Vec<_> = [0u8, 0]
        .into_iter()
        .chain(
            bytes
                .into_iter()
                .flat_map(|b| (0..8).rev().map(move |i| (b >> i) & 1)),
        )
        .collect();
    let mut p = vec![kind];
    p.extend(bits.chunks(3).map(|b| b[0] * 4 + b[1] * 2 + b[2]));
    OctreePath::new(p).unwrap()
}
/// Exact block-coordinate cover, with a conservative full-spatial fallback for
/// enormous sweeps. Traversal is still paginated and bounded by the working set.
pub fn cover(bounds: Aabb) -> Result<Vec<OctreePath>> {
    let min = ergo_world::coordinates::public(bounds.min);
    let max = ergo_world::coordinates::public(bounds.max);
    let lo = [min.x.min(max.x), min.y.min(max.y), min.z.min(max.z)]
        .map(|v| (((v / BLOCK_SIZE).floor() as i64 as u64) ^ (1 << 63)) as u128);
    let hi = [min.x.max(max.x), min.y.max(max.y), min.z.max(max.z)]
        .map(|v| (((v / BLOCK_SIZE).floor() as i64 as u64) ^ (1 << 63)) as u128);
    let mut stack = vec![(root(1), [0u128; 3], 1u128 << 64)];
    let mut regions = Vec::new();
    while let Some((path, origin, size)) = stack.pop() {
        if (0..3).any(|a| origin[a] > hi[a] || origin[a] + size <= lo[a]) {
            continue;
        }
        if (0..3).all(|a| origin[a] >= lo[a] && origin[a] + size - 1 <= hi[a]) {
            regions.push(path);
            continue;
        }
        if regions.len() + stack.len() > 4096 {
            return Ok(vec![root(1)]);
        }
        let half = size / 2;
        for q in 0..8u8 {
            stack.push((
                path.child(q)?,
                std::array::from_fn(|a| origin[a] + if q & (1 << a) != 0 { half } else { 0 }),
                half,
            ));
        }
    }
    Ok(regions)
}
