//! Rigid motions of pieces, and rotation about legitimate pivots: registered
//! revolute connections (Technic axles and pins in round holes, wheel
//! bearings, minifigure shoulders, wrists and hand grips), at any angle.
use crate::{Brick, JointKind, JointSpec, WorldExport};
use anyhow::{Context, Result, ensure};
use glam::{DMat3, DVec3};
use std::collections::{HashMap, HashSet, VecDeque};

/// Move `pieces` rigidly: rotate by the metric rotation about `pivot`
/// (construction units), then translate. A pure translation may snap the
/// result to the stud grid; a rotation never does, so it keeps its pivot.
/// Checks proper poses, the floor, overlaps and partitioning, and moves the
/// anchors and axes of joints whose first piece moves. Returns the point map.
pub fn rigid_motion(
    scene: &mut WorldExport,
    pieces: &HashSet<u32>,
    rotation_metric: DMat3,
    pivot: DVec3,
    translation: DVec3,
    snap: bool,
) -> Result<impl Fn(DVec3) -> DVec3 + use<>> {
    let rotation = crate::physical::world_transform(rotation_metric);
    let moved = |b: &Brick, translation: DVec3| -> Result<Brick> {
        let mesh = scene
            .meshes
            .get(&b.part)
            .context("Missing piece geometry")?;
        let mut destination = b.clone();
        destination.position = pivot + rotation * (b.position - pivot) + translation;
        destination.transform = rotation * b.transform;
        destination.bounds = crate::placement::body_bounds(mesh)
            .transformed(destination.transform, destination.position);
        destination.render_bounds = mesh
            .bounds
            .transformed(destination.transform, destination.position);
        Ok(destination)
    };
    let translation = if snap {
        let preview = scene
            .bricks
            .iter()
            .filter(|b| pieces.contains(&b.id))
            .map(|b| moved(b, translation))
            .collect::<Result<Vec<_>>>()?;
        (translation + crate::placement::snap_bricks_translation(&preview, DVec3::ZERO))
            .map(|v| if v.abs() < 1e-9 { 0. } else { v })
    } else {
        translation
    };
    let mut bricks = scene.bricks.clone();
    for b in &mut bricks {
        if pieces.contains(&b.id) {
            *b = moved(b, translation)?;
            crate::physical::validate_pose(b)?;
            ensure!(
                b.bounds.min.y >= -crate::validation::OVERLAP_TOLERANCE,
                "The motion crosses the Z=0 floor at piece {}",
                b.id
            );
        }
    }
    let tree = crate::spatial::Octree::new(bricks.iter().map(|b| b.bounds).collect());
    for (i, b) in bricks
        .iter()
        .enumerate()
        .filter(|(_, b)| pieces.contains(&b.id))
    {
        for j in tree.query(b.bounds) {
            if i == j {
                continue;
            }
            let other = &bricks[j];
            ensure!(
                !crate::interference::bodies_penetrate(
                    b,
                    other,
                    crate::validation::OVERLAP_TOLERANCE
                ),
                "The motion overlaps piece {} with {}",
                b.id,
                other.id
            );
        }
    }
    crate::partition(&bricks)?;
    let point = move |p: DVec3| pivot + rotation * (p - pivot) + translation;
    for joint in &mut scene.joints {
        if pieces.contains(&joint.a) && pieces.contains(&joint.b) {
            joint.anchor = point(joint.anchor);
            if let JointKind::Revolute { axis, .. } = &mut joint.kind {
                // Axes are construction-unit directions: rotate them with the
                // construction-space rotation, not the metric one.
                *axis = (rotation * *axis).normalize();
            }
        }
    }
    scene.bricks = bricks;
    Ok(point)
}

/// Every joint of the scene: authored, plus those inferred from registered
/// connectors for pairs without an authored joint.
pub fn all_joints(scene: &WorldExport) -> Vec<JointSpec> {
    let pairs: HashSet<_> = scene
        .joints
        .iter()
        .map(|j| (j.a.min(j.b), j.a.max(j.b)))
        .collect();
    let mut joints = scene.joints.clone();
    joints.extend(
        crate::constraints::inferred_joints(&scene.bricks)
            .into_iter()
            .filter(|j| !pairs.contains(&(j.a.min(j.b), j.a.max(j.b)))),
    );
    joints
}

