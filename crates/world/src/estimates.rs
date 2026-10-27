//! Explicit engineering assumptions for local diagnostics, never measurements.
use crate::{Brick, JointKind, JointSpec, constraints, physical::STUD_PITCH_M};
use glam::DVec3;
use serde::Serialize;
use std::collections::{HashMap, HashSet};

pub const SOURCE_PREFIX: &str = "estimate:ergo-stud-ballpark-v2";

/// Joint-frame geometry for separating compressive bearing from clutch demand.
pub struct StudBearing {
    normal_sign: f64,
    half_width_m: f64,
    half_depth_m: f64,
    anchor_to_patch_m: DVec3,
}
impl StudBearing {
    pub fn new(a: &Brick, b: &Brick, anchor: DVec3) -> Self {
        let lo = a.bounds.min.max(b.bounds.min);
        let hi = a.bounds.max.min(b.bounds.max);
        Self {
            normal_sign: if b.bounds.center().y > a.bounds.center().y {
                1.
            } else {
                -1.
            },
            half_width_m: (hi.x - lo.x).max(0.) * STUD_PITCH_M / 2.,
            half_depth_m: (hi.z - lo.z).max(0.) * STUD_PITCH_M / 2.,
            anchor_to_patch_m: crate::physical::to_meters((lo + hi) / 2. - anchor),
        }
    }
    pub fn demands(&self, force_on_a: DVec3, torque_on_a: DVec3) -> (f64, f64) {
        let axial = force_on_a.y * self.normal_sign;
        let compression = (-axial).max(0.);
        let force = DVec3::new(force_on_a.x, axial.max(0.), force_on_a.z).length();
        let at_patch = torque_on_a - self.anchor_to_patch_m.cross(force_on_a);
        let excess = DVec3::new(
            (at_patch.x.abs() - compression * self.half_depth_m).max(0.),
            at_patch.y,
            (at_patch.z.abs() - compression * self.half_width_m).max(0.),
        );
        (force, excess.length())
    }
}

#[derive(Debug, Serialize)]
pub struct StudEstimateReport {
    pub profile: &'static str,
    pub force_per_engaged_stud_n: f64,
    pub torque_lever_rule: &'static str,
    pub estimated_connections: usize,
    pub unsupported_piece_pairs: Vec<[u32; 2]>,
    pub measured: bool,
    pub interpretation: &'static str,
}

