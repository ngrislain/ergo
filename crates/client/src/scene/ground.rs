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
            instances.entry(GeometryKey::Ground).or_default(),
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
            let color = ground.get(&cell).map_or(71, |block| block.color);
            instance(
                instances
                    .entry(GeometryKey::GroundStuds(segments))
                    .or_default(),
                DMat4::from_scale_rotation_translation(
                    DVec3::new(1., weight, 1.),
                    DQuat::IDENTITY,
                    position - camera.origin,
                ),
                ergo_world::color_rgb(color),
                false,
            );
            count += 1;
        }
    }
    (instances, count)
}
