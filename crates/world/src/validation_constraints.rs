//! One fallible joint/anchor collection for publication, with disk-backed pair
//! and incidence membership instead of a second scene-wide hash dictionary.
use super::{
    ValidationRecord as Record, ValidationState,
    working_state::{key, required, unavailable},
};
use crate::JointSpec;
use anyhow::{Result, ensure};

pub struct ValidationConstraints {
    state: Box<dyn ValidationState>,
    joints: usize,
    anchors: usize,
}
impl ValidationConstraints {
    pub fn new(state: Box<dyn ValidationState>) -> Self {
        Self {
            state,
            joints: 0,
            anchors: 0,
        }
    }
    pub fn len(&self) -> usize {
        self.joints
    }
    pub fn is_empty(&self) -> bool {
        self.joints == 0
    }
    pub fn verify(&self) -> Result<()> {
        for i in 0..self.joints {
            let j = self.joint(i)?;
            match required(
                self.state.as_ref(),
                &key(1, &[j.a.min(j.b) as usize, j.a.max(j.b) as usize]),
            )? {
                Record::Index(index) if index == i => {}
                _ => return Err(unavailable("joint pair index lost or changed a record")),
            }
            if !self.has_body(j.a)? || !self.has_body(j.b)? {
                return Err(unavailable("joint incidence marker is absent"));
            }
        }
        for i in 0..self.anchors {
            self.anchor(i)?;
        }
        Ok(())
    }
    pub fn anchor_len(&self) -> usize {
        self.anchors
    }
    pub fn joint(&self, i: usize) -> Result<JointSpec> {
        match required(self.state.as_ref(), &key(0, &[i]))? {
            Record::Joint(j) if i < self.joints => Ok(*j),
            _ => Err(unavailable("validation joint is absent or invalid")),
        }
    }
    pub fn has_pair(&self, a: u32, b: u32) -> Result<bool> {
        match self
            .state
            .get(&key(1, &[a.min(b) as usize, a.max(b) as usize]))?
        {
            None => Ok(false),
            Some(Record::Index(i)) if i < self.joints => {
                let j = self.joint(i)?;
                if (j.a.min(j.b), j.a.max(j.b)) != (a.min(b), a.max(b)) {
                    return Err(unavailable("joint pair identity changed"));
                }
                Ok(true)
            }
            _ => Err(unavailable("invalid joint pair record")),
        }
    }
    pub fn push_joint(&mut self, j: JointSpec) -> Result<()> {
        ensure!(
            !self.has_pair(j.a, j.b)?,
            "Duplicate attachment joint for pieces {} and {}",
            j.a,
            j.b
        );
        let i = self.joints;
        self.state.put(
            key(1, &[j.a.min(j.b) as usize, j.a.max(j.b) as usize]),
            Record::Index(i),
        )?;
        for b in [j.a, j.b] {
            self.state.put(key(2, &[b as usize]), Record::Index(0))?;
        }
        self.state.put(key(0, &[i]), Record::Joint(Box::new(j)))?;
        self.joints += 1;
        Ok(())
    }
    pub fn put_joint(&mut self, i: usize, j: JointSpec) -> Result<()> {
        let old = self.joint(i)?;
        if old.a != j.a || old.b != j.b {
            return Err(unavailable("capacity update changed joint endpoints"));
        }
        self.state.put(key(0, &[i]), Record::Joint(Box::new(j)))
    }
    pub fn has_body(&self, id: u32) -> Result<bool> {
        match self.state.get(&key(2, &[id as usize]))? {
            None => Ok(false),
            Some(Record::Index(0)) => Ok(true),
            _ => Err(unavailable("invalid joint incidence marker")),
        }
    }
    pub fn push_anchor(&mut self, id: u32) -> Result<()> {
        self.state
            .put(key(3, &[self.anchors]), Record::Index(id as usize))?;
        self.anchors += 1;
        Ok(())
    }
    pub fn anchor(&self, i: usize) -> Result<u32> {
        match required(self.state.as_ref(), &key(3, &[i]))? {
            Record::Index(id) if i < self.anchors => u32::try_from(id).map_err(unavailable),
            _ => Err(unavailable("validation anchor is absent or invalid")),
        }
    }
}
