//! Checksummed scalar records for the common component/compression algorithms.
use anyhow::Result;
use ergo_transactions::{Transaction, canonical_cbor, spill::SpillMap};
use ergo_world::{
    equilibrium::Inconclusive,
    validation::{ValidationRecord, ValidationState},
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    cell::RefCell,
    collections::{BTreeMap, VecDeque},
};

const MAX_RECORD_BYTES: usize = 64 * 1024;
const CACHE_BYTES: usize = 256 * 1024;
const CACHE_RECORDS: usize = 256;
#[derive(Default)]
struct Cache {
    rows: BTreeMap<Vec<u8>, (ValidationRecord, usize)>,
    order: VecDeque<Vec<u8>>,
    bytes: usize,
}
impl Cache {
    fn put(&mut self, key: Vec<u8>, value: ValidationRecord) {
        if let Some((_, bytes)) = self.rows.remove(&key) {
            self.bytes -= bytes;
            self.order.retain(|old| old != &key);
        }
        let heap = match &value {
            ValidationRecord::Body { brick, .. } => {
                std::mem::size_of::<ergo_world::Brick>() + brick.part.capacity()
            }
            ValidationRecord::Joint(j) => {
                std::mem::size_of::<ergo_world::JointSpec>()
                    + j.strength_source.as_ref().map_or(0, String::capacity)
            }
            ValidationRecord::Indices(v) => v.capacity() * std::mem::size_of::<usize>(),
            _ => 0,
        };
        let bytes = std::mem::size_of::<ValidationRecord>() + 2 * key.capacity() + heap;
        if bytes > CACHE_BYTES {
            return;
        }
        while self.bytes + bytes > CACHE_BYTES || self.rows.len() >= CACHE_RECORDS {
            let Some(old) = self.order.pop_front() else {
                break;
            };
            if let Some((_, bytes)) = self.rows.remove(&old) {
                self.bytes -= bytes;
            }
        }
        self.order.push_back(key.clone());
        self.bytes += bytes;
        self.rows.insert(key, (value, bytes));
    }
}

