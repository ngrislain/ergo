//! Transaction-local solver plans and checksummed scalar pages. No native
//! duplicate of the coordinator/verifier and no fallback to whole-scene RAM.
use anyhow::Result;
use ergo_transactions::{Transaction, spill::SpillMap};
use ergo_world::equilibrium::{
    Inconclusive, PartitionPlan, ReactionColumn, ReactionRead, ReactionWrite, SolverWorkspace,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{collections::BTreeMap, sync::Mutex};

const PAGE_COLUMNS: usize = 256;
const MAX_PAGES: usize = 32;
const RECORD_BYTES: usize = 40;
fn unavailable(message: impl std::fmt::Display) -> anyhow::Error {
    Inconclusive(format!("native equilibrium workspace: {message}")).into()
}

#[derive(Serialize, Deserialize)]
struct SealedPage {
    index: u64,
    #[serde(with = "byte_data")]
    data: Vec<u8>,
    digest: [u8; 32],
}
mod byte_data {
    use serde::{
        Deserializer, Serializer,
        de::{Error, Visitor},
    };
    pub fn serialize<S: Serializer>(bytes: &[u8], serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_bytes(bytes)
    }
    pub fn deserialize<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Vec<u8>, D::Error> {
        struct Bytes;
        impl<'de> Visitor<'de> for Bytes {
            type Value = Vec<u8>;
            fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
                f.write_str("a bounded solver page byte string")
            }
            fn visit_bytes<E: Error>(self, v: &[u8]) -> Result<Vec<u8>, E> {
                if v.len() != super::PAGE_COLUMNS * super::RECORD_BYTES {
                    return Err(E::custom("invalid solver page length"));
                }
                Ok(v.to_vec())
            }
            fn visit_byte_buf<E: Error>(self, v: Vec<u8>) -> Result<Vec<u8>, E> {
                if v.len() != super::PAGE_COLUMNS * super::RECORD_BYTES {
                    return Err(E::custom("invalid solver page length"));
                }
                Ok(v)
            }
        }
        deserializer.deserialize_byte_buf(Bytes)
    }
}
impl SealedPage {
    fn digest(index: u64, data: &[u8]) -> [u8; 32] {
        let mut hash = Sha256::new();
        hash.update(b"ergo-equilibrium-columns-1");
        hash.update(index.to_le_bytes());
        hash.update(data);
        hash.finalize().into()
    }
    fn encode(index: u64, rows: &[ReactionColumn]) -> Result<Self> {
        if rows.len() != PAGE_COLUMNS {
            return Err(unavailable("invalid scalar page dimensions"));
        }
        let mut data = vec![0; PAGE_COLUMNS * RECORD_BYTES];
        for (row, bytes) in rows.iter().zip(data.chunks_exact_mut(RECORD_BYTES)) {
            if !row.valid() {
                return Err(unavailable("invalid/nonfinite scalar state"));
            }
            for (k, value) in [row.consensus, row.witness, row.sum, row.scale]
                .into_iter()
                .enumerate()
            {
                bytes[k * 8..(k + 1) * 8].copy_from_slice(&value.to_le_bytes());
            }
            bytes[32] = row.copies;
        }
        Ok(Self {
            index,
            digest: Self::digest(index, &data),
            data,
        })
    }
    fn decode(self, index: u64) -> Result<Vec<ReactionColumn>> {
        if self.index != index
            || self.data.len() != PAGE_COLUMNS * RECORD_BYTES
            || self.digest != Self::digest(index, &self.data)
        {
            return Err(unavailable("scalar page identity/checksum changed"));
        }
        self.data
            .chunks_exact(RECORD_BYTES)
            .map(|bytes| {
                let mut values = [0.; 4];
                for (k, value) in values.iter_mut().enumerate() {
                    *value = f64::from_le_bytes(bytes[k * 8..(k + 1) * 8].try_into().unwrap());
                }
                let row = ReactionColumn {
                    consensus: values[0],
                    witness: values[1],
                    sum: values[2],
                    scale: values[3],
                    copies: bytes[32],
                };
                if !row.valid() || bytes[33..].iter().any(|b| *b != 0) {
                    return Err(unavailable("invalid scalar page record"));
                }
                Ok(row)
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ergo_transactions::{OctreePath, Repository};
    use ergo_world::{
        JointKind, JointSpec,
        equilibrium::{self, Bodies, MemoryWorkspace, PartitionOptions, Problem},
    };
    use glam::DVec3;
    use std::sync::Arc;
    async fn transaction() -> ergo_transactions::Transaction {
        let repo = Repository::new(
            Arc::new(object_store::memory::InMemory::new()),
            "solver-workspace",
        )
        .unwrap();
        repo.create("main").await.unwrap();
        repo.begin("main").await.unwrap()
    }
    #[tokio::test]
    async fn scalar_pages_roundtrip_after_eviction_and_checksum_errors_are_not_zero_filled() {
        let tx = transaction().await;
        let mut workspace = NativeSolverWorkspace::new(&tx).unwrap();
        workspace.reset(10000).unwrap();
        for i in 0..10000 {
            workspace
                .put_column(
                    i,
                    ReactionColumn {
                        consensus: -(i as f64),
                        witness: i as f64 + 0.25,
                        sum: 0.,
                        scale: 2.,
                        copies: 2,
                    },
                )
                .unwrap();
        }
        for i in (0..10000).rev() {
            let value = workspace.column(i).unwrap();
            assert_eq!(value.witness, i as f64 + 0.25);
            assert_eq!(value.consensus, -(i as f64));
        }
        let mut values = [0.; 8];
        workspace.read_reactions(254, &mut values).unwrap();
        assert_eq!(values[7], 261.25);
        assert!(workspace.columns.lock().unwrap().cache.len() <= MAX_PAGES);
        {
            let mut state = workspace.columns.lock().unwrap();
            let index = 39;
            state.cache.remove(&index);
            let mut page = state.pages.get(&index).unwrap().unwrap();
            page.data[0] ^= 1;
            state.pages.insert(index, page).unwrap();
        }
        assert!(workspace.column(9999).is_err());
        assert!(workspace.column(1).is_err());
    }
    #[tokio::test]
    async fn missing_written_scalar_pages_never_become_new_zero_pages() {
        let tx = transaction().await;
        let mut workspace = NativeSolverWorkspace::new(&tx).unwrap();
        workspace.reset(10000).unwrap();
        for i in 0..10000 {
            workspace.set_reaction(i, 1.).unwrap();
        }
        {
            let mut state = workspace.columns.lock().unwrap();
            assert!(state.written[0]);
            state.cache.remove(&0);
            state.pages.remove(&0).unwrap();
        }
        assert!(workspace.column(0).is_err());
        assert!(workspace.reset(1).is_err());
    }
    #[tokio::test]
    async fn disk_exhaustion_cannot_publish_a_partial_solver_state() {
        let repo = Repository::new(
            Arc::new(object_store::memory::InMemory::new()),
            "solver-full",
        )
        .unwrap()
        .with_spill_limits(ergo_transactions::spill::SpillLimits {
            database_bytes: 64 * 1024,
            cache_kib: 4,
        })
        .unwrap();
        let initial = repo.create("main").await.unwrap();
        let mut tx = repo.begin("main").await.unwrap();
        tx.put(&OctreePath::new(vec![1]).unwrap(), &1u32)
            .await
            .unwrap();
        let mut workspace = NativeSolverWorkspace::new(&tx).unwrap();
        workspace.reset(20000).unwrap();
        let mut failed = false;
        for i in 0..20000 {
            if workspace
                .put_column(
                    i,
                    ReactionColumn {
                        witness: 1.,
                        copies: 1,
                        scale: 1.,
                        ..Default::default()
                    },
                )
                .is_err()
            {
                failed = true;
                break;
            }
        }
        assert!(failed);
        assert!(workspace.column(0).is_err());
        assert!(
            tx.commit("alice", "must not publish", Default::default())
                .await
                .is_err()
        );
        assert_eq!(repo.checkout("main").await.unwrap().id(), initial.id());
    }
    #[tokio::test]
    async fn resident_and_spilled_coordinators_certify_the_same_real_loads() {
        let tx = transaction().await;
        let mut native = NativeSolverWorkspace::new(&tx).unwrap();
        let mut memory = MemoryWorkspace::default();
        let mut demo = ergo_world::demo::Demo::new();
        for i in 0..8 {
            demo.add("3005", 4, i as f64, 5., 0.);
        }
        let joints: Vec<_> = (1..8)
            .map(|i| JointSpec {
                a: i,
                b: i + 1,
                anchor: DVec3::new(i as f64, 5.5, 0.5),
                kind: JointKind::Fixed,
                break_force: Some(10.),
                break_torque: Some(0.01),
                axial_force: None,
                friction_torque: None,
                strength_source: Some("test".into()),
            })
            .collect();
        let problem = Problem {
            bricks: Bodies::all(&demo.bricks),
            joints: &joints,
            contacts: vec![],
            supports: vec![],
            anchored: vec![0],
            masses: vec![0.001; 8],
            force_tolerance: 1e-6,
            torque_tolerance: 1e-8,
        };
        let options = PartitionOptions {
            core_bodies: 1,
            max_sweeps: 1000,
            ..Default::default()
        };
        let a = equilibrium::certify_partitioned_with_workspace(&problem, options, &mut memory)
            .unwrap();
        let b = equilibrium::certify_partitioned_with_workspace(&problem, options, &mut native)
            .unwrap();
        assert_eq!(a.loaded_joints, b.loaded_joints);
        assert!((a.max_utilisation - b.max_utilisation).abs() < 1e-3);
        assert_eq!(b.solver.spilled_partition_plans, 8);
        assert_eq!(b.solver.spilled_reaction_columns, 48);
        assert!(b.solver.sweeps > 1);
        let weak: Vec<_> = joints
            .iter()
            .cloned()
            .map(|j| JointSpec {
                break_torque: Some(1e-8),
                ..j
            })
            .collect();
        let problem = Problem {
            joints: &weak,
            ..problem
        };
        assert!(
            equilibrium::certify_partitioned_with_workspace(&problem, options, &mut native)
                .is_err()
        );
    }
    #[tokio::test]
    async fn pavilion_uses_native_workspace_without_weakening_the_certificate() {
        let tx = transaction().await;
        let mut workspace = NativeSolverWorkspace::new(&tx).unwrap();
        let demo = ergo_world::demo::pavilion();
        let mut scene = ergo_world::WorldExport {
            schema: ergo_world::SCHEMA_VERSION,
            name: "pavilion".into(),
            geo: Default::default(),
            joints: ergo_world::demo::authored_bonds(&demo.bricks),
            bricks: demo.bricks,
            meshes: demo.meshes,
            fixed_bricks: vec![],
            ground: vec![],
            groups: vec![],
            model: None,
        };
        let measurements = ergo_world::physical::ConnectionMeasurements {
            idealized_fixture: true,
            ..Default::default()
        };
        let report = ergo_world::validation::validate_static_with_workspace(
            &mut scene,
            Default::default(),
            &measurements,
            &mut workspace,
        )
        .unwrap();
        assert!(report.equilibrium_solver.spilled_partition_plans > 1);
        assert!(report.equilibrium_solver.spilled_reaction_columns > 0);
        assert!(report.equilibrium_solver.largest_local_kkt_dimension <= 2048);
    }
}
struct CachedPage {
    rows: Vec<ReactionColumn>,
    dirty: bool,
    used: u64,
}
struct Columns {
    pages: SpillMap<u64, SealedPage>,
    cache: BTreeMap<u64, CachedPage>,
    written: Vec<bool>,
    len: usize,
    clock: u64,
    failed: Option<String>,
}
impl Columns {
    fn reset(&mut self, len: usize) -> Result<()> {
        self.pages.clear().map_err(unavailable)?;
        self.cache.clear();
        self.written = vec![false; len.div_ceil(PAGE_COLUMNS)];
        self.len = len;
        self.clock = 0;
        self.failed = None;
        Ok(())
    }
    fn page(&mut self, index: u64) -> Result<&mut CachedPage> {
        if let Some(error) = &self.failed {
            return Err(unavailable(error));
        }
        if index as usize >= self.written.len() {
            return Err(unavailable("scalar page is out of range"));
        }
        self.clock = self.clock.wrapping_add(1);
        if !self.cache.contains_key(&index) {
            if self.cache.len() >= MAX_PAGES {
                let oldest = *self
                    .cache
                    .iter()
                    .min_by_key(|(_, page)| page.used)
                    .unwrap()
                    .0;
                let page = self.cache.remove(&oldest).unwrap();
                if page.dirty {
                    self.pages
                        .insert(oldest, SealedPage::encode(oldest, &page.rows)?)
                        .map_err(unavailable)?;
                    self.written[oldest as usize] = true;
                }
            }
            let rows = if self.written[index as usize] {
                self.pages
                    .get(&index)
                    .map_err(unavailable)?
                    .ok_or_else(|| unavailable("a written scalar page is absent"))?
                    .decode(index)?
            } else {
                vec![ReactionColumn::default(); PAGE_COLUMNS]
            };
            self.cache.insert(
                index,
                CachedPage {
                    rows,
                    dirty: false,
                    used: self.clock,
                },
            );
        }
        let page = self.cache.get_mut(&index).unwrap();
        page.used = self.clock;
        Ok(page)
    }
    fn range(&self, start: usize, count: usize) -> Result<()> {
        if start.checked_add(count).is_none_or(|end| end > self.len) {
            return Err(unavailable("scalar range is absent"));
        }
        Ok(())
    }
}

pub(super) struct NativeSolverWorkspace {
    store: ergo_transactions::spill::SpillStore,
    plans: SpillMap<u64, PartitionPlan>,
    columns: Mutex<Columns>,
    len: usize,
    components: super::validation_workspace::NativeValidationState,
    compression: super::validation_workspace::NativeValidationState,
    planner: super::validation_workspace::NativeValidationState,
    verifier: super::validation_workspace::NativeValidationState,
}
impl NativeSolverWorkspace {
    pub fn new(tx: &Transaction) -> Result<Self> {
        Ok(Self {
            store: tx.local_store(),
            plans: tx.local_map()?,
            columns: Mutex::new(Columns {
                pages: tx.local_map()?,
                cache: BTreeMap::new(),
                written: vec![],
                len: 0,
                clock: 0,
                failed: None,
            }),
            len: 0,
            components: super::validation_workspace::NativeValidationState::new(tx)?,
            compression: super::validation_workspace::NativeValidationState::new(tx)?,
            planner: super::validation_workspace::NativeValidationState::new(tx)?,
            verifier: super::validation_workspace::NativeValidationState::new(tx)?,
        })
    }
    fn with<T>(&self, f: impl FnOnce(&mut Columns) -> Result<T>) -> Result<T> {
        let mut columns = self
            .columns
            .lock()
            .map_err(|_| unavailable("scalar cache mutex poisoned"))?;
        if let Some(error) = &columns.failed {
            return Err(unavailable(error));
        }
        let result = f(&mut columns);
        if let Err(error) = &result {
            columns.failed = Some(error.to_string());
        }
        result
    }
}
impl ReactionRead for NativeSolverWorkspace {
    fn reaction_len(&self) -> usize {
        self.len
    }
    fn read_reactions(&self, start: usize, out: &mut [f64]) -> Result<()> {
        self.with(|columns| {
            columns.range(start, out.len())?;
            for (offset, value) in out.iter_mut().enumerate() {
                let index = start + offset;
                *value =
                    columns.page((index / PAGE_COLUMNS) as u64)?.rows[index % PAGE_COLUMNS].witness;
            }
            Ok(())
        })
    }
}
impl ReactionWrite for NativeSolverWorkspace {
    fn set_reaction(&mut self, index: usize, value: f64) -> Result<()> {
        if !value.is_finite() {
            return Err(unavailable("nonfinite witness update"));
        }
        self.with(|columns| {
            columns.range(index, 1)?;
            let page = columns.page((index / PAGE_COLUMNS) as u64)?;
            page.rows[index % PAGE_COLUMNS].witness = value;
            page.dirty = true;
            Ok(())
        })
    }
}
impl SolverWorkspace for NativeSolverWorkspace {
    fn new_state(&self) -> Result<Box<dyn ergo_world::validation::ValidationState>> {
        Ok(Box::new(
            super::validation_workspace::NativeValidationState::from_store(&self.store)?,
        ))
    }
    fn planner_state(&self) -> &dyn ergo_world::validation::ValidationState {
        &self.planner
    }
    fn planner_state_mut(&mut self) -> &mut dyn ergo_world::validation::ValidationState {
        &mut self.planner
    }
    fn verifier_state(&self) -> &dyn ergo_world::validation::ValidationState {
        &self.verifier
    }
    fn verifier_state_mut(&mut self) -> &mut dyn ergo_world::validation::ValidationState {
        &mut self.verifier
    }
    fn validation_state(
        &mut self,
    ) -> (
        &mut dyn ergo_world::validation::ValidationState,
        &mut dyn ergo_world::validation::ValidationState,
    ) {
        (&mut self.components, &mut self.compression)
    }
    fn reset(&mut self, columns: usize) -> Result<()> {
        self.plans.clear().map_err(unavailable)?;
        self.with(|state| state.reset(columns))?;
        self.len = columns;
        Ok(())
    }
    fn plan_len(&self) -> usize {
        self.plans.len()
    }
    fn push_plan(&mut self, plan: PartitionPlan) -> Result<()> {
        self.plans
            .insert(self.plans.len() as u64, plan)
            .map_err(unavailable)
    }
    fn plan(&self, index: usize) -> Result<PartitionPlan> {
        self.plans
            .get(&(index as u64))
            .map_err(unavailable)?
            .ok_or_else(|| unavailable("partition plan is absent"))
    }
    fn put_plan(&mut self, index: usize, plan: PartitionPlan) -> Result<()> {
        if index >= self.plans.len() {
            return Err(unavailable("partition index is absent"));
        }
        self.plans.insert(index as u64, plan).map_err(unavailable)
    }
    fn column(&self, index: usize) -> Result<ReactionColumn> {
        self.with(|columns| {
            columns.range(index, 1)?;
            Ok(columns.page((index / PAGE_COLUMNS) as u64)?.rows[index % PAGE_COLUMNS])
        })
    }
    fn put_column(&mut self, index: usize, column: ReactionColumn) -> Result<()> {
        if !column.valid() {
            return Err(unavailable("invalid scalar update"));
        }
        self.with(|columns| {
            columns.range(index, 1)?;
            let page = columns.page((index / PAGE_COLUMNS) as u64)?;
            page.rows[index % PAGE_COLUMNS] = column;
            page.dirty = true;
            Ok(())
        })
    }
    fn spilled(&self) -> bool {
        true
    }
}
