//! A bounded rigid-body sandbox. Float64 world coordinates; nominal brick-body
//! convex hulls, not decorative studs. Explicit compound colliders are a future
//! asset-format extension for hollow/Technic parts.
use crate::physical::{to_meters, to_world, world_transform};
use crate::{
    Aabb, BlockCoord, Brick, JointKind, JointSpec,
    geometry::Mesh,
    spatial::{Octree, relation},
};
use anyhow::{Context, Result, ensure};
use glam::{DMat3, DVec3};
use rapier3d_f64::{na, parry::query, prelude::*};
use serde::Serialize;
use std::collections::{BTreeMap, HashMap, HashSet};

pub use crate::MAX_SIMULATION_BRICKS;
pub const TIMESTEP: f64 = 1.0 / 120.0;

fn vector(p: DVec3) -> Vector<f64> {
    Vector::new(p.x, p.y, p.z)
}
fn point(p: DVec3) -> Point<f64> {
    Point::new(p.x, p.y, p.z)
}
fn dvec(v: &Vector<f64>) -> DVec3 {
    DVec3::new(v.x, v.y, v.z)
}

pub struct Simulation {
    pipeline: PhysicsPipeline,
    islands: IslandManager,
    broad: BroadPhaseMultiSap,
    narrow: NarrowPhase,
    bodies: RigidBodySet,
    colliders: ColliderSet,
    joints: ImpulseJointSet,
    multibody: MultibodyJointSet,
    ccd: CCDSolver,
    parameters: IntegrationParameters,
    handles: HashMap<u32, RigidBodyHandle>,
    originals: Vec<Brick>,
    pub joint_specs: Vec<JointSpec>,
    pub fixed_bricks: Vec<u32>,
    drive: Option<(u32, DVec3)>,
    pub elapsed: f64,
    pub gravity: bool,
    pub inferred_count: usize,
    breakable: HashMap<ImpulseJointHandle, (Option<f64>, Option<f64>)>,
    joint_handles: Vec<ImpulseJointHandle>,
    ground_bodies: Vec<(RigidBodyHandle, DVec3)>,
    earthquake: Option<crate::solidity::Earthquake>,
}

#[derive(Clone, Default, Debug, Serialize)]
pub struct SimulationStats {
    pub bodies: usize,
    pub awake: usize,
    pub moved: usize,
    pub joints: usize,
    pub inferred_joints: usize,
    pub penetrating_pairs: usize,
    pub elapsed: f64,
    pub max_displacement: f64,
}

pub fn inferred_joints(bricks: &[Brick]) -> Vec<JointSpec> {
    let tree = Octree::new(bricks.iter().map(|b| b.bounds).collect());
    let mut out = Vec::new();
    for (i, brick) in bricks.iter().enumerate() {
        for j in tree.query(brick.bounds.expanded(0.003)) {
            if j <= i {
                continue;
            }
            let other = &bricks[j];
            if relation(brick, other).is_some_and(|r| r.certainty == "inferred") {
                out.push(JointSpec {
                    a: brick.id,
                    b: other.id,
                    anchor: (brick.bounds.center() + other.bounds.center()) * 0.5,
                    kind: JointKind::Fixed,
                    break_force: None,
                    break_torque: None,
                    strength_source: None,
                });
            }
        }
    }
    out
}

pub fn validate_joints(bricks: &[Brick], joints: &[JointSpec], fixed: &[u32]) -> Result<()> {
    let ids: HashSet<_> = bricks.iter().map(|b| b.id).collect();
    ensure!(ids.len() == bricks.len(), "Duplicate brick IDs");
    ensure!(
        joints.len() <= bricks.len().saturating_mul(12),
        "Too many constraints"
    );
    for joint in joints {
        ensure!(
            joint.a != joint.b && ids.contains(&joint.a) && ids.contains(&joint.b),
            "Joint references an absent or identical body"
        );
        ensure!(joint.anchor.is_finite(), "Invalid joint anchor");
        ensure!(
            joint.break_force.is_none_or(|f| f.is_finite() && f > 0.0),
            "Break force must be finite and positive"
        );
        ensure!(
            joint.break_torque.is_none_or(|f| f.is_finite() && f > 0.0),
            "Break torque must be finite and positive"
        );
        if let JointKind::Revolute { axis, limits } = &joint.kind {
            ensure!(
                axis.is_finite() && axis.length() > 1e-8,
                "Hinge axis must be nonzero"
            );
            if let Some([lo, hi]) = limits {
                ensure!(
                    lo.is_finite() && hi.is_finite() && lo <= hi,
                    "Invalid hinge limits"
                );
            }
        }
    }
    ensure!(
        fixed.iter().all(|id| ids.contains(id)),
        "Fixed body is absent"
    );
    Ok(())
}

