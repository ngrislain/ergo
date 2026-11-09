//! Fallible transient scalar state for component discovery and compression.
//! Storage adapters do not supply supports, physical edges or certificates.
use crate::equilibrium::Inconclusive;
use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum ValidationRecord {
    Body {
        brick: Box<crate::Brick>,
        mass: f64,
    },
    Joint(Box<crate::JointSpec>),
    Contact {
        lower: usize,
        upper: usize,
        patch: crate::Aabb,
    },
    Support {
        body: usize,
        point: [f64; 3],
    },
    Indices(Vec<usize>),
    Node {
        parent: usize,
        size: usize,
        component: Option<usize>,
        ordinal: Option<usize>,
        anchored: bool,
    },
    Component {
        bodies: usize,
        joints: usize,
        anchored: bool,
    },
    Index(usize),
    Load(CompressionLoad),
}
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub enum CompressionLoad {
    Fresh,
    Pending { mass: f64, moment: [f64; 2] },
    Processed,
}
/// Keys and records are immutable while a returned page is being inspected;
/// pages are owned and no storage cursor is retained. Missing expected records
/// must fail, never become an empty component or a fresh zero load.
pub trait ValidationState: Send {
    fn clear(&mut self) -> Result<()>;
    fn get(&self, key: &[u8]) -> Result<Option<ValidationRecord>>;
    fn put(&mut self, key: Vec<u8>, value: ValidationRecord) -> Result<()>;
    fn page(
        &self,
        prefix: &[u8],
        after: Option<&[u8]>,
        limit: usize,
    ) -> Result<Vec<(Vec<u8>, ValidationRecord)>>;
}
#[derive(Default)]
pub struct MemoryValidationState {
    rows: BTreeMap<Vec<u8>, ValidationRecord>,
}
impl ValidationState for MemoryValidationState {
    fn clear(&mut self) -> Result<()> {
        self.rows.clear();
        Ok(())
    }
    fn get(&self, key: &[u8]) -> Result<Option<ValidationRecord>> {
        Ok(self.rows.get(key).cloned())
    }
    fn put(&mut self, key: Vec<u8>, value: ValidationRecord) -> Result<()> {
        self.rows.insert(key, value);
        Ok(())
    }
    fn page(
        &self,
        prefix: &[u8],
        after: Option<&[u8]>,
        limit: usize,
    ) -> Result<Vec<(Vec<u8>, ValidationRecord)>> {
        if !(1..=128).contains(&limit) {
            return Err(unavailable("validation page limit must be 1..=128"));
        }
        let lower = after.unwrap_or(prefix).to_vec();
        Ok(self
            .rows
            .range(lower..)
            .take_while(|(key, _)| key.starts_with(prefix))
            .filter(|(key, _)| after.is_none_or(|after| key.as_slice() > after))
            .take(limit)
            .map(|(key, value)| (key.clone(), value.clone()))
            .collect())
    }
}
pub(crate) fn unavailable(message: impl std::fmt::Display) -> anyhow::Error {
    Inconclusive(format!("validation working state: {message}")).into()
}
pub(crate) fn key(family: u8, indices: &[usize]) -> Vec<u8> {
    let mut key = vec![family];
    for &index in indices {
        key.extend_from_slice(&(index as u64).to_be_bytes());
    }
    key
}
pub(crate) fn required(state: &dyn ValidationState, key: &[u8]) -> Result<ValidationRecord> {
    state
        .get(key)?
        .ok_or_else(|| unavailable("an expected scalar record is absent"))
}

#[cfg(test)]
pub(crate) mod faults {
    use super::*;
    #[derive(Default)]
    pub struct MissingRowState {
        inner: MemoryValidationState,
        pub missing_get: Option<Vec<u8>>,
        pub missing_page: Option<Vec<u8>>,
    }
    impl ValidationState for MissingRowState {
        fn clear(&mut self) -> Result<()> {
            self.inner.clear()
        }
        fn get(&self, key: &[u8]) -> Result<Option<ValidationRecord>> {
            if self.missing_get.as_deref() == Some(key) {
                Ok(None)
            } else {
                self.inner.get(key)
            }
        }
        fn put(&mut self, key: Vec<u8>, value: ValidationRecord) -> Result<()> {
            self.inner.put(key, value)
        }
        fn page(
            &self,
            prefix: &[u8],
            after: Option<&[u8]>,
            limit: usize,
        ) -> Result<Vec<(Vec<u8>, ValidationRecord)>> {
            let mut page = self.inner.page(prefix, after, limit)?;
            if after.is_none() && self.missing_page.as_deref() == Some(prefix) && !page.is_empty() {
                page.remove(0);
            }
            Ok(page)
        }
    }
}
