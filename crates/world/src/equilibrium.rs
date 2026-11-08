//! General static equilibrium certificate.
//!
//! One sparse convex problem per supplied island: static validation first splits
//! independent contact/joint components and calls this solver only for islands
//! without a constructive compression proof. Every body balances gravity through
//! joint wrenches, nonnegative frictionless contact-patch forces, nonnegative
//! ground reactions and the reactions of anchored bricks. Unlike a spanning
//! tree, load loops (a chain held at both ends, a pinned truss) share their
//! loads. Constraints per joint:
//! - force magnitude ≤ its capacity (a second-order cone), and for revolute
//!   joints the force along the axis ≤ the axial capacity;
//! - bending moment ≤ its capacity, and for revolute joints the moment about
//!   the axis ≤ the friction torque (zero for free bearings);
//! - a joint with no capacity (no measurement, no reviewed estimate) carries
//!   nothing.
//!
//! The objective minimises capacity-weighted squared joint loads, so contacts
//! and the least-loaded paths carry the weight. The solver is Clarabel (pure
//! Rust, WASM-safe); its solution is re-checked independently before it is
//! accepted.
use crate::{Brick, JointKind, JointSpec};
use anyhow::{Result, ensure};
use clarabel::{algebra::CscMatrix, solver::*};
use glam::DVec3;
use std::collections::HashMap;

/// A verified equilibrium.
#[derive(Debug, Clone, Default)]
pub struct Certificate {
    /// Joints that carry more than the force/torque tolerance.
    pub loaded_joints: usize,
    /// Largest load / capacity ratio over every capped joint quantity.
    pub max_utilisation: f64,
}

/// A frictionless contact between two axis-aligned bodies: a horizontal patch
/// in construction units, with `upper` resting on `lower`.
pub struct Contact {
    pub lower: usize,
    pub upper: usize,
    pub patch: crate::Aabb,
}

/// Borrowed bodies of a full scene or one island. Indices preserve original
/// brick identities without cloning the scene's geometry/part strings.
#[derive(Clone, Copy)]
pub struct Bodies<'a> {
    source: &'a [Brick],
    indices: Option<&'a [usize]>,
}
impl<'a> Bodies<'a> {
    pub fn all(source: &'a [Brick]) -> Self {
        Self {
            source,
            indices: None,
        }
    }

    /// Every index must identify a body in `source`.
    pub fn select(source: &'a [Brick], indices: &'a [usize]) -> Self {
        Self {
            source,
            indices: Some(indices),
        }
    }

    pub fn len(&self) -> usize {
        self.indices.map_or(self.source.len(), <[usize]>::len)
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn iter(self) -> impl ExactSizeIterator<Item = &'a Brick> {
        (0..self.len()).map(move |i| &self.source[self.indices.map_or(i, |indices| indices[i])])
    }
}
impl std::ops::Index<usize> for Bodies<'_> {
    type Output = Brick;
    fn index(&self, i: usize) -> &Brick {
        &self.source[self.indices.map_or(i, |indices| indices[i])]
    }
}

pub struct Problem<'a> {
    pub bricks: Bodies<'a>,
    pub joints: &'a [JointSpec],
    pub contacts: Vec<Contact>,
    /// Ground support points (construction units, y = 0) per body index.
    pub supports: Vec<(usize, DVec3)>,
    /// Indices of anchored bodies.
    pub anchored: Vec<usize>,
    pub masses: Vec<f64>,
    pub force_tolerance: f64,
    pub torque_tolerance: f64,
}

#[derive(Clone, Copy)]
enum Cone {
    Zero,
    Nonnegative,
    SecondOrder,
}