impl Simulation {
    pub fn new(
        bricks: &[Brick],
        meshes: &BTreeMap<String, Mesh>,
        explicit: &[JointSpec],
        fixed: &[u32],
        infer: bool,
    ) -> Result<Self> {
        ensure!(
            !bricks.is_empty() && bricks.len() <= MAX_SIMULATION_BRICKS,
            "Simulation supports 1–{MAX_SIMULATION_BRICKS} fully loaded bricks"
        );
        validate_joints(bricks, explicit, fixed)?;
        let mut s = Self {
            pipeline: PhysicsPipeline::new(),
            islands: IslandManager::new(),
            broad: BroadPhaseMultiSap::new(),
            narrow: NarrowPhase::new(),
            bodies: RigidBodySet::new(),
            colliders: ColliderSet::new(),
            joints: ImpulseJointSet::new(),
            multibody: MultibodyJointSet::new(),
            ccd: CCDSolver::new(),
            parameters: IntegrationParameters {
                dt: TIMESTEP,
                length_unit: 0.008,
                normalized_allowed_linear_error: 1e-5,
                contact_natural_frequency: 300.0,
                contact_damping_ratio: 1.0,
                num_solver_iterations: 16.try_into().unwrap(),
                num_internal_pgs_iterations: 2,
                max_ccd_substeps: 4,
                ..Default::default()
            },
            handles: HashMap::new(),
            originals: bricks.to_vec(),
            joint_specs: explicit.to_vec(),
            fixed_bricks: fixed.to_vec(),
            drive: None,
            elapsed: 0.0,
            gravity: true,
            inferred_count: 0,
            breakable: HashMap::new(),
            joint_handles: Vec::new(),
            ground_bodies: Vec::new(),
            earthquake: None,
        };
        let mut floors = HashSet::new();
        for brick in bricks {
            let _mesh = meshes
                .get(&brick.part)
                .context("Missing collision geometry")?;
            let builder = if fixed.contains(&brick.id) {
                RigidBodyBuilder::fixed()
            } else {
                RigidBodyBuilder::dynamic()
            };
            let handle = s.bodies.insert(
                builder
                    .translation(vector(to_meters(brick.position)))
                    .ccd_enabled(true)
                    .linear_damping(0.15)
                    .angular_damping(0.25),
            );
            // Use the analytic cuboid shape, not GJK on an eight-point hull.
            // Exact face-touching boxes then have a valid contact normal at
            // initialization, which is essential for one-step gravity probes.
            let half = to_meters(brick.bounds.size() * 0.5);
            let offset = to_meters(brick.bounds.center() - brick.position);
            let collider = ColliderBuilder::cuboid(half.x, half.y, half.z)
                .translation(vector(offset))
                .mass(
                    _mesh
                        .physical
                        .as_ref()
                        .map(|p| Ok(p.mass_kg))
                        .unwrap_or_else(|| crate::physical::mass_kg(&brick.part))?,
                )
                .friction(0.65)
                .restitution(0.0);
            s.colliders
                .insert_with_parent(collider, handle, &mut s.bodies);
            s.handles.insert(brick.id, handle);
            // Horizontal cells have finite 1024×1024 plates at public Z=0.
            let cell = BlockCoord::at(brick.bounds.min + DVec3::splat(1e-6));
            for dx in -1..=1 {
                for dz in -1..=1 {
                    floors.insert(BlockCoord(cell.0 + dx, 0, cell.2 + dz));
                }
            }
        }
        for cell in floors {
            let origin = cell.origin();
            let half = to_meters(DVec3::new(512.0, 0.25, 512.0));
            let position = to_meters(origin + DVec3::new(512.0, -0.25, 512.0));
            let handle = s
                .bodies
                .insert(RigidBodyBuilder::kinematic_position_based().translation(vector(position)));
            s.colliders.insert_with_parent(
                ColliderBuilder::cuboid(half.x, half.y, half.z).friction(0.75),
                handle,
                &mut s.bodies,
            );
            s.ground_bodies.push((handle, position));
        }
        if infer {
            let existing: HashSet<_> = explicit
                .iter()
                .map(|j| (j.a.min(j.b), j.a.max(j.b)))
                .collect();
            let inferred: Vec<_> = inferred_joints(bricks)
                .into_iter()
                .filter(|j| !existing.contains(&(j.a.min(j.b), j.a.max(j.b))))
                .collect();
            s.inferred_count = inferred.len();
            s.joint_specs.extend(inferred);
        }
        let specs = s.joint_specs.clone();
        for spec in &specs {
            s.insert_joint(spec)?;
        }
        // Initialize parent transforms, broad phase, contact manifolds, and
        // islands without advancing time. A one-step equilibrium probe must not
        // confuse an uninitialized contact graph with an unsupported brick.
        let initialization = IntegrationParameters {
            dt: 0.0,
            ..s.parameters
        };
        s.pipeline.step(
            &Vector::zeros(),
            &initialization,
            &mut s.islands,
            &mut s.broad,
            &mut s.narrow,
            &mut s.bodies,
            &mut s.colliders,
            &mut s.joints,
            &mut s.multibody,
            &mut s.ccd,
            None,
            &(),
            &(),
        );
        Ok(s)
    }

