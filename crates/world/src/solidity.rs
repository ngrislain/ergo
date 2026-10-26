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
#[serde(default)]
pub struct SmallEarthquake {
    pub peak_acceleration_g: f64,
    pub duration_s: f64,
    pub interval_s: f64,
    pub seed: u64,
    pub position_tolerance: f64,
    pub rotation_tolerance_radians: f64,
}
impl Default for SmallEarthquake {
    fn default() -> Self {
        Self {
            peak_acceleration_g: 0.01,
            duration_s: 3.,
            interval_s: 0.1,
            seed: 42,
            position_tolerance: 0.01,
            rotation_tolerance_radians: 0.01,
        }
    }
}
impl SmallEarthquake {
    pub fn validate(&self) -> Result<()> {
        ensure!(
            self.peak_acceleration_g.is_finite()
                && (0.0001..=0.05).contains(&self.peak_acceleration_g),
            "Small-shake acceleration must be 0.0001–0.05 g"
        );
        ensure!(
            self.duration_s.is_finite()
                && (0.1..=5.).contains(&self.duration_s)
                && self.interval_s.is_finite()
                && (0.02..=1.).contains(&self.interval_s),
            "Invalid shake duration/interval"
        );
        ensure!(
            self.position_tolerance.is_finite()
                && (1e-6..=0.1).contains(&self.position_tolerance)
                && self.rotation_tolerance_radians.is_finite()
                && (1e-6..=0.1).contains(&self.rotation_tolerance_radians),
            "Invalid shake pose tolerances"
        );
        Ok(())
    }
    pub fn acceleration(&self, time: f64) -> DVec3 {
        if time < 0. || time >= self.duration_s {
            return DVec3::ZERO;
        }
        fn mixed(mut n: u64) -> f64 {
            n = n.wrapping_add(0x9e3779b97f4a7c15);
            n = (n ^ (n >> 30)).wrapping_mul(0xbf58476d1ce4e5b9);
            n = (n ^ (n >> 27)).wrapping_mul(0x94d049bb133111eb);
            ((n ^ (n >> 31)) >> 11) as f64 / (1u64 << 53) as f64
        }
        let index = (time / self.interval_s).floor() as u64;
        let phase = mixed(self.seed.wrapping_add(index.wrapping_mul(2))) * std::f64::consts::TAU;
        let radius = mixed(
            self.seed
                .wrapping_add(index.wrapping_mul(2))
                .wrapping_add(1),
        )
        .sqrt()
            * self.peak_acceleration_g
            * crate::physical::GRAVITY_M_S2;
        DVec3::new(phase.cos() * radius, 0., phase.sin() * radius)
    }
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
    SmallEarthquake(SmallEarthquake),
    FallingObject(FallingObject),
}

#[derive(Clone, Debug, Serialize)]
pub struct SolidityReport {
    pub case: LoadCase,
    pub statistics: SimulationStats,
    pub peak_displacement_m: f64,
    pub final_rotation_max_degrees: f64,
    pub broken_connections: usize,
    pub connection_breaks: Vec<crate::physics::ConnectionBreak>,
    pub residual_linear_speed_m_s: f64,
    pub residual_angular_speed_rad_s: f64,
    pub settling_window_s: f64,
    pub settling_linear_rate_m_s: f64,
    pub settling_angular_rate_rad_s: f64,
    pub survived_declared_case: bool,
    pub connector_capacities_complete: bool,
    pub omitted_inferred_connections: usize,
    pub unmeasured_connections: usize,
    pub estimated_connections: usize,
    pub shake_acceleration_bound_m_s2: Option<f64>,
    pub dropped_potential_energy_j: Option<f64>,
    pub limitations: Vec<String>,
    pub unstable_pieces: Vec<UnstablePiece>,
}

