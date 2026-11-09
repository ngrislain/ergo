//! Persistence gate: no penetrating nominal body boxes, plus a conservative
//! static support/constraint-wrench certificate. No time integration runs here.
//! declared simplified model, not real ABS stress, clutch strength, or earthquakes.
use crate::equilibrium::Bodies;
use crate::{Brick, WorldExport, constraints, spatial::Octree};
use anyhow::{Result, ensure};
use glam::{DVec2, DVec3};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};

#[path = "validation_components.rs"]
mod components;

pub const OVERLAP_TOLERANCE: f64 = 0.0001;

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub struct StaticTolerances {
    pub position_brick_units: f64,
    pub rotation_radians: f64,
    pub force_newtons: f64,
    pub torque_newton_meters: f64,
}
impl Default for StaticTolerances {
    fn default() -> Self {
        Self {
            position_brick_units: OVERLAP_TOLERANCE,
            rotation_radians: 1e-4,
            force_newtons: 1e-6,
            torque_newton_meters: 1e-8,
        }
    }
}
impl StaticTolerances {
    pub fn validate(self) -> Result<()> {
        ensure!(
            [
                self.position_brick_units,
                self.rotation_radians,
                self.force_newtons,
                self.torque_newton_meters
            ]
            .iter()
            .all(|v| v.is_finite() && *v > 0.0),
            "Static tolerances must be positive and finite"
        );
        Ok(())
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ValidationReport {
    pub collision_model: String,
    pub method: String,
    pub inferred_joints: usize,
    pub checked_bricks: usize,
    pub max_displacement: f64,
    /// None on historical reports. False means the certificate consumes no
    /// joint strength; the method distinguishes contact-only and anchored proofs.
    /// True when the certificate loads at least one joint (its capacity is
    /// then consumed); false for a contact-only proof.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub joint_strength_required: Option<bool>,
    /// Largest joint load / capacity ratio in the certificate.
    #[serde(default)]
    pub max_joint_utilisation: f64,
    /// Independent islands connected by joints or admissible bearing contacts;
    /// zero when unavailable on historical or unvalidated reports.
    #[serde(default)]
    pub stability_components: usize,
    /// Largest island that required the general equilibrium solver (zero when
    /// every island was certified by constructive compression alone, or when
    /// unavailable on historical/unvalidated reports).
    #[serde(default)]
    pub largest_equilibrium_component: usize,
    #[serde(default)]
    pub equilibrium_solver: crate::equilibrium::SolverStats,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub compression_issue: Option<CompressionIssue>,
    #[serde(default)]
    pub physical_calibration: String,
    #[serde(default)]
    pub tolerances: StaticTolerances,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CompressionIssue {
    pub brick_id: u32,
    pub part: String,
    pub reason: String,
    /// Accumulated-load horizontal centre projected onto this brick's bearing plane.
    #[serde(with = "crate::coordinates::vec3")]
    pub loaded_center_world: DVec3,
    pub support_polygon_world: Vec<[f64; 3]>,
}

pub fn validate_ground_anchors(bricks: &[Brick], fixed: &[u32]) -> Result<()> {
    validate_ground_anchors_with_tolerances(bricks, fixed, StaticTolerances::default())
}
pub fn validate_ground_anchors_with_tolerances(
    bricks: &[Brick],
    fixed: &[u32],
    tolerances: StaticTolerances,
) -> Result<()> {
    tolerances.validate()?;
    for id in fixed {
        let brick = bricks
            .iter()
            .find(|b| b.id == *id)
            .ok_or_else(|| anyhow::anyhow!("Anchored brick is absent"))?;
        ensure!(
            brick.bounds.min.y.abs() <= tolerances.position_brick_units,
            "Anchored brick {} is not on its baseplate; a floating world anchor cannot certify a construction",
            id
        );
    }
    Ok(())
}

pub fn validate_static(scene: &mut WorldExport) -> Result<ValidationReport> {
    validate_static_with_tolerances(scene, StaticTolerances::default())
}
pub fn validate_static_with_tolerances(
    scene: &mut WorldExport,
    tolerances: StaticTolerances,
) -> Result<ValidationReport> {
    validate_static_with_measurements(
        scene,
        tolerances,
        &crate::physical::ConnectionMeasurements::default(),
    )
}
pub fn validate_static_with_measurements(
    scene: &mut WorldExport,
    tolerances: StaticTolerances,
    measurements: &crate::physical::ConnectionMeasurements,
) -> Result<ValidationReport> {
    validate_static_with_workspace(
        scene,
        tolerances,
        measurements,
        &mut crate::equilibrium::MemoryWorkspace::default(),
    )
}
/// The same physical gate with caller-supplied coupled-solver storage. Storage
/// is an authoring resource, not a source of supports, capacities or acceptance.
pub fn validate_static_with_workspace(
    scene: &mut WorldExport,
    tolerances: StaticTolerances,
    measurements: &crate::physical::ConnectionMeasurements,
    workspace: &mut dyn crate::equilibrium::SolverWorkspace,
) -> Result<ValidationReport> {
    measurements.normalize_scene(scene)?;
    tolerances.validate()?;
    ensure!(
        !scene.bricks.is_empty(),
        "A static construction must contain at least one brick"
    );
    crate::catalog::validate(&scene.bricks)?;
    crate::groups::validate(&scene.bricks, &scene.groups)?;
    if let Some(model) = &scene.model {
        model.validate_isometries()?;
    }
    for brick in &scene.bricks {
        crate::physical::validate_pose(brick)?;
    }
    constraints::validate_joints(&scene.bricks, &scene.joints, &scene.fixed_bricks)?;
    crate::attachments::validate_joints(
        &scene.bricks,
        &scene.joints,
        tolerances.position_brick_units,
        tolerances.rotation_radians,
    )?;
    validate_ground_anchors_with_tolerances(&scene.bricks, &scene.fixed_bricks, tolerances)?;
    validate_baseplate_registration(&scene.bricks, tolerances)?;
    let tree = Octree::new(scene.bricks.iter().map(|b| b.bounds).collect());
    for (i, brick) in scene.bricks.iter().enumerate() {
        ensure!(
            brick.bounds.min.y >= -tolerances.position_brick_units,
            "Brick {} intersects its baseplate",
            brick.id
        );
        for j in tree.query(brick.bounds) {
            if j <= i {
                continue;
            }
            let other = &scene.bricks[j];
            let overlap =
                brick.bounds.max.min(other.bounds.max) - brick.bounds.min.max(other.bounds.min);
            ensure!(
                !crate::interference::bodies_penetrate(
                    brick,
                    other,
                    tolerances.position_brick_units
                ),
                "Bricks {} and {} overlap by {:.4} brick units; fix the model before saving",
                brick.id,
                other.id,
                overlap.min_element()
            );
        }
    }
    drop(tree);
    if let Some(error) =
        crate::interference::first_stud_collision(&scene.bricks, tolerances.position_brick_units)
    {
        anyhow::bail!(error);
    }
    let pairs: HashSet<_> = scene
        .joints
        .iter()
        .map(|j| (j.a.min(j.b), j.a.max(j.b)))
        .collect();
    let inferred: Vec<_> =
        constraints::inferred_joints_with_tolerance(&scene.bricks, tolerances.position_brick_units)
            .into_iter()
            .filter(|j| !pairs.contains(&(j.a.min(j.b), j.a.max(j.b))))
            .collect();
    let inferred_count = inferred.len();
    scene.joints.extend(inferred);
    drop(pairs);
    for brick in scene
        .bricks
        .iter()
        .filter(|b| !crate::interference::axis_aligned(b))
    {
        ensure!(
            brick.bounds.min.y > tolerances.position_brick_units
                && scene
                    .joints
                    .iter()
                    .any(|j| j.a == brick.id || j.b == brick.id),
            "Cannot certify tilted brick {} ({}): tilted bodies require a joint-supported load path, not a bounding-box bearing face",
            brick.id,
            brick.part
        );
    }
    // Capacities: the measurement book, then reviewed estimates. A joint
    // without either may carry no load.
    measurements.apply(&scene.bricks, &mut scene.joints)?;
    let stability = certify_components(
        &scene.bricks,
        &scene.joints,
        &scene.fixed_bricks,
        tolerances,
        measurements,
        workspace,
    )?;
    let report = ValidationReport {
        collision_model:
            "nominal body boxes with finite registered connector clearance; visual studs excluded"
                .into(),
        method: if stability.contact_only {
            "static compression certificate (zero joint wrench; no simulation)"
        } else {
            "static equilibrium certificate (joints within measured or reviewed capacities; no simulation)"
        }
        .into(),
        inferred_joints: inferred_count,
        checked_bricks: scene.bricks.len(),
        max_displacement: 0.0,
        joint_strength_required: Some(stability.certificate.loaded_joints > 0),
        max_joint_utilisation: stability.certificate.max_utilisation,
        stability_components: stability.components,
        largest_equilibrium_component: stability.largest_equilibrium_component,
        equilibrium_solver:stability.certificate.solver,
        compression_issue: stability.compression_issue,
        physical_calibration:
            "Metric dimensions and catalog masses; joint capacities from the measurement book or reviewed estimates".into(),
        tolerances,
    };
    Ok(report)
}

struct StabilityCertificate {
    certificate: crate::equilibrium::Certificate,
    contact_only: bool,
    compression_issue: Option<CompressionIssue>,
    components: usize,
    largest_equilibrium_component: usize,
}

/// No island is independently supported at an artificial cut: discovery joins
/// every joint and every bearing/contact interface before any island is checked.
/// Only the current island gets a compression frontier or solver matrices.
fn certify_components(
    bricks: &[Brick],
    joints: &[crate::JointSpec],
    fixed: &[u32],
    tol: StaticTolerances,
    measurements: &crate::physical::ConnectionMeasurements,
    workspace: &mut dyn crate::equilibrium::SolverWorkspace,
) -> Result<StabilityCertificate> {
    use std::borrow::Cow;
    let islands = components::partition(bricks, joints, tol)?;
    let anchored: HashSet<_> = fixed.iter().copied().collect();
    let mut result = StabilityCertificate {
        certificate: Default::default(),
        contact_only: true,
        compression_issue: None,
        components: islands.len(),
        largest_equilibrium_component: 0,
    };
    for island in islands {
        let bodies = Bodies::select(bricks, &island.bodies);
        let fixed: Vec<_> = bodies
            .iter()
            .filter(|b| anchored.contains(&b.id))
            .map(|b| b.id)
            .collect();
        // A constructive contact-only proof remains valid even with joints:
        // all their wrenches can be zero, without assuming unknown clutch loads.
        let issue = if fixed.is_empty() {
            compression_issue_for(bodies, tol, measurements)?
        } else {
            None
        };
        if fixed.is_empty() && issue.is_none() {
            continue;
        }
        result.contact_only = false;
        if result.compression_issue.is_none() {
            result.compression_issue = issue;
        }
        result.largest_equilibrium_component =
            result.largest_equilibrium_component.max(bodies.len());
        // Compression-only islands never need a copied joint list, either.
        let constraints: Cow<'_, [crate::JointSpec]> = if result.components == 1 {
            Cow::Borrowed(joints)
        } else {
            Cow::Owned(island.joints.iter().map(|&i| joints[i].clone()).collect())
        };
        let certificate =
            support_certificate(bodies, &constraints, &fixed, tol, measurements, workspace)?;
        result.certificate.solver.merge(&certificate.solver);
        result.certificate.loaded_joints += certificate.loaded_joints;
        result.certificate.max_utilisation = result
            .certificate
            .max_utilisation
            .max(certificate.max_utilisation);
    }
    Ok(result)
}

pub fn validate_baseplate_registration(
    bricks: &[Brick],
    tolerances: StaticTolerances,
) -> Result<()> {
    tolerances.validate()?;
    for brick in bricks
        .iter()
        .filter(|b| b.bounds.min.y.abs() <= tolerances.position_brick_units)
    {
        // Tyres rest on the virtual ground's flat bearing plane. They do not
        // engage ground studs and need no fabricated display plate beneath them.
        if crate::attachments::connectors(&brick.part)
            .iter()
            .any(|c| c.kind == crate::attachments::Kind::Tyre)
            || crate::catalog::canonical_id(&brick.part) == "4624c01.dat"
        {
            continue;
        }
        ensure!(
            (crate::physical::metric_transform(brick.transform).y_axis - DVec3::Y).length()
                <= tolerances.rotation_radians,
            "Grounded brick {} ({}) is not upright for baseplate registration",
            brick.id,
            brick.part
        );
        // Factory feet and specialty profiles register their actual receiving
        // datums, not an envelope edge that may lie inside the moulded outline.
        let grids = crate::part_profiles::receiving_grids(&brick.part);
        let registrations = if !grids.is_empty() {
            grids
                .iter()
                .map(|grid| {
                    let stud = brick.position
                        + brick.transform * DVec3::new(grid.first[0], grid.bottom, grid.first[1]);
                    [stud.x - 0.5, stud.z - 0.5]
                })
                .collect::<Vec<_>>()
        } else {
            vec![[brick.bounds.min.x, brick.bounds.max.z]]
        };
        ensure!(
            registrations
                .iter()
                .flatten()
                .all(|v| (v - v.round()).abs() <= tolerances.position_brick_units),
            "Grounded brick {} ({}) is off the baseplate stud grid at {:?}; snap the whole placement to integer X/Y stud coordinates",
            brick.id,
            brick.part,
            crate::coordinates::public(DVec3::new(
                brick.bounds.min.x,
                brick.bounds.min.y,
                brick.bounds.max.z
            ))
        );
    }
    Ok(())
}

fn ground_support_points(brick: &Brick) -> Vec<DVec3> {
    use crate::attachments::Kind;
    if let Some(port) = crate::attachments::connectors(&brick.part)
        .iter()
        .find(|p| matches!(p.kind, Kind::Tyre | Kind::WheelSocket))
    {
        let axis = (brick.transform
            * crate::ldraw::conversion()
            * (DVec3::from_array(port.end) - DVec3::from_array(port.start)))
        .normalize();
        if axis.y.abs() < 1e-7 {
            // A round tyre bears below its axle, not at the forward/rear corners
            // of its envelope. This avoids inventing a rolling-resistance torque.
            let half = brick.bounds.size() * 0.5;
            let span = half.dot(axis.abs());
            let center = DVec3::new(brick.bounds.center().x, 0., brick.bounds.center().z);
            return vec![center - axis * span, center + axis * span];
        }
    }
    [
        (brick.bounds.min.x, brick.bounds.min.z),
        (brick.bounds.max.x, brick.bounds.min.z),
        (brick.bounds.max.x, brick.bounds.max.z),
        (brick.bounds.min.x, brick.bounds.max.z),
    ]
    .into_iter()
    .map(|(x, z)| DVec3::new(x, 0., z))
    .collect()
}

#[cfg(test)]
mod registration_tests {
    use super::*;
    fn nominal_at(d: &mut crate::demo::Demo, part: &str, x: f64, y: f64, z: f64) {
        d.add(part, 71, x, y, z);
        let b = d.bricks.last_mut().unwrap();
        let local = crate::geometry::nominal_body_bounds(&b.part).unwrap();
        b.position = DVec3::new(x, y, z) - local.min;
        b.bounds = local.transformed(b.transform, b.position);
        b.render_bounds = b.bounds;
    }
    #[test]
    fn wall_wing_ties_and_rear_shelf_have_real_compressive_bearings() {
        let mut d = crate::demo::Demo::new();
        nominal_at(&mut d, "4444", 1., 0., 1.);
        nominal_at(&mut d, "3004", -1., 5., 1.);
        nominal_at(&mut d, "3004", 4., 5., 1.);
        nominal_at(&mut d, "3623", 1., 17. / 3., 0.);
        let measurements = crate::physical::ConnectionMeasurements::default();
        assert!(
            compression_issue(&d.bricks, Default::default(), &measurements)
                .unwrap()
                .is_none()
        );
        d.bricks[3].translated(DVec3::Y * 0.1);
        assert!(
            compression_issue(&d.bricks, Default::default(), &measurements)
                .unwrap()
                .is_some(),
            "A shelf does not supply pull-off or floating support"
        );
    }
    #[test]
    fn an_arch_cannot_bear_on_the_empty_middle_of_its_envelope() {
        let measurements = crate::physical::ConnectionMeasurements::default();
        let mut invalid = crate::demo::Demo::new();
        nominal_at(&mut invalid, "3003", 3., 0., 0.);
        nominal_at(&mut invalid, "3308a", 0., 1., 0.);
        assert!(
            compression_issue(&invalid.bricks, Default::default(), &measurements)
                .unwrap()
                .is_some()
        );
        let mut valid = crate::demo::Demo::new();
        nominal_at(&mut valid, "3005", 0., 0., 0.);
        nominal_at(&mut valid, "3005", 7., 0., 0.);
        nominal_at(&mut valid, "3308a", 0., 1., 0.);
        assert!(
            compression_issue(&valid.bricks, Default::default(), &measurements)
                .unwrap()
                .is_none()
        );
    }
    #[test]
    fn horse_registers_all_four_hoof_holes_not_its_head_and_tail_envelope() {
        let mut scene = crate::demo::Demo::new();
        scene.add("4493c01", 70, 0., 0., 0.);
        let horse = &mut scene.bricks[0];
        horse.position = DVec3::new(0., 57. / 24., 0.);
        horse.bounds = crate::geometry::nominal_body_bounds(&horse.part)
            .unwrap()
            .transformed(horse.transform, horse.position);
        assert!(validate_baseplate_registration(&scene.bricks, Default::default()).is_err());
        let correction = crate::placement::snap_bricks_translation(&scene.bricks, DVec3::ZERO);
        assert!(
            (crate::coordinates::public(correction) - DVec3::new(0., -0.25, 0.)).length() < 1e-9
        );
        scene.bricks[0].translated(correction);
        validate_baseplate_registration(&scene.bricks, Default::default()).unwrap();
        assert_eq!(
            crate::part_profiles::receiving_grids("4493c01.dat").len(),
            4
        );
        scene.bricks[0].translated(DVec3::new(0.1, 0., 0.));
        assert!(validate_baseplate_registration(&scene.bricks, Default::default()).is_err());
    }
    #[test]
    fn grounded_tiles_also_need_registration_even_without_studs_of_their_own() {
        let mut d = crate::demo::Demo::new();
        d.add("3070b", 19, -3., 0., -8.);
        validate_baseplate_registration(&d.bricks, Default::default()).unwrap();
        d.bricks[0].translated(DVec3::new(0.201, 0., 0.471));
        assert!(validate_baseplate_registration(&d.bricks, Default::default()).is_err());
        d.bricks[0].translated(DVec3::new(-0.201, 0., -0.471));
        d.bricks[0].translated(DVec3::splat(1e-8));
        validate_baseplate_registration(&d.bricks, Default::default()).unwrap();
    }
}

/// One nominal horizontal contact, shared by island discovery and the general
/// solver so a load-bearing edge cannot disappear at a partition boundary.
fn nominal_contact(
    bricks: Bodies<'_>,
    a: usize,
    b: usize,
    tol: StaticTolerances,
) -> Option<crate::equilibrium::Contact> {
    let (brick, other) = (&bricks[a], &bricks[b]);
    let overlap = brick.bounds.max.min(other.bounds.max) - brick.bounds.min.max(other.bounds.min);
    if !crate::interference::axis_aligned(brick)
        || !crate::interference::axis_aligned(other)
        || overlap.y.abs() > tol.position_brick_units
        || overlap.x <= tol.position_brick_units
        || overlap.z <= tol.position_brick_units
    {
        return None;
    }
    let min = brick.bounds.min.max(other.bounds.min);
    let max = brick.bounds.max.min(other.bounds.max);
    let (lower, upper) = if brick.bounds.center().y < other.bounds.center().y {
        (a, b)
    } else {
        (b, a)
    };
    Some(crate::equilibrium::Contact {
        lower,
        upper,
        patch: crate::Aabb::new(
            DVec3::new(min.x, (min.y + max.y) * 0.5, min.z),
            DVec3::new(max.x, (min.y + max.y) * 0.5, max.z),
        ),
    })
}

/// Certify static equilibrium of one island with the general solver
/// ([`crate::equilibrium`]): joints within their capacities, frictionless
/// unilateral contact patches between axis-aligned bodies, ground supports
/// and anchored bricks.
fn support_certificate(
    bricks: Bodies<'_>,
    joints: &[crate::JointSpec],
    fixed: &[u32],
    tol: StaticTolerances,
    measurements: &crate::physical::ConnectionMeasurements,
    workspace: &mut dyn crate::equilibrium::SolverWorkspace,
) -> Result<crate::equilibrium::Certificate> {
    let n = bricks.len();
    let index: HashMap<_, _> = bricks.iter().enumerate().map(|(i, b)| (b.id, i)).collect();
    let mut parent = components::Partition::new(n);
    for joint in joints {
        let (a, b) = (index[&joint.a], index[&joint.b]);
        parent.join(a, b);
    }
    let mut contacts = Vec::new();
    let tree = Octree::new(bricks.iter().map(|b| b.bounds).collect());
    for (a, brick) in bricks.iter().enumerate() {
        for b in tree.query(brick.bounds.expanded(tol.position_brick_units)) {
            // Touching axis-aligned bodies bear in compression whether or not
            // a joint also holds them: the joint only adds its capacity.
            if b <= a {
                continue;
            }
            if let Some(contact) = nominal_contact(bricks, a, b, tol) {
                contacts.push(contact);
                parent.join(a, b);
            }
        }
    }
    let mut supports = Vec::new();
    let mut supported = vec![false; n];
    for (i, brick) in bricks.iter().enumerate() {
        if brick.bounds.min.y.abs() <= tol.position_brick_units {
            for point in ground_support_points(brick) {
                supports.push((i, point));
            }
            let r = parent.root(i);
            supported[r] = true;
        }
    }
    let anchored: Vec<usize> = bricks
        .iter()
        .enumerate()
        .filter(|(_, b)| fixed.contains(&b.id))
        .map(|(i, _)| i)
        .collect();
    for &i in &anchored {
        let r = parent.root(i);
        supported[r] = true;
    }
    for (i, brick) in bricks.iter().enumerate() {
        let r = parent.root(i);
        ensure!(
            supported[r],
            "Construction is not static: component containing brick {} has no ground support",
            brick.id
        );
    }
    let masses = bricks
        .iter()
        .map(|b| measurements.mass_kg(&b.part))
        .collect::<Result<Vec<_>>>()?;
    crate::equilibrium::certify_with_workspace(
        &crate::equilibrium::Problem {
            bricks,
            joints,
            contacts,
            supports,
            anchored,
            masses,
            force_tolerance: tol.force_newtons,
            torque_tolerance: tol.torque_newton_meters,
        },
        workspace,
    )
}

pub fn compression_issue(
    bricks: &[Brick],
    tol: StaticTolerances,
    measurements: &crate::physical::ConnectionMeasurements,
) -> Result<Option<CompressionIssue>> {
    compression_issue_for(Bodies::all(bricks), tol, measurements)
}

fn compression_issue_for(
    bricks: Bodies<'_>,
    tol: StaticTolerances,
    measurements: &crate::physical::ConnectionMeasurements,
) -> Result<Option<CompressionIssue>> {
    // Both paths construct nonnegative reactions and check the same force/moment
    // tolerances. Prefer a direct centred reaction on a real bearing patch;
    // corner-only triangulation can needlessly tip the next supporting piece.
    let preferred = compression_issue_with_strategy(bricks, tol, measurements, true)?;
    if preferred.is_none() {
        return Ok(None);
    }
    if compression_issue_with_strategy(bricks, tol, measurements, false)?.is_none() {
        Ok(None)
    } else {
        Ok(preferred)
    }
}

/// Only bodies awaiting an accumulated load are resident here. A body's own
/// weight is initialized on first use; a processed body's load is removed.
/// This preserves the existing addition order (own weight, then reactions) while
/// avoiding a load record for every body in a tall structure.
struct CompressionLoads<'a> {
    entries: HashMap<usize, (f64, DVec2)>,
    bricks: Bodies<'a>,
    origin: DVec3,
    measurements: &'a crate::physical::ConnectionMeasurements,
}
impl<'a> CompressionLoads<'a> {
    fn new(
        bricks: Bodies<'a>,
        origin: DVec3,
        measurements: &'a crate::physical::ConnectionMeasurements,
    ) -> Self {
        Self {
            entries: HashMap::new(),
            bricks,
            origin,
            measurements,
        }
    }