/// A legitimate pivot of a piece: a revolute connection to a neighbour.
#[derive(Clone, Debug, serde::Serialize)]
pub struct Pivot {
    pub other: u32,
    pub other_part: String,
    pub label: &'static str,
    /// World anchor (construction units) and metric unit axis.
    pub anchor: DVec3,
    pub axis_metric: DVec3,
    pub bearing: crate::attachments::Bearing,
    /// Pieces that turn with `piece` (its side of the pivot), or None when the
    /// piece is also held another way (a closed loop) or anchored.
    pub moving: Option<Vec<u32>>,
}

/// The pivots of `piece`: its revolute connections to other pieces.
pub fn pivots(scene: &WorldExport, piece: u32) -> Result<Vec<Pivot>> {
    let index: HashMap<u32, &Brick> = scene.bricks.iter().map(|b| (b.id, b)).collect();
    let brick = index.get(&piece).context("Piece is absent")?;
    let joints = all_joints(scene);
    let mut out = Vec::new();
    for joint in &joints {
        if joint.a != piece && joint.b != piece {
            continue;
        }
        let JointKind::Revolute { axis, .. } = joint.kind else {
            continue;
        };
        let other = if joint.a == piece { joint.b } else { joint.a };
        let other_brick = index.get(&other).context("Joint endpoint is absent")?;
        let Some(connection) = crate::attachments::connection(
            brick,
            other_brick,
            crate::validation::OVERLAP_TOLERANCE,
        ) else {
            continue;
        };
        out.push(Pivot {
            other,
            other_part: other_brick.part.clone(),
            label: connection.label,
            anchor: joint.anchor,
            axis_metric: crate::physical::to_meters(axis).normalize(),
            bearing: connection.bearing,
            moving: moving_side(scene, &joints, piece, other),
        });
    }
    Ok(out)
}

/// Pieces reached from `piece` through every joint except the one to
/// `other`; None if `other` is reached too (a loop) or a piece is anchored.
fn moving_side(
    scene: &WorldExport,
    joints: &[JointSpec],
    piece: u32,
    other: u32,
) -> Option<Vec<u32>> {
    let mut adjacency: HashMap<u32, Vec<u32>> = HashMap::new();
    for j in joints {
        if (j.a, j.b) == (piece, other) || (j.a, j.b) == (other, piece) {
            continue;
        }
        adjacency.entry(j.a).or_default().push(j.b);
        adjacency.entry(j.b).or_default().push(j.a);
    }
    let mut seen = HashSet::from([piece]);
    let mut queue = VecDeque::from([piece]);
    while let Some(i) = queue.pop_front() {
        for &n in adjacency.get(&i).into_iter().flatten() {
            if n == other {
                return None;
            }
            if seen.insert(n) {
                queue.push_back(n);
            }
        }
    }
    if scene.fixed_bricks.iter().any(|f| seen.contains(f)) {
        return None;
    }
    let mut ids: Vec<u32> = seen.into_iter().collect();
    ids.sort_unstable();
    Some(ids)
}