/// Opt-in approximation: 2 N per engaged stud; torque = force × one quarter of
/// the narrower engagement span in meters. Neither value is a measured bound.
/// Only registered ordinary stud pairs get estimates. Other joints stay unknown.
pub fn apply_stud_estimates(bricks: &[Brick], joints: &mut [JointSpec]) -> StudEstimateReport {
    let eligible: HashSet<_> = constraints::inferred_joints(bricks)
        .iter()
        .map(|j| (j.a.min(j.b), j.a.max(j.b)))
        .collect();
    let index: HashMap<_, _> = bricks.iter().map(|b| (b.id, b)).collect();
    let mut report = StudEstimateReport {
        profile: "ergo-stud-ballpark-v2",
        force_per_engaged_stud_n: 2.,
        torque_lever_rule: "one quarter of the narrower horizontal engagement span in meters",
        estimated_connections: 0,
        unsupported_piece_pairs: vec![],
        measured: false,
        interpretation: "Ballpark engineering assumptions for this diagnostic only; not measured calibration or a guaranteed strength bound. Compressive bearing inside the contact rectangle is not charged to clutch capacity; remaining tension/shear and excess bending/torsion use approximate scalar thresholds. Brick/stud crushing is not modeled.",
    };
    for joint in joints {
        joint.break_force = None;
        joint.break_torque = None;
        joint.strength_source = None;
        let pair = (joint.a.min(joint.b), joint.a.max(joint.b));
        if matches!(joint.kind, JointKind::Fixed)
            && eligible.contains(&pair)
            && let (Some(a), Some(b)) = (index.get(&joint.a), index.get(&joint.b))
            && crate::geometry::part_spec(&a.part).is_some()
            && crate::geometry::part_spec(&b.part).is_some()
        {
            let overlap = a.bounds.max.min(b.bounds.max) - a.bounds.min.max(b.bounds.min);
            let [width, depth] = [overlap.x.round(), overlap.z.round()];
            if width >= 1.
                && depth >= 1.
                && (overlap.x - width).abs() < 1e-4
                && (overlap.z - depth).abs() < 1e-4
            {
                let force = report.force_per_engaged_stud_n * width * depth;
                joint.break_force = Some(force);
                joint.break_torque = Some(force * width.min(depth) * STUD_PITCH_M / 4.);
                joint.strength_source = Some(SOURCE_PREFIX.into());
                report.estimated_connections += 1;
                continue;
            }
        }
        report.unsupported_piece_pairs.push([joint.a, joint.b]);
    }
    report
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn estimates_follow_engagement_and_do_not_glue_arbitrary_contacts() {
        let mut d = crate::demo::Demo::new();
        d.add("3003", 19, 2., 0., 2.);
        d.add("3003", 19, 2., 1., 2.);
        let mut joints = constraints::inferred_joints(&d.bricks);
        let report = apply_stud_estimates(&d.bricks, &mut joints);
        assert!(!report.measured);
        assert_eq!(report.estimated_connections, 1);
        assert_eq!(joints[0].break_force, Some(8.));
        assert_eq!(joints[0].break_torque, Some(0.032));
        joints[0].kind = JointKind::Revolute {
            axis: glam::DVec3::Y,
            limits: None,
        };
        let report = apply_stud_estimates(&d.bricks, &mut joints);
        assert_eq!(report.unsupported_piece_pairs.len(), 1);
        assert!(joints[0].break_force.is_none() && joints[0].strength_source.is_none());
    }
    #[test]
    fn bearing_does_not_spend_clutch_strength_on_supported_compression() {
        let mut d = crate::demo::Demo::new();
        d.add("3003", 19, 2., 0., 2.);
        d.add("3003", 19, 2., 1., 2.);
        let joint = constraints::inferred_joints(&d.bricks).remove(0);
        let frame = StudBearing::new(&d.bricks[0], &d.bricks[1], joint.anchor);
        // Force on the lower body: downward is compressive, upward is tensile.
        assert_eq!(frame.demands(DVec3::NEG_Y * 10., DVec3::ZERO), (0., 0.));
        assert_eq!(frame.demands(DVec3::Y * 10., DVec3::ZERO), (10., 0.));
        assert_eq!(frame.demands(DVec3::NEG_Y * 10., DVec3::X * 0.04), (0., 0.));
        assert!((frame.demands(DVec3::NEG_Y * 10., DVec3::X * 0.1).1 - 0.02).abs() < 1e-12);
        assert!((frame.demands(DVec3::NEG_Y * 10., DVec3::Y * 0.1).1 - 0.1).abs() < 1e-12);
        let reverse = StudBearing::new(&d.bricks[1], &d.bricks[0], joint.anchor);
        assert_eq!(
            frame.demands(DVec3::NEG_Y * 10., DVec3::X * 0.1),
            reverse.demands(DVec3::Y * 10., DVec3::NEG_X * 0.1)
        );
    }

    #[test]
    fn bearing_transports_an_off_center_joint_wrench_to_the_contact_patch() {
        let mut d = crate::demo::Demo::new();
        d.add("3001", 19, 2., 0., 2.);
        d.add("3003", 19, 2., 1., 2.);
        let joint = constraints::inferred_joints(&d.bricks).remove(0);
        let a = &d.bricks[0];
        let b = &d.bricks[1];
        let patch = (a.bounds.min.max(b.bounds.min) + a.bounds.max.min(b.bounds.max)) / 2.;
        let offset = crate::physical::to_meters(patch - joint.anchor);
        let force = DVec3::NEG_Y * 10.;
        let (linear, angular) =
            StudBearing::new(a, b, joint.anchor).demands(force, offset.cross(force));
        assert_eq!(linear, 0.);
        assert!(angular < 1e-12);
    }
}
