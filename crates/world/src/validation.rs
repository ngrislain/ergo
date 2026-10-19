//! Persistence gate: no penetrating nominal body boxes, plus a conservative
//! static support/constraint-wrench certificate. No time integration runs here.
//! declared simplified model, not real ABS stress, clutch strength, or earthquakes.
use crate::{Brick, JointKind, WorldExport, constraints, spatial::Octree};
use anyhow::{Result, ensure};
use glam::{DVec2, DVec3};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};

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
    #[serde(default)]
    pub physical_calibration: String,
    #[serde(default)]
    pub tolerances: StaticTolerances,
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
    tolerances.validate()?;
    ensure!(
        !scene.bricks.is_empty(),
        "A static construction must contain at least one brick"
    );
    crate::catalog::validate(&scene.bricks)?;
    crate::groups::validate(&scene.bricks, &scene.groups)?;
    for brick in &scene.bricks {
        crate::physical::validate_pose(brick)?;
    }
    constraints::validate_joints(&scene.bricks, &scene.joints, &scene.fixed_bricks)?;
    validate_ground_anchors_with_tolerances(&scene.bricks, &scene.fixed_bricks, tolerances)?;
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
                overlap.min_element() <= tolerances.position_brick_units,
                "Bricks {} and {} overlap by {:.4} brick units; fix the model before saving",
                brick.id,
                other.id,
                overlap.min_element()
            );
        }
    }
    let pairs: HashSet<_> = scene
        .joints
        .iter()
        .map(|j| (j.a.min(j.b), j.a.max(j.b)))
        .collect();
    let inferred: Vec<_> = constraints::inferred_joints(&scene.bricks)
        .into_iter()
        .filter(|j| !pairs.contains(&(j.a.min(j.b), j.a.max(j.b))))
        .collect();
    let inferred_count = inferred.len();
    scene.joints.extend(inferred);
    let report = ValidationReport {
        collision_model: "nominal body bounding boxes; visual studs excluded".into(),
        method: "static support and constraint-wrench certificate (no simulation)".into(),
        inferred_joints: inferred_count,
        checked_bricks: scene.bricks.len(),
        max_displacement: 0.0,
        physical_calibration:
            "Metric dimensions and catalog masses; check connector calibration separately".into(),
        tolerances,
    };
    support_certificate(
        &scene.bricks,
        &scene.joints,
        &scene.fixed_bricks,
        tolerances,
    )?;
    Ok(report)
}

