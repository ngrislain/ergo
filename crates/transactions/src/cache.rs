use bytes::Bytes;
use serde::Serialize;
use std::collections::{BTreeMap, HashMap};

#[derive(Clone, Copy, Debug, Default, Serialize)]
pub struct CacheStats {
    pub hits: u64,
    pub misses: u64,
    pub evictions: u64,
    pub resident_bytes: usize,
    pub capacity_bytes: usize,
}

#[derive(Debug, Default)]
pub(crate) struct Cache {
    values: HashMap<String, (Bytes, u64)>,
    order: BTreeMap<u64, String>,
    clock: u64,
    pub stats: CacheStats,
}

impl Cache {
    pub fn new(capacity_bytes: usize) -> Self {
        Self {
            stats: CacheStats {
                capacity_bytes,
                ..Default::default()
            },
            ..Default::default()
        }
    }

    pub fn get(&mut self, id: &str) -> Option<Bytes> {
        self.clock = self.clock.checked_add(1).unwrap_or_else(|| {
            self.values.clear();
            self.order.clear();
            self.stats.resident_bytes = 0;
            1
        });
        match self.values.get_mut(id) {
            Some((value, stamp)) => {
                self.stats.hits += 1;
                self.order.remove(stamp);
                *stamp = self.clock;
                self.order.insert(*stamp, id.to_owned());
                Some(value.clone())
            }
            None => {
                self.stats.misses += 1;
                None
            }
        }
    }

    pub fn insert(&mut self, id: String, bytes: Bytes) {
        if bytes.len() > self.stats.capacity_bytes || self.values.contains_key(&id) {
            return;
        }
        while self.stats.resident_bytes > self.stats.capacity_bytes - bytes.len() {
            let Some((_, oldest)) = self.order.pop_first() else {
                break;
            };
            if let Some((value, _)) = self.values.remove(&oldest) {
                self.stats.resident_bytes -= value.len();
                self.stats.evictions += 1;
            }
        }
        self.clock = self.clock.checked_add(1).unwrap_or_else(|| {
            self.values.clear();
            self.order.clear();
            self.stats.resident_bytes = 0;
            1
        });
        self.stats.resident_bytes += bytes.len();
        self.order.insert(self.clock, id.clone());
        self.values.insert(id, (bytes, self.clock));
    }

    pub fn clear(&mut self) {
        self.values.clear();
        self.order.clear();
        self.stats.resident_bytes = 0;
    }
}