    fn insert_joint(&mut self, spec: &JointSpec) -> Result<()> {
        let a = self.handles[&spec.a];
        let b = self.handles[&spec.b];
        let anchor_a = self.bodies[a]
            .position()
            .inverse_transform_point(&point(to_meters(spec.anchor)));
        let anchor_b = self.bodies[b]
            .position()
            .inverse_transform_point(&point(to_meters(spec.anchor)));
        let handle = match spec.kind {
            JointKind::Fixed => {
                let j = FixedJointBuilder::new()
                    .local_anchor1(anchor_a)
                    .local_anchor2(anchor_b)
                    .contacts_enabled(false);
                self.joints.insert(a, b, j, true)
            }
            JointKind::Revolute { axis, limits } => {
                let mut j = RevoluteJointBuilder::new(na::Unit::new_normalize(vector(axis)))
                    .local_anchor1(anchor_a)
                    .local_anchor2(anchor_b)
                    .contacts_enabled(false);
                if let Some(limits) = limits {
                    j = j.limits(limits);
                }
                self.joints.insert(a, b, j, true)
            }
        };
        if spec.break_force.is_some() || spec.break_torque.is_some() {
            self.breakable
                .insert(handle, (spec.break_force, spec.break_torque));
        }
        self.joint_handles.push(handle);
        Ok(())
    }

    /// A force-limited spring drives a *dynamic* body. Contact constraints can stop
    /// it and joints pull its neighbors. No teleporting kinematic body through walls.
    pub fn drive(&mut self, id: u32, target: DVec3) -> Result<()> {
        ensure!(
            target.is_finite() && self.handles.contains_key(&id),
            "Invalid manipulation target"
        );
        ensure!(!self.fixed_bricks.contains(&id), "This brick is anchored");
        self.drive = Some((id, target));
        for (_, body) in self.bodies.iter_mut() {
            body.wake_up(true);
        }
        Ok(())
    }

    pub fn release_drive(&mut self) {
        self.drive = None;
    }

    pub fn earthquake(&mut self, case: crate::solidity::Earthquake) -> Result<()> {
        case.validate()?;
        for &id in &self.fixed_bricks {
            let original = self.originals.iter().find(|b| b.id == id).unwrap();
            let handle = self.handles[&id];
            self.bodies[handle].set_body_type(RigidBodyType::KinematicPositionBased, true);
            self.ground_bodies
                .push((handle, to_meters(original.position)));
        }
        self.earthquake = Some(case);
        Ok(())
    }

    pub fn falling_object(&mut self, case: &crate::solidity::FallingObject) -> Result<()> {
        case.validate()?;
        let center = to_meters(case.target_world) + DVec3::Y * case.release_height_m;
        let half = case.dimensions_m * 0.5;
        let bounds = Aabb::new(to_world(center - half), to_world(center + half));
        for brick in &self.originals {
            let overlap = bounds.max.min(brick.bounds.max) - bounds.min.max(brick.bounds.min);
            ensure!(
                overlap.min_element() <= crate::validation::OVERLAP_TOLERANCE,
                "Dropped object starts inside brick {}; increase release height or change its target/dimensions",
                brick.id
            );
        }
        let body = self.bodies.insert(
            RigidBodyBuilder::dynamic()
                .translation(vector(center))
                .ccd_enabled(true),
        );
        self.colliders.insert_with_parent(
            ColliderBuilder::cuboid(half.x, half.y, half.z)
                .mass(case.mass_kg)
                .friction(0.65)
                .restitution(0.0),
            body,
            &mut self.bodies,
        );
        Ok(())
    }

