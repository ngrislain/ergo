//! Transaction-local normalized poses and conservative spatial candidates for
//! the common physical gate. Complete mechanical islands still feed equilibrium.
use anyhow::Result;
use ergo_transactions::{
    ContentId, Transaction, canonical_cbor,
    spill::{SpillMap, SpillSpatial},
};
use ergo_world::{
    Aabb, Brick,
    equilibrium::Inconclusive,
    physical::ConnectionMeasurements,
    validation::{ValidationInput, validation_envelope},
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{borrow::Cow, cell::RefCell, collections::VecDeque};

const CACHE_BODIES: usize = 64;
const MAX_BODY_BYTES: usize = 16 * 1024;
#[cfg(test)]
#[path = "tests_validation_input.rs"]
mod tests;
fn unavailable(message: impl std::fmt::Display) -> anyhow::Error {
    Inconclusive(format!("native physical input: {message}")).into()
}

#[derive(Serialize, Deserialize)]
struct SealedBody {
    ordinal: u64,
    body: Brick,
    digest: [u8; 32],
}

#[derive(Serialize, Deserialize)]
struct LocalIdentity {
    id: u32,
    digest: [u8; 32],
}
impl LocalIdentity {
    fn digest(global: &ContentId, id: u32) -> [u8; 32] {
        let mut hash = Sha256::new();
        hash.update(b"ergo-physical-identity-1");
        hash.update(global.as_str().as_bytes());
        hash.update(id.to_le_bytes());
        hash.finalize().into()
    }
    fn decode(self, global: &ContentId, count: usize) -> Result<u32> {
        if self.id == 0 || self.id as usize > count || self.digest != Self::digest(global, self.id)
        {
            return Err(unavailable("global/local identity checksum changed"));
        }
        Ok(self.id)
    }
}
impl SealedBody {
    fn digest(ordinal: u64, body: &Brick) -> Result<[u8; 32]> {
        let bytes = canonical_cbor(body).map_err(unavailable)?;
        if bytes.len() > MAX_BODY_BYTES {
            return Err(unavailable("physical body exceeds record budget"));
        }
        let mut hash = Sha256::new();
        hash.update(b"ergo-physical-body-1");
        hash.update(ordinal.to_le_bytes());
        hash.update(bytes);
        Ok(hash.finalize().into())
    }
    fn decode(self, ordinal: u64) -> Result<Brick> {
        if self.ordinal != ordinal
            || u64::from(self.body.id) != ordinal + 1
            || self.digest != Self::digest(ordinal, &self.body)?
        {
            return Err(unavailable("body identity/checksum changed"));
        }
        Ok(self.body)
    }
}

pub(super) struct NativeValidationInput {
    bodies: SpillMap<u64, SealedBody>,
    global: SpillMap<ContentId, LocalIdentity>,
    spatial: SpillSpatial,
    cache: RefCell<VecDeque<(usize, Brick)>>,
}
impl NativeValidationInput {
    pub fn new(tx: &Transaction) -> Result<Self> {
        Ok(Self {
            bodies: tx.local_map()?,
            global: tx.local_map()?,
            spatial: tx.local_spatial()?,
            cache: RefCell::new(VecDeque::new()),
        })
    }
    pub fn push(
        &mut self,
        global: ContentId,
        mut body: Brick,
        measurements: &ConnectionMeasurements,
    ) -> Result<()> {
        let ordinal = self.bodies.len() as u64;
        let id = u32::try_from(ordinal + 1).map_err(unavailable)?;
        if body.id != id || self.global.contains_key(&global)? {
            return Err(unavailable("invalid/duplicate physical body identity"));
        }
        measurements.normalize_brick(&mut body)?;
        ergo_world::catalog::validate(std::slice::from_ref(&body))?;
        ergo_world::physical::validate_pose(&body)?;
        let envelope = validation_envelope(&body);
        let digest = SealedBody::digest(ordinal, &body)?;
        self.bodies.insert(
            ordinal,
            SealedBody {
                ordinal,
                body,
                digest,
            },
        )?;
        let identity = LocalIdentity {
            id,
            digest: LocalIdentity::digest(&global, id),
        };
        self.global.insert(global, identity)?;
        self.spatial
            .insert(id - 1, envelope.min.to_array(), envelope.max.to_array())?;
        Ok(())
    }
    pub fn local_id(&self, global: &ContentId) -> Result<Option<u32>> {
        self.global
            .get(global)?
            .map(|identity| identity.decode(global, self.bodies.len()))
            .transpose()
    }
    pub fn finish(self) -> Result<Self> {
        if self.bodies.len() != self.global.len() || self.bodies.len() != self.spatial.len() {
            return Err(unavailable("physical input/index count mismatch"));
        }
        self.spatial.check_count()?;
        // Iterator count checks reject missing mapping rows, even if no joint
        // happens to reference them. Each checksum binds both sides of the map.
        for row in self.global.iter() {
            let (global, identity) = row?;
            identity.decode(&global, self.bodies.len())?;
        }
        Ok(self)
    }
}
struct Candidates<'a> {
    index: &'a SpillSpatial,
    bounds: Aabb,
    after: Option<u32>,
    page: VecDeque<u32>,
    done: bool,
}
impl Iterator for Candidates<'_> {
    type Item = Result<usize>;
    fn next(&mut self) -> Option<Self::Item> {
        if self.page.is_empty() {
            if self.done {
                return None;
            }
            match self.index.page(
                self.bounds.min.to_array(),
                self.bounds.max.to_array(),
                self.after,
                128,
            ) {
                Ok(page) => {
                    if page.is_empty() {
                        self.done = true;
                        return None;
                    }
                    self.page = page.into();
                }
                Err(error) => {
                    self.done = true;
                    return Some(Err(error.into()));
                }
            }
        }
        let id = self.page.pop_front()?;
        self.after = Some(id);
        Some(Ok(id as usize))
    }
}
impl ValidationInput for NativeValidationInput {
    fn len(&self) -> usize {
        self.bodies.len()
    }
    fn body(&self, index: usize) -> Result<Cow<'_, Brick>> {
        if index >= self.len() {
            return Err(unavailable("body is absent"));
        }
        if let Some((_, body)) = self.cache.borrow().iter().find(|(i, _)| *i == index) {
            return Ok(Cow::Owned(body.clone()));
        }
        let body = self
            .bodies
            .get(&(index as u64))
            .map_err(unavailable)?
            .ok_or_else(|| unavailable("body record is absent"))?
            .decode(index as u64)?;
        let mut cache = self.cache.borrow_mut();
        if cache.len() == CACHE_BODIES {
            cache.pop_front();
        }
        cache.push_back((index, body.clone()));
        Ok(Cow::Owned(body))
    }
    fn body_index(&self, id: u32) -> Result<Option<usize>> {
        Ok(id
            .checked_sub(1)
            .map(|i| i as usize)
            .filter(|&i| i < self.len()))
    }
    fn query(&self, bounds: Aabb) -> Result<Box<dyn Iterator<Item = Result<usize>> + '_>> {
        Ok(Box::new(Candidates {
            index: &self.spatial,
            bounds,
            after: None,
            page: VecDeque::new(),
            done: false,
        }))
    }
    fn check_indexed_bounds(&self, index: usize, expected: Aabb) -> Result<()> {
        self.spatial.check_contains(
            u32::try_from(index).map_err(unavailable)?,
            expected.min.to_array(),
            expected.max.to_array(),
        )?;
        Ok(())
    }
}
