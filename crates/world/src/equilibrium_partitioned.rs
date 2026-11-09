//! Two-block consensus ADMM for a fixed-pose reaction problem. Computational
//! cuts never become anchors. A single reconciled witness must pass the same
//! independent physical checker used by a one-partition solve.
use super::*;
use std::collections::BTreeSet;

const MAX_WITNESS_COLUMNS: usize = 1_000_000;
const MAX_LOCAL_SOLVES: usize = 20_000;
const RELAXATION: f64 = 1.8;
// Upper bound on the original KKT dimension, before presolve. This caps even a
// maximally filled local factor's dimensions, not merely input nonzero counts.
pub(super) const MAX_LOCAL_KKT_DIMENSION: usize = 2048;
pub(super) fn kkt_bound(
    core: usize,
    joints: usize,
    contacts: usize,
    supports: usize,
    columns: usize,
) -> usize {
    columns
        .saturating_add(core.saturating_mul(6))
        .saturating_add(joints.saturating_mul(14))
        .saturating_add(contacts.saturating_mul(4))
        .saturating_add(supports)
}

#[derive(Clone, Copy, Debug)]
pub struct PartitionOptions {
    pub core_bodies: usize,
    pub max_local_bodies: usize,
    pub max_local_columns: usize,
    pub max_sweeps: usize,
    pub rho: f64,
}
impl Default for PartitionOptions {
    fn default() -> Self {
        Self {
            core_bodies: 256,
            max_local_bodies: 2048,
            max_local_columns: 32768,
            max_sweeps: 500,
            rho: 10.,
        }
    }
}

type Plan = PartitionPlan;

fn inconclusive(message: impl Into<String>) -> anyhow::Error {
    Inconclusive(message.into()).into()
}

fn load_plan(
    problem: &Problem,
    layout: &Layout,
    options: PartitionOptions,
    workspace: &dyn SolverWorkspace,
    index: usize,
) -> Result<Plan> {
    let plan = workspace.plan(index)?;
    ensure!(
        plan.core > 0
            && plan.core <= plan.bodies.len()
            && plan.bodies.len() <= options.max_local_bodies
            && plan.columns.len() <= options.max_local_columns,
        "Invalid partition plan dimensions"
    );
    let bodies: BTreeSet<_> = plan.bodies.iter().copied().collect();
    ensure!(
        bodies.len() == plan.bodies.len() && bodies.iter().all(|i| *i < problem.bricks.len()),
        "Invalid partition body indices"
    );
    ensure!(
        plan.source_indices.len() == plan.bodies.len()
            && plan
                .source_indices
                .iter()
                .zip(&plan.bodies)
                .all(|(&source, &i)| source
                    == problem.bricks.indices.map_or(i, |indices| indices[i])),
        "Partition source identities changed"
    );
    let ids: BTreeSet<_> = plan.bodies.iter().map(|&i| problem.bricks[i].id).collect();
    ensure!(
        plan.joints.iter().all(|&j| problem
            .joints
            .get(j)
            .is_some_and(|j| ids.contains(&j.a) && ids.contains(&j.b)))
            && plan.contacts.iter().all(|&c| problem
                .contacts
                .get(c)
                .is_some_and(|c| bodies.contains(&c.lower) && bodies.contains(&c.upper))),
        "Partition lost an endpoint"
    );
    ensure!(
        plan.supports.iter().all(|&k| problem
            .supports
            .get(k)
            .is_some_and(|v| bodies.contains(&v.0)))
            && plan
                .anchored
                .iter()
                .all(|&k| problem.anchored.get(k).is_some_and(|i| bodies.contains(i))),
        "Invalid partition support indices"
    );
    let columns = plan.joints.len() * 6
        + plan.contacts.len() * 4
        + plan.supports.len()
        + plan.anchored.len() * 6;
    ensure!(
        columns == plan.columns.len()
            && plan.columns.iter().all(|c| *c < layout.columns)
            && kkt_bound(
                plan.core,
                plan.joints.len(),
                plan.contacts.len(),
                plan.supports.len(),
                columns
            ) <= MAX_LOCAL_KKT_DIMENSION,
        "Invalid partition column/KKT mapping"
    );
    ensure!(
        plan.boundary
            .iter()
            .all(|b| plan.columns.get(b.local) == Some(&b.global)
                && [b.dual, b.value, b.input].iter().all(|v| v.is_finite())),
        "Invalid partition boundary record"
    );
    Ok(plan)
}