    pub fn max_speed(&self) -> f64 {
        self.bodies
            .iter()
            .map(|(_, b)| (b.linvel().norm() / 0.008).max(b.angvel().norm()))
            .fold(0.0, f64::max)
    }

    pub fn max_displacement_m(&self) -> f64 {
        self.originals
            .iter()
            .map(|original| {
                (dvec(self.bodies[self.handles[&original.id]].translation())
                    - to_meters(original.position))
                .length()
            })
            .fold(0.0, f64::max)
    }

    pub fn max_linear_speed_m_s(&self) -> f64 {
        self.bodies
            .iter()
            .map(|(_, b)| b.linvel().norm())
            .fold(0.0, f64::max)
    }

    pub fn max_angular_speed_rad_s(&self) -> f64 {
        self.bodies
            .iter()
            .map(|(_, b)| b.angvel().norm())
            .fold(0.0, f64::max)
    }

    pub fn broken_count(&self) -> usize {
        self.joint_specs.len() - self.joints.len()
    }

    pub fn current_joints(&self) -> Vec<JointSpec> {
        self.joint_specs
            .iter()
            .zip(&self.joint_handles)
            .filter_map(|(spec, handle)| {
                self.joints.get(*handle)?;
                let body = &self.bodies[self.handles[&spec.a]];
                let original = self.originals.iter().find(|b| b.id == spec.a).unwrap();
                let mut joint = spec.clone();
                joint.anchor = to_world(dvec(
                    &(body.rotation() * vector(to_meters(spec.anchor - original.position))
                        + body.translation()),
                ));
                if let JointKind::Revolute { axis, .. } = &mut joint.kind {
                    *axis = dvec(&(body.rotation() * vector(*axis))).normalize();
                }
                Some(joint)
            })
            .collect()
    }

    pub fn step(&mut self) {
        if let Some(case) = &self.earthquake {
            let displacement = case.displacement(self.elapsed + TIMESTEP);
            for &(handle, position) in &self.ground_bodies {
                let p = position + displacement;
                self.bodies[handle]
                    .set_next_kinematic_position(Isometry::translation(p.x, p.y, p.z));
            }
        }
        if let Some((id, target)) = self.drive {
            let body = &mut self.bodies[self.handles[&id]];
            let error = vector(to_meters(target)) - body.translation();
            let compensation = Vector::new(
                0.0,
                if self.gravity {
                    crate::physical::GRAVITY_M_S2
                } else {
                    0.0
                },
                0.0,
            );
            let acceleration =
                (error * 2500.0 - body.linvel() * 100.0 + compensation).cap_magnitude(50.0);
            body.apply_impulse(acceleration * body.mass() * TIMESTEP, true);
        }
        let gravity = Vector::new(
            0.0,
            if self.gravity {
                -crate::physical::GRAVITY_M_S2
            } else {
                0.0
            },
            0.0,
        );
        self.pipeline.step(
            &gravity,
            &self.parameters,
            &mut self.islands,
            &mut self.broad,
            &mut self.narrow,
            &mut self.bodies,
            &mut self.colliders,
            &mut self.joints,
            &mut self.multibody,
            &mut self.ccd,
            None,
            &(),
            &(),
        );
        let broken: Vec<_> = self
            .breakable
            .iter()
            .filter_map(|(&handle, &(force, torque))| {
                self.joints
                    .get(handle)
                    .filter(|joint| {
                        force.is_some_and(|limit| {
                            joint.impulses.fixed_rows::<3>(0).norm() / TIMESTEP > limit
                        }) || torque.is_some_and(|limit| {
                            joint.impulses.fixed_rows::<3>(3).norm() / TIMESTEP > limit
                        })
                    })
                    .map(|_| handle)
            })
            .collect();
        for handle in broken {
            self.joints.remove(handle, true);
            self.breakable.remove(&handle);
        }
        self.elapsed += TIMESTEP;
    }

