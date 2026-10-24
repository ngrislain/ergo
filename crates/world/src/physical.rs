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
    let error = rotation.transpose() * rotation - DMat3::IDENTITY;
    ensure!(
        error.to_cols_array().iter().all(|n| n.abs() < 1e-5)
            && (rotation.determinant().abs() - 1.0).abs() < 1e-5,
        "Brick {} ({}) is scaled or sheared in metric space; it can be viewed/exported, but simulation and publication require nominal rigid part dimensions",
        brick.id,
        brick.part
    );
    Ok(())
}

/// BrickLink item-info weights, retrieved 2026-10-17. Tile aliases map to the
/// unsuffixed BrickLink catalog IDs. Mold/color variation is not modeled.
pub fn mass_kg(part: &str) -> Result<f64> {
    let grams = match part {
        "3005.dat" => 0.44,
        "3004.dat" => 0.83,
        "3010.dat" => 1.64,
        "3009.dat" => 2.42,
        "3008.dat" => 3.21,
        "3003.dat" => 1.35,
        "3002.dat" => 1.92,
        "3001.dat" => 2.32,
        "2456.dat" => 3.74,
        "3007.dat" => 4.75,
        "3024.dat" => 0.20,
        "3023.dat" => 0.36,
        "3710.dat" => 0.71,
        "3666.dat" => 1.06,
        "3022.dat" => 0.64,
        "3021.dat" => 0.93,
        "3020.dat" => 1.20,
        "3795.dat" => 1.74,
        "3034.dat" => 2.27,
        "3031.dat" => 2.22,
        "3032.dat" => 3.30,
        "3035.dat" => 4.70,
        "41539.dat" => 9.40,
        "3070b.dat" => 0.16,
        "3069b.dat" => 0.26,
        "3068b.dat" => 0.49,
        "2431.dat" => 0.54,
        _ => anyhow::bail!("No sourced mass for {part}"),
    };
    Ok(grams / 1000.0)
}
pub fn mass_source(part: &str) -> String {
    let id = part.trim_end_matches(".dat");
    let id = match id {
        "3070b" => "3070",
        "3069b" => "3069",
        "3068b" => "3068",
        _ => id,
    };
    format!("https://www.bricklink.com/v2/catalog/catalogitem.page?P={id}")
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
    pub fn validate(&self) -> Result<()> {
        let mut names = std::collections::BTreeSet::new();
        for part in &self.parts {
            ensure!(
                crate::catalog::valid_part(&part.part)
                    && crate::catalog::canonical_part(&part.part) == part.part
                    && names.insert(&part.part),
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
        self.parts
            .iter()
            .find(|p| p.part == part)
            .map(|p| Ok(p.mass_kg))
            .unwrap_or_else(|| mass_kg(part))
    }
    /// Override client mass/bounds claims with this operator-owned catalog.
    pub fn normalize_scene(&self, scene: &mut crate::WorldExport) -> Result<()> {
        self.validate()?;
        for mesh in scene.meshes.values_mut() {
            mesh.physical = self.parts.iter().find(|p| p.part == mesh.part).cloned();
            if let Some(record) = &mesh.physical {
                mesh.body_bounds = record.body_bounds;
            }
        }
        for brick in &mut scene.bricks {
            let bounds = if let Some(record) = self.parts.iter().find(|p| p.part == brick.part) {
                record.body_bounds
            } else if crate::geometry::part_spec(&brick.part).is_some() {
                crate::geometry::fallback_mesh(&brick.part).body_bounds
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
            let measurement=self.entries.iter().find(|e|((e.part_a==a.part&&e.part_b==b.part)||(e.part_a==b.part&&e.part_b==a.part))&&e.kind==kind&&e.overlap_studs==footprint)
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
        for (part, _) in crate::catalog::PARTS {
            assert!(mass_kg(part).unwrap() > 0.0);
        }
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