fn plans(
    problem: &Problem,
    options: PartitionOptions,
    layout: &Layout,
    workspace: &mut dyn SolverWorkspace,
) -> Result<(usize, usize)> {
    let n = problem.bricks.len();
    let index: HashMap<_, _> = problem
        .bricks
        .iter()
        .enumerate()
        .map(|(i, b)| (b.id, i))
        .collect();
    let mut order: Vec<_> = (0..n).collect();
    let bounds = problem
        .bricks
        .iter()
        .map(|b| b.bounds)
        .reduce(crate::Aabb::union)
        .expect("nonempty validated problem");
    let side = bounds.size().max_element().max(1e-9);
    let spatial_key = |i: usize| {
        let p = (problem.bricks[i].bounds.center() - bounds.min) / side;
        let q = [p.x, p.y, p.z].map(|v| (v.clamp(0., 1.) * ((1u64 << 21) - 1) as f64) as u64);
        let mut key = 0;
        for bit in 0..21 {
            for axis in 0..3 {
                key |= ((q[axis] >> bit) & 1) << (3 * bit + axis);
            }
        }
        (key, problem.bricks[i].id)
    };
    order.sort_unstable_by_key(|&i| spatial_key(i));
    // Pack each core against its actual incident workload. A single high-degree
    // body must not force every otherwise cheap partition down to the same tiny
    // uniform size.
    let mut joints_at = vec![Vec::new(); n];
    let mut contacts_at = vec![Vec::new(); n];
    let mut support_count = vec![0usize; n];
    let mut anchor_count = vec![0usize; n];
    for (j, spec) in problem.joints.iter().enumerate() {
        joints_at[index[&spec.a]].push(j);
        joints_at[index[&spec.b]].push(j);
    }
    for (k, c) in problem.contacts.iter().enumerate() {
        contacts_at[c.lower].push(k);
        contacts_at[c.upper].push(k);
    }
    for &(i, _) in &problem.supports {
        support_count[i] += 1;
    }
    for &i in &problem.anchored {
        anchor_count[i] += 1;
    }
    let mut cores = Vec::new();
    let mut core = Vec::new();
    let mut js = BTreeSet::new();
    let mut cs = BTreeSet::new();
    let mut bs = BTreeSet::new();
    let mut sc = 0;
    let mut ac = 0;
    for i in order {
        loop {
            bs.insert(i);
            for &j in &joints_at[i] {
                js.insert(j);
                let spec = &problem.joints[j];
                bs.extend([index[&spec.a], index[&spec.b]]);
            }
            for &k in &contacts_at[i] {
                cs.insert(k);
                let c = problem.contacts[k];
                bs.extend([c.lower, c.upper]);
            }
            let columns =
                js.len() * 6 + cs.len() * 4 + sc + support_count[i] + (ac + anchor_count[i]) * 6;
            let fits = bs.len() <= options.max_local_bodies
                && columns <= options.max_local_columns
                && kkt_bound(
                    core.len() + 1,
                    js.len(),
                    cs.len(),
                    sc + support_count[i],
                    columns,
                ) <= MAX_LOCAL_KKT_DIMENSION;
            if !fits {
                if core.is_empty() {
                    return Err(inconclusive(
                        "one body's incident interface exceeds the local workspace budget",
                    ));
                }
                cores.push(std::mem::take(&mut core));
                js.clear();
                cs.clear();
                bs.clear();
                sc = 0;
                ac = 0;
                continue;
            }
            core.push(i);
            sc += support_count[i];
            ac += anchor_count[i];
            break;
        }
        if core.len() == options.core_bodies {
            cores.push(std::mem::take(&mut core));
            js.clear();
            cs.clear();
            bs.clear();
            sc = 0;
            ac = 0;
        }
    }
    if !core.is_empty() {
        cores.push(core);
    }
    let mut owner = vec![0; n];
    for (p, core) in cores.iter().enumerate() {
        for &i in core {
            owner[i] = p;
        }
    }
    for (j, spec) in problem.joints.iter().enumerate() {
        let (a, b) = (owner[index[&spec.a]], owner[index[&spec.b]]);
        if a != b {
            for k in 0..6 {
                let col = j * 6 + k;
                let mut state = workspace.column(col)?;
                state.copies = 2;
                workspace.put_column(col, state)?;
            }
        }
    }
    for (k, c) in problem.contacts.iter().enumerate() {
        let (a, b) = (owner[c.lower], owner[c.upper]);
        if a != b {
            for i in 0..4 {
                let col = layout.contact + k * 4 + i;
                let mut state = workspace.column(col)?;
                state.copies = 2;
                workspace.put_column(col, state)?;
            }
        }
    }
    let (mut largest_bodies, mut largest_kkt) = (0, 0);
    for (p, core) in cores.into_iter().enumerate() {
        // Materialize only this core's bounded incident workload. Reusing the
        // planner's own incidence preserves canonical edge order without using
        // the independent verifier's separately derived incidence table.
        let joints: BTreeSet<_> = core
            .iter()
            .flat_map(|&i| joints_at[i].iter().copied())
            .collect();
        let contacts: BTreeSet<_> = core
            .iter()
            .flat_map(|&i| contacts_at[i].iter().copied())
            .collect();
        let mut plan = Plan {
            core: core.len(),
            bodies: core,
            source_indices: vec![],
            joints: joints.into_iter().collect(),
            contacts: contacts.into_iter().collect(),
            supports: problem
                .supports
                .iter()
                .enumerate()
                .filter(|(_, v)| owner[v.0] == p)
                .map(|(k, _)| k)
                .collect(),
            anchored: problem
                .anchored
                .iter()
                .enumerate()
                .filter(|(_, i)| owner[**i] == p)
                .map(|(k, _)| k)
                .collect(),
            columns: vec![],
            boundary: vec![],
        };
        let mut extra = BTreeSet::new();
        let core: BTreeSet<_> = plan.bodies.iter().copied().collect();
        for &j in &plan.joints {
            let spec = &problem.joints[j];
            extra.extend([index[&spec.a], index[&spec.b]]);
            plan.columns.extend((0..6).map(|k| j * 6 + k));
        }
        for &k in &plan.contacts {
            let c = &problem.contacts[k];
            extra.extend([c.lower, c.upper]);
            plan.columns
                .extend((0..4).map(|i| layout.contact + k * 4 + i));
        }
        for &k in &plan.supports {
            plan.columns.push(layout.support + k);
        }
        for &k in &plan.anchored {
            plan.columns
                .extend((0..6).map(|i| layout.anchor + k * 6 + i));
        }
        plan.bodies
            .extend(extra.into_iter().filter(|i| !core.contains(i)));
        if plan.bodies.len() > options.max_local_bodies
            || plan.columns.len() > options.max_local_columns
        {
            return Err(inconclusive(
                "local partition/halo exceeds its body or reaction-column budget",
            ));
        }
        if kkt_bound(
            plan.core,
            plan.joints.len(),
            plan.contacts.len(),
            plan.supports.len(),
            plan.columns.len(),
        ) > MAX_LOCAL_KKT_DIMENSION
        {
            return Err(inconclusive(
                "local KKT dimension exceeds its factorization budget",
            ));
        }
        plan.source_indices = plan
            .bodies
            .iter()
            .map(|&i| problem.bricks.indices.map_or(i, |indices| indices[i]))
            .collect();
        for (local, &global) in plan.columns.iter().enumerate() {
            if workspace.column(global)?.copies == 2 {
                plan.boundary.push(Boundary {
                    local,
                    global,
                    dual: 0.,
                    value: 0.,
                    input: 0.,
                });
            }
        }
        largest_bodies = largest_bodies.max(plan.bodies.len());
        largest_kkt = largest_kkt.max(kkt_bound(
            plan.core,
            plan.joints.len(),
            plan.contacts.len(),
            plan.supports.len(),
            plan.columns.len(),
        ));
        workspace.push_plan(plan)?;
    }
    Ok((largest_bodies, largest_kkt))
}

