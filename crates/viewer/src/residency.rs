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
}