struct Rows {
    triplets: (Vec<usize>, Vec<usize>, Vec<f64>),
    b: Vec<f64>,
    cones: Vec<(Cone, usize)>,
}
impl Rows {
    fn new() -> Self {
        Self {
            triplets: (vec![], vec![], vec![]),
            b: vec![],
            cones: vec![],
        }
    }
    /// One constraint row `b - a·x` in the given cone block.
    fn row(&mut self, coefficients: &[(usize, f64)], b: f64) {
        let row = self.b.len();
        for &(column, value) in coefficients {
            if value != 0. {
                self.triplets.0.push(row);
                self.triplets.1.push(column);
                self.triplets.2.push(value);
            }
        }
        self.b.push(b);
    }
    fn block(&mut self, cone: Cone, rows: usize) {
        match (self.cones.last_mut(), cone) {
            (Some((Cone::Zero, n)), Cone::Zero)
            | (Some((Cone::Nonnegative, n)), Cone::Nonnegative) => *n += rows,
            _ => self.cones.push((cone, rows)),
        }
    }
}

fn cross_matrix(p: DVec3) -> [[f64; 3]; 3] {
    // p × F as a matrix acting on F.
    [[0., -p.z, p.y], [p.z, 0., -p.x], [-p.y, p.x, 0.]]
}

fn basis(axis: DVec3) -> (DVec3, DVec3) {
    let helper = if axis.x.abs() < 0.9 {
        DVec3::X
    } else {
        DVec3::Y
    };
    let e1 = axis.cross(helper).normalize();
    (e1, axis.cross(e1))
}

/// Metric (m) joint axis of a revolute joint.
fn metric_axis(joint: &JointSpec) -> Option<DVec3> {
    match &joint.kind {
        JointKind::Revolute { axis, .. } => Some(crate::physical::to_meters(*axis).normalize()),
        JointKind::Fixed => None,
    }
}

struct Layout {
    joint: usize,
    contact: usize,
    support: usize,
    anchor: usize,
    columns: usize,
}

