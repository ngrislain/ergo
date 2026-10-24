//! Reproducible, bounded dynamic load cases. A surviving run is evidence under
//! the supplied model and waveform, not a universal structural guarantee.
use crate::{
    Brick, JointSpec, WorldExport,
    physics::{Simulation, SimulationStats, TIMESTEP},
};
use anyhow::{Result, ensure};
use glam::DVec3;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Earthquake {
    pub displacement_amplitude_m: f64,
    pub frequency_hz: f64,
    pub duration_s: f64,
    #[serde(with = "crate::coordinates::vec3")]
    pub direction: DVec3,
}
impl Earthquake {
    pub fn validate(&self) -> Result<()> {
        ensure!(
            self.displacement_amplitude_m.is_finite()
                && (0.0..=0.05).contains(&self.displacement_amplitude_m),
            "Shake amplitude must be 0–0.05 meters"
        );
        ensure!(
            self.frequency_hz.is_finite() && (0.1..=20.0).contains(&self.frequency_hz),
            "Shake frequency must be 0.1–20 Hz"
        );
        ensure!(
            self.duration_s.is_finite() && (0.1..=4.0).contains(&self.duration_s),
            "Shake duration must be 0.1–4 seconds"
        );
        ensure!(
            self.direction.is_finite()
                && self.direction.y.abs() < 1e-10
                && self.direction.length() > 1e-8,
            "Earthquake direction must be a nonzero horizontal vector"
        );
        Ok(())
    }
    /// Sine-squared envelope starts/ends at zero displacement and velocity.
    pub fn displacement(&self, t: f64) -> DVec3 {
        if t <= 0.0 || t >= self.duration_s {
            return DVec3::ZERO;
        }
        let envelope = (std::f64::consts::PI * t / self.duration_s).sin().powi(2);
        self.direction.normalize()
            * self.displacement_amplitude_m
            * (std::f64::consts::TAU * self.frequency_hz * t).sin()
            * envelope
    }
    pub fn acceleration_upper_bound(&self) -> f64 {
        let w = std::f64::consts::TAU * self.frequency_hz;
        let k = std::f64::consts::PI / self.duration_s;
        self.displacement_amplitude_m * (w * w + 2.0 * w * k + 2.0 * k * k)
    }
}

/// Project diagnostic preset, not a seismic-code intensity classification.
/// The displacement is chosen so the analytic acceleration bound is 0.15 g.
pub fn moderate_earthquake(direction: DVec3) -> Earthquake {
    let frequency_hz = 4.0;
    let duration_s = 3.0;
    let w = std::f64::consts::TAU * frequency_hz;
    let k = std::f64::consts::PI / duration_s;
    Earthquake {
        displacement_amplitude_m: 0.15 * crate::physical::GRAVITY_M_S2
            / (w * w + 2.0 * w * k + 2.0 * k * k),
        frequency_hz,
        duration_s,
        direction,
    }
}

