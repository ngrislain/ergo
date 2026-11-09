//! Persistence gate: no penetrating nominal body boxes, plus a conservative
//! static support/constraint-wrench certificate. No time integration runs here.
//! declared simplified model, not real ABS stress, clutch strength, or earthquakes.
#[cfg(test)]
use crate::equilibrium::Bodies;
use crate::equilibrium::ProblemInput;
use crate::{Brick, WorldExport, constraints};
use anyhow::{Result, ensure};
use glam::{DVec2, DVec3};
use serde::{Deserialize, Serialize};

#[path = "validation_components.rs"]
mod components;
#[path = "validation_input.rs"]
mod input;
pub use input::{ValidationInput, validation_envelope};
#[path = "validation_constraints.rs"]
mod constraints_input;
#[path = "validation_workspace.rs"]
pub(crate) mod working_state;
pub use constraints_input::ValidationConstraints;
pub use working_state::{
    CompressionLoad, MemoryValidationState, ValidationRecord, ValidationState,
};

pub const OVERLAP_TOLERANCE: f64 = 0.0001;
const MAX_COMPRESSION_SUPPORT_POINTS: usize = 16_384;

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
        validate_ground_anchor(brick, tolerances)?;
    }
    Ok(())
}
fn validate_ground_anchor(brick: &Brick, tolerances: StaticTolerances) -> Result<()> {
    ensure!(
        brick.bounds.min.y.abs() <= tolerances.position_brick_units,
        "Anchored brick {} is not on its baseplate; a floating world anchor cannot certify a construction",
        brick.id
    );
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
    crate::groups::validate(&scene.bricks, &scene.groups)?;
    if let Some(model) = &scene.model {
        model.validate_isometries()?;
    }
    let input = input::SliceInput::new(&scene.bricks);
    validate_input_with_workspace(
        &input,
        &mut scene.joints,
        &scene.fixed_bricks,
        tolerances,
        measurements,
        workspace,
    )
}
/// Physical gate over normalized, indexed body access. Authoring group/model
/// metadata is validated by its owner; it is never a substitute physical weld.
pub fn validate_input_with_workspace(
    input: &dyn ValidationInput,
    joints: &mut Vec<crate::JointSpec>,
    fixed: &[u32],
    tolerances: StaticTolerances,
    measurements: &crate::physical::ConnectionMeasurements,
    workspace: &mut dyn crate::equilibrium::SolverWorkspace,
) -> Result<ValidationReport> {
    let mut constraints = ValidationConstraints::new(workspace.new_state()?);
    for joint in joints.iter() {
        constraints.push_joint(joint.clone())?;
    }
    for &id in fixed {
        constraints.push_anchor(id)?;
    }
    let report = validate_constraints_with_workspace(
        input,
        &mut constraints,
        tolerances,
        measurements,
        workspace,
    )?;
    *joints = (0..constraints.len())
        .map(|i| constraints.joint(i))
        .collect::<Result<_>>()?;
    Ok(report)
}
pub fn validate_constraints_with_workspace(
    input: &dyn ValidationInput,
    joints: &mut ValidationConstraints,
    tolerances: StaticTolerances,
    measurements: &crate::physical::ConnectionMeasurements,
    workspace: &mut dyn crate::equilibrium::SolverWorkspace,
) -> Result<ValidationReport> {
    use anyhow::Context;
    measurements.validate()?;
    tolerances.validate()?;
    ensure!(
        !input.is_empty(),
        "A static construction must contain at least one brick"
    );
    joints.verify()?;
    for i in 0..input.len() {
        let brick = input.body(i)?;
        crate::catalog::validate(std::slice::from_ref(brick.as_ref()))?;
        crate::physical::validate_pose(&brick)?;
        let mut expected = brick.clone().into_owned();
        measurements.normalize_brick(&mut expected)?;
        ensure!(
            brick.bounds.min == expected.bounds.min && brick.bounds.max == expected.bounds.max,
            "Physical body bounds were not normalized from the frozen model"
        );
        input.check_indexed_bounds(i, validation_envelope(&brick))?;
        ensure!(
            input.body_index(brick.id)? == Some(i),
            "Duplicate brick IDs or physical body identity/index mismatch"
        );
    }
    ensure!(
        joints.len() <= input.len().saturating_mul(12),
        "Too many constraints"
    );
    for i in 0..joints.len() {
        let joint = joints.joint(i)?;
        ensure!(
            joint.a != joint.b,
            "Joint references an absent or identical body"
        );
        constraints::validate_joint_spec(&joint)?;
        let a = input.body(
            input
                .body_index(joint.a)?
                .context("Attachment endpoint is absent")?,
        )?;
        let b = input.body(
            input
                .body_index(joint.b)?
                .context("Attachment endpoint is absent")?,
        )?;
        crate::attachments::validate_pair(
            &a,
            &b,
            &joint,
            tolerances.position_brick_units,
            tolerances.rotation_radians,
        )?;
    }
    for i in 0..joints.anchor_len() {
        let id = joints.anchor(i)?;
        let brick = input.body(input.body_index(id)?.context("Anchored brick is absent")?)?;
        validate_ground_anchor(&brick, tolerances)?;
    }
    for i in 0..input.len() {
        let brick = input.body(i)?;
        validate_baseplate_brick(&brick, tolerances)?;
        ensure!(
            brick.bounds.min.y >= -tolerances.position_brick_units,
            "Brick {} intersects its baseplate",
            brick.id
        );
        for candidate in input.query(brick.bounds)? {
            let j = candidate?;
            ensure!(j < input.len(), "Spatial candidate is absent");
            if j <= i {
                continue;
            }
            let other = input.body(j)?;
            let overlap =
                brick.bounds.max.min(other.bounds.max) - brick.bounds.min.max(other.bounds.min);
            ensure!(
                !crate::interference::bodies_penetrate(
                    &brick,
                    &other,
                    tolerances.position_brick_units
                ),
                "Bricks {} and {} overlap by {:.4} brick units; fix the model before saving",
                brick.id,
                other.id,
                overlap.min_element()
            );
        }
    }
    for i in 0..input.len() {
        let source = input.body(i)?;
        if let Some(bounds) = crate::interference::stud_bounds(&source) {
            for candidate in input.query(bounds)? {
                let j = candidate?;
                ensure!(j < input.len(), "Spatial candidate is absent");
                let target = input.body(j)?;
                if let Some(error) = crate::interference::stud_collision_pair(
                    &source,
                    &target,
                    tolerances.position_brick_units,
                ) {
                    anyhow::bail!(error);
                }
            }
        }
    }
    let mut inferred_count = 0;
    for i in 0..input.len() {
        let a = input.body(i)?;
        for candidate in input.query(
            crate::attachments::bounds(&a)
                .expanded(constraints::INFERRED_JOINT_HALO.max(tolerances.position_brick_units)),
        )? {
            let j = candidate?;
            ensure!(j < input.len(), "Spatial candidate is absent");
            if j <= i {
                continue;
            }
            let b = input.body(j)?;
            if !joints.has_pair(a.id, b.id)?
                && let Some(joint) =
                    constraints::inferred_pair(&a, &b, tolerances.position_brick_units)
            {
                joints.push_joint(joint)?;
                inferred_count += 1;
            }
        }
    }
    joints.verify()?;
    for i in 0..input.len() {
        let b = input.body(i)?;
        if !crate::interference::axis_aligned(&b) {
            ensure!(
                b.bounds.min.y > tolerances.position_brick_units && joints.has_body(b.id)?,
                "Cannot certify tilted brick {} ({}): tilted bodies require a joint-supported load path, not a bounding-box bearing face",
                b.id,
                b.part
            );
        }
    }
    for i in 0..joints.len() {
        let mut joint = joints.joint(i)?;
        let a = input.body(
            input
                .body_index(joint.a)?
                .context("Missing joint endpoint")?,
        )?;
        let b = input.body(
            input
                .body_index(joint.b)?
                .context("Missing joint endpoint")?,
        )?;
        measurements.apply_pair(&a, &b, &mut joint)?;
        joints.put_joint(i, joint)?;
    }
    let stability = certify_input_components(input, joints, tolerances, measurements, workspace)?;
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
        checked_bricks: input.len(),
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
#[cfg(test)]
fn certify_components(
    bricks: &[Brick],
    joints: &[crate::JointSpec],
    fixed: &[u32],
    tol: StaticTolerances,
    measurements: &crate::physical::ConnectionMeasurements,
    workspace: &mut dyn crate::equilibrium::SolverWorkspace,
) -> Result<StabilityCertificate> {
    let mut constraints = ValidationConstraints::new(workspace.new_state()?);
    for joint in joints {
        constraints.push_joint(joint.clone())?;
    }
    for &id in fixed {
        constraints.push_anchor(id)?;
    }
    certify_input_components(
        &input::SliceInput::new(bricks),
        &constraints,
        tol,
        measurements,
        workspace,
    )
}
fn certify_input_components(
    input: &dyn ValidationInput,
    joints: &ValidationConstraints,
    tol: StaticTolerances,
    measurements: &crate::physical::ConnectionMeasurements,
    workspace: &mut dyn crate::equilibrium::SolverWorkspace,
) -> Result<StabilityCertificate> {
    let count = {
        let (state, _) = workspace.validation_state();
        components::partition_input(input, joints, tol, state)?
    };
    let mut result = StabilityCertificate {
        certificate: Default::default(),
        contact_only: true,
        compression_issue: None,
        components: count,
        largest_equilibrium_component: 0,
    };
    for component in 0..count {
        let store = workspace.new_state()?;
        let (problem, issue) = {
            let (state, compression) = workspace.validation_state();
            let island = components::ComponentInput::new(input, state, component)?;
            // A constructive contact-only proof remains valid even with joints:
            // all their wrenches can be zero, without assuming unknown clutch loads.
            let issue = if !island.anchored {
                compression_issue_input(&island, tol, measurements, compression)?
            } else {
                None
            };
            if !island.anchored && issue.is_none() {
                continue;
            }
            let constraints =
                (0..island.joints).map(|i| island.joint_index(i).and_then(|j| joints.joint(j)));
            let problem = build_problem(
                &island,
                constraints,
                |i| island.is_anchored(i),
                tol,
                measurements,
                store,
            )?;
            (problem, issue)
        };
        result.contact_only = false;
        if result.compression_issue.is_none() {
            result.compression_issue = issue;
        }
        result.largest_equilibrium_component = result
            .largest_equilibrium_component
            .max(problem.size().bodies);
        // Compression-only islands never need a copied joint list, either.
        let certificate = crate::equilibrium::certify_input_with_workspace(&problem, workspace)?;
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
    for brick in bricks {
        validate_baseplate_brick(brick, tolerances)?;
    }
    Ok(())
}
fn validate_baseplate_brick(brick: &Brick, tolerances: StaticTolerances) -> Result<()> {
    if brick.bounds.min.y.abs() > tolerances.position_brick_units {
        return Ok(());
    }
    // Tyres rest on the virtual ground's flat bearing plane. They do not
    // engage ground studs and need no fabricated display plate beneath them.
    if crate::attachments::connectors(&brick.part)
        .iter()
        .any(|c| c.kind == crate::attachments::Kind::Tyre)
        || crate::catalog::canonical_id(&brick.part) == "4624c01.dat"
    {
        return Ok(());
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
#[cfg(test)]
fn nominal_contact(
    bricks: Bodies<'_>,
    a: usize,
    b: usize,
    tol: StaticTolerances,
) -> Option<crate::equilibrium::Contact> {
    let (brick, other) = (&bricks[a], &bricks[b]);
    nominal_body_contact(brick, other, a, b, tol)
}
fn nominal_body_contact(
    brick: &Brick,
    other: &Brick,
    a: usize,
    b: usize,
    tol: StaticTolerances,
) -> Option<crate::equilibrium::Contact> {
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
#[cfg(test)]
fn support_certificate(
    bricks: Bodies<'_>,
    joints: &[crate::JointSpec],
    fixed: &[u32],
    tol: StaticTolerances,
    measurements: &crate::physical::ConnectionMeasurements,
    workspace: &mut dyn crate::equilibrium::SolverWorkspace,
) -> Result<crate::equilibrium::Certificate> {
    let bodies = bricks.iter().cloned().collect::<Vec<_>>();
    let input = input::SliceInput::new(&bodies);
    let problem = build_problem(
        &input,
        joints.iter().cloned().map(Ok),
        |i| Ok(fixed.contains(&bodies[i].id)),
        tol,
        measurements,
        workspace.new_state()?,
    )?;
    crate::equilibrium::certify_input_with_workspace(&problem, workspace)
}

fn build_problem(
    input: &dyn ValidationInput,
    joints: impl Iterator<Item = Result<crate::JointSpec>>,
    anchored: impl Fn(usize) -> Result<bool>,
    tol: StaticTolerances,
    measurements: &crate::physical::ConnectionMeasurements,
    store: Box<dyn ValidationState>,
) -> Result<crate::equilibrium::StoredProblem> {
    let mut problem =
        crate::equilibrium::StoredProblem::new(store, tol.force_newtons, tol.torque_newton_meters);
    let mut supported = false;
    for i in 0..input.len() {
        let b = input.body(i)?;
        let mass = measurements.mass_kg(&b.part)?;
        problem.push_body(b.clone().into_owned(), mass)?;
        if anchored(i)? {
            problem.push_anchor(i)?;
            supported = true;
        }
        if b.bounds.min.y.abs() <= tol.position_brick_units {
            for point in ground_support_points(&b) {
                problem.push_support(i, point)?;
                supported = true;
            }
        }
        for candidate in input.query(validation_envelope(&b).expanded(tol.position_brick_units))? {
            let j = candidate?;
            if j <= i {
                continue;
            }
            let other = input.body(j)?;
            for c in body_contacts(&b, &other, i, j, tol) {
                problem.push_contact(c)?;
            }
        }
    }
    for joint in joints {
        problem.push_joint(joint?)?;
    }
    ensure!(
        supported,
        "Construction is not static: component containing brick {} has no ground support",
        input.body(0)?.id
    );
    Ok(problem)
}
fn body_contacts(
    a: &Brick,
    b: &Brick,
    i: usize,
    j: usize,
    tol: StaticTolerances,
) -> Vec<crate::equilibrium::Contact> {
    let mut contacts = nominal_body_contact(a, b, i, j, tol)
        .into_iter()
        .collect::<Vec<_>>();
    if !crate::interference::axis_aligned(a) || !crate::interference::axis_aligned(b) {
        return contacts;
    }
    let (upper, lower, u, l) = if a.bounds.min.y > b.bounds.min.y {
        (a, b, i, j)
    } else if b.bounds.min.y > a.bounds.min.y {
        (b, a, j, i)
    } else {
        return contacts;
    };
    for foot in crate::part_profiles::lower_bearing_regions(upper) {
        for face in crate::part_profiles::upper_bearing_regions(lower) {
            let lo = foot.min.max(face.min);
            let hi = foot.max.min(face.max);
            if (face.max.y - foot.min.y).abs() > tol.position_brick_units
                || hi.x - lo.x <= tol.position_brick_units
                || hi.z - lo.z <= tol.position_brick_units
            {
                continue;
            }
            let y = (face.max.y + foot.min.y) * 0.5;
            let patch = crate::Aabb::new(DVec3::new(lo.x, y, lo.z), DVec3::new(hi.x, y, hi.z));
            if !contacts.iter().any(|c| {
                c.lower == l && c.upper == u && c.patch.min == patch.min && c.patch.max == patch.max
            }) {
                contacts.push(crate::equilibrium::Contact {
                    lower: l,
                    upper: u,
                    patch,
                });
            }
        }
    }
    contacts
}

pub fn compression_issue(
    bricks: &[Brick],
    tol: StaticTolerances,
    measurements: &crate::physical::ConnectionMeasurements,
) -> Result<Option<CompressionIssue>> {
    compression_issue_input(
        &input::SliceInput::new(bricks),
        tol,
        measurements,
        &mut MemoryValidationState::default(),
    )
}

fn compression_issue_input(
    input: &dyn ValidationInput,
    tol: StaticTolerances,
    measurements: &crate::physical::ConnectionMeasurements,
    state: &mut dyn ValidationState,
) -> Result<Option<CompressionIssue>> {
    // Both paths construct nonnegative reactions and check the same force/moment
    // tolerances. Prefer a direct centred reaction on a real bearing patch;
    // corner-only triangulation can needlessly tip the next supporting piece.
    let preferred = compression_issue_with_strategy(input, tol, measurements, state, true)?;
    if preferred.is_none() {
        return Ok(None);
    }
    if compression_issue_with_strategy(input, tol, measurements, state, false)?.is_none() {
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
    state: &'a mut dyn ValidationState,
    input: &'a dyn ValidationInput,
    origin: DVec3,
    measurements: &'a crate::physical::ConnectionMeasurements,
    pending: usize,
}
impl<'a> CompressionLoads<'a> {
    fn new(
        input: &'a dyn ValidationInput,
        state: &'a mut dyn ValidationState,
        origin: DVec3,
        measurements: &'a crate::physical::ConnectionMeasurements,
    ) -> Self {
        Self {
            state,
            input,
            origin,
            measurements,
            pending: 0,
        }
    }

    fn initial(&self, i: usize) -> Result<(f64, DVec2)> {
        let brick = self.input.body(i)?;
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
        let load = match working_state::required(self.state, &working_state::key(0, &[i]))? {
            ValidationRecord::Load(CompressionLoad::Fresh) => self.initial(i)?,
            ValidationRecord::Load(CompressionLoad::Pending { mass, moment }) => {
                self.pending = self.pending.checked_sub(1).ok_or_else(|| {
                    working_state::unavailable("compression pending count changed")
                })?;
                (mass, DVec2::from_array(moment))
            }
            _ => {
                return Err(working_state::unavailable(
                    "compression load is absent, invalid or already processed",
                ));
            }
        };
        if !load.0.is_finite() || load.0 <= 0. || !load.1.is_finite() {
            return Err(working_state::unavailable("invalid compression load"));
        }
        self.state.put(
            working_state::key(0, &[i]),
            ValidationRecord::Load(CompressionLoad::Processed),
        )?;
        Ok(load)
    }

    fn add(&mut self, i: usize, mass: f64, moment: DVec2) -> Result<()> {
        let mut load = match working_state::required(self.state, &working_state::key(0, &[i]))? {
            ValidationRecord::Load(CompressionLoad::Fresh) => {
                self.pending += 1;
                self.initial(i)?
            }
            ValidationRecord::Load(CompressionLoad::Pending { mass, moment }) => {
                (mass, DVec2::from_array(moment))
            }
            _ => {
                return Err(working_state::unavailable(
                    "compression load is absent, invalid or already processed",
                ));
            }
        };
        load.0 += mass;
        load.1 += moment;
        if !(load.0 * crate::physical::GRAVITY_M_S2).is_finite() || !load.1.is_finite() {
            return Err(crate::equilibrium::Inconclusive(
                "compression load propagation overflowed".into(),
            )
            .into());
        }
        self.state.put(
            working_state::key(0, &[i]),
            ValidationRecord::Load(CompressionLoad::Pending {
                mass: load.0,
                moment: load.1.to_array(),
            }),
        )?;
        Ok(())
    }
}

fn compression_issue_with_strategy(
    input: &dyn ValidationInput,
    tol: StaticTolerances,
    measurements: &crate::physical::ConnectionMeasurements,
    state: &mut dyn ValidationState,
    centered: bool,
) -> Result<Option<CompressionIssue>> {
    use crate::physical::{GRAVITY_M_S2, to_meters};
    ensure!(
        !input.is_empty(),
        "Compression analysis needs at least one brick"
    );
    state.clear()?;
    for i in 0..input.len() {
        let brick = input.body(i)?;
        if !crate::interference::axis_aligned(&brick) {
            return Ok(Some(CompressionIssue {
            brick_id: brick.id, part: brick.part.clone(),
            reason: "Tilted body requires a joint-supported load path; an envelope face is not a bearing patch".into(),
            loaded_center_world: brick.bounds.center(), support_polygon_world: vec![],
        }));
        }
        state.put(
            working_state::key(0, &[i]),
            ValidationRecord::Load(CompressionLoad::Fresh),
        )?;
        state.put(
            compression_order_key(brick.bounds.min.y, i)?,
            ValidationRecord::Index(i),
        )?;
    }
    // Audit the complete ordered frontier before any early physical diagnostic
    // can bypass an unread/missing queue row. Loads have explicit fresh markers;
    // a lost pending record can never be reinitialized with only its own weight.
    let mut after = None;
    let mut queued = 0;
    loop {
        let page = state.page(&[1], after.as_deref(), 128)?;
        if page.is_empty() {
            break;
        }
        for (key, record) in page {
            let ValidationRecord::Index(i) = record else {
                return Err(working_state::unavailable(
                    "invalid compression order record",
                ));
            };
            if i >= input.len() || key != compression_order_key(input.body(i)?.bounds.min.y, i)? {
                return Err(working_state::unavailable(
                    "compression order identity changed",
                ));
            }
            if !matches!(
                working_state::required(state, &working_state::key(0, &[i]))?,
                ValidationRecord::Load(CompressionLoad::Fresh)
            ) {
                return Err(working_state::unavailable(
                    "compression initial marker changed",
                ));
            }
            queued += 1;
            after = Some(key);
        }
    }
    if queued != input.len() {
        return Err(working_state::unavailable(
            "compression order lost or duplicated bodies",
        ));
    }
    let origin = input.body(0)?.bounds.center();
    let mut loads = CompressionLoads::new(input, state, origin, measurements);
    let mut after = None;
    let mut processed = 0;
    loop {
        let page = loads.state.page(&[1], after.as_deref(), 128)?;
        if page.is_empty() {
            break;
        }
        for (key, record) in page {
            let ValidationRecord::Index(i) = record else {
                return Err(working_state::unavailable(
                    "invalid compression order record",
                ));
            };
            if i >= input.len() {
                return Err(working_state::unavailable("compression body is absent"));
            }
            let brick = input.body(i)?;
            if key != compression_order_key(brick.bounds.min.y, i)?
                || after.as_ref().is_some_and(|old| *old >= key)
            {
                return Err(working_state::unavailable(
                    "compression order identity changed",
                ));
            }
            after = Some(key);
            processed += 1;
            let (mass, moment) = loads.take(i)?;
            let center = moment / mass;
            let mut supports = Vec::new();
            let mut patches = Vec::new();
            if brick.bounds.min.y.abs() <= tol.position_brick_units {
                for point in ground_support_points(&brick) {
                    let p = to_meters(point - origin);
                    supports.push((DVec2::new(p.x, p.z), None));
                }
            } else {
                for candidate in input.query(brick.bounds.expanded(tol.position_brick_units))? {
                    let j = candidate?;
                    if j == i {
                        continue;
                    }
                    let lower = input.body(j)?;
                    if lower.bounds.min.y >= brick.bounds.min.y {
                        continue;
                    }
                    for foot in crate::part_profiles::lower_bearing_regions(&brick) {
                        for face in crate::part_profiles::upper_bearing_regions(&lower) {
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
                            if supports.len() + 4 > MAX_COMPRESSION_SUPPORT_POINTS {
                                return Err(working_state::unavailable(
                                    "compression per-body bearing workspace budget exceeded",
                                ));
                            }
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
    }
    if processed != input.len() || loads.pending != 0 {
        return Err(working_state::unavailable("compression order lost bodies"));
    }
    loads.state.clear()?;
    Ok(None)
}

fn compression_order_key(height: f64, index: usize) -> Result<Vec<u8>> {
    if !height.is_finite() {
        return Err(working_state::unavailable("nonfinite compression height"));
    }
    let bits = height.to_bits();
    let ascending = if bits >> 63 != 0 {
        !bits
    } else {
        bits ^ (1 << 63)
    };
    let mut key = vec![1];
    key.extend_from_slice(&(!ascending).to_be_bytes());
    key.extend_from_slice(&(index as u64).to_be_bytes());
    Ok(key)
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
    fn missing_compression_queue_and_load_markers_are_inconclusive() {
        let mut demo = Demo::new();
        demo.add("3005", 4, 0., 0., 0.);
        demo.add("3005", 4, 0., 1., 0.);
        let input = input::SliceInput::new(&demo.bricks);
        for queue in [false, true] {
            let mut state = working_state::faults::MissingRowState::default();
            if queue {
                state.missing_page = Some(vec![1]);
            } else {
                state.missing_get = Some(working_state::key(0, &[1]));
            }
            let error = compression_issue_input(
                &input,
                Default::default(),
                &Default::default(),
                &mut state,
            )
            .unwrap_err();
            assert!(
                error
                    .downcast_ref::<crate::equilibrium::Inconclusive>()
                    .is_some(),
                "{error:#}"
            );
        }
    }
    #[test]
    fn a_lost_joint_pair_index_cannot_duplicate_joint_capacity() {
        let mut demo = Demo::new();
        demo.add("3003", 4, 0., 0., 0.);
        demo.add("3003", 4, 0., 1., 0.);
        let mut state = working_state::faults::MissingRowState::default();
        state.missing_get = Some(working_state::key(1, &[1, 2]));
        let mut constraints = ValidationConstraints::new(Box::new(state));
        let joint = crate::constraints::inferred_joints(&demo.bricks).remove(0);
        constraints.push_joint(joint).unwrap();
        assert!(
            constraints
                .verify()
                .unwrap_err()
                .downcast_ref::<crate::equilibrium::Inconclusive>()
                .is_some()
        );
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
                &input::SliceInput::new(&demo.bricks),
                Default::default(),
                &measurements,
                &mut MemoryValidationState::default(),
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
        let islands = components::partition_input(
            &input::SliceInput::new(&demo.bricks),
            &ValidationConstraints::new(Box::new(MemoryValidationState::default())),
            Default::default(),
            &mut MemoryValidationState::default(),
        )
        .unwrap();
        assert_eq!(islands, 1);
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
        let islands = components::partition_input(
            &input::SliceInput::new(&demo.bricks),
            &ValidationConstraints::new(Box::new(MemoryValidationState::default())),
            Default::default(),
            &mut MemoryValidationState::default(),
        )
        .unwrap();
        assert_eq!(
            islands, 1,
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
        let input = input::SliceInput::new(&demo.bricks);
        let mut state = MemoryValidationState::default();
        for i in 0..demo.bricks.len() {
            state
                .put(
                    working_state::key(0, &[i]),
                    ValidationRecord::Load(CompressionLoad::Fresh),
                )
                .unwrap();
        }
        let mut loads = CompressionLoads::new(
            &input,
            &mut state,
            demo.bricks[0].bounds.center(),
            &measurements,
        );
        let mass = measurements.mass_kg("3005.dat").unwrap();
        for i in (0..demo.bricks.len()).rev() {
            let (weight, moment) = loads.take(i).unwrap();
            assert_eq!(loads.pending, 0);
            assert!((weight - mass * (demo.bricks.len() - i) as f64).abs() < 1e-10);
            if i > 0 {
                loads.add(i - 1, weight, moment).unwrap();
            }
            assert!(
                loads.pending <= 1,
                "Frontier size must not scale with stack height"
            );
        }
        assert_eq!(loads.pending, 0);
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