/// Solve with capacities (`relaxed = false`), or without them to diagnose
/// which joints would need more (`relaxed = true`).
fn solve(problem: &Problem, relaxed: bool) -> Result<(SolverStatus, Vec<f64>, Layout)> {
    use crate::physical::{GRAVITY_M_S2, to_meters};
    let n = problem.bricks.len();
    let layout = Layout {
        joint: 0,
        contact: problem.joints.len() * 6,
        support: problem.joints.len() * 6 + problem.contacts.len() * 4,
        anchor: problem.joints.len() * 6 + problem.contacts.len() * 4 + problem.supports.len(),
        columns: problem.joints.len() * 6
            + problem.contacts.len() * 4
            + problem.supports.len()
            + problem.anchored.len() * 6,
    };
    let origin = to_meters(problem.bricks[0].bounds.center());
    let at = |p: DVec3| to_meters(p) - origin;
    // Equilibrium rows: 6 per body (force, then moment about the origin).
    let mut equations: Vec<Vec<(usize, f64)>> = vec![Vec::new(); n * 6];
    let mut rhs = vec![0.0; n * 6];
    for (i, brick) in problem.bricks.iter().enumerate() {
        let weight = DVec3::NEG_Y * problem.masses[i] * GRAVITY_M_S2;
        let moment = at(brick.bounds.center()).cross(weight);
        for k in 0..3 {
            rhs[i * 6 + k] = -weight[k];
            rhs[i * 6 + 3 + k] = -moment[k];
        }
    }
    let add_force = |eq: &mut Vec<Vec<(usize, f64)>>,
                     body: usize,
                     point: DVec3,
                     column: usize,
                     direction: DVec3,
                     sign: f64| {
        let m = point.cross(direction);
        for k in 0..3 {
            eq[body * 6 + k].push((column, sign * direction[k]));
            eq[body * 6 + 3 + k].push((column, sign * m[k]));
        }
    };
    let index: HashMap<u32, usize> = problem
        .bricks
        .iter()
        .enumerate()
        .map(|(i, b)| (b.id, i))
        .collect();
    for (j, joint) in problem.joints.iter().enumerate() {
        let (a, b) = (index[&joint.a], index[&joint.b]);
        let p = at(joint.anchor);
        let c = layout.joint + j * 6;
        let px = cross_matrix(p);
        // Wrench on b: force F (columns c..c+3), moment M (c+3..c+6) at p.
        for k in 0..3 {
            for (body, sign) in [(b, 1.0), (a, -1.0)] {
                equations[body * 6 + k].push((c + k, sign));
                for (r, row) in px.iter().enumerate() {
                    equations[body * 6 + 3 + r].push((c + k, sign * row[k]));
                }
                equations[body * 6 + 3 + k].push((c + 3 + k, sign));
            }
        }
    }
    for (k, contact) in problem.contacts.iter().enumerate() {
        let p = contact.patch;
        for (corner, (x, z)) in [
            (p.min.x, p.min.z),
            (p.max.x, p.min.z),
            (p.max.x, p.max.z),
            (p.min.x, p.max.z),
        ]
        .into_iter()
        .enumerate()
        {
            let point = at(DVec3::new(x, p.min.y, z));
            let column = layout.contact + k * 4 + corner;
            add_force(&mut equations, contact.upper, point, column, DVec3::Y, 1.0);
            add_force(&mut equations, contact.lower, point, column, DVec3::Y, -1.0);
        }
    }
    for (s, &(body, point)) in problem.supports.iter().enumerate() {
        add_force(
            &mut equations,
            body,
            at(point),
            layout.support + s,
            DVec3::Y,
            1.0,
        );
    }
    for (f, &body) in problem.anchored.iter().enumerate() {
        let c = layout.anchor + f * 6;
        let p = at(problem.bricks[body].bounds.center());
        let px = cross_matrix(p);
        for k in 0..3 {
            equations[body * 6 + k].push((c + k, 1.0));
            for (r, row) in px.iter().enumerate() {
                equations[body * 6 + 3 + r].push((c + k, row[k]));
            }
            equations[body * 6 + 3 + k].push((c + 3 + k, 1.0));
        }
    }
    let mut rows = Rows::new();
    for (equation, value) in equations.iter().zip(&rhs) {
        rows.row(equation, *value);
    }
    rows.block(Cone::Zero, n * 6);
    // Capacity constraints.
    let mut zero: Vec<Row> = Vec::new();
    let mut nonnegative: Vec<Row> = Vec::new();
    let mut second_order: Vec<Vec<Row>> = Vec::new();
    for k in 0..problem.contacts.len() * 4 + problem.supports.len() {
        nonnegative.push((vec![(layout.contact + k, -1.)], 0.));
    }
    if !relaxed {
        for (j, joint) in problem.joints.iter().enumerate() {
            let c = layout.joint + j * 6;
            let axis = metric_axis(joint);
            let force = joint.break_force.unwrap_or(0.);
            let torque = joint.break_torque.unwrap_or(0.);
            let vector = |offset: usize, d: DVec3| -> Vec<(usize, f64)> {
                (0..3).map(|k| (c + offset + k, d[k])).collect()
            };
            if force > 0. {
                second_order.push(
                    std::iter::once((vec![], force))
                        .chain((0..3).map(|k| (vec![(c + k, -1.)], 0.)))
                        .collect(),
                );
            } else {
                for k in 0..3 {
                    zero.push((vec![(c + k, 1.)], 0.));
                }
            }
            match axis {
                None => {
                    if torque > 0. {
                        second_order.push(
                            std::iter::once((vec![], torque))
                                .chain((0..3).map(|k| (vec![(c + 3 + k, -1.)], 0.)))
                                .collect(),
                        );
                    } else {
                        for k in 0..3 {
                            zero.push((vec![(c + 3 + k, 1.)], 0.));
                        }
                    }
                }
                Some(u) => {
                    let (e1, e2) = basis(u);
                    if force > 0. {
                        let axial = joint.axial_force.unwrap_or(force).min(force);
                        let along = vector(0, u);
                        nonnegative.push((along.clone(), axial));
                        nonnegative.push((along.iter().map(|&(c, v)| (c, -v)).collect(), axial));
                    }
                    if torque > 0. {
                        second_order.push(vec![
                            (vec![], torque),
                            (vector(3, e1).iter().map(|&(c, v)| (c, -v)).collect(), 0.),
                            (vector(3, e2).iter().map(|&(c, v)| (c, -v)).collect(), 0.),
                        ]);
                    } else {
                        zero.push((vector(3, e1), 0.));
                        zero.push((vector(3, e2), 0.));
                    }
                    let friction = joint.friction_torque.unwrap_or(0.);
                    let about = vector(3, u);
                    if friction > 0. && force > 0. {
                        nonnegative.push((about.clone(), friction));
                        nonnegative.push((about.iter().map(|&(c, v)| (c, -v)).collect(), friction));
                    } else {
                        zero.push((about, 0.));
                    }
                }
            }
        }
    }
    for (row, b) in &zero {
        rows.row(row, *b);
    }
    rows.block(Cone::Zero, zero.len());
    for (row, b) in &nonnegative {
        rows.row(row, *b);
    }
    rows.block(Cone::Nonnegative, nonnegative.len());
    for cone in &second_order {
        for (row, b) in cone {
            rows.row(row, *b);
        }
        rows.block(Cone::SecondOrder, cone.len());
    }
    // Objective: the sum of squared utilisations, each load component weighed
    // against its own capacity (radial and axial force, bending and the moment
    // about a revolute axis), so a weak capacity such as bearing friction is
    // used only where equilibrium needs it. A tiny regularisation of contact,
    // support and anchor reactions keeps the problem strictly convex.
    let mut triplets: (Vec<usize>, Vec<usize>, Vec<f64>) = (vec![], vec![], vec![]);
    let mut block = |start: usize, across: f64, along: f64, axis: Option<DVec3>| {
        let (across, along) = (1. / (across * across), 1. / (along * along));
        for r in 0..3 {
            for c in r..3 {
                let value = match axis {
                    // across (I - u u^T) + along u u^T
                    Some(u) => {
                        across * f64::from(u8::from(r == c)) + (along - across) * u[r] * u[c]
                    }
                    None => across * f64::from(u8::from(r == c)),
                };
                if value != 0. {
                    triplets.0.push(start + r);
                    triplets.1.push(start + c);
                    triplets.2.push(value);
                }
            }
        }
    };
    for (j, joint) in problem.joints.iter().enumerate() {
        let c = layout.joint + j * 6;
        let force = joint.break_force.filter(|f| *f > 0.).unwrap_or(1e-3);
        let torque = joint.break_torque.filter(|t| *t > 0.).unwrap_or(1e-5);
        let axis = metric_axis(joint);
        let axial = joint
            .axial_force
            .filter(|f| *f > 0.)
            .unwrap_or(force)
            .min(force);
        let friction = joint.friction_torque.filter(|f| *f > 0.).unwrap_or(torque);
        block(c, force, axial, axis);
        block(c + 3, torque, friction, axis);
    }
    let regularised = layout.joint + problem.joints.len() * 6;
    for column in (0..layout.joint).chain(regularised..layout.columns) {
        triplets.0.push(column);
        triplets.1.push(column);
        triplets.2.push(1e-6);
    }
    let p = CscMatrix::new_from_triplets(
        layout.columns,
        layout.columns,
        triplets.0,
        triplets.1,
        triplets.2,
    );
    let q = vec![0.0; layout.columns];
    let (r, c, v) = rows.triplets;
    let a = CscMatrix::new_from_triplets(rows.b.len(), layout.columns, r, c, v);
    let cones: Vec<SupportedConeT<f64>> = rows
        .cones
        .iter()
        .filter(|(_, n)| *n > 0)
        .map(|&(cone, n)| match cone {
            Cone::Zero => ZeroConeT(n),
            Cone::Nonnegative => NonnegativeConeT(n),
            Cone::SecondOrder => SecondOrderConeT(n),
        })
        .collect();
    let settings = DefaultSettingsBuilder::default()
        .verbose(false)
        .max_iter(200)
        .tol_gap_abs(1e-12)
        .tol_gap_rel(1e-12)
        .tol_feas(1e-12)
        .build()
        .map_err(|e| anyhow::anyhow!("Equilibrium solver settings: {e:?}"))?;
    let mut solver = DefaultSolver::new(&p, &q, &a, &rows.b, &cones, settings)
        .map_err(|e| anyhow::anyhow!("Equilibrium problem: {e:?}"))?;
    solver.solve();
    Ok((solver.solution.status, solver.solution.x.clone(), layout))
}