fn initialize_columns(
    problem: &Problem,
    layout: &Layout,
    workspace: &mut dyn SolverWorkspace,
) -> Result<()> {
    // A contact typically transfers a few bodies' weight, not the entire
    // island's. Scaling by total island weight makes consensus artificially
    // weaker as a model grows and stalls large compression-dominated scenes.
    let weight = representative_force(problem);
    workspace.reset(layout.columns)?;
    for column in 0..layout.columns {
        workspace.put_column(
            column,
            ReactionColumn {
                scale: 1. / weight,
                copies: 1,
                ..Default::default()
            },
        )?;
    }
    for (j, spec) in problem.joints.iter().enumerate() {
        let force = [spec.break_force, spec.axial_force]
            .into_iter()
            .flatten()
            .filter(|v| *v > 0.)
            .fold(f64::INFINITY, f64::min);
        let torque = [spec.break_torque, spec.friction_torque]
            .into_iter()
            .flatten()
            .filter(|v| *v > 0.)
            .fold(f64::INFINITY, f64::min);
        let f = if force.is_finite() {
            force.min(weight).max(1e-3)
        } else {
            weight
        };
        let t = if torque.is_finite() {
            torque.min(weight * 0.008).max(1e-5)
        } else {
            (weight * 0.008).max(1e-5)
        };
        for k in 0..6 {
            let column = j * 6 + k;
            let mut state = workspace.column(column)?;
            state.scale = 1. / if k < 3 { f } else { t };
            workspace.put_column(column, state)?;
        }
    }
    Ok(())
}

