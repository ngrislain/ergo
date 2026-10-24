//! Stud detail has its own projected-size transition, separate from body LOD.
pub const STUD_FLAT_PX: f64 = 1.25;
pub const STUD_FULL_PX: f64 = 6.0;
pub const GROUND_STUD_RADIUS: f64 = 94.0;
pub const GROUND_GRID_RADIUS: i64 = 96;
fn smoothstep(a: f64, b: f64, x: f64) -> f64 {
    let t = ((x - a) / (b - a)).clamp(0., 1.);
    t * t * (3. - 2. * t)
}
pub fn stud_geometry(pixels: f64) -> f64 {
    smoothstep(STUD_FLAT_PX, STUD_FULL_PX, pixels)
}
pub fn ground_geometry(distance: f64, projection_pixels: f64, range: f64) -> f64 {
    stud_geometry(projection_pixels * 0.6 / distance.max(0.1))
        * (1. - smoothstep(range * 0.6, range, distance))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn stud_transition_is_continuous_monotonic_and_not_a_body_size_cutoff() {
        assert_eq!(stud_geometry(1.), 0.);
        assert_eq!(stud_geometry(8.), 1.);
        let values = (0..1000)
            .map(|i| stud_geometry(i as f64 / 100.))
            .collect::<Vec<_>>();
        assert!(
            values
                .windows(2)
                .all(|v| v[1] >= v[0] && v[1] - v[0] < 0.004)
        );
        let values = (1..6000)
            .map(|i| ground_geometry(i as f64 / 100., 600., GROUND_STUD_RADIUS))
            .collect::<Vec<_>>();
        assert!(
            values
                .windows(2)
                .all(|v| v[1] <= v[0] && v[0] - v[1] < 0.002)
        );
        assert_eq!(
            ground_geometry(GROUND_STUD_RADIUS, 600., GROUND_STUD_RADIUS),
            0.
        );
    }
}