fn unavailable(message: impl std::fmt::Display) -> anyhow::Error {
    Inconclusive(format!("native validation state: {message}")).into()
}
#[derive(Serialize, Deserialize)]
struct SealedRecord {
    value: ValidationRecord,
    digest: [u8; 32],
}
impl SealedRecord {
    fn digest(key: &[u8], value: &ValidationRecord) -> Result<[u8; 32]> {
        let mut hash = Sha256::new();
        hash.update(b"ergo-validation-state-1");
        hash.update((key.len() as u64).to_le_bytes());
        hash.update(key);
        let bytes = canonical_cbor(value).map_err(unavailable)?;
        if bytes.len() > MAX_RECORD_BYTES {
            return Err(unavailable(
                "validation record exceeds its encoded byte budget",
            ));
        }
        hash.update(bytes);
        Ok(hash.finalize().into())
    }
    fn decode(self, key: &[u8]) -> Result<ValidationRecord> {
        if self.digest != Self::digest(key, &self.value)? {
            return Err(unavailable("scalar record identity/checksum changed"));
        }
        Ok(self.value)
    }
}
pub(super) struct NativeValidationState {
    rows: SpillMap<Vec<u8>, SealedRecord>,
    cache: RefCell<Cache>,
}
impl NativeValidationState {
    pub fn new(tx: &Transaction) -> Result<Self> {
        Ok(Self {
            rows: tx.local_map()?,
            cache: RefCell::default(),
        })
    }
    pub fn from_store(store: &ergo_transactions::spill::SpillStore) -> Result<Self> {
        Ok(Self {
            rows: store.map()?,
            cache: RefCell::default(),
        })
    }
}
impl ValidationState for NativeValidationState {
    fn clear(&mut self) -> Result<()> {
        *self.cache.get_mut() = Cache::default();
        self.rows.clear().map_err(unavailable)
    }
    fn get(&self, key: &[u8]) -> Result<Option<ValidationRecord>> {
        if let Some((record, _)) = self.cache.borrow().rows.get(key) {
            return Ok(Some(record.clone()));
        }
        let value = self
            .rows
            .get(&key.to_vec())
            .map_err(unavailable)?
            .map(|row| row.decode(key))
            .transpose()?;
        if let Some(value) = &value {
            self.cache.borrow_mut().put(key.to_vec(), value.clone());
        }
        Ok(value)
    }
    fn put(&mut self, key: Vec<u8>, value: ValidationRecord) -> Result<()> {
        let digest = SealedRecord::digest(&key, &value)?;
        self.rows
            .insert(
                key.clone(),
                SealedRecord {
                    value: value.clone(),
                    digest,
                },
            )
            .map_err(unavailable)?;
        self.cache.get_mut().put(key, value);
        Ok(())
    }
    fn page(
        &self,
        prefix: &[u8],
        after: Option<&[u8]>,
        limit: usize,
    ) -> Result<Vec<(Vec<u8>, ValidationRecord)>> {
        let after = after.map(<[u8]>::to_vec);
        self.rows
            .page(prefix, after.as_ref(), limit)
            .map_err(unavailable)?
            .into_iter()
            .map(|(key, row)| row.decode(&key).map(|value| (key, value)))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ergo_transactions::{OctreePath, Repository, spill::SpillLimits};
    use std::sync::Arc;
    async fn transaction() -> Transaction {
        let repo = Repository::new(
            Arc::new(object_store::memory::InMemory::new()),
            "validation-state",
        )
        .unwrap();
        repo.create("main").await.unwrap();
        repo.begin("main").await.unwrap()
    }
    #[tokio::test]
    async fn validation_records_evict_under_both_cache_limits_and_bind_keys() {
        let tx = transaction().await;
        let mut state = NativeValidationState::new(&tx).unwrap();
        for i in 0..2048u64 {
            state
                .put(
                    i.to_be_bytes().to_vec(),
                    ValidationRecord::Indices((0..256).collect()),
                )
                .unwrap();
            assert!(state.cache.borrow().bytes <= CACHE_BYTES);
            assert!(state.cache.borrow().rows.len() <= CACHE_RECORDS);
        }
        assert!(
            matches!(state.get(&0u64.to_be_bytes()).unwrap(),Some(ValidationRecord::Indices(v)) if v.len()==256)
        );
        let row = state
            .rows
            .get(&0u64.to_be_bytes().to_vec())
            .unwrap()
            .unwrap();
        state.rows.insert(1u64.to_be_bytes().to_vec(), row).unwrap();
        *state.cache.get_mut() = Cache::default();
        let error = state.get(&1u64.to_be_bytes()).unwrap_err();
        assert!(error.downcast_ref::<Inconclusive>().is_some());
        assert!(error.to_string().contains("checksum"));
    }
    #[tokio::test]
    async fn changed_scalar_or_body_records_cannot_be_read_as_valid_state() {
        for attack in ["scalar", "body"] {
            let tx = transaction().await;
            let mut state = NativeValidationState::new(&tx).unwrap();
            let key = vec![42];
            state.put(key.clone(), ValidationRecord::Index(7)).unwrap();
            let mut row = state.rows.get(&key).unwrap().unwrap();
            row.value = if attack == "scalar" {
                ValidationRecord::Index(8)
            } else {
                let mut d = ergo_world::demo::Demo::new();
                d.add("3003", 4, 0., 0., 0.);
                ValidationRecord::Body {
                    brick: Box::new(d.bricks.remove(0)),
                    mass: 1.,
                }
            };
            state.rows.insert(key.clone(), row).unwrap();
            *state.cache.get_mut() = Cache::default();
            assert!(
                state
                    .get(&key)
                    .unwrap_err()
                    .downcast_ref::<Inconclusive>()
                    .is_some()
            );
        }
    }
    #[tokio::test]
    async fn validation_scratch_exhaustion_cannot_publish_a_staged_edit() {
        let repo = Repository::new(
            Arc::new(object_store::memory::InMemory::new()),
            "validation-disk-full",
        )
        .unwrap()
        .with_spill_limits(SpillLimits {
            database_bytes: 64 * 1024,
            cache_kib: 4,
        })
        .unwrap();
        let head = repo.create("main").await.unwrap();
        let mut tx = repo.begin("main").await.unwrap();
        tx.put(&OctreePath::new(vec![1]).unwrap(), &42u32)
            .await
            .unwrap();
        let mut state = NativeValidationState::new(&tx).unwrap();
        let mut error = None;
        for i in 0..1000u64 {
            if let Err(e) = state.put(
                i.to_be_bytes().to_vec(),
                ValidationRecord::Indices((0..256).collect()),
            ) {
                error = Some(e);
                break;
            }
        }
        assert!(
            error
                .expect("tiny scratch budget must fail")
                .downcast_ref::<Inconclusive>()
                .is_some()
        );
        assert!(matches!(
            tx.commit("alice", "must not publish", Default::default())
                .await,
            Err(ergo_transactions::Error::Scratch(_))
        ));
        assert_eq!(repo.checkout("main").await.unwrap().id(), head.id());
    }
}