/// A bounded sequence of local solves over one physical problem. The default
/// workspace is resident; shared-native callers supply paged plan/reaction storage.
/// Canonical inputs and planner/verifier indexes still reside in memory.
/// Resource/nonconvergence exits are typed Inconclusive, never a static proof.
pub fn certify_partitioned(problem: &Problem, options: PartitionOptions) -> Result<Certificate> {
    certify_partitioned_with_workspace(problem, options, &mut MemoryWorkspace::default())
}
pub fn certify_partitioned_with_workspace(
    problem: &Problem,
    options: PartitionOptions,
    workspace: &mut dyn SolverWorkspace,
) -> Result<Certificate> {
    validate_problem(problem)?;
    ensure!(
        (1..=512).contains(&options.core_bodies)
            && options.max_local_bodies >= options.core_bodies
            && options.max_local_bodies <= 8192
            && (1..=65536).contains(&options.max_local_columns)
            && options.max_sweeps <= 2000
            && options.rho.is_finite()
            && options.rho > 0.
            && options.rho <= 1e6,
        "Invalid partition solver options"
    );
    let layout = Layout::of(problem);
    if layout.columns > MAX_WITNESS_COLUMNS {
        return Err(inconclusive(
            "global reaction witness exceeds its configured column budget",
        ));
    }
    run(problem, options, &layout, workspace).map_err(|error| {
        if error.downcast_ref::<Inconclusive>().is_some() {
            error
        } else {
            inconclusive(format!("coupled workspace/solve failed: {error:#}"))
        }
    })
}
fn run(
    problem: &Problem,
    options: PartitionOptions,
    layout: &Layout,
    workspace: &mut dyn SolverWorkspace,
) -> Result<Certificate> {
    initialize_columns(problem, layout, workspace)?;
    let (largest_local_bodies, largest_local_kkt_dimension) =
        plans(problem, options, layout, workspace)?;
    let force_unit = representative_force(problem);
    let verifier = WitnessVerifier::new(problem)?;
    let mut rho = options.rho;
    let mut stats = SolverStats {
        partitions: workspace.plan_len(),
        largest_local_bodies,
        largest_local_kkt_dimension,
        spilled_partition_plans: if workspace.spilled() {
            workspace.plan_len()
        } else {
            0
        },
        spilled_reaction_columns: if workspace.spilled() {
            layout.columns
        } else {
            0
        },
        ..Default::default()
    };
    let mut last_witness_error = "no sweep completed".to_owned();
    for sweep in 1..=options.max_sweeps {
        for col in 0..layout.columns {
            let mut state = workspace.column(col)?;
            state.sum = 0.;
            workspace.put_column(col, state)?;
        }
        let mut local_failure = None;
        // All plans see the preceding sweep's z. Even though executed serially,
        // this is one separable ADMM block, not unsupported multi-block updates.
        for partition in 0..workspace.plan_len() {
            let mut plan = load_plan(problem, layout, options, workspace, partition)?;
            if stats.local_solves >= MAX_LOCAL_SOLVES {
                return Err(inconclusive(format!(
                    "local-solve work budget exceeded; last witness check: {last_witness_error}"
                )));
            }
            stats.local_solves += 1;
            let local_index: HashMap<_, _> = plan
                .bodies
                .iter()
                .enumerate()
                .map(|(i, &global)| (global, i))
                .collect();
            let joints: Vec<_> = plan
                .joints
                .iter()
                .map(|&j| problem.joints[j].clone())
                .collect();
            let local = Problem {
                bricks: Bodies {
                    source: problem.bricks.source,
                    indices: Some(&plan.source_indices),
                },
                joints: &joints,
                contacts: plan
                    .contacts
                    .iter()
                    .map(|&k| {
                        let c = problem.contacts[k];
                        Contact {
                            lower: local_index[&c.lower],
                            upper: local_index[&c.upper],
                            patch: c.patch,
                        }
                    })
                    .collect(),
                supports: plan
                    .supports
                    .iter()
                    .map(|&k| {
                        let (i, p) = problem.supports[k];
                        (local_index[&i], p)
                    })
                    .collect(),
                anchored: plan
                    .anchored
                    .iter()
                    .map(|&k| local_index[&problem.anchored[k]])
                    .collect(),
                masses: plan.bodies.iter().map(|&i| problem.masses[i]).collect(),
                force_tolerance: problem.force_tolerance,
                torque_tolerance: problem.torque_tolerance,
            };
            let owned: Vec<_> = (0..plan.bodies.len()).map(|i| i < plan.core).collect();
            for b in &mut plan.boundary {
                b.input = workspace.column(b.global)?.consensus - b.dual;
            }
            let penalties: Vec<_> = plan
                .boundary
                .iter()
                .map(|b| {
                    Ok(Penalty {
                        column: b.local,
                        target: b.input,
                        diagonal: rho * workspace.column(b.global)?.scale.powi(2),
                    })
                })
                .collect::<Result<_>>()?;
            // Every physical objective term has total weight one, including
            // duplicated six-component boundary wrenches and corner reactions.
            let weights: Vec<_> = plan
                .columns
                .iter()
                .map(|&g| Ok(1. / workspace.column(g)?.copies as f64))
                .collect::<Result<_>>()?;
            let (status, x, local_layout) = solve_local(
                &local,
                false,
                Some(&owned),
                &penalties,
                Some(&weights),
                force_unit,
            )
            .map_err(|e| inconclusive(format!("local subproblem failed: {e}")))?;
            if x.len() != plan.columns.len() || x.iter().any(|v| !v.is_finite()) {
                return Err(inconclusive(
                    "local subproblem returned an invalid reaction vector",
                ));
            }
            if let Err(error) = verify_subset(&local, &x, &local_layout, Some(&owned)) {
                if !matches!(status, SolverStatus::Solved | SolverStatus::AlmostSolved) {
                    local_failure = Some(format!(
                        "partition {partition} at sweep {sweep}: {status:?}; {error:#}"
                    ));
                }
            }
            for (i, &g) in plan.columns.iter().enumerate() {
                let mut state = workspace.column(g)?;
                if state.copies == 1 {
                    state.witness = x[i];
                } else {
                    state.sum += x[i];
                }
                workspace.put_column(g, state)?;
            }
            for b in &mut plan.boundary {
                b.value = x[b.local];
            }
            workspace.put_plan(partition, plan)?;
            // Local matrices, solver factorization and full x die here before
            // the next partition is assembled; no all-partition solver cache.
        }
        let mut dual_residual: f64 = 0.;
        for g in 0..layout.columns {
            let mut state = workspace.column(g)?;
            if state.copies == 2 {
                let mean = state.sum / 2.;
                let next = RELAXATION * mean + (1. - RELAXATION) * state.consensus;
                dual_residual =
                    dual_residual.max(rho * state.scale * (next - state.consensus).abs());
                state.consensus = next;
                state.witness = mean;
            }
            state.sum = 0.;
            workspace.put_column(g, state)?;
        }
        let mut primal: f64 = 0.;
        let mut force: f64 = 0.;
        let mut torque: f64 = 0.;
        for partition in 0..workspace.plan_len() {
            let mut plan = load_plan(problem, layout, options, workspace, partition)?;
            for b in &mut plan.boundary {
                let mut state = workspace.column(b.global)?;
                let r = b.value - state.witness;
                primal = primal.max(state.scale * r.abs());
                if b.global < layout.contact && b.global % 6 >= 3 {
                    torque = torque.max(r.abs());
                } else {
                    force = force.max(r.abs());
                }
                b.dual += RELAXATION * r;
                state.sum += b.dual;
                workspace.put_column(b.global, state)?;
            }
            workspace.put_plan(partition, plan)?;
        }
        // Numerical centering preserves the zero-sum dual subspace. The physical
        // witness is the convex mean; overrelaxed z need not be cone-admissible.
        for partition in 0..workspace.plan_len() {
            let mut plan = load_plan(problem, layout, options, workspace, partition)?;
            for b in &mut plan.boundary {
                b.dual -= workspace.column(b.global)?.sum / 2.;
                if !b.dual.is_finite() {
                    return Err(inconclusive("interface dual update overflowed"));
                }
            }
            workspace.put_plan(partition, plan)?;
        }
        stats.sweeps = sweep;
        stats.interface_force_residual_newtons = force;
        stats.interface_torque_residual_newton_meters = torque;
        // Averages keep the convex physical interface cones; the one canonical
        // witness is nevertheless rechecked independently, with *global island*
        // tolerances, all body balances, capacities and real application points.
        match verifier.verify(problem, &*workspace, layout, None) {
            Ok(mut certificate) => {
                certificate.solver = stats;
                return Ok(certificate);
            }
            Err(error) => last_witness_error = format!("{error:#}"),
        }
        if matches!(sweep, 20 | 100 | 200 | 400) {
            stats.balance_correction_iterations +=
                polish::correct_workspace(problem, layout, workspace)?;
            match verifier.verify(problem, &*workspace, layout, None) {
                Ok(mut certificate) => {
                    certificate.solver = stats;
                    return Ok(certificate);
                }
                Err(error) => last_witness_error = format!("balance correction: {error:#}"),
            }
        }
        if let Some(failure) = local_failure {
            return Err(inconclusive(format!(
                "local solver stopped in {failure}; no reconciled physical witness passed"
            )));
        }
        if sweep % 20 == 0 {
            let next = if primal > 10. * dual_residual {
                (rho * 2.).min(1e6)
            } else if dual_residual > 10. * primal {
                (rho / 2.).max(1e-6)
            } else {
                rho
            };
            if next != rho {
                for partition in 0..workspace.plan_len() {
                    let mut plan = load_plan(problem, layout, options, workspace, partition)?;
                    for b in &mut plan.boundary {
                        b.dual *= rho / next;
                    }
                    workspace.put_plan(partition, plan)?;
                }
                rho = next;
            }
        }
    }
    Err(inconclusive(format!(
        "interface consensus exceeded {} sweeps without a verified force-and-moment certificate; last witness check: {last_witness_error}",
        options.max_sweeps
    )))
}
