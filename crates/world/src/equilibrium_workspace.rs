//! Storage adapters for the one coupled algorithm. Native callers may spill
//! records; browser/local callers keep them resident. Neither backend certifies.
use super::*;

pub trait ReactionRead {
    fn reaction_len(&self) -> usize;
    fn read_reactions(&self, start: usize, out: &mut [f64]) -> Result<()>;
    fn reaction(&self, index: usize) -> Result<f64> {
        let mut value = [0.];
        self.read_reactions(index, &mut value)?;
        Ok(value[0])
    }
}
pub trait ReactionWrite: ReactionRead {
    fn set_reaction(&mut self, index: usize, value: f64) -> Result<()>;
}
impl ReactionRead for [f64] {
    fn reaction_len(&self) -> usize {
        self.len()
    }
    fn read_reactions(&self, start: usize, out: &mut [f64]) -> Result<()> {
        let end = start
            .checked_add(out.len())
            .context("Reaction range overflow")?;
        out.copy_from_slice(self.get(start..end).context("Reaction range is absent")?);
        Ok(())
    }
}
impl ReactionWrite for [f64] {
    fn set_reaction(&mut self, index: usize, value: f64) -> Result<()> {
        *self.get_mut(index).context("Reaction is absent")? = value;
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct ReactionColumn {
    pub consensus: f64,
    pub witness: f64,
    pub sum: f64,
    pub scale: f64,
    pub copies: u8,
}
impl ReactionColumn {
    pub fn valid(self) -> bool {
        [self.consensus, self.witness, self.sum, self.scale]
            .iter()
            .all(|v| v.is_finite())
            && self.copies <= 2
    }
}
#[derive(Clone, Serialize, Deserialize)]
pub(super) struct Boundary {
    pub local: usize,
    pub global: usize,
    pub dual: f64,
    pub value: f64,
    pub input: f64,
}
#[derive(Clone, Serialize, Deserialize)]
pub struct PartitionPlan {
    pub(super) bodies: Vec<usize>,
    pub(super) source_indices: Vec<usize>,
    pub(super) core: usize,
    pub(super) joints: Vec<usize>,
    pub(super) contacts: Vec<usize>,
    pub(super) supports: Vec<usize>,
    pub(super) anchored: Vec<usize>,
    pub(super) columns: Vec<usize>,
    pub(super) boundary: Vec<Boundary>,
}

/// Reset per physical island. Missing/corrupt records must error, never truncate
/// a plan, invent an anchor or return an incomplete reaction vector.
pub trait SolverWorkspace: ReactionWrite {
    fn new_state(&self) -> Result<Box<dyn crate::validation::ValidationState>>;
    fn planner_state(&self) -> &dyn crate::validation::ValidationState;
    fn planner_state_mut(&mut self) -> &mut dyn crate::validation::ValidationState;
    fn verifier_state(&self) -> &dyn crate::validation::ValidationState;
    fn verifier_state_mut(&mut self) -> &mut dyn crate::validation::ValidationState;
    /// Independent of per-island numerical resets. Component membership and
    /// compression state have disjoint stores so indexed body reads can borrow
    /// the former while propagation updates the latter.
    fn validation_state(
        &mut self,
    ) -> (
        &mut dyn crate::validation::ValidationState,
        &mut dyn crate::validation::ValidationState,
    );
    fn reset(&mut self, columns: usize) -> Result<()>;
    fn plan_len(&self) -> usize;
    fn push_plan(&mut self, plan: PartitionPlan) -> Result<()>;
    fn plan(&self, index: usize) -> Result<PartitionPlan>;
    fn put_plan(&mut self, index: usize, plan: PartitionPlan) -> Result<()>;
    fn column(&self, index: usize) -> Result<ReactionColumn>;
    fn put_column(&mut self, index: usize, column: ReactionColumn) -> Result<()>;
    fn spilled(&self) -> bool {
        false
    }
}
#[derive(Default)]
pub struct MemoryWorkspace {
    plans: Vec<PartitionPlan>,
    columns: Vec<ReactionColumn>,
    components: crate::validation::MemoryValidationState,
    compression: crate::validation::MemoryValidationState,
    planner: crate::validation::MemoryValidationState,
    verifier: crate::validation::MemoryValidationState,
}
impl ReactionRead for MemoryWorkspace {
    fn reaction_len(&self) -> usize {
        self.columns.len()
    }
    fn read_reactions(&self, start: usize, out: &mut [f64]) -> Result<()> {
        let end = start
            .checked_add(out.len())
            .context("Reaction range overflow")?;
        for (value, column) in out.iter_mut().zip(
            self.columns
                .get(start..end)
                .context("Reaction range is absent")?,
        ) {
            *value = column.witness;
        }
        Ok(())
    }
}
impl ReactionWrite for MemoryWorkspace {
    fn set_reaction(&mut self, index: usize, value: f64) -> Result<()> {
        self.columns
            .get_mut(index)
            .context("Reaction is absent")?
            .witness = value;
        Ok(())
    }
}
impl SolverWorkspace for MemoryWorkspace {
    fn new_state(&self) -> Result<Box<dyn crate::validation::ValidationState>> {
        Ok(Box::new(crate::validation::MemoryValidationState::default()))
    }
    fn planner_state(&self) -> &dyn crate::validation::ValidationState {
        &self.planner
    }
    fn planner_state_mut(&mut self) -> &mut dyn crate::validation::ValidationState {
        &mut self.planner
    }
    fn verifier_state(&self) -> &dyn crate::validation::ValidationState {
        &self.verifier
    }
    fn verifier_state_mut(&mut self) -> &mut dyn crate::validation::ValidationState {
        &mut self.verifier
    }
    fn validation_state(
        &mut self,
    ) -> (
        &mut dyn crate::validation::ValidationState,
        &mut dyn crate::validation::ValidationState,
    ) {
        (&mut self.components, &mut self.compression)
    }
    fn reset(&mut self, columns: usize) -> Result<()> {
        self.plans.clear();
        self.columns = vec![ReactionColumn::default(); columns];
        Ok(())
    }
    fn plan_len(&self) -> usize {
        self.plans.len()
    }
    fn push_plan(&mut self, plan: PartitionPlan) -> Result<()> {
        self.plans.push(plan);
        Ok(())
    }
    fn plan(&self, index: usize) -> Result<PartitionPlan> {
        Ok(self
            .plans
            .get(index)
            .context("Partition plan is absent")?
            .clone())
    }
    fn put_plan(&mut self, index: usize, plan: PartitionPlan) -> Result<()> {
        *self
            .plans
            .get_mut(index)
            .context("Partition plan is absent")? = plan;
        Ok(())
    }
    fn column(&self, index: usize) -> Result<ReactionColumn> {
        Ok(*self
            .columns
            .get(index)
            .context("Reaction column is absent")?)
    }
    fn put_column(&mut self, index: usize, column: ReactionColumn) -> Result<()> {
        ensure!(column.valid(), "Invalid reaction column");
        *self
            .columns
            .get_mut(index)
            .context("Reaction column is absent")? = column;
        Ok(())
    }
}

#[cfg(test)]
mod storage_tests {
    use super::*;
    use crate::validation::{
        ValidationState,
        working_state::{faults::MissingRowState, key},
    };
    struct FaultWorkspace {
        inner: MemoryWorkspace,
        planner: MissingRowState,
        verifier: MissingRowState,
    }
    impl ReactionRead for FaultWorkspace {
        fn reaction_len(&self) -> usize {
            self.inner.reaction_len()
        }
        fn read_reactions(&self, start: usize, out: &mut [f64]) -> Result<()> {
            self.inner.read_reactions(start, out)
        }
    }
    impl ReactionWrite for FaultWorkspace {
        fn set_reaction(&mut self, i: usize, value: f64) -> Result<()> {
            self.inner.set_reaction(i, value)
        }
    }
    impl SolverWorkspace for FaultWorkspace {
        fn new_state(&self) -> Result<Box<dyn ValidationState>> {
            self.inner.new_state()
        }
        fn planner_state(&self) -> &dyn ValidationState {
            &self.planner
        }
        fn planner_state_mut(&mut self) -> &mut dyn ValidationState {
            &mut self.planner
        }
        fn verifier_state(&self) -> &dyn ValidationState {
            &self.verifier
        }
        fn verifier_state_mut(&mut self) -> &mut dyn ValidationState {
            &mut self.verifier
        }
        fn validation_state(&mut self) -> (&mut dyn ValidationState, &mut dyn ValidationState) {
            self.inner.validation_state()
        }
        fn reset(&mut self, n: usize) -> Result<()> {
            self.inner.reset(n)
        }
        fn plan_len(&self) -> usize {
            self.inner.plan_len()
        }
        fn push_plan(&mut self, p: PartitionPlan) -> Result<()> {
            self.inner.push_plan(p)
        }
        fn plan(&self, i: usize) -> Result<PartitionPlan> {
            self.inner.plan(i)
        }
        fn put_plan(&mut self, i: usize, p: PartitionPlan) -> Result<()> {
            self.inner.put_plan(i, p)
        }
        fn column(&self, i: usize) -> Result<ReactionColumn> {
            self.inner.column(i)
        }
        fn put_column(&mut self, i: usize, c: ReactionColumn) -> Result<()> {
            self.inner.put_column(i, c)
        }
    }
    #[test]
    fn lost_planner_or_independent_verifier_incidence_never_certifies() {
        let mut d = crate::demo::Demo::new();
        d.add("3005", 4, 0., 0., 0.);
        d.add("3005", 4, 0., 1., 0.);
        let joints = [crate::JointSpec {
            a: 1,
            b: 2,
            anchor: glam::DVec3::new(0.5, 1., 0.5),
            kind: crate::JointKind::Fixed,
            break_force: Some(10.),
            break_torque: Some(1.),
            axial_force: None,
            friction_torque: None,
            strength_source: None,
        }];
        let problem = Problem {
            bricks: Bodies::all(&d.bricks),
            joints: &joints,
            contacts: vec![],
            supports: vec![],
            anchored: vec![0],
            masses: vec![0.0016; 2],
            force_tolerance: 1e-6,
            torque_tolerance: 1e-8,
        };
        for planner in [true, false] {
            let mut workspace = FaultWorkspace {
                inner: MemoryWorkspace::default(),
                planner: MissingRowState::default(),
                verifier: MissingRowState::default(),
            };
            if planner {
                workspace.planner.missing_page = Some(key(2, &[0]));
            } else {
                workspace.verifier.missing_page = Some(key(1, &[0]));
            }
            let error = certify_partitioned_with_workspace(
                &problem,
                PartitionOptions {
                    core_bodies: 1,
                    ..Default::default()
                },
                &mut workspace,
            )
            .unwrap_err();
            assert!(error.downcast_ref::<Inconclusive>().is_some(), "{error:#}");
            assert!(error.to_string().contains("incidence"), "{error:#}");
        }
    }
    #[test]
    fn canonical_body_loss_is_inconclusive_before_any_local_solve() {
        let mut d = crate::demo::Demo::new();
        d.add("3005", 4, 0., 0., 0.);
        d.add("3005", 4, 0., 1., 0.);
        let mut state = MissingRowState::default();
        state.missing_get = Some(key(0, &[1]));
        let mut problem = StoredProblem::new(Box::new(state), 1e-6, 1e-8);
        for body in d.bricks {
            problem.push_body(body, 0.0016).unwrap();
        }
        problem.push_anchor(0).unwrap();
        let mut workspace = MemoryWorkspace::default();
        assert!(
            certify_input_with_workspace(&problem, &mut workspace)
                .unwrap_err()
                .downcast_ref::<Inconclusive>()
                .is_some()
        );
        assert_eq!(workspace.plan_len(), 0);
    }
}