/// Rotate `piece`, and everything joined to it on its side of the pivot, by
/// `angle` radians about the revolute connection between `piece` and
/// `other`. The right-hand rule applies about the joint's metric axis.
/// Returns the moved piece IDs.
pub fn rotate_about_joint(
    scene: &mut WorldExport,
    piece: u32,
    other: u32,
    angle: f64,
) -> Result<Vec<u32>> {
    ensure!(angle.is_finite(), "Invalid rotation angle");
    let pivot = pivots(scene, piece)?
        .into_iter()
        .find(|p| p.other == other)
        .with_context(|| {
            format!("Pieces {piece} and {other} are not joined by a registered pivot (axle, pin, wheel bearing, shoulder, wrist or hand grip)")
        })?;
    let moving = pivot.moving.with_context(|| {
        format!(
            "Piece {piece} cannot turn about its pivot with {other} alone: its side is anchored or also joined to the other side"
        )
    })?;
    let pieces: HashSet<u32> = moving.iter().copied().collect();
    // Keep the authored and inferred joints that move with the piece aligned.
    let rotation = DMat3::from_axis_angle(pivot.axis_metric, angle);
    let _ = rigid_motion(scene, &pieces, rotation, pivot.anchor, DVec3::ZERO, false)?;
    let index: HashMap<u32, &Brick> = scene.bricks.iter().map(|b| (b.id, b)).collect();
    ensure!(
        crate::attachments::connection(
            index[&piece],
            index[&other],
            crate::validation::OVERLAP_TOLERANCE
        )
        .is_some(),
        "The rotation disengaged the pivot between {piece} and {other}"
    );
    Ok(moving)
}

/// Constraint between two pieces restored by [`project_joints`].
enum Link {
    /// Registered revolute connection: ports coaxial (any angle, any slide
    /// within its engagement).
    Coaxial,
    /// Studs, keyed axles and other fixed connections: their reference
    /// relative pose.
    Rigid {
        relative_rotation: DMat3,
        relative_offset: DVec3,
    },
}

fn metric_pose(b: &Brick) -> (DMat3, DVec3) {
    (
        crate::physical::metric_transform(b.transform),
        crate::physical::to_meters(b.position),
    )
}

/// Rotate a piece by the metric rotation `r` about the metric point `q`,
/// then translate it by the metric `t`.
fn move_piece(b: &mut Brick, r: DMat3, q: DVec3, t: DVec3) {
    let (rotation, position) = metric_pose(b);
    let position = q + r * (position - q) + t;
    // Re-orthonormalise to keep proper rotations after many small steps.
    let rotation = DMat3::from_quat(glam::DQuat::from_mat3(&(r * rotation)).normalize());
    b.transform = crate::physical::world_transform(rotation);
    b.position = crate::physical::to_world(position);
}