#[derive(Clone, Debug, Serialize)]
pub struct UnstablePiece {
    #[serde(flatten)]
    pub motion: crate::physics::PieceMotion,
    pub peak_displacement_native: f64,
    pub peak_displacement_m: f64,
    pub peak_rotation_radians: f64,
    pub nearby_piece_ids: Vec<u32>,
    pub first_moved_at_s: f64,
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
        LoadCase::SmallEarthquake(case) => {
            case.validate()?;
            (
                case.duration_s + 2.,
                Some(case.peak_acceleration_g * crate::physical::GRAVITY_M_S2),
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
    let (drift_limit, rotation_limit) = match &case {
        LoadCase::Gravity(settings) => (
            settings.position_tolerance,
            settings.rotation_tolerance_radians.to_degrees(),
        ),
        LoadCase::SmallEarthquake(settings) => (
            settings.position_tolerance,
            settings.rotation_tolerance_radians.to_degrees(),
        ),
        _ => (0.1, 1.),
    };
    let mut motion_peaks = vec![(0.0_f64, 0.0_f64, 0.0_f64); scene.bricks.len()];
    let mut first_motion = vec![None; scene.bricks.len()];
    let settling_steps = ((0.25 / TIMESTEP).round() as usize).max(1).min(step_count);
    let mut settling_reference = None;
    for step in 0..step_count {
        if step == step_count - settling_steps {
            settling_reference = Some(simulation.bricks());
        }
        if let LoadCase::SmallEarthquake(case) = &case {
            simulation.set_horizontal_acceleration(case.acceleration(step as f64 * TIMESTEP))?;
        }
        simulation.step();
        peak = peak.max(simulation.max_displacement_m());
        for (index, (maximum, motion)) in motion_peaks
            .iter_mut()
            .zip(simulation.piece_motions())
            .enumerate()
        {
            maximum.0 = maximum.0.max(motion.displacement_world.length());
            maximum.1 = maximum.1.max(motion.displacement_m);
            maximum.2 = maximum.2.max(motion.rotation_radians);
            if first_motion[index].is_none()
                && (maximum.0 > drift_limit || maximum.2.to_degrees() > rotation_limit)
            {
                first_motion[index] = Some((step + 1) as f64 * TIMESTEP);
            }
        }
    }
    let stats = simulation.stats();
    let broken = simulation.broken_count();
    let speed = simulation.max_speed();
    let supplied_pairs: std::collections::HashSet<_> = scene
        .joints
        .iter()
        .map(|j| (j.a.min(j.b), j.a.max(j.b)))
        .collect();
    let omitted_inferred_connections = crate::constraints::inferred_joints(&scene.bricks)
        .iter()
        .filter(|j| !supplied_pairs.contains(&(j.a.min(j.b), j.a.max(j.b))))
        .count();
    let unmeasured_connections = scene
        .joints
        .iter()
        .filter(|j| {
            !j.break_force.is_some_and(|f| f.is_finite() && f > 0.)
                || !j.break_torque.is_some_and(|t| t.is_finite() && t > 0.)
                || !j
                    .strength_source
                    .as_ref()
                    .is_some_and(|s| !s.trim().is_empty())
        })
        .count();
    let estimated_connections = scene
        .joints
        .iter()
        .filter(|j| {
            j.strength_source
                .as_ref()
                .is_some_and(|s| s.starts_with("estimate:"))
        })
        .count();
    let calibrated = omitted_inferred_connections == 0
        && unmeasured_connections == 0
        && estimated_connections == 0;
    let final_bricks = simulation.bricks();
    let settling_window_s = settling_steps as f64 * TIMESTEP;
    let (settling_linear_rate_m_s, settling_angular_rate_rad_s) = settling_rates(
        &settling_reference.unwrap(),
        &final_bricks,
        settling_window_s,
    );
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
    let tree = crate::spatial::Octree::new(scene.bricks.iter().map(|b| b.bounds).collect());
    let mut unstable_pieces = simulation
        .piece_motions()
        .zip(motion_peaks)
        .enumerate()
        .filter_map(|(index, (motion, maximum))| {
            (maximum.0 > drift_limit || maximum.2.to_degrees() > rotation_limit).then(|| {
                let nearby_piece_ids = tree
                    .query(scene.bricks[index].bounds.expanded(0.05))
                    .into_iter()
                    .filter_map(|j| (j != index).then_some(scene.bricks[j].id))
                    .collect();
                UnstablePiece {
                    motion,
                    peak_displacement_native: maximum.0,
                    peak_displacement_m: maximum.1,
                    peak_rotation_radians: maximum.2,
                    nearby_piece_ids,
                    first_moved_at_s: first_motion[index].unwrap(),
                }
            })
        })
        .collect::<Vec<_>>();
    unstable_pieces.sort_by(|a, b| {
        a.first_moved_at_s
            .total_cmp(&b.first_moved_at_s)
            .then(a.motion.id.cmp(&b.motion.id))
    });
    // Instantaneous solver velocities contain micro-jitter even when every pose
    // stays bounded. The small-shake settling criterion uses a disclosed final
    // quarter-second net rate; transient peak pose limits still apply throughout.
    let settling_speed = if matches!(case, LoadCase::SmallEarthquake(_)) {
        (settling_linear_rate_m_s / crate::physical::STUD_PITCH_M).max(settling_angular_rate_rad_s)
    } else {
        speed
    };
    let survived = broken == 0
        && stats.max_displacement <= drift_limit
        && rotation <= rotation_limit
        && stats.penetrating_pairs == 0
        && unstable_pieces.is_empty()
        && (matches!(case, LoadCase::Gravity(_)) || settling_speed < 0.03);
    let report=SolidityReport {
        case,statistics:stats,peak_displacement_m:peak,final_rotation_max_degrees:rotation,unstable_pieces,
        broken_connections:broken,connection_breaks:simulation.break_events().to_vec(),residual_linear_speed_m_s:simulation.max_linear_speed_m_s(),
        residual_angular_speed_rad_s:simulation.max_angular_speed_rad_s(),
        settling_window_s,settling_linear_rate_m_s,settling_angular_rate_rad_s,
        survived_declared_case:survived,connector_capacities_complete:calibrated,
        omitted_inferred_connections,unmeasured_connections,estimated_connections,
        shake_acceleration_bound_m_s2:accel,dropped_potential_energy_j:energy,
        limitations:vec![
            "A bounded load-case test is not proof against arbitrary earthquakes or impacts.".into(),
            "Collision boxes and box inertia approximate hollow geometry; friction is assumed.".into(),
            "Iterative contact-solver error can destabilize tall loose stacks; check numerical convergence before attributing failure to the construction.".into(),
            if omitted_inferred_connections > 0 {"Inferred stud connections were omitted: this is an unbonded contact diagnostic, not a test of the assembled stud-clutch model."}
            else {"All geometrically inferred stud pairs are represented; see unmeasured_connections for missing capacities."}.into(),
            if estimated_connections > 0 {"Connector thresholds include explicit ballpark estimates, not measured capacities; survival is conditional on those assumptions."}
            else if calibrated {"Capacity thresholds are impulse-derived; validate their timestep dependence against experiments."}
            else {"Connector capacities are missing: idealized-joint results are diagnostic, not calibrated strength certification."}.into(),
        ],
    };
    Ok((final_bricks, simulation.current_joints(), report))
}

fn settling_rates(before: &[Brick], after: &[Brick], duration: f64) -> (f64, f64) {
    before
        .iter()
        .zip(after)
        .map(|(a, b)| {
            let translation =
                crate::physical::to_meters(b.position - a.position).length() / duration;
            let rotation = crate::physical::metric_transform(b.transform * a.transform.inverse());
            let angle = (((rotation.x_axis.x + rotation.y_axis.y + rotation.z_axis.z - 1.) * 0.5)
                .clamp(-1., 1.))
            .acos();
            (translation, angle / duration)
        })
        .fold((0f64, 0f64), |(v, w), (next_v, next_w)| {
            (v.max(next_v), w.max(next_w))
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn small_random_acceleration_is_seeded_horizontal_and_bounded() {
        let case = SmallEarthquake::default();
        case.validate().unwrap();
        let mut changed = case.clone();
        changed.seed += 1;
        assert_ne!(case.acceleration(0.2), changed.acceleration(0.2));
        for i in 0..600 {
            let t = i as f64 / 120.;
            let a = case.acceleration(t);
            assert_eq!(a, case.acceleration(t));
            assert_eq!(a.y, 0.);
            assert!(a.length() <= case.peak_acceleration_g * crate::physical::GRAVITY_M_S2 + 1e-12);
        }
        assert_eq!(case.acceleration(case.duration_s), DVec3::ZERO);
    }
    #[test]
    fn settling_rates_detect_translation_and_changes_of_rotation_axis() {
        let mut d = crate::demo::Demo::new();
        d.add("3003", 19, 2., 0., 2.);
        let mut before = d.bricks;
        let mut after = before.clone();
        after[0].position += DVec3::X;
        let (linear, angular) = settling_rates(&before, &after, 0.25);
        assert!((linear - 0.032).abs() < 1e-12);
        assert_eq!(angular, 0.);
        before[0].transform = crate::physical::world_transform(glam::DMat3::from_rotation_x(0.5));
        after[0].transform = crate::physical::world_transform(glam::DMat3::from_rotation_y(0.5));
        assert!(settling_rates(&before, &after, 0.25).1 > 1.);
    }
    #[test]
    fn omitted_stud_connections_are_not_vacuously_calibrated() {
        let mut d = crate::demo::Demo::new();
        d.add("3003", 19, 2., 0., 2.);
        d.add("3003", 19, 2., 1., 2.);
        let mut scene = WorldExport {
            schema: crate::SCHEMA_VERSION,
            name: "stud stack".into(),
            geo: Default::default(),
            bricks: d.bricks,
            meshes: d.meshes,
            joints: vec![],
            fixed_bricks: vec![],
            ground: vec![],
            groups: vec![],
            model: None,
        };
        let (_, _, omitted) = test(&scene, LoadCase::Gravity(Default::default())).unwrap();
        assert_eq!(omitted.omitted_inferred_connections, 1);
        assert!(!omitted.connector_capacities_complete);
        scene.joints = crate::constraints::inferred_joints(&scene.bricks);
        let (_, _, unmeasured) = test(&scene, LoadCase::Gravity(Default::default())).unwrap();
        assert_eq!(unmeasured.omitted_inferred_connections, 0);
        assert_eq!(unmeasured.unmeasured_connections, 1);
        assert!(!unmeasured.connector_capacities_complete);
    }
    #[test]
    fn tilted_loose_roof_tips_and_identifies_its_piece() {
        let mut d = crate::demo::Demo::new();
        d.add("3031", 14, 0., 0., 0.);
        let b = &mut d.bricks[0];
        let local = d.meshes[&b.part].body_bounds;
        b.transform =
            crate::physical::world_transform(glam::DMat3::from_rotation_z(25f64.to_radians()));
        let rotated = local.transformed(b.transform, DVec3::ZERO);
        b.position = DVec3::new(2., -rotated.min.y, 2.);
        b.bounds = local.transformed(b.transform, b.position);
        b.render_bounds = d.meshes[&b.part]
            .bounds
            .transformed(b.transform, b.position);
        let scene = WorldExport {
            schema: crate::SCHEMA_VERSION,
            name: "loose roof".into(),
            geo: Default::default(),
            bricks: d.bricks,
            meshes: d.meshes,
            joints: vec![],
            fixed_bricks: vec![],
            ground: vec![],
            groups: vec![],
            model: None,
        };
        let (bodies, _, report) = test(
            &scene,
            LoadCase::SmallEarthquake(SmallEarthquake::default()),
        )
        .unwrap();
        assert!(!report.survived_declared_case);
        assert!(report.final_rotation_max_degrees > 10., "{report:?}");
        assert_eq!(report.unstable_pieces[0].motion.id, 1);
        assert_eq!(report.unstable_pieces[0].motion.part, "3031.dat");
        assert_ne!(bodies[0].transform, scene.bricks[0].transform);
        let (_, _, repeat) = test(
            &scene,
            LoadCase::SmallEarthquake(SmallEarthquake::default()),
        )
        .unwrap();
        assert_eq!(
            serde_json::to_value(&report).unwrap(),
            serde_json::to_value(&repeat).unwrap()
        );
    }
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