    fn initial(&self, i: usize) -> Result<(f64, DVec2)> {
        let brick = &self.bricks[i];
        let mass = self.measurements.mass_kg(&brick.part)?;
        let center = crate::physical::to_meters(brick.bounds.center() - self.origin);
        let moment = DVec2::new(center.x, center.z) * mass;
        if !(mass * crate::physical::GRAVITY_M_S2).is_finite() || !moment.is_finite() {
            return Err(crate::equilibrium::Inconclusive(
                "compression load initialization overflowed".into(),
            )
            .into());
        }
        Ok((mass, moment))
    }

    fn take(&mut self, i: usize) -> Result<(f64, DVec2)> {
        self.entries.remove(&i).map_or_else(|| self.initial(i), Ok)
    }

    fn add(&mut self, i: usize, mass: f64, moment: DVec2) -> Result<()> {
        if !self.entries.contains_key(&i) {
            let initial = self.initial(i)?;
            self.entries.insert(i, initial);
        }
        let load = self.entries.get_mut(&i).unwrap();
        load.0 += mass;
        load.1 += moment;
        if !(load.0 * crate::physical::GRAVITY_M_S2).is_finite() || !load.1.is_finite() {
            return Err(crate::equilibrium::Inconclusive(
                "compression load propagation overflowed".into(),
            )
            .into());
        }
        Ok(())
    }
}

fn compression_issue_with_strategy(
    bricks: Bodies<'_>,
    tol: StaticTolerances,
    measurements: &crate::physical::ConnectionMeasurements,
    centered: bool,
) -> Result<Option<CompressionIssue>> {
    use crate::physical::{GRAVITY_M_S2, to_meters};
    ensure!(
        !bricks.is_empty(),
        "Compression analysis needs at least one brick"
    );
    if let Some(brick) = bricks
        .iter()
        .find(|b| !crate::interference::axis_aligned(b))
    {
        return Ok(Some(CompressionIssue {
            brick_id: brick.id, part: brick.part.clone(),
            reason: "Tilted body requires a joint-supported load path; an envelope face is not a bearing patch".into(),
            loaded_center_world: brick.bounds.center(), support_polygon_world: vec![],
        }));
    }
    let origin = bricks[0].bounds.center();
    let tree = Octree::new(
        bricks
            .iter()
            .map(crate::part_profiles::bearing_bounds)
            .collect(),
    );
    let mut loads = CompressionLoads::new(bricks, origin, measurements);
    let mut order: Vec<_> = (0..bricks.len()).collect();
    order.sort_by(|&a, &b| bricks[b].bounds.min.y.total_cmp(&bricks[a].bounds.min.y));
    for i in order {
        let brick = &bricks[i];
        let (mass, moment) = loads.take(i)?;
        let center = moment / mass;
        let mut supports = Vec::new();
        let mut patches = Vec::new();
        if brick.bounds.min.y.abs() <= tol.position_brick_units {
            for point in ground_support_points(brick) {
                let p = to_meters(point - origin);
                supports.push((DVec2::new(p.x, p.z), None));
            }
        } else {
            for j in tree.query(brick.bounds.expanded(tol.position_brick_units)) {
                if j == i {
                    continue;
                }
                let lower = &bricks[j];
                if lower.bounds.min.y >= brick.bounds.min.y {
                    continue;
                }
                for foot in crate::part_profiles::lower_bearing_regions(brick) {
                    for face in crate::part_profiles::upper_bearing_regions(lower) {
                        if (face.max.y - foot.min.y).abs() > tol.position_brick_units {
                            continue;
                        }
                        let lo = foot.min.max(face.min);
                        let hi = foot.max.min(face.max);
                        if hi.x - lo.x <= tol.position_brick_units
                            || hi.z - lo.z <= tol.position_brick_units
                        {
                            continue;
                        }
                        let a = to_meters(lo - origin);
                        let b = to_meters(hi - origin);
                        patches.push((DVec2::new(a.x, a.z), DVec2::new(b.x, b.z), j));
                        for (x, z) in [(lo.x, lo.z), (hi.x, lo.z), (hi.x, hi.z), (lo.x, hi.z)] {
                            let p = to_meters(DVec3::new(x, foot.min.y, z) - origin);
                            supports.push((DVec2::new(p.x, p.z), Some(j)));
                        }
                    }
                }
            }
        }
        let hull = convex_hull(supports.iter().map(|(point, _)| *point).collect());
        let issue = |reason: &str| CompressionIssue {
            brick_id: brick.id,
            part: brick.part.clone(),
            reason: reason.into(),
            loaded_center_world: DVec3::new(
                origin.x + center.x / 0.008,
                brick.bounds.min.y,
                origin.z + center.y / 0.008,
            ),
            support_polygon_world: hull
                .iter()
                .map(|p| {
                    crate::coordinates::public(DVec3::new(
                        origin.x + p.x / 0.008,
                        brick.bounds.min.y,
                        origin.z + p.y / 0.008,
                    ))
                    .to_array()
                })
                .collect(),
        };
        if hull.len() < 3 {
            return Ok(Some(issue(
                "No two-dimensional bearing patch under the loaded brick",
            )));
        }
        let mut reactions = None;
        if centered
            && let Some((_, _, lower)) = patches
                .iter()
                .filter(|(lo, hi, _)| center.cmpge(*lo).all() && center.cmple(*hi).all())
                .max_by(|(a, b, _), (c, d, _)| {
                    ((b.x - a.x) * (b.y - a.y)).total_cmp(&((d.x - c.x) * (d.y - c.y)))
                })
        {
            reactions = Some(vec![(center, 1., Some(*lower))]);
        }
        for k in 1..hull.len() - 1 {
            if reactions.is_some() {
                break;
            }
            let [a, b, c] = [hull[0], hull[k], hull[k + 1]];
            let area = (b - a).perp_dot(c - a);
            if area <= 1e-15 {
                continue;
            }
            let wa = (b - center).perp_dot(c - center) / area;
            let wb = (c - center).perp_dot(a - center) / area;
            let wc = 1. - wa - wb;
            if [wa, wb, wc].iter().any(|w| *w < -1e-10) {
                continue;
            }
            let weights = [wa.max(0.), wb.max(0.), wc.max(0.)];
            let sum: f64 = weights.iter().sum();
            let points = [a, b, c];
            let residual = points
                .iter()
                .zip(weights)
                .map(|(p, w)| *p * (w / sum))
                .sum::<DVec2>()
                - center;
            if residual.length() * mass * GRAVITY_M_S2 > tol.torque_newton_meters {
                continue;
            }
            reactions = Some(
                points
                    .into_iter()
                    .zip(weights)
                    .map(|(point, weight)| {
                        (
                            point,
                            weight / sum,
                            supports.iter().find(|(p, _)| *p == point).unwrap().1,
                        )
                    })
                    .collect::<Vec<_>>(),
            );
            break;
        }
        let Some(reactions) = reactions else {
            return Ok(Some(issue(
                "No admissible compressive reactions balance the accumulated load",
            )));
        };
        for (point, weight, lower) in reactions {
            if let Some(j) = lower {
                loads.add(j, mass * weight, point * (mass * weight))?;
            }
        }
    }
    debug_assert!(loads.entries.is_empty());
    Ok(None)
}

fn convex_hull(mut points: Vec<DVec2>) -> Vec<DVec2> {
    points.sort_by(|a, b| a.x.total_cmp(&b.x).then(a.y.total_cmp(&b.y)));
    points.dedup();
    if points.len() < 3 {
        return points;
    }
    let mut hull = Vec::new();
    for p in &points {
        while hull.len() >= 2 {
            let n = hull.len();
            let a: DVec2 = hull[n - 2];
            let b: DVec2 = hull[n - 1];
            if (b - a).perp_dot(*p - b) > 0.0 {
                break;
            }
            hull.pop();
        }
        hull.push(*p);
    }
    let lower = hull.len();
    for p in points.iter().rev().skip(1) {
        while hull.len() > lower {
            let n = hull.len();
            if (hull[n - 1] - hull[n - 2]).perp_dot(*p - hull[n - 1]) > 0.0 {
                break;
            }
            hull.pop();
        }
        hull.push(*p);
    }
    hull.pop();
    hull
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{GeoReference, SCHEMA_VERSION, demo::Demo};
    fn scene(d: Demo) -> WorldExport {
        WorldExport {
            schema: SCHEMA_VERSION,
            name: "test".into(),
            geo: GeoReference::default(),
            bricks: d.bricks,
            meshes: d.meshes,
            joints: Vec::new(),
            fixed_bricks: Vec::new(),
            ground: Vec::new(),
            groups: Vec::new(),
            model: None,
        }
    }

    #[test]
    fn loose_multi_pillar_roof_balances_compression_independent_of_piece_order() {
        let mut demo = Demo::new();
        for (x, z) in [(0., 0.), (3., 0.), (0., 3.), (3., 3.)] {
            for h in 0..6 {
                demo.add("3070b", 19, x, h as f64 / 3., z);
            }
        }
        demo.add("3031", 14, 0., 2., 0.);
        let mut model = scene(demo);
        for _ in 0..25 {
            validate_static(&mut model).unwrap();
            assert!(model.joints.is_empty());
            model.bricks.rotate_left(1);
        }
    }

    #[test]
    fn centered_bearings_do_not_artificially_tip_split_plates_under_a_balanced_roof() {
        let mut demo = Demo::new();
        demo.add("3001", 19, 0., 0., 1.);
        for x in [0., 2.] {
            demo.add("3020", 19, 0., 1., 0.);
            let brick = demo.bricks.last_mut().unwrap();
            let mesh = &demo.meshes[&brick.part];
            brick.transform = glam::DMat3::from_rotation_y(std::f64::consts::FRAC_PI_2);
            let bounds = mesh.body_bounds.transformed(brick.transform, DVec3::ZERO);
            brick.position = DVec3::new(x, 1., 0.) - bounds.min;
            brick.bounds = mesh
                .body_bounds
                .transformed(brick.transform, brick.position);
            brick.render_bounds = mesh.bounds.transformed(brick.transform, brick.position);
        }
        for layer in 0..4 {
            demo.add("3031", 15, 0., 4. / 3. + f64::from(layer) / 3., 0.);
        }
        let measurements = crate::physical::ConnectionMeasurements::default();
        assert!(
            compression_issue_with_strategy(
                Bodies::all(&demo.bricks),
                Default::default(),
                &measurements,
                false
            )
            .unwrap()
            .is_some()
        );
        assert!(
            compression_issue(&demo.bricks, Default::default(), &measurements)
                .unwrap()
                .is_none()
        );
        let report = validate_static(&mut scene(demo)).unwrap();
        assert_eq!(report.joint_strength_required, Some(false));
    }

    #[test]
    fn upright_nine_course_columns_need_no_clutch_for_gravity_balance() {
        let mut d = Demo::new();
        for (x, z) in [(0., 0.), (6., 0.), (0., 6.), (6., 6.)] {
            for level in 0..9 {
                d.add("3003", 19, x, f64::from(level), z);
            }
        }
        d.add("41539", 14, 0., 9., 0.);
        let mut model = scene(d);
        for _ in 0..8 {
            let report = validate_static(&mut model).unwrap();
            assert!(!model.joints.is_empty());
            assert_eq!(report.joint_strength_required, Some(false));
            assert!(model.joints.iter().all(|joint| joint.break_force.is_none()));
            model.bricks.rotate_left(1);
        }
    }

    #[test]
    fn saves_stable_stack_but_rejects_overlap_and_floating_parts() {
        let mut d = Demo::new();
        d.add("3003", 4, 2.0, 0.0, 2.0);
        d.add("3003", 1, 2.0, 1.0, 2.0);
        assert!(validate_static(&mut scene(d)).is_ok());
        let mut d = Demo::new();
        d.add("3003", 4, 2.0, 0.0, 2.0);
        d.add("3003", 1, 2.0, 0.5, 2.0);
        assert!(
            validate_static(&mut scene(d))
                .unwrap_err()
                .to_string()
                .contains("overlap")
        );
        let mut d = Demo::new();
        d.add("3003", 4, 2.0, 4.0, 2.0);
        assert!(
            validate_static(&mut scene(d))
                .unwrap_err()
                .to_string()
                .contains("not static")
        );
    }

    #[test]
    fn rejects_top_heavy_grounded_rigid_assembly() {
        let mut d = Demo::new();
        d.add("3005", 4, 4.0, 0.0, 4.0);
        d.add("3001", 1, 4.0, 1.0, 1.0);
        assert!(validate_static(&mut scene(d)).is_err());
    }

    #[test]
    fn pavilion_has_a_scalable_ground_support_certificate() {
        // The pavilion's authored bonds are idealized (a read-only example):
        // without measured or reviewed capacities they cannot carry load.
        let d = crate::demo::pavilion();
        let joints = crate::demo::authored_bonds(&d.bricks);
        let mut s = scene(d);
        s.joints = joints;
        assert!(validate_static(&mut s.clone()).is_err());
        let idealized = crate::physical::ConnectionMeasurements {
            idealized_fixture: true,
            ..Default::default()
        };
        let report =
            validate_static_with_measurements(&mut s, StaticTolerances::default(), &idealized)
                .unwrap();
        assert!(report.equilibrium_solver.partitions > 1);
        assert!(report.equilibrium_solver.balance_correction_iterations > 0);
        assert!(report.equilibrium_solver.sweeps <= 500);
        assert!(report.equilibrium_solver.largest_local_kkt_dimension <= 2048);
    }

    #[test]
    fn independent_stacks_are_certified_separately_but_a_shared_roof_keeps_one_island() {
        let mut demo = Demo::new();
        for x in [-128., 128.] {
            for h in 0..9 {
                demo.add("3005", 4, x, h as f64, 0.);
            }
        }
        let mut model = scene(demo);
        let report = validate_static(&mut model).unwrap();
        assert_eq!(report.stability_components, 2);
        assert_eq!(report.largest_equilibrium_component, 0);
        assert_eq!(report.joint_strength_required, Some(false));
        model.bricks.reverse();
        let order: Vec<_> = model.bricks.iter().map(|b| b.id).collect();
        assert_eq!(validate_static(&mut model).unwrap().stability_components, 2);
        assert_eq!(model.bricks.iter().map(|b| b.id).collect::<Vec<_>>(), order);

        // No explicit joints are needed to couple the two pillars through a
        // loose roof. Storage boundaries and separate ground points are not cuts.
        let mut demo = Demo::new();
        demo.add("3005", 4, 127., 0., 0.);
        demo.add("3005", 4, 130., 0., 0.);
        demo.add("3001", 4, 127., 1., 0.);
        let islands = components::partition(&demo.bricks, &[], Default::default()).unwrap();
        assert_eq!(islands.len(), 1);
        let report = validate_static(&mut scene(demo)).unwrap();
        assert_eq!(report.stability_components, 1);
    }

    #[test]
    fn reviewed_wing_bearings_are_not_cut_at_the_mass_envelope() {
        let mut demo = Demo::new();
        demo.add("4444", 71, 0., 0., 0.);
        let wing = crate::part_profiles::upper_bearing_regions(&demo.bricks[0])[1];
        demo.add("3005", 71, wing.min.x, wing.min.y, wing.min.z);
        assert!(nominal_contact(Bodies::all(&demo.bricks), 0, 1, Default::default()).is_none());
        let islands = components::partition(&demo.bricks, &[], Default::default()).unwrap();
        assert_eq!(
            islands.len(),
            1,
            "The real wing still transmits the upper brick's weight"
        );
    }

    #[test]
    fn only_the_loaded_island_needs_equilibrium_and_aggregate_results_match_a_whole_solve() {
        // This internal solver fixture supplies test capacities directly; it
        // does not bypass catalogue-attachment checks in the public save gate.
        let mut demo = Demo::new();
        demo.add("3005", 4, 4., 0., 4.);
        demo.add("3001", 4, 4., 1., 1.);
        let joint = crate::JointSpec {
            a: 1,
            b: 2,
            anchor: DVec3::new(4.5, 1., 4.5),
            kind: crate::JointKind::Fixed,
            break_force: Some(10.),
            break_torque: Some(0.1),
            axial_force: None,
            friction_torque: None,
            strength_source: Some("test fixture".into()),
        };
        for i in 0..20 {
            demo.add("3005", 4, 20. + i as f64 * 4., 0., 0.);
        }
        let measurements = crate::physical::ConnectionMeasurements::default();
        let tol = StaticTolerances::default();
        let joints = [joint];
        let result = certify_components(
            &demo.bricks,
            &joints,
            &[1],
            tol,
            &measurements,
            &mut crate::equilibrium::MemoryWorkspace::default(),
        )
        .unwrap();
        let whole = support_certificate(
            Bodies::all(&demo.bricks),
            &joints,
            &[1],
            tol,
            &measurements,
            &mut crate::equilibrium::MemoryWorkspace::default(),
        )
        .unwrap();
        assert_eq!(result.components, 21);
        assert_eq!(result.largest_equilibrium_component, 2);
        assert!(!result.contact_only);
        assert_eq!(result.certificate.loaded_joints, whole.loaded_joints);
        assert!(result.certificate.loaded_joints > 0);
        assert!((result.certificate.max_utilisation - whole.max_utilisation).abs() < 1e-3);
        // A far-away good island cannot supply fictitious support to a floater.
        demo.add("3005", 4, 200., 10., 0.);
        assert!(
            certify_components(
                &demo.bricks,
                &joints,
                &[1],
                tol,
                &measurements,
                &mut crate::equilibrium::MemoryWorkspace::default()
            )
            .is_err()
        );
        // Nor can partitioning hide an insufficient joint capacity.
        let weak = [crate::JointSpec {
            break_torque: Some(0.),
            ..joints[0].clone()
        }];
        assert!(
            certify_components(
                &demo.bricks[..22],
                &weak,
                &[1],
                tol,
                &measurements,
                &mut crate::equilibrium::MemoryWorkspace::default()
            )
            .is_err()
        );
    }

    #[test]
    fn compression_frontier_releases_processed_loads_and_keeps_accumulated_weight() {
        let mut demo = Demo::new();
        for h in 0..2048 {
            demo.add("3005", 4, 0., h as f64, 0.);
        }
        let measurements = crate::physical::ConnectionMeasurements::default();
        let mut loads = CompressionLoads::new(
            Bodies::all(&demo.bricks),
            demo.bricks[0].bounds.center(),
            &measurements,
        );
        let mass = measurements.mass_kg("3005.dat").unwrap();
        for i in (0..demo.bricks.len()).rev() {
            let (weight, moment) = loads.take(i).unwrap();
            assert!(loads.entries.is_empty());
            assert!((weight - mass * (demo.bricks.len() - i) as f64).abs() < 1e-10);
            if i > 0 {
                loads.add(i - 1, weight, moment).unwrap();
            }
            assert!(
                loads.entries.len() <= 1,
                "Frontier size must not scale with stack height"
            );
        }
        assert!(loads.entries.is_empty());
        assert!(
            compression_issue(&demo.bricks, Default::default(), &measurements)
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn a_load_sharing_truss_is_one_island_and_losing_a_member_still_fails() {
        // Low-level force fixture, as in equilibrium::tests: the two paths to
        // the apex must share its weight, not receive invented boundary anchors.
        let mut demo = Demo::new();
        for (x, y) in [(0., 0.), (4., 0.), (1., 2.), (3., 2.), (2., 4.)] {
            demo.add("3005", 4, x, y, 0.);
        }
        let joints: Vec<_> = [
            (1, 3, 0.5, 1.),
            (3, 5, 1.5, 3.),
            (2, 4, 4.5, 1.),
            (4, 5, 3.5, 3.),
        ]
        .into_iter()
        .map(|(a, b, x, y)| crate::JointSpec {
            a,
            b,
            anchor: DVec3::new(x, y, 0.5),
            kind: crate::JointKind::Revolute {
                axis: DVec3::Z,
                limits: None,
            },
            break_force: Some(150.),
            break_torque: Some(0.3),
            axial_force: Some(0.2),
            friction_torque: Some(0.),
            strength_source: Some("test fixture".into()),
        })
        .collect();
        demo.add("3005", 4, -128., 0., 0.);
        let measurements = crate::physical::ConnectionMeasurements::default();
        let result = certify_components(
            &demo.bricks,
            &joints,
            &[1, 2],
            Default::default(),
            &measurements,
            &mut crate::equilibrium::MemoryWorkspace::default(),
        )
        .unwrap();
        assert_eq!(result.components, 2);
        assert_eq!(result.largest_equilibrium_component, 5);
        assert!(result.certificate.loaded_joints > 0);
        assert!(
            certify_components(
                &demo.bricks,
                &joints[..3],
                &[1, 2],
                Default::default(),
                &measurements,
                &mut crate::equilibrium::MemoryWorkspace::default()
            )
            .is_err()
        );
    }
}
