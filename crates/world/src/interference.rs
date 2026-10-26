//! Nominal stud clearance for rectangular catalogue parts. Body-only boxes omit
//! studs, but their volume is permitted inside another body only at a registered
//! bottom cavity, never through a sideways solid face.
use crate::{
    Brick,
    geometry::part_spec,
    physical::{SCALE, STUD_DIAMETER_M, STUD_HEIGHT_M, metric_transform},
    spatial::Octree,
};
use glam::DVec3;

pub fn axis_aligned(brick: &Brick) -> bool {
    let rotation = metric_transform(brick.transform);
    rotation.to_cols_array_2d().iter().all(|axis| {
        axis.iter().filter(|value| value.abs() > 1e-8).count() == 1
            && axis.iter().any(|value| (value.abs() - 1.).abs() < 1e-8)
    })
}

pub fn first_stud_collision(bricks: &[Brick], tolerance: f64) -> Option<String> {
    let tree = Octree::new(bricks.iter().map(|b| b.bounds).collect());
    let radius = STUD_DIAMETER_M / 2.;
    let epsilon = tolerance * SCALE.min_element();
    for source in bricks {
        let Some(spec) = part_spec(&source.part).filter(|spec| spec.studs) else {
            continue;
        };
        if !axis_aligned(source) {
            continue;
        }
        let up = metric_transform(source.transform) * DVec3::Y;
        let axis = if up.x.abs() > 0.5 {
            0
        } else if up.y.abs() > 0.5 {
            1
        } else {
            2
        };
        for x in 0..spec.width {
            for z in 0..spec.depth {
                let local = DVec3::new(
                    f64::from(x) + 0.5 - f64::from(spec.width) / 2.,
                    0.,
                    f64::from(z) + 0.5 - f64::from(spec.depth) / 2.,
                );
                let base = source.position + source.transform * local;
                let center = base * SCALE + up * STUD_HEIGHT_M / 2.;
                let mut half = DVec3::splat(radius);
                half[axis] = STUD_HEIGHT_M / 2.;
                let query = crate::Aabb::new((center - half) / SCALE, (center + half) / SCALE);
                for index in tree.query(query) {
                    let target = &bricks[index];
                    if target.id == source.id || !axis_aligned(target) {
                        continue;
                    }
                    let lo = target.bounds.min * SCALE;
                    let hi = target.bounds.max * SCALE;
                    if (center[axis] + half[axis]).min(hi[axis])
                        - (center[axis] - half[axis]).max(lo[axis])
                        <= epsilon
                    {
                        continue;
                    }
                    let nearest = center.clamp(lo, hi);
                    let mut radial = center - nearest;
                    radial[axis] = 0.;
                    if radial.length() >= radius - epsilon {
                        continue;
                    }
                    let mut cavity = false;
                    if let Some(other) = part_spec(&target.part) {
                        let target_up = metric_transform(target.transform) * DVec3::Y;
                        if up.dot(target_up) > 1. - 1e-8 {
                            let point = target.transform.inverse() * (base - target.position);
                            let u = point.x + f64::from(other.width) / 2. - 0.5;
                            let v = point.z + f64::from(other.depth) / 2. - 0.5;
                            cavity = point.y <= -other.height + tolerance
                                && point.y >= -other.height - STUD_HEIGHT_M / SCALE.y - tolerance
                                && (u - u.round()).abs() < tolerance
                                && (v - v.round()).abs() < tolerance
                                && u >= -tolerance
                                && u <= f64::from(other.width - 1) + tolerance
                                && v >= -tolerance
                                && v <= f64::from(other.depth - 1) + tolerance;
                        }
                    }
                    if !cavity {
                        return Some(format!(
                            "Stud interference: brick {} ({}) at {:?} penetrates the solid body of brick {} ({}) at {:?}; no registered bottom cavity matches this orientation",
                            source.id,
                            source.part,
                            crate::coordinates::public(source.position),
                            target.id,
                            target.part,
                            crate::coordinates::public(target.position)
                        ));
                    }
                }
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn studs_enter_registered_bottoms_but_not_sideways_solids() {
        let mut d = crate::demo::Demo::new();
        d.add("41539", 19, 0., 0., 0.);
        d.add("3003", 14, 2., 1. / 3., 2.);
        assert!(first_stud_collision(&d.bricks, 1e-4).is_none());
        let brick = &mut d.bricks[1];
        brick.transform = crate::physical::world_transform(glam::DMat3::from_rotation_z(
            std::f64::consts::FRAC_PI_2,
        ));
        let local = d.meshes["3003.dat"]
            .body_bounds
            .transformed(brick.transform, DVec3::ZERO);
        brick.position = DVec3::new(3., 1. / 3. - local.min.y, 3.);
        brick.bounds = d.meshes["3003.dat"]
            .body_bounds
            .transformed(brick.transform, brick.position);
        let error = first_stud_collision(&d.bricks, 1e-4).unwrap();
        assert!(error.contains("brick 1") && error.contains("brick 2"));
    }
}