/// One sparse constraint row `(coefficients, constant)`: `constant - a·x` lies in
/// the row's cone.
type Row = (Vec<(usize, f64)>, f64);

fn wrench(x: &[f64], layout: &Layout, j: usize) -> (DVec3, DVec3) {
    let c = layout.joint + j * 6;
    (
        DVec3::new(x[c], x[c + 1], x[c + 2]),
        DVec3::new(x[c + 3], x[c + 4], x[c + 5]),
    )
}

/// Find and independently verify a static equilibrium.
pub fn certify(problem: &Problem) -> Result<Certificate> {
    let (status, x, layout) = solve(problem, false)?;
    // The independent check is the certificate, whatever the solver reports:
    // an interior-point solver may stop short of its own tolerance on a nearly
    // degenerate but feasible problem, and its status must not reject (or
    // admit) a construction on its own.
    match verify(problem, &x, &layout) {
        Ok(certificate) => Ok(certificate),
        Err(error) if matches!(status, SolverStatus::Solved | SolverStatus::AlmostSolved) => {
            Err(error)
        }
        Err(_) => Err(diagnose(problem)),
    }
}

/// Re-check balance and every capacity of a candidate solution, with the
/// certificate's own tolerances (scaled to the total weight for balance).
fn verify(problem: &Problem, x: &[f64], layout: &Layout) -> Result<Certificate> {
    use crate::physical::{GRAVITY_M_S2, to_meters};
    let total: f64 = problem.masses.iter().sum::<f64>() * GRAVITY_M_S2;
    let force_tol = problem.force_tolerance.max(1e-7 * total);
    let torque_tol = problem.torque_tolerance.max(1e-9 * total);
    let origin = to_meters(problem.bricks[0].bounds.center());
    let at = |p: DVec3| to_meters(p) - origin;
    let n = problem.bricks.len();
    let mut force = vec![DVec3::ZERO; n];
    let mut moment = vec![DVec3::ZERO; n];
    for (i, brick) in problem.bricks.iter().enumerate() {
        let weight = DVec3::NEG_Y * problem.masses[i] * GRAVITY_M_S2;
        force[i] += weight;
        moment[i] += at(brick.bounds.center()).cross(weight);
    }
    let index: HashMap<u32, usize> = problem
        .bricks
        .iter()
        .enumerate()
        .map(|(i, b)| (b.id, i))
        .collect();
    let mut certificate = Certificate::default();
    for (j, joint) in problem.joints.iter().enumerate() {
        let (f, m) = wrench(x, layout, j);
        let p = at(joint.anchor);
        let (a, b) = (index[&joint.a], index[&joint.b]);
        force[b] += f;
        moment[b] += p.cross(f) + m;
        force[a] -= f;
        moment[a] -= p.cross(f) + m;
        if f.length() > force_tol || m.length() > torque_tol {
            certificate.loaded_joints += 1;
        }
        let capped = |load: f64, capacity: Option<f64>, tolerance: f64| -> Result<f64> {
            let capacity = capacity.unwrap_or(0.);
            ensure!(
                load <= capacity * (1. + 1e-6) + tolerance,
                "Construction is not static: joint {} ({}) ↔ {} ({}) carries {load:.3e} with capacity {capacity:.3e} (tolerance {tolerance:.1e})",
                joint.a,
                problem.bricks[a].part,
                joint.b,
                problem.bricks[b].part
            );
            Ok(if capacity > 0. { load / capacity } else { 0. })
        };
        let mut utilisation = capped(f.length(), joint.break_force, force_tol)?;
        match metric_axis(joint) {
            None => {
                utilisation = utilisation.max(capped(m.length(), joint.break_torque, torque_tol)?)
            }
            Some(u) => {
                let axial = f.dot(u).abs();
                let about = m.dot(u).abs();
                let bending = (m - u * m.dot(u)).length();
                let axial_capacity = joint
                    .break_force
                    .map(|cap| joint.axial_force.unwrap_or(cap).min(cap));
                utilisation = utilisation
                    .max(capped(axial, axial_capacity, force_tol)?)
                    .max(capped(bending, joint.break_torque, torque_tol)?)
                    .max(capped(
                        about,
                        joint
                            .friction_torque
                            .filter(|_| joint.break_force.is_some_and(|f| f > 0.)),
                        torque_tol,
                    )?);
            }
        }
        certificate.max_utilisation = certificate.max_utilisation.max(utilisation);
    }
    for (k, contact) in problem.contacts.iter().enumerate() {
        let p = contact.patch;
        for (corner, (cx, cz)) in [
            (p.min.x, p.min.z),
            (p.max.x, p.min.z),
            (p.max.x, p.max.z),
            (p.min.x, p.max.z),
        ]
        .into_iter()
        .enumerate()
        {
            let value = x[layout.contact + k * 4 + corner];
            ensure!(
                value >= -force_tol,
                "Construction is not static: tensile contact"
            );
            let point = at(DVec3::new(cx, p.min.y, cz));
            let f = DVec3::Y * value;
            force[contact.upper] += f;
            moment[contact.upper] += point.cross(f);
            force[contact.lower] -= f;
            moment[contact.lower] -= point.cross(f);
        }
    }
    for (s, &(body, point)) in problem.supports.iter().enumerate() {
        let value = x[layout.support + s];
        ensure!(
            value >= -force_tol,
            "Construction is not static: tensile ground support"
        );
        let f = DVec3::Y * value;
        force[body] += f;
        moment[body] += at(point).cross(f);
    }
    for (f, &body) in problem.anchored.iter().enumerate() {
        let c = layout.anchor + f * 6;
        let r = DVec3::new(x[c], x[c + 1], x[c + 2]);
        force[body] += r;
        moment[body] += at(problem.bricks[body].bounds.center()).cross(r)
            + DVec3::new(x[c + 3], x[c + 4], x[c + 5]);
    }
    for i in 0..n {
        ensure!(
            force[i].length() <= force_tol && moment[i].length() <= torque_tol,
            "Construction is not static: brick {} ({}) is not in equilibrium",
            problem.bricks[i].id,
            problem.bricks[i].part
        );
    }
    Ok(certificate)
}

