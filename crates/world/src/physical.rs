//! Construction coordinates are not SI and geographic addressing is not a metric.
//! Masses are catalog averages, not inferred from solid bounding-box volume.
use crate::{Brick, JointKind, JointSpec};
use anyhow::{Context, Result, ensure};
use glam::{DMat3, DVec3};
use serde::{Deserialize, Serialize};

pub const STUD_PITCH_M: f64 = 0.008;
pub const BRICK_HEIGHT_M: f64 = 0.0096;
pub const PLATE_HEIGHT_M: f64 = 0.0032;
pub const STUD_DIAMETER_M: f64 = 0.0048;
pub const STUD_HEIGHT_M: f64 = 0.0016;
pub const GRAVITY_M_S2: f64 = 9.80665;
pub const SCALE: DVec3 = DVec3::new(STUD_PITCH_M, BRICK_HEIGHT_M, STUD_PITCH_M);

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct GravityProbeSettings {
    pub steps: u32,
    /// Construction units; Z is 9.6 mm, horizontal units are 8 mm.
    pub position_tolerance: f64,
    pub rotation_tolerance_radians: f64,
}
impl Default for GravityProbeSettings {
    fn default() -> Self {
        Self {
            steps: 1,
            position_tolerance: 1e-3,
            rotation_tolerance_radians: 1e-4,
        }
    }
}
impl GravityProbeSettings {
    pub fn validate(&self) -> Result<()> {
        ensure!(
            (1..=120).contains(&self.steps),
            "Gravity probe steps must be 1–120"
        );
        ensure!(
            self.position_tolerance.is_finite()
                && (1e-12..=0.1).contains(&self.position_tolerance)
                && self.rotation_tolerance_radians.is_finite()
                && (1e-12..=0.1).contains(&self.rotation_tolerance_radians),
            "Gravity probe tolerances must be finite and between 1e-12 and 0.1"
        );
        Ok(())
    }
}
pub fn to_meters(p: DVec3) -> DVec3 {
    p * SCALE
}
pub fn to_world(p: DVec3) -> DVec3 {
    p / SCALE
}
pub fn metric_transform(m: DMat3) -> DMat3 {
    DMat3::from_diagonal(SCALE) * m * DMat3::from_diagonal(SCALE.recip())
}
pub fn world_transform(m: DMat3) -> DMat3 {
    DMat3::from_diagonal(SCALE.recip()) * m * DMat3::from_diagonal(SCALE)
}

pub fn is_proper_rotation(rotation: DMat3) -> bool {
    let error = rotation.transpose() * rotation - DMat3::IDENTITY;
    rotation.is_finite()
        && error.to_cols_array().iter().all(|n| n.abs() < 1e-5)
        && (rotation.determinant() - 1.).abs() < 1e-5
}

/// Safe visual/model pose, including the affine transforms permitted by LDraw.
pub fn validate_affine_pose(brick: &Brick) -> Result<()> {
    ensure!(
        brick.position.is_finite()
            && brick.position.abs().max_element() <= crate::MAX_WORLD_COORD
            && brick.transform.is_finite()
            && brick.transform.determinant().abs() > 1e-10,
        "Non-finite, singular or out-of-range brick pose"
    );
    Ok(())
}

pub fn validate_pose(brick: &Brick) -> Result<()> {
    validate_affine_pose(brick)?;
    let rotation = metric_transform(brick.transform);
    ensure!(
        is_proper_rotation(rotation),
        "Brick {} ({}) must be a proper metric isometry (translation + rotation): scale, shear and reflection are not allowed for simulation or publication",
        brick.id,
        brick.part
    );
    Ok(())
}

/// Reviewed catalogue averages from the pinned part library. Official identity
/// aliases select the same record; source-specific physical measurements remain separate.
pub fn mass_kg(part: &str) -> Result<f64> {
    crate::part_library::get(part)
        .map(|entry| entry.catalogue.mass_kg)
        .with_context(|| format!("No sourced mass for {part}"))
}
pub fn mass_source(part: &str) -> String {
    crate::part_library::get(part).map_or_else(
        || {
            format!(
                "https://www.bricklink.com/v2/catalog/catalogitem.page?P={}",
                part.trim_end_matches(".dat")
            )
        },
        |entry| entry.catalogue.mass_source.clone(),
    )
}