/// Bring every connection of `bricks` back onto its registered geometry after
/// a simulation: revolute ports exactly coaxial, fixed connections at the
/// relative pose they had in `reference`. Pieces in `held` never move.
/// Corrections are split between the two pieces and iterated, so a closed
/// loop (a chain held at both ends) also closes. Returns the largest
/// remaining metric residual.
pub fn project_joints(reference: &[Brick], bricks: &mut [Brick], held: &HashSet<u32>) -> f64 {
    let index: HashMap<u32, usize> = bricks.iter().enumerate().map(|(i, b)| (b.id, i)).collect();
    let tolerance = crate::validation::OVERLAP_TOLERANCE;
    let mut links: Vec<(usize, usize, Link)> = Vec::new();
    let tree =
        crate::spatial::Octree::new(reference.iter().map(crate::attachments::bounds).collect());
    for (i, a) in reference.iter().enumerate() {
        for j in tree.query(crate::attachments::bounds(a)) {
            if j <= i {
                continue;
            }
            let b = &reference[j];
            let link = if let Some(m) = crate::attachments::mating(a, b, tolerance, 1e-4) {
                if m.bearing == crate::attachments::Bearing::Fixed {
                    None
                } else {
                    Some(Link::Coaxial)
                }
            } else if crate::attachments::stud_connection(a, b, tolerance)
                || crate::attachments::stud_connection(b, a, tolerance)
            {
                None
            } else {
                continue;
            };
            let link = link.unwrap_or_else(|| {
                let (ra, pa) = metric_pose(a);
                let (rb, pb) = metric_pose(b);
                Link::Rigid {
                    relative_rotation: ra.transpose() * rb,
                    relative_offset: ra.transpose() * (pb - pa),
                }
            });
            if let (Some(&x), Some(&y)) = (index.get(&a.id), index.get(&b.id)) {
                links.push((x, y, link));
            }
        }
    }
    let weights: Vec<f64> = bricks
        .iter()
        .map(|b| if held.contains(&b.id) { 0. } else { 1. })
        .collect();
    let mut residual = 0.;
    for _ in 0..400 {
        residual = 0.0_f64;
        for (a, b, link) in &links {
            let (wa, wb) = (weights[*a], weights[*b]);
            if wa + wb == 0. {
                continue;
            }
            let (sa, sb) = (wa / (wa + wb), wb / (wa + wb));
            match link {
                Link::Coaxial => {
                    let Some(m) = crate::attachments::mating(&bricks[*a], &bricks[*b], 0.05, 0.05)
                    else {
                        continue;
                    };
                    // Align axes: rotate each piece about its port centre.
                    let axis = m.a_axis.cross(m.b_axis);
                    let angle = axis.length().asin();
                    if angle > 1e-12 {
                        let axis = axis.normalize();
                        move_piece(
                            &mut bricks[*a],
                            DMat3::from_axis_angle(axis, angle * sa),
                            m.a_centre,
                            DVec3::ZERO,
                        );
                        move_piece(
                            &mut bricks[*b],
                            DMat3::from_axis_angle(axis, -angle * sb),
                            m.b_centre,
                            DVec3::ZERO,
                        );
                    }
                    let Some(m) = crate::attachments::mating(&bricks[*a], &bricks[*b], 0.05, 0.05)
                    else {
                        continue;
                    };
                    let delta = m.b_centre - m.a_centre;
                    let radial = delta - m.a_axis * delta.dot(m.a_axis);
                    move_piece(&mut bricks[*a], DMat3::IDENTITY, DVec3::ZERO, radial * sa);
                    move_piece(&mut bricks[*b], DMat3::IDENTITY, DVec3::ZERO, -radial * sb);
                    residual = residual.max(radial.length()).max(angle * 0.01);
                }
                Link::Rigid {
                    relative_rotation,
                    relative_offset,
                } => {
                    let (ra, pa) = metric_pose(&bricks[*a]);
                    let (rb, pb) = metric_pose(&bricks[*b]);
                    let target_rotation = ra * *relative_rotation;
                    let error =
                        glam::DQuat::from_mat3(&(target_rotation * rb.transpose())).normalize();
                    let (axis, angle) = error.to_axis_angle();
                    let angle = if angle > std::f64::consts::PI {
                        angle - std::f64::consts::TAU
                    } else {
                        angle
                    };
                    if angle.abs() > 1e-12 && axis.is_finite() {
                        move_piece(
                            &mut bricks[*b],
                            DMat3::from_axis_angle(axis, angle * sb),
                            pb,
                            DVec3::ZERO,
                        );
                        move_piece(
                            &mut bricks[*a],
                            DMat3::from_axis_angle(axis, -angle * sa),
                            pa,
                            DVec3::ZERO,
                        );
                    }
                    let (ra, pa) = metric_pose(&bricks[*a]);
                    let (_, pb) = metric_pose(&bricks[*b]);
                    let delta = pa + ra * *relative_offset - pb;
                    move_piece(&mut bricks[*b], DMat3::IDENTITY, DVec3::ZERO, delta * sb);
                    move_piece(&mut bricks[*a], DMat3::IDENTITY, DVec3::ZERO, -delta * sa);
                    residual = residual.max(delta.length()).max(angle.abs() * 0.01);
                }
            }
        }
        if residual < 1e-9 {
            break;
        }
    }
    residual
}

/// Velocity damping (1/s) during [`settle`]: a 2.4 cm pendulum (about 20
/// rad/s) loses e^-40 of its energy in 8 s.
#[cfg(feature = "simulation")]
const SETTLING_DAMPING: f64 = 5.0;
#[cfg(feature = "simulation")]
const QUIET_STEPS: usize = 60;

/// Result of [`settle`].
#[derive(Clone, Debug, serde::Serialize)]
pub struct Settled {
    pub seconds: f64,
    pub steps: usize,
    pub settled: bool,
    pub kinetic_energy_j: f64,
    /// Largest connector residual after projection (m).
    pub projection_residual_m: f64,
}

