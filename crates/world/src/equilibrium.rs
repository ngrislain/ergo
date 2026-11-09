//! General static equilibrium certificate.
//!
//! One coupled convex problem per supplied island: static validation first splits
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
//! The objective combines capacity-weighted joint loads with dimensionless
//! reaction regularization. The solver is Clarabel (pure
//! Rust, WASM-safe); its solution is re-checked independently before it is
//! accepted.
use crate::{Brick, JointKind, JointSpec};
use anyhow::{Context, Result, ensure};
use clarabel::{algebra::CscMatrix, solver::*};
use glam::DVec3;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[path = "equilibrium_input.rs"]
mod input;
#[path = "equilibrium_partitioned.rs"]
mod partitioned;
#[path = "equilibrium_planner.rs"]
mod planner;
#[path = "equilibrium_polish.rs"]
mod polish;
#[path = "equilibrium_verify.rs"]
mod verification;
#[path = "equilibrium_workspace.rs"]
mod workspace;
pub use input::{ProblemInput, ProblemSize, StoredProblem};
pub use partitioned::{PartitionOptions, certify_partitioned, certify_partitioned_with_workspace};
use verification::WitnessVerifier;
use workspace::Boundary;
pub use workspace::{
    MemoryWorkspace, PartitionPlan, ReactionColumn, ReactionRead, ReactionWrite, SolverWorkspace,
};

/// Numerical-work diagnostics, not an alternative acceptance certificate.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct SolverStats {
    pub partitions: usize,
    pub sweeps: usize,
    pub local_solves: usize,
    pub balance_correction_iterations: usize,
    pub spilled_partition_plans: usize,
    pub spilled_reaction_columns: usize,
    pub largest_local_bodies: usize,
    pub largest_local_kkt_dimension: usize,
    pub interface_force_residual_newtons: f64,
    pub interface_torque_residual_newton_meters: f64,
}
impl SolverStats {
    pub fn merge(&mut self, other: &Self) {
        self.partitions += other.partitions;
        self.sweeps = self.sweeps.max(other.sweeps);
        self.local_solves += other.local_solves;
        self.balance_correction_iterations += other.balance_correction_iterations;
        self.spilled_partition_plans += other.spilled_partition_plans;
        self.spilled_reaction_columns += other.spilled_reaction_columns;
        self.largest_local_bodies = self.largest_local_bodies.max(other.largest_local_bodies);
        self.largest_local_kkt_dimension = self
            .largest_local_kkt_dimension
            .max(other.largest_local_kkt_dimension);
        self.interface_force_residual_newtons = self
            .interface_force_residual_newtons
            .max(other.interface_force_residual_newtons);
        self.interface_torque_residual_newton_meters = self
            .interface_torque_residual_newton_meters
            .max(other.interface_torque_residual_newton_meters);
    }
}

/// No physical infeasibility is claimed when a bounded numerical solve stops.
#[derive(Debug)]
pub struct Inconclusive(pub String);
impl std::fmt::Display for Inconclusive {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Equilibrium inconclusive; not certified: {}", self.0)
    }
}
impl std::error::Error for Inconclusive {}

/// A verified equilibrium.
#[derive(Debug, Clone, Default)]
pub struct Certificate {
    /// Joints that carry more than the force/torque tolerance.
    pub loaded_joints: usize,
    /// Largest load / capacity ratio over every capped joint quantity.
    pub max_utilisation: f64,
    pub solver: SolverStats,
}

/// A frictionless contact between two axis-aligned bodies: a horizontal patch
/// in construction units, with `upper` resting on `lower`.
#[derive(Clone, Copy)]
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

