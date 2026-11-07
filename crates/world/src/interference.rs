//! Nominal stud clearance for rectangular catalogue parts. Body-only boxes omit
//! studs, but their volume is permitted inside another body only at a registered
//! bottom cavity, never through a sideways solid face.
use crate::{
    Brick,
    part_profiles::{receiving_grids, stud_centres},
    physical::{SCALE, STUD_DIAMETER_M, STUD_HEIGHT_M, metric_transform},
    spatial::Octree,
};
use glam::DVec3;

/// Reviewed clearance of a 3.2 mm antenna/lance shaft through the moulded flag bore.
/// This recognizes only coaxial, upright catalogue pairs within the straight
/// shaft's axial range, not arbitrary overlap, retention or joint strength.
pub fn registered_bar_clearance(a: &Brick, b: &Brick, tolerance: f64) -> bool {
    let lance_pair = match (
        crate::catalog::canonical_id(&a.part),
        crate::catalog::canonical_id(&b.part),
    ) {
        ("3849.dat", "4495a.dat") => Some((a, b)),
        ("4495a.dat", "3849.dat") => Some((b, a)),
        _ => None,
    };
    if let Some((pole, flag)) = lance_pair {
        if !axis_aligned(pole)
            || !axis_aligned(flag)
            || (flag.transform.y_axis - DVec3::Y).length() > 1e-8
        {
            return false;
        }
        let c = crate::ldraw::conversion();
        // Official 3849.dat: radius-4 LDU straight shaft, source Z -146..-14.
        // The guard, grip and rounded tip are excluded from the clearance.
        let low = pole.position + pole.transform * (c * DVec3::new(0., 0., -14.));
        let high = pole.position + pole.transform * (c * DVec3::new(0., 0., -146.));
        return (high.x - low.x).abs() <= tolerance
            && (high.z - low.z).abs() <= tolerance
            && high.y > low.y
            && (flag.position.x - low.x).abs() <= tolerance
            && (flag.position.z - low.z).abs() <= tolerance
            && flag.position.y - 1. >= low.y - tolerance
            && flag.position.y + 1. / 6. <= high.y + tolerance;
    }
    let (pole, flag) = match (
        crate::catalog::canonical_id(&a.part),
        crate::catalog::canonical_id(&b.part),
    ) {
        ("3957a.dat", "4495a.dat") => (a, b),
        ("4495a.dat", "3957a.dat") => (b, a),
        _ => return false,
    };
    if !axis_aligned(pole)
        || !axis_aligned(flag)
        || (pole.transform.y_axis - DVec3::Y).length() > 1e-8
        || (flag.transform.y_axis - DVec3::Y).length() > 1e-8
    {
        return false;
    }
    let delta = flag.position - pole.position;
    delta.x.abs() <= tolerance
        && delta.z.abs() <= tolerance
        // The lower cup must clear the wider antenna foot. The top of the
        // hollow stud may reach the rounded end, but never pass above it.
        && delta.y - 1. >= 1. / 6. - tolerance
        && delta.y + 1. / 6. <= 11. / 3. + tolerance
}