    pub fn bricks(&self) -> Vec<Brick> {
        self.originals
            .iter()
            .map(|original| {
                let body = &self.bodies[self.handles[&original.id]];
                let rotation = body.rotation().to_rotation_matrix();
                let m = rotation.matrix();
                let delta =
                    world_transform(DMat3::from_cols_array(m.as_slice().try_into().unwrap()));
                let position = to_world(dvec(body.translation()));
                let mut brick = original.clone();
                brick.transform = delta * original.transform;
                brick.position = position;
                let offset = position - delta * original.position;
                brick.bounds = original.bounds.transformed(delta, offset);
                brick.render_bounds = original.render_bounds.transformed(delta, offset);
                brick
            })
            .collect()
    }

    pub fn stats(&self) -> SimulationStats {
        let mut stats = SimulationStats {
            bodies: self.originals.len(),
            joints: self.joint_specs.len(),
            inferred_joints: self.inferred_count,
            elapsed: self.elapsed,
            ..Default::default()
        };
        for original in &self.originals {
            let body = &self.bodies[self.handles[&original.id]];
            let distance = (to_world(dvec(body.translation())) - original.position).length();
            if !body.is_sleeping() && body.is_dynamic() {
                stats.awake += 1;
            }
            if distance > 0.1 {
                stats.moved += 1;
            }
            stats.max_displacement = stats.max_displacement.max(distance);
        }
        stats.penetrating_pairs = self
            .narrow
            .contact_pairs()
            .filter(|pair| {
                pair.manifolds
                    .iter()
                    .any(|m| m.points.iter().any(|p| p.dist < -0.00016))
            })
            .count();
        stats
    }
}

/// Preflight rigid translations with the same narrow-phase shapes used for physics.
/// Checks many small increments to avoid skipping thin obstacles; large moves are
/// rejected rather than undersampled. Revolute manipulation belongs to Simulation.
pub fn can_translate(bricks: &[Brick], ids: &HashSet<u32>, delta: DVec3) -> Result<()> {
    ensure!(
        delta.is_finite() && delta.length() <= 32.0,
        "Edit moves are limited to 32 brick units"
    );
    let steps = (delta.length() / 0.05).ceil().max(1.0) as usize;
    let tree = Octree::new(bricks.iter().map(|b| b.bounds).collect());
    for brick in bricks.iter().filter(|b| ids.contains(&b.id)) {
        let half = brick.bounds.size() * 0.5;
        let shape = Cuboid::new(vector(half));
        for step in 1..=steps {
            let p = brick.bounds.center() + delta * (step as f64 / steps as f64);
            let box_ = Aabb::new(p - half, p + half);
            ensure!(box_.min.y >= -0.002, "Move intersects the block baseplate");
            for other in tree.query(box_) {
                let other = &bricks[other];
                if ids.contains(&other.id) {
                    continue;
                }
                let other_shape = Cuboid::new(vector(other.bounds.size() * 0.5));
                let hit = query::contact(
                    &Isometry::translation(p.x, p.y, p.z),
                    &shape,
                    &Isometry::translation(
                        other.bounds.center().x,
                        other.bounds.center().y,
                        other.bounds.center().z,
                    ),
                    &other_shape,
                    0.0,
                )?;
                ensure!(
                    !hit.is_some_and(|c| c.dist < -0.002),
                    "Move blocked by brick {} (conservative body bounds)",
                    other.id
                );
            }
        }
    }
    Ok(())
}