/// Explain an infeasible equilibrium: the joints that would need more
/// capacity than they have, from a solution without capacity limits.
fn diagnose(problem: &Problem) -> anyhow::Error {
    let Ok((status, x, layout)) = solve(problem, true) else {
        return anyhow::anyhow!(
            "Construction is not static: the equilibrium problem could not be solved"
        );
    };
    if !matches!(status, SolverStatus::Solved | SolverStatus::AlmostSolved) {
        return anyhow::anyhow!(
            "Construction is not static: no ground support, contact or joint arrangement can balance gravity ({status:?})"
        );
    }
    let mut needs: Vec<(f64, String)> = Vec::new();
    for (j, joint) in problem.joints.iter().enumerate() {
        let (f, m) = wrench(&x, &layout, j);
        let force_cap = joint.break_force.unwrap_or(0.);
        let torque_cap = joint.break_torque.unwrap_or(0.);
        let (force_load, torque_load) = match metric_axis(joint) {
            None => (f.length(), m.length()),
            Some(u) => (f.length(), (m - u * m.dot(u)).length() + m.dot(u).abs()),
        };
        let excess = (force_load - force_cap).max(0.) / force_cap.max(1e-3)
            + (torque_load - torque_cap).max(0.) / torque_cap.max(1e-5);
        if excess > 1e-3 {
            needs.push((
                excess,
                format!(
                    "{} ↔ {} needs {force_load:.4} N / {torque_load:.6} N m (capacity {force_cap} N / {torque_cap} N m, {})",
                    joint.a,
                    joint.b,
                    joint.strength_source.as_deref().unwrap_or("no measurement or reviewed estimate")
                ),
            ));
        }
    }
    needs.sort_by(|a, b| b.0.total_cmp(&a.0));
    if needs.is_empty() {
        return anyhow::anyhow!(
            "Construction is not static: no equilibrium respects contact unilaterality"
        );
    }
    let shown: Vec<String> = needs.iter().take(6).map(|(_, text)| text.clone()).collect();
    anyhow::anyhow!(
        "Construction is not static: {} joint(s) would need more capacity than measured or estimated: {}",
        needs.len(),
        shown.join("; ")
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::JointKind;

    /// `n` 1x1-brick "links" in a row along X at height 5, one unit apart.
    fn links(n: usize) -> Vec<Brick> {
        let mut demo = crate::demo::Demo::new();
        for i in 0..n {
            demo.add("3005", 4, i as f64, 5., 0.);
        }
        demo.bricks
    }
    fn hinge(a: u32, b: u32, x: f64, friction: f64) -> JointSpec {
        JointSpec {
            a,
            b,
            anchor: DVec3::new(x, 5.5, 0.5),
            kind: JointKind::Revolute {
                axis: DVec3::Z,
                limits: None,
            },
            break_force: Some(150.),
            break_torque: Some(0.3),
            axial_force: Some(0.2),
            friction_torque: Some(friction),
            strength_source: Some("test".into()),
        }
    }
    fn problem<'a>(
        bricks: &'a [Brick],
        joints: &'a [JointSpec],
        anchored: Vec<usize>,
    ) -> Problem<'a> {
        Problem {
            bricks: Bodies::all(bricks),
            joints,
            contacts: vec![],
            supports: vec![],
            anchored,
            masses: vec![0.001; bricks.len()],
            force_tolerance: 1e-6,
            torque_tolerance: 1e-8,
        }
    }

    /// A hanging chain of `heights.len() - 1` links between two anchored end
    /// bodies: pins at the vertices, each link's mass at its midpoint.
    fn chain(heights: &[f64]) -> (Vec<Brick>, Vec<JointSpec>) {
        let mut demo = crate::demo::Demo::new();
        let n = heights.len() - 1;
        let mut centres = vec![DVec3::new(-0.5, heights[0], 0.5)];
        centres.extend(
            (0..n).map(|i| DVec3::new(i as f64 + 0.5, (heights[i] + heights[i + 1]) / 2., 0.5)),
        );
        centres.push(DVec3::new(n as f64 + 0.5, heights[n], 0.5));
        for _ in &centres {
            demo.add("3005", 4, 0., 0., 0.);
        }
        let mut bricks = demo.bricks;
        for (b, c) in bricks.iter_mut().zip(&centres) {
            b.bounds = crate::Aabb::new(*c - DVec3::splat(0.5), *c + DVec3::splat(0.5));
        }
        let joints = (0..=n)
            .map(|i| JointSpec {
                anchor: DVec3::new(i as f64, heights[i], 0.5),
                ..hinge(i as u32 + 1, i as u32 + 2, 0., 0.)
            })
            .collect();
        (bricks, joints)
    }

    #[test]
    fn a_sagging_chain_held_at_both_ends_is_static_only_in_its_funicular_shape() {
        // Equal link weights and spans: slopes step by a constant (-1.5k, -0.5k,
        // 0.5k, 1.5k), the discrete catenary of a uniformly loaded chain.
        let k = 0.5;
        let (bricks, joints) = chain(&[5., 5. - 1.5 * k, 5. - 2. * k, 5. - 1.5 * k, 5.]);
        let anchored = vec![0, bricks.len() - 1];
        let certificate = certify(&problem(&bricks, &joints, anchored.clone())).unwrap();
        assert_eq!(certificate.loaded_joints, 5);
        // Any other polygon is a mechanism on free pins...
        let (bricks, joints) = chain(&[5., 4.6, 4.0, 4.6, 5.]);
        assert!(certify(&problem(&bricks, &joints, anchored.clone())).is_err());
        // ...that friction pins can hold up to their friction torque.
        let held: Vec<_> = joints
            .into_iter()
            .map(|j| JointSpec {
                friction_torque: Some(0.005),
                ..j
            })
            .collect();
        certify(&problem(&bricks, &held, anchored)).unwrap();
    }

    #[test]
    fn free_pins_cannot_hold_a_horizontal_cantilever_but_friction_pins_can() {
        let bricks = links(3);
        let free: Vec<_> = (1..3).map(|i| hinge(i, i + 1, i as f64, 0.)).collect();
        let error = certify(&problem(&bricks, &free, vec![0]))
            .unwrap_err()
            .to_string();
        assert!(error.contains("not static"), "{error}");
        // Two links of 1 g, lever arms of 0.5 and 1.5 studs: about 1.6e-4 N m
        // at the first pin, within a 0.005 N m friction pin.
        let friction: Vec<_> = (1..3).map(|i| hinge(i, i + 1, i as f64, 0.005)).collect();
        certify(&problem(&bricks, &friction, vec![0])).unwrap();
        let weak: Vec<_> = (1..3).map(|i| hinge(i, i + 1, i as f64, 1e-5)).collect();
        assert!(certify(&problem(&bricks, &weak, vec![0])).is_err());
    }

    #[test]
    fn a_pinned_triangle_truss_carries_load_without_moment_at_any_pin() {
        // Two anchored feet and an apex link joined by free pins: the apex is
        // carried by the two inclined members only through pin forces.
        let mut demo = crate::demo::Demo::new();
        demo.add("3005", 4, 0., 0., 0.);
        demo.add("3005", 4, 4., 0., 0.);
        demo.add("3005", 4, 1., 2., 0.);
        demo.add("3005", 4, 3., 2., 0.);
        demo.add("3005", 4, 2., 4., 0.);
        let bricks = demo.bricks;
        let pin = |a: u32, b: u32, p: DVec3| JointSpec {
            anchor: p,
            ..hinge(a, b, 0., 0.)
        };
        let joints = vec![
            pin(1, 3, DVec3::new(0.5, 1., 0.5)),
            pin(3, 5, DVec3::new(1.5, 3., 0.5)),
            pin(2, 4, DVec3::new(4.5, 1., 0.5)),
            pin(4, 5, DVec3::new(3.5, 3., 0.5)),
        ];
        certify(&problem(&bricks, &joints, vec![0, 1])).unwrap();
        // Remove one member: the apex is left on a free pin with a moment.
        assert!(certify(&problem(&bricks, &joints[..3], vec![0, 1])).is_err());
    }

    #[test]
    fn joints_without_capacity_carry_nothing() {
        let bricks = links(2);
        let mut joint = hinge(1, 2, 1., 0.);
        joint.break_force = None;
        joint.break_torque = None;
        let error = certify(&problem(&bricks, &[joint], vec![0]))
            .unwrap_err()
            .to_string();
        assert!(error.contains("would need more capacity"), "{error}");
    }
}