pub fn earthquake_with_acceleration_bound(
    peak_ground_acceleration_g: f64,
    frequency_hz: f64,
    duration_s: f64,
    direction: DVec3,
) -> Result<Earthquake> {
    ensure!(
        peak_ground_acceleration_g.is_finite()
            && (0.001..=2.0).contains(&peak_ground_acceleration_g),
        "Earthquake shaking intensity must be 0.001–2.0 g PGA, not Richter/moment magnitude"
    );
    let w = std::f64::consts::TAU * frequency_hz;
    let k = std::f64::consts::PI / duration_s;
    let case = Earthquake {
        displacement_amplitude_m: peak_ground_acceleration_g * crate::physical::GRAVITY_M_S2
            / (w * w + 2.0 * w * k + 2.0 * k * k),
        frequency_hz,
        duration_s,
        direction,
    };
    case.validate()?;
    Ok(case)
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct FallingObject {
    pub mass_kg: f64,
    #[serde(with = "crate::coordinates::vec3")]
    pub dimensions_m: DVec3,
    /// Construction-space release reference; release_height_m is added vertically.
    #[serde(with = "crate::coordinates::vec3")]
    pub target_world: DVec3,
    pub release_height_m: f64,
}
impl FallingObject {
    pub fn validate(&self) -> Result<()> {
        ensure!(
            self.mass_kg.is_finite() && (0.0001..=10.0).contains(&self.mass_kg),
            "Dropped mass must be 0.0001–10 kg"
        );
        ensure!(
            self.dimensions_m.is_finite()
                && self.dimensions_m.min_element() >= 0.001
                && self.dimensions_m.max_element() <= 1.0,
            "Test-object dimensions must be 0.001–1 meter"
        );
        ensure!(
            self.target_world.is_finite()
                && self.release_height_m.is_finite()
                && (0.01..=2.0).contains(&self.release_height_m),
            "Release height must be 0.01–2 meters"
        );
        Ok(())
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum LoadCase {
    Gravity(crate::physical::GravityProbeSettings),
    Earthquake(Earthquake),
    FallingObject(FallingObject),
}

#[derive(Clone, Debug, Serialize)]
pub struct SolidityReport {
    pub case: LoadCase,
    pub statistics: SimulationStats,
    pub peak_displacement_m: f64,
    pub final_rotation_max_degrees: f64,
    pub broken_connections: usize,
    pub residual_linear_speed_m_s: f64,
    pub residual_angular_speed_rad_s: f64,
    pub survived_declared_case: bool,
    pub connector_capacities_complete: bool,
    pub shake_acceleration_bound_m_s2: Option<f64>,
    pub dropped_potential_energy_j: Option<f64>,
    pub limitations: Vec<String>,
}

pub fn test(
    scene: &WorldExport,
    case: LoadCase,
) -> Result<(Vec<Brick>, Vec<JointSpec>, SolidityReport)> {
    let anchor_tolerances = match &case {
        LoadCase::Gravity(settings) => crate::validation::StaticTolerances {
            position_brick_units: settings.position_tolerance,
            rotation_radians: settings.rotation_tolerance_radians,
            ..Default::default()
        },
        _ => Default::default(),
    };
    crate::validation::validate_ground_anchors_with_tolerances(
        &scene.bricks,
        &scene.fixed_bricks,
        anchor_tolerances,
    )?;
    let mut simulation = Simulation::new(
        &scene.bricks,
        &scene.meshes,
        &scene.joints,
        &scene.fixed_bricks,
        false,
    )?;
    let (duration, accel, energy) = match &case {
        LoadCase::Gravity(settings) => {
            settings.validate()?;
            (settings.steps as f64 * TIMESTEP, None, None)
        }
        LoadCase::Earthquake(case) => {
            simulation.earthquake(case.clone())?;
            (
                case.duration_s + 2.0,
                Some(case.acceleration_upper_bound()),
                None,
            )
        }
        LoadCase::FallingObject(case) => {
            simulation.falling_object(case)?;
            (
                4.0,
                None,
                Some(case.mass_kg * crate::physical::GRAVITY_M_S2 * case.release_height_m),
            )
        }
    };
    let mut peak: f64 = 0.0;
    let step_count = match &case {
        LoadCase::Gravity(settings) => settings.steps as usize,
        _ => (duration / TIMESTEP).ceil() as usize,
    };
    for _ in 0..step_count {
        simulation.step();
        peak = peak.max(simulation.max_displacement_m());
    }
    let stats = simulation.stats();
    let broken = simulation.broken_count();
    let speed = simulation.max_speed();
    let calibrated = scene.joints.iter().all(|j| {
        j.break_force.is_some()
            && j.break_torque.is_some()
            && j.strength_source.as_ref().is_some_and(|s| !s.is_empty())
    });
    let final_bricks = simulation.bricks();
    let rotation = scene
        .bricks
        .iter()
        .zip(&final_bricks)
        .map(|(original, current)| {
            let r =
                crate::physical::metric_transform(current.transform * original.transform.inverse());
            (((r.x_axis.x + r.y_axis.y + r.z_axis.z - 1.0) * 0.5).clamp(-1.0, 1.0))
                .acos()
                .to_degrees()
        })
        .fold(0.0, f64::max);
    let (drift_limit, rotation_limit) = match &case {
        LoadCase::Gravity(settings) => (
            settings.position_tolerance,
            settings.rotation_tolerance_radians.to_degrees(),
        ),
        _ => (0.1, 1.0),
    };
    let survived = broken == 0
        && stats.max_displacement <= drift_limit
        && rotation <= rotation_limit
        && stats.penetrating_pairs == 0
        && (matches!(case, LoadCase::Gravity(_)) || speed < 0.03);
    let report=SolidityReport {
        case,statistics:stats,peak_displacement_m:peak,final_rotation_max_degrees:rotation,
        broken_connections:broken,residual_linear_speed_m_s:simulation.max_linear_speed_m_s(),
        residual_angular_speed_rad_s:simulation.max_angular_speed_rad_s(),
        survived_declared_case:survived,connector_capacities_complete:calibrated,
        shake_acceleration_bound_m_s2:accel,dropped_potential_energy_j:energy,
        limitations:vec![
            "A bounded load-case test is not proof against arbitrary earthquakes or impacts.".into(),
            "Collision boxes and box inertia approximate hollow geometry; friction is assumed.".into(),
            if calibrated {"Capacity thresholds are impulse-derived; validate their timestep dependence against experiments."}
            else {"Connector capacities are missing: idealized-joint results are diagnostic, not calibrated strength certification."}.into(),
        ],
    };
    Ok((final_bricks, simulation.current_joints(), report))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn gravity_probe_is_one_step_by_default_and_tolerance_is_explicit() {
        for (height, expected) in [(0.0, true), (5.0, false)] {
            let mut d = crate::demo::Demo::new();
            d.add("3003", 4, 4.0, height, 4.0);
            let scene = WorldExport {
                schema: crate::SCHEMA_VERSION,
                name: "probe".into(),
                geo: Default::default(),
                bricks: d.bricks,
                meshes: d.meshes,
                joints: Vec::new(),
                fixed_bricks: Vec::new(),
                ground: Vec::new(),
                groups: Vec::new(),
                model: None,
            };
            let settings = crate::physical::GravityProbeSettings::default();
            assert_eq!(settings.steps, 1);
            let (_, _, report) = test(&scene, LoadCase::Gravity(settings)).unwrap();
            assert_eq!(report.survived_declared_case, expected, "{report:?}");
            assert!((report.statistics.elapsed - TIMESTEP).abs() < 1e-14);
        }
    }
    #[test]
    fn shake_starts_and_ends_at_rest_and_validates_units() {
        let case = Earthquake {
            displacement_amplitude_m: 0.002,
            frequency_hz: 5.0,
            duration_s: 2.0,
            direction: DVec3::X,
        };
        assert!(case.validate().is_ok());
        assert_eq!(case.displacement(0.0), DVec3::ZERO);
        assert_eq!(case.displacement(2.0), DVec3::ZERO);
        assert!(case.acceleration_upper_bound() > 1.9);
    }
    #[test]
    fn moderate_preset_has_an_explicit_acceleration_bound() {
        for direction in [DVec3::X, DVec3::Z] {
            let case = moderate_earthquake(direction);
            assert!(case.validate().is_ok());
            assert!(
                (case.acceleration_upper_bound() / crate::physical::GRAVITY_M_S2 - 0.15).abs()
                    < 1e-12
            );
            assert_eq!(case.frequency_hz, 4.0);
            assert_eq!(case.duration_s, 3.0);
        }
    }
    #[test]
    fn impact_is_bounded_and_reported_in_joules() {
        let mut d = crate::demo::Demo::new();
        d.add("3003", 4, 4.0, 0.0, 4.0);
        let scene = WorldExport {
            schema: 1,
            name: "test".into(),
            geo: Default::default(),
            bricks: d.bricks,
            meshes: d.meshes,
            joints: Vec::new(),
            fixed_bricks: Vec::new(),
            ground: Vec::new(),
            groups: Vec::new(),
            model: None,
        };
        let case = FallingObject {
            mass_kg: 0.05,
            dimensions_m: DVec3::splat(0.01),
            target_world: DVec3::new(5.0, 1.0, 5.0),
            release_height_m: 0.1,
        };
        let (_, _, report) = test(&scene, LoadCase::FallingObject(case)).unwrap();
        assert!((report.dropped_potential_energy_j.unwrap() - 0.04903325).abs() < 1e-8);
    }
}