#[derive(Clone, Copy)]
enum EdgeKind {
    Joint(usize),
    Contact(crate::Aabb),
}
struct Edge {
    a: usize,
    b: usize,
    kind: EdgeKind,
}
fn support_certificate(
    bricks: &[Brick],
    joints: &[crate::JointSpec],
    fixed: &[u32],
    tol: StaticTolerances,
) -> Result<()> {
    use crate::physical::{GRAVITY_M_S2, mass_kg, to_meters};
    let n = bricks.len();
    let index: HashMap<_, _> = bricks.iter().enumerate().map(|(i, b)| (b.id, i)).collect();
    let mut edges = Vec::new();
    let mut pairs = HashSet::new();
    for (j, joint) in joints.iter().enumerate() {
        let a = index[&joint.a];
        let b = index[&joint.b];
        pairs.insert((a.min(b), a.max(b)));
        edges.push(Edge {
            a,
            b,
            kind: EdgeKind::Joint(j),
        });
    }
    let tree = Octree::new(bricks.iter().map(|b| b.bounds).collect());
    for (a, brick) in bricks.iter().enumerate() {
        for b in tree.query(brick.bounds.expanded(tol.position_brick_units)) {
            if b <= a || pairs.contains(&(a, b)) {
                continue;
            }
            let overlap = brick.bounds.max.min(bricks[b].bounds.max)
                - brick.bounds.min.max(bricks[b].bounds.min);
            if overlap.y.abs() <= tol.position_brick_units
                && overlap.x > tol.position_brick_units
                && overlap.z > tol.position_brick_units
            {
                let min = brick.bounds.min.max(bricks[b].bounds.min);
                let max = brick.bounds.max.min(bricks[b].bounds.max);
                edges.push(Edge {
                    a,
                    b,
                    kind: EdgeKind::Contact(crate::Aabb::new(
                        DVec3::new(min.x, (min.y + max.y) * 0.5, min.z),
                        DVec3::new(max.x, (min.y + max.y) * 0.5, max.z),
                    )),
                });
            }
        }
    }
    // Select a forest globally in edge order: authored/inferred joints first,
    // then unilateral contacts. BFS alone could take a contact shortcut through
    // an already rigid component and invent unnecessary tipping reactions.
    let mut component: Vec<_> = (0..n).collect();
    fn root(p: &mut [usize], i: usize) -> usize {
        if p[i] != i {
            p[i] = root(p, p[i]);
        }
        p[i]
    }
    let mut adjacency = vec![Vec::new(); n];
    for (i, e) in edges.iter().enumerate() {
        let a = root(&mut component, e.a);
        let b = root(&mut component, e.b);
        if a != b {
            component[a] = b;
            adjacency[e.a].push(i);
            adjacency[e.b].push(i);
        }
    }
    let mut seen = vec![false; n];
    let mut parent = vec![usize::MAX; n];
    let mut parent_edge = vec![usize::MAX; n];
    let mut forces = vec![DVec3::ZERO; n];
    let mut moments = forces.clone();
    for start in 0..n {
        if seen[start] {
            continue;
        }
        let mut group = vec![start];
        seen[start] = true;
        let mut cursor = 0;
        while cursor < group.len() {
            let i = group[cursor];
            cursor += 1;
            for &e in &adjacency[i] {
                let edge = &edges[e];
                let other = if edge.a == i { edge.b } else { edge.a };
                if !seen[other] {
                    seen[other] = true;
                    group.push(other);
                }
            }
        }
        let root = group
            .iter()
            .copied()
            .find(|&i| fixed.contains(&bricks[i].id))
            .unwrap_or(start);
        let origin = to_meters(bricks[root].bounds.center());
        let mut mass = 0.0;
        let mut center = DVec3::ZERO;
        let mut supports = Vec::new();
        for &i in &group {
            let m = mass_kg(&bricks[i].part)?;
            let p = to_meters(bricks[i].bounds.center()) - origin;
            mass += m;
            center += p * m;
            forces[i] = DVec3::NEG_Y * m * GRAVITY_M_S2;
            moments[i] = p.cross(forces[i]);
            if bricks[i].bounds.min.y.abs() <= tol.position_brick_units {
                for (x, z) in [
                    (bricks[i].bounds.min.x, bricks[i].bounds.min.z),
                    (bricks[i].bounds.max.x, bricks[i].bounds.min.z),
                    (bricks[i].bounds.max.x, bricks[i].bounds.max.z),
                    (bricks[i].bounds.min.x, bricks[i].bounds.max.z),
                ] {
                    let p = to_meters(DVec3::new(x, 0.0, z)) - origin;
                    supports.push((DVec2::new(p.x, p.z), i));
                }
            }
        }
        ensure!(
            !supports.is_empty(),
            "Construction is not static: component containing brick {} has no ground support",
            bricks[start].id
        );
        center /= mass;
        if fixed.contains(&bricks[root].id) {
            forces[root] += DVec3::Y * mass * GRAVITY_M_S2;
            let total_moment = group.iter().map(|&i| moments[i]).sum::<DVec3>();
            moments[root] -= total_moment;
        } else {
            let hull = convex_hull(supports.iter().map(|(p, _)| *p).collect());
            let p = DVec2::new(center.x, center.z);
            ensure!(
                hull.len() >= 3
                    && (0..hull.len()).all(|i| {
                        let edge = hull[(i + 1) % hull.len()] - hull[i];
                        edge.perp_dot(p - hull[i])
                            >= -tol.position_brick_units * 0.008 * edge.length()
                    }),
                "Construction is not static: center of mass lies outside the ground support polygon near brick {}",
                bricks[start].id
            );
            let mut distributed = false;
            for i in 1..hull.len() - 1 {
                let [a, b, c] = [hull[0], hull[i], hull[i + 1]];
                let area = (b - a).perp_dot(c - a);
                if area <= 1e-15 {
                    continue;
                }
                let wa = (b - p).perp_dot(c - p) / area;
                let wb = (c - p).perp_dot(a - p) / area;
                let wc = 1.0 - wa - wb;
                if [wa, wb, wc].iter().all(|w| *w >= -1e-10) {
                    for (point, w) in [(a, wa), (b, wb), (c, wc)] {
                        let node = supports.iter().find(|(q, _)| *q == point).unwrap().1;
                        let force = DVec3::Y * (mass * GRAVITY_M_S2 * w.max(0.0));
                        forces[node] += force;
                        moments[node] += DVec3::new(point.x, -origin.y, point.y).cross(force);
                    }
                    distributed = true;
                    break;
                }
            }
            ensure!(
                distributed,
                "No conservative nonnegative ground-reaction distribution found"
            );
        }
        let mut order = vec![root];
        parent[root] = root;
        let mut cursor = 0;
        while cursor < order.len() {
            let i = order[cursor];
            cursor += 1;
            for &e in &adjacency[i] {
                let edge = &edges[e];
                let other = if edge.a == i { edge.b } else { edge.a };
                if parent[other] == usize::MAX {
                    parent[other] = i;
                    parent_edge[other] = e;
                    order.push(other);
                }
            }
        }
        for &i in order.iter().rev() {
            if i == root {
                continue;
            }
            let e = &edges[parent_edge[i]];
            let anchor = match e.kind {
                EdgeKind::Joint(j) => joints[j].anchor,
                EdgeKind::Contact(patch) => patch.center(),
            };
            let anchor = to_meters(anchor) - origin;
            let force = -forces[i];
            let torque = -(moments[i] - anchor.cross(forces[i]));
            match e.kind {
                EdgeKind::Joint(j) => {
                    let joint = &joints[j];
                    ensure!(joint.break_force.is_none_or(|cap|force.length()<=cap+tol.force_newtons)&&joint.break_torque.is_none_or(|cap|torque.length()<=cap+tol.torque_newton_meters),"Gravity certificate overloads connection {} ↔ {}: {:.4} N / {:.6} N m",joint.a,joint.b,force.length(),torque.length());
                    if let JointKind::Revolute { axis, limits } = &joint.kind {
                        ensure!(
                            torque.dot(axis.normalize()).abs() <= tol.torque_newton_meters,
                            "Construction is not static: hinge {} ↔ {} has unbalanced torque about its free axis",
                            joint.a,
                            joint.b
                        );
                        ensure!(
                            limits.is_none_or(|[lo, hi]| lo <= tol.rotation_radians
                                && hi >= -tol.rotation_radians),
                            "Reference hinge pose violates its angle limits"
                        );
                    }
                }
                EdgeKind::Contact(patch) => {
                    let other = if e.a == i { e.b } else { e.a };
                    let normal = if bricks[i].bounds.center().y > bricks[other].bounds.center().y {
                        1.0
                    } else {
                        -1.0
                    };
                    ensure!(
                        force.y * normal >= -tol.force_newtons
                            && force.x.abs() <= tol.force_newtons
                            && force.z.abs() <= tol.force_newtons
                            && torque.y.abs() <= tol.torque_newton_meters,
                        "Construction is not static: unsupported/tensile box contact near brick {}",
                        bricks[i].id
                    );
                    if force.y.abs() <= tol.force_newtons {
                        ensure!(
                            torque.length() <= tol.torque_newton_meters,
                            "Unbalanced contact moment"
                        );
                    } else {
                        let cop = anchor + DVec3::new(torque.z / force.y, 0.0, -torque.x / force.y);
                        let min = to_meters(patch.min) - origin;
                        let max = to_meters(patch.max) - origin;
                        let epsilon = tol.position_brick_units * 0.008;
                        ensure!(
                            cop.x >= min.x - epsilon
                                && cop.x <= max.x + epsilon
                                && cop.z >= min.z - epsilon
                                && cop.z <= max.z + epsilon,
                            "Construction is not static: contact support moment tips brick {}",
                            bricks[i].id
                        );
                    }
                }
            }
            let p = parent[i];
            let f = forces[i];
            let m = moments[i];
            forces[p] += f;
            moments[p] += m;
        }
        ensure!(
            forces[root].length() <= tol.force_newtons
                && moments[root].length() <= tol.torque_newton_meters,
            "Static certificate has unbalanced residual wrench"
        );
    }
    Ok(())
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
        let d = crate::demo::pavilion();
        let joints = crate::demo::authored_bonds(&d.bricks);
        support_certificate(&d.bricks, &joints, &[], StaticTolerances::default()).unwrap();
        let mut s = scene(d);
        s.joints = joints;
        validate_static(&mut s).unwrap();
    }
}