#[derive(Clone, Default, Serialize, Deserialize)]
pub struct ConnectionMeasurements {
    pub entries: Vec<ConnectionMeasurement>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub parts: Vec<PartMeasurement>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PartMeasurement {
    pub part: String,
    pub mass_kg: f64,
    pub body_bounds: crate::Aabb,
    pub source: String,
    pub samples: u32,
}
#[derive(Clone, Serialize, Deserialize)]
pub struct ConnectionMeasurement {
    pub part_a: String,
    pub part_b: String,
    pub kind: String,
    /// Contact footprint in stud cells. Do not scale a full overlap measurement
    /// linearly to partial overlap without an independently verified rule.
    pub overlap_studs: [u32; 2],
    pub conservative_force_limit_n: f64,
    pub conservative_torque_limit_nm: f64,
    pub source: String,
    pub samples: u32,
}

impl ConnectionMeasurements {
    pub fn part_record(&self, part: &str) -> Option<&PartMeasurement> {
        let canonical = crate::catalog::canonical_part(part);
        self.parts
            .iter()
            .find(|record| crate::catalog::canonical_part(&record.part) == canonical)
    }
    pub fn validate(&self) -> Result<()> {
        let mut names = std::collections::BTreeSet::new();
        for part in &self.parts {
            ensure!(
                crate::catalog::valid_part(&part.part)
                    && names.insert(crate::catalog::canonical_part(&part.part)),
                "Invalid or duplicate physical part ID"
            );
            ensure!(
                part.mass_kg.is_finite()
                    && part.mass_kg > 0.
                    && part.samples > 0
                    && !part.source.trim().is_empty(),
                "Part measurements require positive mass, source, and sample count"
            );
            ensure!(
                part.body_bounds.min.is_finite()
                    && part.body_bounds.max.is_finite()
                    && part.body_bounds.size().min_element() > 0.,
                "Invalid measured nominal part bounds"
            );
        }
        for e in &self.entries {
            ensure!(
                e.conservative_force_limit_n.is_finite()
                    && e.conservative_force_limit_n > 0.0
                    && e.conservative_torque_limit_nm.is_finite()
                    && e.conservative_torque_limit_nm > 0.0,
                "Connector capacities must be positive finite SI values"
            );
            ensure!(
                e.samples > 0 && !e.source.trim().is_empty(),
                "Connection measurements require a source and sample count"
            );
        }
        Ok(())
    }
    pub fn mass_kg(&self, part: &str) -> Result<f64> {
        self.part_record(part)
            .map(|p| Ok(p.mass_kg))
            .unwrap_or_else(|| mass_kg(part))
    }
    /// Override client mass/bounds claims with this operator-owned catalog.
    pub fn normalize_scene(&self, scene: &mut crate::WorldExport) -> Result<()> {
        self.validate()?;
        for mesh in scene.meshes.values_mut() {
            let canonical = crate::catalog::canonical_part(&mesh.part);
            mesh.physical = self
                .parts
                .iter()
                .find(|p| crate::catalog::canonical_part(&p.part) == canonical)
                .cloned()
                .map(|mut record| {
                    record.part = mesh.part.clone();
                    record
                });
            if let Some(record) = &mesh.physical {
                mesh.body_bounds = record.body_bounds;
            } else if let Some(bounds) = crate::geometry::nominal_body_bounds(&mesh.part) {
                mesh.body_bounds = bounds;
            }
        }
        for brick in &mut scene.bricks {
            let canonical = crate::catalog::canonical_part(&brick.part);
            let bounds = if let Some(record) = self
                .parts
                .iter()
                .find(|p| crate::catalog::canonical_part(&p.part) == canonical)
            {
                record.body_bounds
            } else if let Some(bounds) = crate::geometry::nominal_body_bounds(&brick.part) {
                bounds
            } else {
                anyhow::bail!(
                    "No sourced mass and nominal body bounds for {}. Add a sourced parts record to ERGO_CONNECTION_MEASUREMENTS",
                    brick.part
                );
            };
            brick.bounds = bounds.transformed(brick.transform, brick.position);
            brick.render_bounds = brick.render_bounds.union(brick.bounds);
        }
        Ok(())
    }
    /// Ignore capacity claims from an untrusted client; use only this server-owned
    /// measurement book for publication. No made-up default clutch strength.
    pub fn apply(&self, bricks: &[Brick], joints: &mut [JointSpec]) -> Result<()> {
        self.validate()?;
        let index: std::collections::HashMap<_, _> = bricks.iter().map(|b| (b.id, b)).collect();
        let mut matched = Vec::new();
        for joint in joints.iter() {
            let a = index.get(&joint.a).context("Missing joint endpoint")?;
            let b = index.get(&joint.b).context("Missing joint endpoint")?;
            let overlap = a.bounds.max.min(b.bounds.max) - a.bounds.min.max(b.bounds.min);
            let footprint = [
                overlap.x.max(0.0).round() as u32,
                overlap.z.max(0.0).round() as u32,
            ];
            let kind = if matches!(joint.kind, JointKind::Fixed) {
                "fixed"
            } else {
                "revolute"
            };
            let a_part = crate::catalog::canonical_part(&a.part);
            let b_part = crate::catalog::canonical_part(&b.part);
            let measurement=self.entries.iter().find(|e|{
                let left=crate::catalog::canonical_part(&e.part_a);let right=crate::catalog::canonical_part(&e.part_b);
                ((left==a_part&&right==b_part)||(left==b_part&&right==a_part))&&e.kind==kind&&e.overlap_studs==footprint
            })
                .with_context(||format!("Missing measured connector capacity for {} ↔ {} ({kind}, footprint {}×{}). Configure ERGO_CONNECTION_MEASUREMENTS; no realistic strength is assumed",a.part,b.part,footprint[0],footprint[1]))?;
            matched.push(measurement);
        }
        for (joint, measurement) in joints.iter_mut().zip(matched) {
            joint.break_force = Some(measurement.conservative_force_limit_n);
            joint.break_torque = Some(measurement.conservative_torque_limit_nm);
            joint.strength_source = Some(measurement.source.clone());
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn proper_metric_isometries_accept_rotations_but_reject_scale_shear_and_reflection() {
        let mut d = crate::demo::Demo::new();
        d.add("3003", 19, 2., 0., 2.);
        let b = &mut d.bricks[0];
        for rotation in [
            DMat3::from_rotation_x(0.7),
            DMat3::from_rotation_y(1.3),
            DMat3::from_rotation_z(std::f64::consts::FRAC_PI_2),
        ] {
            b.transform = world_transform(rotation);
            validate_pose(b).unwrap();
        }
        for invalid in [
            DMat3::from_diagonal(DVec3::new(-1., 1., 1.)),
            DMat3::from_diagonal(DVec3::new(2., 1., 1.)),
            DMat3::from_cols(DVec3::X, DVec3::new(0.2, 1., 0.), DVec3::Z),
        ] {
            b.transform = world_transform(invalid);
            assert!(validate_pose(b).is_err());
        }
        b.transform = DMat3::IDENTITY;
        b.position.x = f64::INFINITY;
        assert!(validate_pose(b).is_err());
    }
    #[cfg(feature = "simulation")]
    #[test]
    fn visual_affine_pose_cannot_be_used_as_a_rigid_simulation_body() {
        let mut demo = crate::demo::Demo::new();
        demo.add("3003", 4, 2., 0., 2.);
        demo.bricks[0].transform *= 2.;
        validate_affine_pose(&demo.bricks[0]).unwrap();
        assert!(validate_pose(&demo.bricks[0]).is_err());
        assert!(
            crate::physics::Simulation::new(&demo.bricks, &demo.meshes, &[], &[], false).is_err()
        );
    }
    #[test]
    fn brick_dimensions_and_mass_are_metric_and_sourced() {
        assert_eq!(
            to_meters(DVec3::new(2.0, 1.0, 4.0)),
            DVec3::new(0.016, 0.0096, 0.032)
        );
        assert!((mass_kg("3001.dat").unwrap() - 0.00232).abs() < 1e-12);
        for (part, _) in crate::catalog::PARTS.iter() {
            assert!(mass_kg(part).unwrap() > 0.0);
        }
    }
    #[test]
    fn official_identity_aliases_preserve_frozen_operator_measurements() {
        let mut book = ConnectionMeasurements::default();
        book.parts.push(PartMeasurement {
            part: "3023.dat".into(),
            mass_kg: 0.00123,
            body_bounds: crate::geometry::fallback_mesh("3023b.dat").body_bounds,
            source: "Frozen operator fixture".into(),
            samples: 3,
        });
        book.validate().unwrap();
        assert_eq!(book.mass_kg("3023b.dat").unwrap(), 0.00123);
        assert_eq!(book.part_record("3023b.dat").unwrap().part, "3023.dat");
        book.parts.push(PartMeasurement {
            part: "3023b.dat".into(),
            ..book.parts[0].clone()
        });
        assert!(
            book.validate().is_err(),
            "Aliased duplicate physical records must not become ambiguous"
        );
    }
    #[test]
    fn missing_strength_is_not_replaced_with_an_invented_default() {
        let mut d = crate::demo::Demo::new();
        d.add("3003", 4, 2.0, 0.0, 2.0);
        d.add("3003", 1, 2.0, 1.0, 2.0);
        let mut joints = crate::constraints::inferred_joints(&d.bricks);
        assert!(
            ConnectionMeasurements::default()
                .apply(&d.bricks, &mut joints)
                .is_err()
        );
        assert!(joints[0].break_force.is_none());
    }
}
