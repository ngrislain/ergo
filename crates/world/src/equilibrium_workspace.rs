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