#[derive(PartialEq, Eq)]
struct Layout {
    joint: usize,
    contact: usize,
    support: usize,
    anchor: usize,
    columns: usize,
}
impl Layout {
    fn of_input(problem: &dyn ProblemInput) -> Self {
        let s = problem.size();
        let contact = s.joints.saturating_mul(6);
        let support = contact.saturating_add(s.contacts.saturating_mul(4));
        let anchor = support.saturating_add(s.supports);
        Self {
            joint: 0,
            contact,
            support,
            anchor,
            columns: anchor.saturating_add(s.anchors.saturating_mul(6)),
        }
    }
    fn of(problem: &Problem) -> Self {
        Self {
            joint: 0,
            contact: problem.joints.len() * 6,
            support: problem.joints.len() * 6 + problem.contacts.len() * 4,
            anchor: problem.joints.len() * 6 + problem.contacts.len() * 4 + problem.supports.len(),
            columns: problem.joints.len() * 6
                + problem.contacts.len() * 4
                + problem.supports.len()
                + problem.anchored.len() * 6,
        }
    }
}

struct Penalty {
    column: usize,
    target: f64,
    diagonal: f64,
}

/// A numerical search bound, not a measured capacity. Local failure is always
/// inconclusive; a candidate must still satisfy the original physical problem.
const LOCAL_REACTION_UNITS: f64 = 10_000.;

/// Solve with capacities (`relaxed = false`), or without them to diagnose
/// which joints would need more (`relaxed = true`).
fn solve(problem: &Problem, relaxed: bool) -> Result<(SolverStatus, Vec<f64>, Layout)> {
    solve_local(
        problem,
        relaxed,
        None,
        &[],
        None,
        representative_force(problem),
    )
}

fn representative_force(problem: &Problem) -> f64 {
    (problem.masses.iter().sum::<f64>() * crate::physical::GRAVITY_M_S2
        / problem.bricks.len() as f64)
        .max(1e-3)
}