/// Let gravity act for up to `seconds` (stopping early once the kinetic
/// energy stays negligible), keep the resulting pose, and project every
/// connection back onto its exact registered geometry. Joints are then
/// re-inferred from that geometry rather than copied from the simulation.
#[cfg(feature = "simulation")]
pub fn settle(scene: &mut WorldExport, seconds: f64) -> Result<Settled> {
    use crate::physics::{Simulation, TIMESTEP};
    ensure!((0.0..=8.0).contains(&seconds), "Duration must be 0–8 s");
    let joints = all_joints(scene);
    let mut sim = Simulation::new(
        &scene.bricks,
        &scene.meshes,
        &joints,
        &scene.fixed_bricks,
        false,
    )?;
    sim.settling_damping(SETTLING_DAMPING);
    let limit = (seconds / TIMESTEP).ceil() as usize;
    // Settled once every body moves slower than about 14 µm/s for 0.5 s.
    let quiet_energy = 1e-10 * sim.dynamic_mass();
    let (mut steps, mut quiet) = (0, 0);
    let mut energy = 0.;
    while steps < limit {
        sim.step();
        steps += 1;
        energy = sim.kinetic_energy();
        quiet = if energy < quiet_energy { quiet + 1 } else { 0 };
        if quiet >= QUIET_STEPS {
            break;
        }
    }
    let reference = scene.bricks.clone();
    let mut bricks = sim.bricks();
    let mut held: HashSet<u32> = scene.fixed_bricks.iter().copied().collect();
    // A square piece resting on the floor that moved only by contact-solver
    // jitter keeps its exact pose; a piece that really slid or tipped keeps
    // its simulated pose and validation judges it.
    let initial: HashMap<u32, &Brick> = reference.iter().map(|b| (b.id, b)).collect();
    for b in &mut bricks {
        let Some(&r) = initial.get(&b.id) else {
            continue;
        };
        if !crate::interference::axis_aligned(r)
            || r.bounds.min.y.abs() > crate::validation::OVERLAP_TOLERANCE
        {
            continue;
        }
        let (rotation, position) = metric_pose(b);
        let (rest, origin) = metric_pose(r);
        let angle = glam::DQuat::from_mat3(&(rotation * rest.transpose()))
            .normalize()
            .angle_between(glam::DQuat::IDENTITY);
        if (position - origin).length() <= 5e-4 && angle <= 0.5_f64.to_radians() {
            *b = r.clone();
            held.insert(b.id);
        }
    }
    let residual = project_joints(&reference, &mut bricks, &held);
    for b in &mut bricks {
        let mesh = scene
            .meshes
            .get(&b.part)
            .context("Missing piece geometry")?;
        b.bounds = crate::placement::body_bounds(mesh).transformed(b.transform, b.position);
        b.render_bounds = mesh.bounds.transformed(b.transform, b.position);
    }
    scene.bricks = bricks;
    // Authored joints keep their identity; inferred ones are re-derived.
    let authored: HashSet<(u32, u32)> = scene
        .joints
        .iter()
        .map(|j| (j.a.min(j.b), j.a.max(j.b)))
        .collect();
    let fresh = crate::constraints::inferred_joints(&scene.bricks);
    scene
        .joints
        .retain(|j| authored.contains(&(j.a.min(j.b), j.a.max(j.b))));
    for joint in &mut scene.joints {
        if let Some(f) = fresh
            .iter()
            .find(|f| (f.a.min(f.b), f.a.max(f.b)) == (joint.a.min(joint.b), joint.a.max(joint.b)))
        {
            joint.anchor = f.anchor;
            joint.kind = f.kind.clone();
        }
    }
    Ok(Settled {
        seconds: steps as f64 * TIMESTEP,
        steps,
        settled: quiet >= QUIET_STEPS,
        kinetic_energy_j: energy,
        projection_residual_m: residual,
    })
}