pub fn bodies_penetrate(a: &Brick, b: &Brick, tolerance: f64) -> bool {
    let overlap = a.bounds.max.min(b.bounds.max) - a.bounds.min.max(b.bounds.min);
    overlap.min_element() > tolerance
        && !registered_bar_clearance(a, b, tolerance)
        && !crate::attachments::registered_clearance(a, b, tolerance)
        && crate::attachments::boxes_penetrate(a, b, tolerance).unwrap_or(true)
}

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
        for local in stud_centres(&source.part) {
            let base = source.position + source.transform * local;
            let center = base * SCALE + up * STUD_HEIGHT_M / 2.;
            let mut half = DVec3::splat(radius);
            half[axis] = STUD_HEIGHT_M / 2.;
            let query = crate::Aabb::new((center - half) / SCALE, (center + half) / SCALE);
            for index in tree.query(query) {
                let target = &bricks[index];
                if target.id == source.id
                    || !axis_aligned(target)
                    || registered_bar_clearance(source, target, tolerance)
                {
                    continue;
                }
                // New compound mould profiles keep empty space between horse
                // legs/arch feet empty. Existing profiles retain their original
                // stud-clearance envelope; no prior part is reinterpreted.
                let explicit = crate::part_library::get(&target.part)
                    .and_then(|p| p.catalogue.nominal.as_ref())
                    .is_some_and(|n| !n.collision_boxes_ldu.is_empty());
                let bodies = if explicit {
                    crate::part_profiles::collision_boxes(&target.part)
                        .into_iter()
                        .map(|bounds| bounds.transformed(target.transform, target.position))
                        .collect()
                } else {
                    vec![target.bounds]
                };
                let intersects = bodies.iter().any(|bounds| {
                    let lo = bounds.min * SCALE;
                    let hi = bounds.max * SCALE;
                    if (center[axis] + half[axis]).min(hi[axis])
                        - (center[axis] - half[axis]).max(lo[axis])
                        <= epsilon
                    {
                        return false;
                    }
                    let mut radial = center - center.clamp(lo, hi);
                    radial[axis] = 0.;
                    radial.length() < radius - epsilon
                });
                if !intersects {
                    continue;
                }
                let mut cavity = false;
                for other in receiving_grids(&target.part) {
                    let target_up = metric_transform(target.transform) * DVec3::Y;
                    if up.dot(target_up) > 1. - 1e-8 {
                        let point = target.transform.inverse() * (base - target.position);
                        let u = point.x - other.first[0];
                        let v = point.z - other.first[1];
                        cavity |= point.y <= other.bottom + tolerance
                            && point.y >= other.bottom - STUD_HEIGHT_M / SCALE.y - tolerance
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
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    fn part(id: &str, position: DVec3) -> Brick {
        let mut demo = crate::demo::Demo::new();
        demo.add(id.trim_end_matches(".dat"), 0, 0., 0., 0.);
        let mut brick = demo.bricks.remove(0);
        brick.position = position;
        brick.bounds = crate::geometry::nominal_body_bounds(id)
            .unwrap()
            .transformed(brick.transform, position);
        brick
    }

    #[test]
    fn flag_bore_clearance_is_coaxial_and_restricted_to_the_real_antenna_shaft() {
        let pole = part("3957a.dat", DVec3::ZERO);
        let mut flag = part("4495a.dat", DVec3::new(0., 1.5, 0.));
        let tolerance = crate::validation::OVERLAP_TOLERANCE;
        assert!(registered_bar_clearance(&pole, &flag, tolerance));
        assert!(!bodies_penetrate(&pole, &flag, tolerance));
        flag.translated(DVec3::X * 0.01);
        assert!(bodies_penetrate(&pole, &flag, tolerance));
        flag.translated(DVec3::X * -0.01);
        flag.translated(DVec3::Y * -0.5);
        assert!(bodies_penetrate(&pole, &flag, tolerance));
        flag.translated(DVec3::Y * 3.);
        assert!(!registered_bar_clearance(&pole, &flag, tolerance));
        flag = part("4495a.dat", DVec3::new(0., 1.5, 0.));
        flag.transform =
            crate::physical::world_transform(glam::DMat3::from_rotation_z(std::f64::consts::PI));
        assert!(!registered_bar_clearance(&pole, &flag, tolerance));
        let unrelated = part("3005.dat", DVec3::new(0., 1.5, 0.));
        assert!(bodies_penetrate(&pole, &unrelated, tolerance));
    }
    #[test]
    fn a_jousting_lance_clears_only_the_flag_bore_on_its_straight_upright_shaft() {
        let mut pole = part("3849.dat", DVec3::ZERO);
        pole.transform = crate::physical::world_transform(glam::DMat3::from_rotation_x(
            -std::f64::consts::FRAC_PI_2,
        ));
        // Native X rotation puts the source north-pointing lance upwards.
        let c = crate::ldraw::conversion();
        if (pole.transform * (c * DVec3::new(0., 0., -146.))).y < 0. {
            pole.transform = crate::physical::world_transform(glam::DMat3::from_rotation_x(
                std::f64::consts::FRAC_PI_2,
            ));
        }
        pole.bounds = crate::geometry::nominal_body_bounds(&pole.part)
            .unwrap()
            .transformed(pole.transform, pole.position);
        let mut flag = part("4495a.dat", DVec3::new(0., 4., 0.));
        assert!(registered_bar_clearance(&pole, &flag, 0.002));
        flag.translated(DVec3::X * 0.02);
        assert!(!registered_bar_clearance(&pole, &flag, 0.002));
        flag.translated(-DVec3::X * 0.02 - DVec3::Y * 4.);
        assert!(!registered_bar_clearance(&pole, &flag, 0.002));
        flag.translated(DVec3::Y * 10.);
        assert!(!registered_bar_clearance(&pole, &flag, 0.002));
    }
    #[test]
    fn catalogue_frame_receives_registered_brick_studs_but_rejects_half_stud_shift() {
        let mut d = crate::demo::Demo::new();
        d.add("3001", 15, 0., 0., 0.);
        d.add("60594", 15, 0., 1., 0.);
        let bounds = crate::geometry::nominal_body_bounds("60594.dat").unwrap();
        let frame = &mut d.bricks[1];
        frame.position = DVec3::new(2., 4., 0.5);
        frame.bounds = bounds.transformed(frame.transform, frame.position);
        d.add("3001", 15, 0., 4., 0.);
        assert!(first_stud_collision(&d.bricks, 1e-4).is_none());
        let mut joints = crate::constraints::inferred_joints(&d.bricks);
        assert_eq!(joints.len(), 2);
        assert!(
            crate::physical::ConnectionMeasurements::default()
                .apply(&d.bricks, &mut joints)
                .is_err()
        );
        let estimates = crate::estimates::apply_stud_estimates(&d.bricks, &mut joints);
        assert_eq!(estimates.estimated_connections, 0);
        assert_eq!(estimates.unsupported_piece_pairs.len(), 2);
        d.bricks[1].translated(DVec3::X * 0.5);
        assert!(first_stud_collision(&d.bricks, 1e-4).is_some());
    }
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