/// The same local SOCP kernel for one whole island or one partition. Unowned
/// endpoint records supply geometry/IDs only; no balance rows or fictitious
/// anchors are created for them. Their reactions are coupled by consensus.
fn solve_local(
    problem: &Problem,
    relaxed: bool,
    owned: Option<&[bool]>,
    penalties: &[Penalty],
    weights: Option<&[f64]>,
    force_unit: f64,
) -> Result<(SolverStatus, Vec<f64>, Layout)> {
    use crate::physical::{GRAVITY_M_S2, to_meters};
    let n = problem.bricks.len();
    let layout = Layout::of(problem);
    let torque_unit = force_unit * 0.008;
    // Moment equations about each body's own centre are exactly equivalent
    // under force balance, but do not couple small torques to a distant origin.
    let at = |body: usize, p: DVec3| to_meters(p - problem.bricks[body].bounds.center());
    // Equilibrium rows: 6 per body (force, then moment about its centre).
    let mut equations: Vec<Vec<(usize, f64)>> = vec![Vec::new(); n * 6];
    let mut rhs = vec![0.0; n * 6];
    for (i, brick) in problem.bricks.iter().enumerate() {
        let weight = DVec3::NEG_Y * problem.masses[i] * GRAVITY_M_S2;
        let moment = at(i, brick.bounds.center()).cross(weight);
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
        let m = at(body, point).cross(direction);
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
        let c = layout.joint + j * 6;
        // Wrench on b: force F (columns c..c+3), moment M (c+3..c+6) at p.
        for k in 0..3 {
            for (body, sign) in [(b, 1.0), (a, -1.0)] {
                let px = cross_matrix(at(body, joint.anchor));
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
            let point = DVec3::new(x, p.min.y, z);
            let column = layout.contact + k * 4 + corner;
            add_force(&mut equations, contact.upper, point, column, DVec3::Y, 1.0);
            add_force(&mut equations, contact.lower, point, column, DVec3::Y, -1.0);
        }
    }
    for (s, &(body, point)) in problem.supports.iter().enumerate() {
        add_force(
            &mut equations,
            body,
            point,
            layout.support + s,
            DVec3::Y,
            1.0,
        );
    }
    for (f, &body) in problem.anchored.iter().enumerate() {
        let c = layout.anchor + f * 6;
        let p = DVec3::ZERO;
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
    for (row, (equation, value)) in equations.iter().zip(&rhs).enumerate() {
        if owned.is_some_and(|owned| !owned[row / 6]) {
            continue;
        }
        rows.row(equation, *value);
    }
    rows.block(
        Cone::Zero,
        owned.map_or(n, |o| o.iter().filter(|v| **v).count()) * 6,
    );
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
            // A bounded numerical reaction search is stricter than the physical
            // capacities, never a replacement for the independent checker.
            // It avoids enormous inactive cones in local proximal problems.
            // Failure of this search is inconclusive, not physical infeasibility.
            let force = joint.break_force.unwrap_or(0.).min(if owned.is_some() {
                force_unit * LOCAL_REACTION_UNITS
            } else {
                f64::INFINITY
            });
            let torque = joint.break_torque.unwrap_or(0.).min(if owned.is_some() {
                torque_unit * LOCAL_REACTION_UNITS
            } else {
                f64::INFINITY
            });
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
                    let friction = joint.friction_torque.unwrap_or(0.).min(if owned.is_some() {
                        torque_unit * LOCAL_REACTION_UNITS
                    } else {
                        f64::INFINITY
                    });
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
    if layout.columns.saturating_add(rows.b.len()) > partitioned::MAX_LOCAL_KKT_DIMENSION {
        return Err(Inconclusive(
            "assembled local KKT dimension exceeds its workspace budget".into(),
        )
        .into());
    }
    // Objective: the sum of squared utilisations, each load component weighed
    // against its own capacity (radial and axial force, bending and the moment
    // about a revolute axis), so a weak capacity such as bearing friction is
    // used only where equilibrium needs it. A dimensionless regularisation of contact,
    // support, anchor and joint reactions keeps the problem strictly convex,
    // including effectively unbreakable internal load-sharing loops. Every
    // partition uses the canonical island's same physical regularization units.
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
    for column in 0..layout.columns {
        triplets.0.push(column);
        triplets.1.push(column);
        let unit = if (column < layout.contact && column % 6 >= 3)
            || (column >= layout.anchor && (column - layout.anchor) % 6 >= 3)
        {
            torque_unit
        } else {
            force_unit
        };
        triplets.2.push(1e-2 / (unit * unit));
    }
    if let Some(weights) = weights {
        for k in 0..triplets.2.len() {
            triplets.2[k] *= (weights[triplets.0[k]] * weights[triplets.1[k]]).sqrt();
        }
    }
    let mut q = vec![0.0; layout.columns];
    for penalty in penalties {
        triplets.0.push(penalty.column);
        triplets.1.push(penalty.column);
        triplets.2.push(penalty.diagonal);
        q[penalty.column] -= penalty.diagonal * penalty.target;
    }
    // Solve in representative body-weight / stud-moment units, returning SI
    // reactions afterwards. Merely scaling the ADMM penalty leaves torque
    // columns and effectively unbreakable capacity cones badly conditioned.
    let units: Vec<_> = (0..layout.columns)
        .map(|column| {
            if (column < layout.contact && column % 6 >= 3)
                || (column >= layout.anchor && (column - layout.anchor) % 6 >= 3)
            {
                torque_unit
            } else {
                force_unit
            }
        })
        .collect();
    for k in 0..triplets.2.len() {
        triplets.2[k] *= units[triplets.0[k]] * units[triplets.1[k]];
    }
    for (column, value) in q.iter_mut().enumerate() {
        *value *= units[column];
    }
    // A positive common objective rescale leaves each local minimizer intact.
    let objective_scale = triplets
        .2
        .iter()
        .chain(q.iter())
        .fold(1f64, |m, v| m.max(v.abs()));
    for value in triplets.2.iter_mut().chain(q.iter_mut()) {
        *value /= objective_scale;
    }
    for k in 0..rows.triplets.2.len() {
        rows.triplets.2[k] *= units[rows.triplets.1[k]];
    }
    let mut row_norm: Vec<_> = rows.b.iter().map(|b| b.abs()).collect();
    for (&row, &value) in rows.triplets.0.iter().zip(&rows.triplets.2) {
        row_norm[row] = row_norm[row].max(value.abs());
    }
    let mut start = 0;
    for &(cone, count) in &rows.cones {
        // SOC rows must share one scale to preserve the physical cone.
        if matches!(cone, Cone::SecondOrder) {
            let norm = row_norm[start..start + count]
                .iter()
                .copied()
                .fold(0f64, f64::max);
            row_norm[start..start + count].fill(norm);
        }
        start += count;
    }
    for norm in &mut row_norm {
        if *norm == 0. {
            *norm = 1.;
        }
    }
    for k in 0..rows.triplets.2.len() {
        rows.triplets.2[k] /= row_norm[rows.triplets.0[k]];
    }
    for (value, norm) in rows.b.iter_mut().zip(row_norm) {
        *value /= norm;
    }
    let p = CscMatrix::new_from_triplets(
        layout.columns,
        layout.columns,
        triplets.0,
        triplets.1,
        triplets.2,
    );
    let (r, c, v) = rows.triplets;
    if rhs
        .iter()
        .chain(rows.b.iter())
        .chain(v.iter())
        .chain(q.iter())
        .any(|v| !v.is_finite())
        || p.nzval.iter().any(|v| !v.is_finite())
    {
        return Err(Inconclusive("nonfinite local SOCP coefficients".into()).into());
    }
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
        .min_terminate_step_length(1e-10)
        .static_regularization_constant(1e-10)
        .iterative_refinement_max_iter(30)
        .build()
        .map_err(|e| anyhow::anyhow!("Equilibrium solver settings: {e:?}"))?;
    let mut solver = DefaultSolver::new(&p, &q, &a, &rows.b, &cones, settings)
        .map_err(|e| anyhow::anyhow!("Equilibrium problem: {e:?}"))?;
    solver.solve();
    let x = solver
        .solution
        .x
        .iter()
        .zip(units)
        .map(|(value, unit)| value * unit)
        .collect();
    Ok((solver.solution.status, x, layout))
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
fn reaction_wrench<R: ReactionRead + ?Sized>(
    x: &R,
    layout: &Layout,
    j: usize,
) -> Result<(DVec3, DVec3)> {
    let mut values = [0.; 6];
    x.read_reactions(layout.joint + j * 6, &mut values)?;
    Ok((
        DVec3::new(values[0], values[1], values[2]),
        DVec3::new(values[3], values[4], values[5]),
    ))
}

/// Find and independently verify a static equilibrium.
pub fn certify(problem: &Problem) -> Result<Certificate> {
    certify_with_workspace(problem, &mut MemoryWorkspace::default())
}
pub fn certify_with_workspace(
    problem: &Problem,
    workspace: &mut dyn SolverWorkspace,
) -> Result<Certificate> {
    certify_input_with_workspace(problem, workspace)
}
pub fn certify_input_with_workspace(
    problem: &dyn ProblemInput,
    workspace: &mut dyn SolverWorkspace,
) -> Result<Certificate> {
    validate_problem_input(problem)?;
    let size = problem.size();
    if size.bodies > PartitionOptions::default().core_bodies
        || partitioned::kkt_bound(
            size.bodies,
            size.joints,
            size.contacts,
            size.supports,
            Layout::of_input(problem).columns,
        ) > partitioned::MAX_LOCAL_KKT_DIMENSION
    {
        return partitioned::certify_input_partitioned(
            problem,
            PartitionOptions::default(),
            workspace,
        );
    }
    let local = input::materialize(
        problem,
        &(0..size.bodies).collect::<Vec<_>>(),
        &(0..size.joints).collect::<Vec<_>>(),
        &(0..size.contacts).collect::<Vec<_>>(),
        &(0..size.supports).collect::<Vec<_>>(),
        &(0..size.anchors).collect::<Vec<_>>(),
    )?;
    certify_local(&local.problem())
}
fn certify_local(problem: &Problem) -> Result<Certificate> {
    let (status, x, layout) = solve(problem, false)
        .map_err(|error| Inconclusive(format!("local SOCP failed: {error:#}")))?;
    // The independent check is the certificate, whatever the solver reports:
    // an interior-point solver may stop short of its own tolerance on a nearly
    // degenerate but feasible problem, and its status must not reject (or
    // admit) a construction on its own.
    match verify(problem, &x, &layout) {
        Ok(mut certificate) => {
            certificate.solver = SolverStats {
                partitions: 1,
                sweeps: 1,
                local_solves: 1,
                largest_local_bodies: problem.bricks.len(),
                largest_local_kkt_dimension: partitioned::kkt_bound(
                    problem.bricks.len(),
                    problem.joints.len(),
                    problem.contacts.len(),
                    problem.supports.len(),
                    layout.columns,
                ),
                ..Default::default()
            };
            Ok(certificate)
        }
        Err(error) if error.downcast_ref::<Inconclusive>().is_some() => Err(error),
        Err(_)
            if matches!(
                status,
                SolverStatus::PrimalInfeasible | SolverStatus::AlmostPrimalInfeasible
            ) =>
        {
            Err(diagnose(problem))
        }
        Err(error) => Err(Inconclusive(format!(
            "local solver stopped with {status:?}; independent witness check failed: {error:#}"
        ))
        .into()),
    }
}

fn validate_problem_input(problem: &dyn ProblemInput) -> Result<()> {
    problem.validate_storage()?;
    let n = problem.size().bodies;
    ensure!(n > 0, "Invalid equilibrium body/mass dimensions");
    let (f, t) = problem.tolerances();
    ensure!(
        f.is_finite() && f > 0. && t.is_finite() && t > 0.,
        "Invalid equilibrium tolerances"
    );
    for i in 0..n {
        let b = problem.body(i)?;
        let m = problem.mass(i)?;
        ensure!(
            problem.body_index(b.id)? == Some(i)
                && b.bounds.min.is_finite()
                && b.bounds.max.is_finite(),
            "Invalid equilibrium bodies"
        );
        ensure!(m.is_finite() && m >= 0., "Invalid equilibrium masses");
    }
    input::weight(problem)?;
    for i in 0..problem.size().joints {
        let j = problem.joint(i)?;
        ensure!(
            j.a != j.b
                && problem.body_index(j.a)?.is_some()
                && problem.body_index(j.b)?.is_some()
                && j.anchor.is_finite(),
            "Invalid equilibrium joint endpoints"
        );
        ensure!(
            [
                j.break_force,
                j.break_torque,
                j.axial_force,
                j.friction_torque
            ]
            .into_iter()
            .flatten()
            .all(|v| v.is_finite() && v >= 0.),
            "Invalid joint capacity"
        );
        if let JointKind::Revolute { axis, .. } = j.kind {
            ensure!(
                axis.is_finite() && axis.length().is_finite() && axis.length() > 1e-12,
                "Invalid joint axis"
            );
        }
    }
    for i in 0..problem.size().contacts {
        let c = problem.contact(i)?;
        ensure!(
            c.lower < n
                && c.upper < n
                && c.lower != c.upper
                && c.patch.min.is_finite()
                && c.patch.max.is_finite(),
            "Invalid equilibrium contacts"
        );
    }
    for i in 0..problem.size().supports {
        let (b, p) = problem.support(i)?;
        ensure!(b < n && p.is_finite(), "Invalid equilibrium supports");
    }
    for i in 0..problem.size().anchors {
        ensure!(problem.anchor(i)? < n, "Invalid equilibrium supports");
    }
    Ok(())
}

/// Re-check balance and every capacity of a candidate solution, with the
/// certificate's own tolerances (scaled to the total weight for balance).
fn verify(problem: &Problem, x: &[f64], layout: &Layout) -> Result<Certificate> {
    verify_subset(problem, x, layout, None)
}
fn verify_subset(
    problem: &Problem,
    x: &[f64],
    layout: &Layout,
    owned: Option<&[bool]>,
) -> Result<Certificate> {
    let mut state = crate::validation::MemoryValidationState::default();
    WitnessVerifier::new(problem, &mut state)?.verify(problem, x, layout, owned, &state)
}

/// Explain an infeasible equilibrium: the joints that would need more
/// capacity than they have, from a solution without capacity limits.
fn diagnose(problem: &Problem) -> anyhow::Error {
    let (status, x, layout) = match solve(problem, true) {
        Ok(result) => result,
        Err(error) => {
            return Inconclusive(format!("capacity diagnostic solve failed: {error:#}")).into();
        }
    };
    if !matches!(status, SolverStatus::Solved | SolverStatus::AlmostSolved) {
        return if matches!(
            status,
            SolverStatus::PrimalInfeasible | SolverStatus::AlmostPrimalInfeasible
        ) {
            anyhow::anyhow!(
                "Construction is not static: no ground support, contact or joint arrangement can balance gravity ({status:?})"
            )
        } else {
            Inconclusive(format!(
                "capacity diagnostic solver stopped with {status:?}"
            ))
            .into()
        };
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
        let split = certify_partitioned(
            &problem(&bricks, &joints, vec![0, 1]),
            PartitionOptions {
                core_bodies: 1,
                max_sweeps: 1000,
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(split.solver.partitions, 5);
        assert!(split.loaded_joints > 0);
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

    #[test]
    fn partitioned_two_ended_chain_reconciles_real_forces_without_cut_anchors() {
        let (bricks, joints) = chain(&[5., 4.25, 4., 4.25, 5.]);
        let p = problem(&bricks, &joints, vec![0, bricks.len() - 1]);
        let whole = certify(&p).unwrap();
        let mut iterated = false;
        for core in [1, 2, 3] {
            let certificate = certify_partitioned(
                &p,
                PartitionOptions {
                    core_bodies: core,
                    max_sweeps: 1000,
                    ..Default::default()
                },
            )
            .unwrap();
            assert_eq!(certificate.loaded_joints, whole.loaded_joints);
            assert!(certificate.solver.partitions > 1);
            assert!(certificate.solver.sweeps >= 1);
            iterated |= certificate.solver.sweeps > 1;
            assert!(certificate.solver.largest_local_bodies <= core + 2);
        }
        assert!(
            iterated,
            "At least one cut must exercise iterative force propagation"
        );
    }

    #[test]
    fn partitioned_solver_never_accepts_unreconciled_loads_or_a_floater() {
        let (bricks, joints) = chain(&[5., 4.25, 4., 4.25, 5.]);
        let p = problem(&bricks, &joints, vec![0, bricks.len() - 1]);
        let e = certify_partitioned(
            &p,
            PartitionOptions {
                core_bodies: 1,
                max_sweeps: 0,
                ..Default::default()
            },
        )
        .unwrap_err();
        assert!(e.downcast_ref::<Inconclusive>().is_some());
        let floating = problem(&bricks, &joints, vec![]);
        assert!(
            certify_partitioned(
                &floating,
                PartitionOptions {
                    core_bodies: 1,
                    max_sweeps: 20,
                    ..Default::default()
                }
            )
            .is_err()
        );
        let mut forged = vec![0.; Layout::of(&p).columns];
        forged[0] = f64::NAN;
        assert!(verify(&p, &forged, &Layout::of(&p)).is_err());
    }

    #[test]
    fn partitioned_cantilever_respects_the_weakest_joint_capacity() {
        let bricks = links(3);
        let strong: Vec<_> = (1..3).map(|i| hinge(i, i + 1, i as f64, 0.005)).collect();
        let p = problem(&bricks, &strong, vec![0]);
        let certificate = certify_partitioned(
            &p,
            PartitionOptions {
                core_bodies: 1,
                max_sweeps: 1000,
                ..Default::default()
            },
        )
        .unwrap();
        assert!(certificate.loaded_joints > 0);
        let weak: Vec<_> = strong
            .into_iter()
            .map(|j| JointSpec {
                friction_torque: Some(1e-5),
                ..j
            })
            .collect();
        assert!(
            certify_partitioned(
                &problem(&bricks, &weak, vec![0]),
                PartitionOptions {
                    core_bodies: 1,
                    max_sweeps: 40,
                    ..Default::default()
                }
            )
            .is_err()
        );
    }

    #[test]
    fn partitioned_contact_forces_propagate_weight_and_eccentric_moments() {
        let mut demo = crate::demo::Demo::new();
        demo.add("3003", 4, 0., 0., 0.);
        demo.add("3003", 4, 4., 0., 0.);
        demo.add("3008", 4, 0., 1., 0.);
        let bricks = demo.bricks;
        let mut supports = Vec::new();
        for (i, b) in bricks[..2].iter().enumerate() {
            for (x, z) in [
                (b.bounds.min.x, b.bounds.min.z),
                (b.bounds.max.x, b.bounds.min.z),
                (b.bounds.max.x, b.bounds.max.z),
                (b.bounds.min.x, b.bounds.max.z),
            ] {
                supports.push((i, DVec3::new(x, 0., z)));
            }
        }
        let contacts = vec![
            Contact {
                lower: 0,
                upper: 2,
                patch: crate::Aabb::new(DVec3::new(0., 1., 0.), DVec3::new(2., 1., 1.)),
            },
            Contact {
                lower: 1,
                upper: 2,
                patch: crate::Aabb::new(DVec3::new(4., 1., 0.), DVec3::new(6., 1., 1.)),
            },
        ];
        let p = Problem {
            bricks: Bodies::all(&bricks),
            joints: &[],
            contacts,
            supports,
            anchored: vec![],
            masses: vec![0.01, 0.01, 0.1],
            force_tolerance: 1e-6,
            torque_tolerance: 1e-8,
        };
        certify(&p).unwrap();
        let certificate = certify_partitioned(
            &p,
            PartitionOptions {
                core_bodies: 1,
                max_sweeps: 1000,
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(certificate.loaded_joints, 0);
        assert!(certificate.solver.sweeps > 1);
        assert_eq!(certificate.solver.partitions, 3);
        let mut shifted = bricks.clone();
        shifted[2].translated(DVec3::X * 10.);
        let impossible = Problem {
            bricks: Bodies::all(&shifted),
            ..p
        };
        assert!(
            certify_partitioned(
                &impossible,
                PartitionOptions {
                    core_bodies: 1,
                    max_sweeps: 20,
                    ..Default::default()
                }
            )
            .is_err()
        );
    }

    #[test]
    fn partition_workspace_limits_never_fall_back_to_an_unbounded_solve() {
        let (bricks, joints) = chain(&[5., 4.25, 4., 4.25, 5.]);
        let p = problem(&bricks, &joints, vec![0, bricks.len() - 1]);
        let e = certify_partitioned(
            &p,
            PartitionOptions {
                core_bodies: 1,
                max_local_bodies: 1,
                ..Default::default()
            },
        )
        .unwrap_err();
        assert!(e.downcast_ref::<Inconclusive>().is_some());
        let c = certify_partitioned(
            &p,
            PartitionOptions {
                core_bodies: 6,
                max_local_columns: 20,
                max_sweeps: 1000,
                ..Default::default()
            },
        )
        .unwrap();
        assert!(c.solver.partitions > 1);
        assert!(c.solver.largest_local_kkt_dimension <= partitioned::MAX_LOCAL_KKT_DIMENSION);
    }
}
