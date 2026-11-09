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
    problem: &dyn ProblemInput,
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
        bodies.len() == plan.bodies.len() && bodies.iter().all(|i| *i < problem.size().bodies),
        "Invalid partition body indices"
    );
    let expected = plan
        .bodies
        .iter()
        .map(|&i| problem.body(i).map(|b| b.id as usize))
        .collect::<Result<Vec<_>>>()?;
    ensure!(
        plan.source_indices == expected,
        "Partition source identities changed"
    );
    let ids: BTreeSet<_> = expected.iter().map(|&i| i as u32).collect();
    for &j in &plan.joints {
        let j = problem.joint(j)?;
        ensure!(
            ids.contains(&j.a) && ids.contains(&j.b),
            "Partition lost an endpoint"
        );
    }
    for &c in &plan.contacts {
        let c = problem.contact(c)?;
        ensure!(
            bodies.contains(&c.lower) && bodies.contains(&c.upper),
            "Partition lost an endpoint"
        );
    }
    for &s in &plan.supports {
        ensure!(
            bodies.contains(&problem.support(s)?.0),
            "Invalid partition support indices"
        );
    }
    for &a in &plan.anchored {
        ensure!(
            bodies.contains(&problem.anchor(a)?),
            "Invalid partition support indices"
        );
    }
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

fn initialize_columns(
    problem: &dyn ProblemInput,
    layout: &Layout,
    workspace: &mut dyn SolverWorkspace,
) -> Result<()> {
    // A contact typically transfers a few bodies' weight, not the entire
    // island's. Scaling by total island weight makes consensus artificially
    // weaker as a model grows and stalls large compression-dominated scenes.
    let weight = input::force_unit(problem)?;
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
    for j in 0..problem.size().joints {
        let spec = problem.joint(j)?;
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
/// Canonical inputs and independently derived planner/verifier indexes use
/// fallible storage; only bounded local problems are materialized.
/// Resource/nonconvergence exits are typed Inconclusive, never a static proof.
pub fn certify_partitioned(problem: &Problem, options: PartitionOptions) -> Result<Certificate> {
    certify_partitioned_with_workspace(problem, options, &mut MemoryWorkspace::default())
}
pub fn certify_partitioned_with_workspace(
    problem: &Problem,
    options: PartitionOptions,
    workspace: &mut dyn SolverWorkspace,
) -> Result<Certificate> {
    certify_input_partitioned(problem, options, workspace)
}
pub(super) fn certify_input_partitioned(
    problem: &dyn ProblemInput,
    options: PartitionOptions,
    workspace: &mut dyn SolverWorkspace,
) -> Result<Certificate> {
    validate_problem_input(problem)?;
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
    let layout = Layout::of_input(problem);
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
    problem: &dyn ProblemInput,
    options: PartitionOptions,
    layout: &Layout,
    workspace: &mut dyn SolverWorkspace,
) -> Result<Certificate> {
    initialize_columns(problem, layout, workspace)?;
    let (largest_local_bodies, largest_local_kkt_dimension) =
        planner::plans(problem, options, layout, workspace)?;
    let force_unit = input::force_unit(problem)?;
    let verifier = WitnessVerifier::new(problem, workspace.verifier_state_mut())?;
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
            let local_input = input::materialize(
                problem,
                &plan.bodies,
                &plan.joints,
                &plan.contacts,
                &plan.supports,
                &plan.anchored,
            )?;
            let local = local_input.problem();
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
        match verifier.verify(
            problem,
            &*workspace,
            layout,
            None,
            workspace.verifier_state(),
        ) {
            Ok(mut certificate) => {
                certificate.solver = stats;
                return Ok(certificate);
            }
            Err(error) if error.downcast_ref::<Inconclusive>().is_some() => return Err(error),
            Err(error) => last_witness_error = format!("{error:#}"),
        }
        if matches!(sweep, 20 | 100 | 200 | 400) {
            stats.balance_correction_iterations +=
                polish::correct_workspace(problem, layout, workspace)?;
            match verifier.verify(
                problem,
                &*workspace,
                layout,
                None,
                workspace.verifier_state(),
            ) {
                Ok(mut certificate) => {
                    certificate.solver = stats;
                    return Ok(certificate);
                }
                Err(error) if error.downcast_ref::<Inconclusive>().is_some() => return Err(error),
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
