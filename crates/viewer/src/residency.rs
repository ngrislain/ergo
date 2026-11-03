//! Coverage is measured in world units, independently of storage-cell size.
pub const PREFETCH_DISTANCE: f64 = ergo_client::scene::PREFETCH_DISTANCE;
pub const MAX_RADIUS: u8 = 32;
pub const MIN_RADIUS: u8 = 5;

pub fn radius(fog_distance: f64) -> u8 {
    ((fog_distance + PREFETCH_DISTANCE) / ergo_world::BLOCK_SIZE)
        .ceil()
        .clamp(f64::from(MIN_RADIUS), f64::from(MAX_RADIUS)) as u8
}

pub fn visible_distance(radius: u8) -> f64 {
    (f64::from(radius) * ergo_world::BLOCK_SIZE - PREFETCH_DISTANCE).max(200.)
}

pub fn contains(centre: [i64; 3], radius: u8, cell: ergo_world::BlockCoord) -> bool {
    let cell: [i64; 3] = cell.into();
    (0..3)
        .all(|axis| (i128::from(cell[axis]) - i128::from(centre[axis])).abs() <= i128::from(radius))
}

/// Background/caster residency must not zoom the camera out to distant models.
/// Fit complete resident assemblies that intersect the requested local area.
pub fn focus_bounds(
    bricks: &[ergo_world::Brick],
    assemblies: &std::collections::BTreeMap<u32, String>,
    centre: [i64; 3],
) -> ergo_world::Aabb {
    let origin = ergo_world::BlockCoord::from(centre).origin();
    let span = glam::DVec3::splat(ergo_world::BLOCK_SIZE);
    let local = ergo_world::Aabb::new(origin - span, origin + span * 2.);
    let nearby: std::collections::HashSet<_> = bricks
        .iter()
        .filter(|b| b.render_bounds.intersects(local))
        .filter_map(|b| assemblies.get(&b.id))
        .collect();
    bricks
        .iter()
        .filter(|b| {
            b.render_bounds.intersects(local)
                || assemblies.get(&b.id).is_some_and(|id| nearby.contains(id))
        })
        .map(|b| b.render_bounds)
        .reduce(ergo_world::Aabb::union)
        .unwrap_or(ergo_world::Aabb::new(origin, origin + span))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn viewing_distance_and_prefetch_survive_small_cells() {
        let r = radius(2200.);
        assert!(f64::from(r) * ergo_world::BLOCK_SIZE >= 2200. + PREFETCH_DISTANCE);
        assert!(visible_distance(r) >= 2200.);
        assert!(contains([0, 0, 0], r, [6, -4, 0].into()));
        assert!(!contains([0, 0, 0], r, [i64::from(r) + 1, 0, 0].into()));
        assert!(radius(4096.) <= MAX_RADIUS);
    }
    #[test]
    fn coverage_handles_negative_north_and_vertical_cells() {
        assert!(contains([-4, -8, -2], 4, [-8, -12, -6].into()));
        assert!(!contains([-4, -8, -2], 4, [-8, -13, -6].into()));
    }

    #[test]
    fn framing_includes_the_whole_nearby_assembly_but_not_distant_prefetch() {
        let mut demo = ergo_world::demo::Demo::new();
        demo.add("3003", 4, 640., 0., 0.);
        demo.add("3003", 4, 1008., 0., 0.);
        demo.add("3003", 4, 2304., 0., 0.);
        let assemblies = std::collections::BTreeMap::from([
            (1, "park".into()),
            (2, "park".into()),
            (3, "district".into()),
        ]);
        let fitted = focus_bounds(&demo.bricks, &assemblies, [5, 0, 0]);
        assert!(fitted.intersects(demo.bricks[0].render_bounds));
        assert!(fitted.intersects(demo.bricks[1].render_bounds));
        assert!(!fitted.intersects(demo.bricks[2].render_bounds));
        let empty = focus_bounds(&demo.bricks, &assemblies, [50, 0, 0]);
        assert_eq!(empty.min, ergo_world::BlockCoord::from([50, 0, 0]).origin());
    }
}
