//! Bounded baseplate/stud instance planning; no browser or GPU dependencies.
use super::{Camera, DISPLAY_SCALE, GeometryKey, Instances, instance};
use ergo_world::{
    Aabb,
    terrain::{GROUND_TOP, GroundBlock},
};
use glam::{DMat4, DQuat, DVec3};
use std::collections::{HashMap, HashSet};

pub fn visible_cells(camera: &Camera, far: f64) -> Vec<(i64, i64)> {
    let cell = ergo_world::BlockCoord::at(camera.eye);
    let radius = (far / 1024.).ceil() as i64 + 1;
    let mut cells = Vec::new();
    for x in cell.0 - radius..=cell.0 + radius {
        for y in cell.2 - radius..=cell.2 + radius {
            let min = DVec3::new(x as f64 * 1024., -1. / 3., y as f64 * 1024.);
            if camera
                .visibility
                .contains(Aabb::new(min, min + DVec3::new(1024., 1. / 3., 1024.)))
            {
                cells.push((x, y));
            }
        }
    }
    cells
}

pub fn stud_range(pixels: f64) -> f64 {
    (pixels * 0.6 / crate::lod::STUD_FLAT_PX).min(crate::lod::GROUND_STUD_RADIUS)
}

pub fn plan(
    camera: &Camera,
    far: f64,
    ground: &HashMap<(i64, i64), GroundBlock>,
) -> (Instances, usize) {
    let cells = visible_cells(camera, far);
    let mut instances = Instances::new();
    for &(x, y) in &cells {
        let color = ground.get(&(x, y)).map_or(71, |block| block.color);
        let center = DVec3::new(
            (x as f64 + 0.5) * 1024.,
            GROUND_TOP,
            (y as f64 + 0.5) * 1024.,
        ) - camera.origin;
        instance(
            instances.entry(GeometryKey::Ground(x, y)).or_default(),
            DMat4::from_scale_rotation_translation(
                DVec3::new(1024., 1. / 3., 1024.),
                DQuat::IDENTITY,
                center,
            ),
            ergo_world::color_rgb(color),
            false,
        );
    }
    let range = stud_range(camera.projection_pixels);
    let mut count = 0;
    if cells.is_empty() || (camera.eye.y - GROUND_TOP).abs() * DISPLAY_SCALE.y >= range {
        return (instances, count);
    }
    let cells: HashSet<_> = cells.into_iter().collect();
    let center_x = camera.eye.x.floor() as i64;
    let center_z = camera.eye.z.floor() as i64;
    for x in center_x - crate::lod::GROUND_GRID_RADIUS..=center_x + crate::lod::GROUND_GRID_RADIUS {
        for z in
            center_z - crate::lod::GROUND_GRID_RADIUS..=center_z + crate::lod::GROUND_GRID_RADIUS
        {
            let cell = (x.div_euclid(1024), z.div_euclid(1024));
            if !cells.contains(&cell) {
                continue;
            }
            let position = DVec3::new(x as f64 + 0.5, GROUND_TOP, z as f64 + 0.5);
            let distance = ((position - camera.eye) * DISPLAY_SCALE).length();
            if distance > range
                || !camera.visibility.contains(Aabb::new(
                    position - DVec3::new(0.3, 0., 0.3),
                    position + DVec3::new(0.3, 1. / 6., 0.3),
                ))
            {
                continue;
            }
            let weight = crate::lod::ground_geometry(distance, camera.projection_pixels, range);
            if weight <= 0.005 {
                continue;
            }
            let segments = if camera.projection_pixels * 0.6 / distance.max(0.1) < 5. {
                8
            } else {
                16
            };
            let rgb = ground
                .get(&cell)
                .map_or(ergo_world::color_rgb(71), |block| {
                    block
                        .surface
                        .as_ref()
                        .map_or(ergo_world::color_rgb(block.color), |surface| {
                            surface.rgb_at(
                                x.rem_euclid(1024) as f64 + 0.5,
                                1024. - (z.rem_euclid(1024) as f64 + 0.5),
                            )
                        })
                });
            instance(
                instances
                    .entry(GeometryKey::GroundStuds(segments))
                    .or_default(),
                DMat4::from_scale_rotation_translation(
                    DVec3::new(1., weight, 1.),
                    DQuat::IDENTITY,
                    position - camera.origin,
                ),
                rgb,
                false,
            );
            count += 1;
        }
    }
    (instances, count)
}

#[cfg(test)]
mod tests {
    use super::*;
    use ergo_world::terrain::{GroundSurface, SAMPLE_GRID_SIZE};

    #[test]
    fn nearby_studs_share_the_surface_field_with_correct_negative_and_north_axes() {
        let mut block = GroundBlock::fallback(-1, -1, "fixture".into());
        block.surface = Some(GroundSurface {
            format: "ergo-ground-surface-1".into(),
            rgb: (0..SAMPLE_GRID_SIZE)
                .flat_map(|y| {
                    (0..SAMPLE_GRID_SIZE).map(move |x| ((x * 3) as u32) << 16 | (y * 3) as u32)
                })
                .collect(),
        });
        let camera = Camera::new(
            DVec3::new(-512., 12., 512.),
            DVec3::new(-512., 0., 530.),
            800.,
            600.,
            45.,
            1000.,
        );
        let (instances, count) = plan(&camera, 1000., &HashMap::from([((-1, 0), block.clone())]));
        assert!(instances.contains_key(&GeometryKey::Ground(-1, 0)));
        assert!(count > 0 && count <= 193 * 193);
        let mut colours = HashSet::new();
        for (key, data) in &instances {
            if !matches!(key, GeometryKey::GroundStuds(_)) {
                continue;
            }
            for item in data.chunks_exact(20) {
                let east = (item[12] as f64 + camera.origin.x).rem_euclid(1024.);
                let north = 1024. - (item[14] as f64 + camera.origin.z).rem_euclid(1024.);
                let expected = block.surface.as_ref().unwrap().rgb_at(east, north);
                let actual = [16, 17, 18]
                    .into_iter()
                    .fold(0, |out, i| (out << 8) | (item[i] * 255.).round() as u32);
                assert_eq!(actual, expected);
                colours.insert(actual);
            }
        }
        assert!(
            colours.len() > 10,
            "Nearby studs must not inherit one plate colour"
        );
    }
}
