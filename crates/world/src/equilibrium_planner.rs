//! Bounded core packing over independently stored canonical incidence/owners.
use super::*;
use crate::validation::{
    ValidationRecord as Record, ValidationState,
    working_state::{key, required, unavailable},
};
use std::collections::BTreeSet;

fn owner(state: &dyn ValidationState, body: usize) -> Result<usize> {
    match required(state, &key(0, &[body]))? {
        Record::Index(p) => Ok(p),
        _ => Err(unavailable("partition owner is absent")),
    }
}
fn incidence_count(state: &dyn ValidationState, body: usize) -> Result<usize> {
    match required(state, &key(4, &[body]))? {
        Record::Index(n) => Ok(n),
        _ => Err(unavailable("planner incidence count is absent")),
    }
}
fn add_incidence(
    state: &mut dyn ValidationState,
    body: usize,
    kind: usize,
    i: usize,
) -> Result<()> {
    let n = incidence_count(state, body)?
        .checked_add(1)
        .ok_or_else(|| unavailable("planner incidence count overflow"))?;
    state.put(key(2, &[body, kind, i]), Record::Index((i << 2) | kind))?;
    state.put(key(4, &[body]), Record::Index(n))
}
fn fits(
    core: usize,
    js: &BTreeSet<usize>,
    cs: &BTreeSet<usize>,
    bs: &BTreeSet<usize>,
    ss: &BTreeSet<usize>,
    aa: &BTreeSet<usize>,
    options: PartitionOptions,
) -> bool {
    let columns = js.len() * 6 + cs.len() * 4 + ss.len() + aa.len() * 6;
    bs.len() <= options.max_local_bodies
        && columns <= options.max_local_columns
        && partitioned::kkt_bound(core, js.len(), cs.len(), ss.len(), columns)
            <= partitioned::MAX_LOCAL_KKT_DIMENSION
}
fn incident(
    problem: &dyn ProblemInput,
    state: &dyn ValidationState,
    body: usize,
    core: usize,
    js: &mut BTreeSet<usize>,
    cs: &mut BTreeSet<usize>,
    bs: &mut BTreeSet<usize>,
    ss: &mut BTreeSet<usize>,
    aa: &mut BTreeSet<usize>,
    options: PartitionOptions,
) -> Result<bool> {
    bs.insert(body);
    let prefix = key(2, &[body]);
    let mut after = None;
    let mut seen = 0;
    loop {
        let page = state.page(&prefix, after.as_deref(), 128)?;
        if page.is_empty() {
            break;
        }
        for (k, value) in page {
            seen += 1;
            let Record::Index(entry) = value else {
                return Err(unavailable("invalid planner incidence"));
            };
            let i = entry >> 2;
            match entry & 3 {
                0 => {
                    js.insert(i);
                    let j = problem.joint(i)?;
                    bs.extend([
                        input::body_index(problem, j.a)?,
                        input::body_index(problem, j.b)?,
                    ]);
                }
                1 => {
                    cs.insert(i);
                    let c = problem.contact(i)?;
                    bs.extend([c.lower, c.upper]);
                }
                2 => {
                    ss.insert(i);
                }
                3 => {
                    aa.insert(i);
                }
                _ => unreachable!(),
            }
            if !fits(core, js, cs, bs, ss, aa, options) {
                return Ok(false);
            }
            after = Some(k);
        }
    }
    if seen != incidence_count(state, body)? {
        return Err(unavailable("planner incidence lost or gained records"));
    }
    Ok(true)
}
pub(super) fn plans(
    problem: &dyn ProblemInput,
    options: PartitionOptions,
    layout: &Layout,
    workspace: &mut dyn SolverWorkspace,
) -> Result<(usize, usize)> {
    let n = problem.size().bodies;
    let mut bounds = problem.body(0)?.bounds;
    for i in 1..n {
        bounds = bounds.union(problem.body(i)?.bounds);
    }
    let side = bounds.size().max_element().max(1e-9);
    let state = workspace.planner_state_mut();
    state.clear()?;
    for i in 0..n {
        let body = problem.body(i)?;
        let p = (body.bounds.center() - bounds.min) / side;
        let q = [p.x, p.y, p.z].map(|v| (v.clamp(0., 1.) * ((1u64 << 21) - 1) as f64) as u64);
        let mut morton = 0u64;
        for bit in 0..21 {
            for axis in 0..3 {
                morton |= ((q[axis] >> bit) & 1) << (3 * bit + axis);
            }
        }
        let mut k = vec![1];
        k.extend_from_slice(&morton.to_be_bytes());
        k.extend_from_slice(&body.id.to_be_bytes());
        state.put(k, Record::Index(i))?;
        state.put(key(4, &[i]), Record::Index(0))?;
    }
    for i in 0..problem.size().joints {
        let j = problem.joint(i)?;
        for b in [
            input::body_index(problem, j.a)?,
            input::body_index(problem, j.b)?,
        ] {
            add_incidence(state, b, 0, i)?;
        }
    }
    for i in 0..problem.size().contacts {
        let c = problem.contact(i)?;
        for b in [c.lower, c.upper] {
            add_incidence(state, b, 1, i)?;
        }
    }
    for i in 0..problem.size().supports {
        add_incidence(state, problem.support(i)?.0, 2, i)?;
    }
    for i in 0..problem.size().anchors {
        add_incidence(state, problem.anchor(i)?, 3, i)?;
    }
    let (mut js, mut cs, mut bs, mut ss, mut aa) = (
        BTreeSet::new(),
        BTreeSet::new(),
        BTreeSet::new(),
        BTreeSet::new(),
        BTreeSet::new(),
    );
    let mut core = Vec::new();
    let mut parts = 0;
    let mut seen = 0;
    let mut after = None;
    loop {
        let page = state.page(&[1], after.as_deref(), 128)?;
        if page.is_empty() {
            break;
        }
        for (k, value) in page {
            let Record::Index(i) = value else {
                return Err(unavailable("invalid Morton order"));
            };
            loop {
                if !incident(
                    problem,
                    state,
                    i,
                    core.len() + 1,
                    &mut js,
                    &mut cs,
                    &mut bs,
                    &mut ss,
                    &mut aa,
                    options,
                )? {
                    if core.is_empty() {
                        return Err(Inconclusive(
                            "one body's incident interface exceeds the local workspace budget"
                                .into(),
                        )
                        .into());
                    }
                    state.put(key(3, &[parts]), Record::Indices(std::mem::take(&mut core)))?;
                    parts += 1;
                    js.clear();
                    cs.clear();
                    bs.clear();
                    ss.clear();
                    aa.clear();
                    continue;
                }
                state.put(key(0, &[i]), Record::Index(parts))?;
                core.push(i);
                seen += 1;
                break;
            }
            if core.len() == options.core_bodies {
                state.put(key(3, &[parts]), Record::Indices(std::mem::take(&mut core)))?;
                parts += 1;
                js.clear();
                cs.clear();
                bs.clear();
                ss.clear();
                aa.clear();
            }
            after = Some(k);
        }
    }
    if !core.is_empty() {
        state.put(key(3, &[parts]), Record::Indices(core))?;
        parts += 1;
    }
    if seen != n {
        return Err(unavailable("Morton ordering lost or duplicated bodies"));
    }
    for j in 0..problem.size().joints {
        let spec = problem.joint(j)?;
        if owner(
            workspace.planner_state(),
            input::body_index(problem, spec.a)?,
        )? != owner(
            workspace.planner_state(),
            input::body_index(problem, spec.b)?,
        )? {
            for k in 0..6 {
                let c = j * 6 + k;
                let mut row = workspace.column(c)?;
                row.copies = 2;
                workspace.put_column(c, row)?;
            }
        }
    }
    for k in 0..problem.size().contacts {
        let c = problem.contact(k)?;
        if owner(workspace.planner_state(), c.lower)? != owner(workspace.planner_state(), c.upper)?
        {
            for i in 0..4 {
                let c = layout.contact + k * 4 + i;
                let mut row = workspace.column(c)?;
                row.copies = 2;
                workspace.put_column(c, row)?;
            }
        }
    }
    let (mut largest, mut kkt) = (0, 0);
    for part in 0..parts {
        let Record::Indices(core) = required(workspace.planner_state(), &key(3, &[part]))? else {
            return Err(unavailable("partition core is absent"));
        };
        if core.is_empty() || core.len() > options.core_bodies {
            return Err(unavailable("invalid partition core dimensions"));
        }
        js.clear();
        cs.clear();
        bs.clear();
        ss.clear();
        aa.clear();
        for &i in &core {
            if owner(workspace.planner_state(), i)? != part
                || !incident(
                    problem,
                    workspace.planner_state(),
                    i,
                    core.len(),
                    &mut js,
                    &mut cs,
                    &mut bs,
                    &mut ss,
                    &mut aa,
                    options,
                )?
            {
                return Err(unavailable("partition incidence/owner changed"));
            }
        }
        let core_set: BTreeSet<_> = core.iter().copied().collect();
        let mut plan = PartitionPlan {
            core: core.len(),
            bodies: core,
            source_indices: vec![],
            joints: js.iter().copied().collect(),
            contacts: cs.iter().copied().collect(),
            supports: ss.iter().copied().collect(),
            anchored: aa.iter().copied().collect(),
            columns: vec![],
            boundary: vec![],
        };
        plan.bodies
            .extend(bs.iter().copied().filter(|i| !core_set.contains(i)));
        for &i in &plan.bodies {
            plan.source_indices.push(problem.body(i)?.id as usize);
        }
        for &j in &plan.joints {
            plan.columns.extend((0..6).map(|k| j * 6 + k));
        }
        for &c in &plan.contacts {
            plan.columns
                .extend((0..4).map(|k| layout.contact + c * 4 + k));
        }
        for &s in &plan.supports {
            plan.columns.push(layout.support + s);
        }
        for &a in &plan.anchored {
            plan.columns
                .extend((0..6).map(|k| layout.anchor + a * 6 + k));
        }
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
        largest = largest.max(plan.bodies.len());
        kkt = kkt.max(partitioned::kkt_bound(
            plan.core,
            plan.joints.len(),
            plan.contacts.len(),
            plan.supports.len(),
            plan.columns.len(),
        ));
        workspace.push_plan(plan)?;
    }
    Ok((largest, kkt))
}