pub fn connected_group(id: u32, joints: &[JointSpec]) -> HashSet<u32> {
    let mut group = HashSet::from([id]);
    let mut changed = true;
    while changed {
        changed = false;
        for joint in joints {
            if matches!(joint.kind, JointKind::Fixed)
                && (group.contains(&joint.a) || group.contains(&joint.b))
            {
                changed |= group.insert(joint.a);
                changed |= group.insert(joint.b);
            }
        }
    }
    group
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::demo::Demo;
    #[test]
    fn resting_body_contacts_are_initialized_before_short_probe() {
        let mut d = Demo::new();
        d.add("3003", 4, 4.0, 0.0, 4.0);
        let mut sim = Simulation::new(&d.bricks, &d.meshes, &[], &[], false).unwrap();
        assert!(
            sim.narrow.contact_pairs().any(|p| p.has_any_active_contact
                && p.manifolds.iter().any(|m| m.data.normal.norm() > 0.99))
        );
        sim.step();
        assert!(sim.stats().max_displacement < 0.001, "{:?}", sim.stats());
    }

    #[test]
    fn gravity_settles_a_flying_brick_onto_baseplate() {
        let mut d = Demo::new();
        d.add("3003", 4, 8.0, 8.0, 8.0);
        let mut sim = Simulation::new(&d.bricks, &d.meshes, &[], &[], false).unwrap();
        for _ in 0..600 {
            sim.step();
        }
        let brick = &sim.bricks()[0];
        assert!(
            brick.bounds.min.y.abs() < 0.04,
            "floor: {}",
            brick.bounds.min.y
        );
        assert!(sim.stats().max_displacement > 7.0);
    }

    #[test]
    fn supported_catalog_stacks_remain_static_in_si_units() {
        let d = crate::demo::physics_lab();
        let joints = inferred_joints(&d.bricks);
        let mut sim = Simulation::new(&d.bricks, &d.meshes, &joints, &[], false).unwrap();
        for _ in 0..480 {
            sim.step();
        }
        assert!(sim.stats().max_displacement < 0.025, "{:?}", sim.stats());
    }

    #[test]
    fn weak_test_connection_breaks_under_its_supported_bricks_weight() {
        let mut d = Demo::new();
        let a = d.add("3003", 4, 2.0, 0.0, 2.0);
        d.add("3003", 1, 2.0, 1.0, 2.0);
        let mut joints = inferred_joints(&d.bricks);
        // Deliberately synthetic failure thresholds for a units/solver regression,
        // not a shipped or claimed real-world connector calibration.
        joints[0].break_force = Some(1e-8);
        joints[0].break_torque = Some(1e-8);
        joints[0].strength_source = Some("synthetic unit-test fixture".into());
        let mut sim = Simulation::new(&d.bricks, &d.meshes, &joints, &[a], false).unwrap();
        for _ in 0..12 {
            sim.step();
        }
        assert_eq!(sim.broken_count(), 1);
        assert!(sim.current_joints().is_empty());
    }

    #[test]
    fn fixed_joint_moves_neighbors_and_collision_blocks_edit() {
        let mut d = Demo::new();
        let a = d.add("3003", 4, 2.0, 0.0, 2.0);
        let b = d.add("3003", 1, 2.0, 1.0, 2.0);
        d.add("3003", 14, 5.0, 0.0, 2.0);
        let joints = inferred_joints(&d.bricks);
        let group = connected_group(a, &joints);
        assert!(group.contains(&b));
        assert!(can_translate(&d.bricks, &group, DVec3::X * 4.0).is_err());
        let mut sim = Simulation::new(&d.bricks, &d.meshes, &joints, &[], false).unwrap();
        sim.gravity = false;
        sim.drive(a, d.bricks[0].position + DVec3::Z * 2.0).unwrap();
        for _ in 0..360 {
            sim.step();
        }
        let moved = sim.bricks();
        assert!(moved[1].position.z > d.bricks[1].position.z + 1.0);
        assert!(((moved[0].position - moved[1].position).length() - 1.0).abs() < 0.03);
    }

    #[test]
    fn revolute_joint_preserves_anchor_but_allows_rotation() {
        let mut d = Demo::new();
        let a = d.add("3003", 4, 5.0, 4.0, 5.0);
        let b = d.add("3001", 1, 5.0, 4.0, 8.0);
        let anchor = DVec3::new(6.0, 4.5, 7.5);
        let joint = JointSpec {
            a,
            b,
            anchor,
            kind: JointKind::Revolute {
                axis: DVec3::X,
                limits: None,
            },
            break_force: None,
            break_torque: None,
            strength_source: None,
        };
        let mut sim = Simulation::new(&d.bricks, &d.meshes, &[joint], &[a], false).unwrap();
        for _ in 0..240 {
            sim.step();
        }
        let moved = sim.bricks();
        let local_anchor = anchor - d.bricks[1].position;
        let world_anchor = moved[1].position + moved[1].transform * local_anchor;
        assert!((world_anchor - anchor).length() < 0.04);
        assert!(
            (moved[1].transform - DMat3::IDENTITY).x_axis.length()
                + (moved[1].transform - DMat3::IDENTITY).y_axis.length()
                > 0.1
        );
    }
}
