//! Canonical physical problems behind fallible accessors. Only bounded local
//! kernels materialize records; storage is not a physical partition boundary.
use super::*;
use crate::validation::{
    ValidationRecord as Record, ValidationState,
    working_state::{key, required, unavailable},
};
use std::borrow::Cow;

#[derive(Clone, Copy, Default)]
pub struct ProblemSize {
    pub bodies: usize,
    pub joints: usize,
    pub contacts: usize,
    pub supports: usize,
    pub anchors: usize,
}
pub trait ProblemInput {
    fn validate_storage(&self) -> Result<()> {
        Ok(())
    }
    fn size(&self) -> ProblemSize;
    fn tolerances(&self) -> (f64, f64);
    fn body(&self, i: usize) -> Result<Cow<'_, Brick>>;
    fn body_index(&self, id: u32) -> Result<Option<usize>>;
    fn mass(&self, i: usize) -> Result<f64>;
    fn joint(&self, i: usize) -> Result<Cow<'_, JointSpec>>;
    fn contact(&self, i: usize) -> Result<Contact>;
    fn support(&self, i: usize) -> Result<(usize, DVec3)>;
    fn anchor(&self, i: usize) -> Result<usize>;
}
impl ProblemInput for Problem<'_> {
    fn validate_storage(&self) -> Result<()> {
        ensure!(
            self.masses.len() == self.bricks.len(),
            "Invalid equilibrium body/mass dimensions"
        );
        Ok(())
    }
    fn size(&self) -> ProblemSize {
        ProblemSize {
            bodies: self.bricks.len(),
            joints: self.joints.len(),
            contacts: self.contacts.len(),
            supports: self.supports.len(),
            anchors: self.anchored.len(),
        }
    }
    fn tolerances(&self) -> (f64, f64) {
        (self.force_tolerance, self.torque_tolerance)
    }
    fn body(&self, i: usize) -> Result<Cow<'_, Brick>> {
        ensure!(i < self.bricks.len(), "Equilibrium body is absent");
        Ok(Cow::Borrowed(&self.bricks[i]))
    }
    fn body_index(&self, id: u32) -> Result<Option<usize>> {
        Ok(self.bricks.iter().position(|b| b.id == id))
    }
    fn mass(&self, i: usize) -> Result<f64> {
        self.masses
            .get(i)
            .copied()
            .context("Equilibrium mass is absent")
    }
    fn joint(&self, i: usize) -> Result<Cow<'_, JointSpec>> {
        Ok(Cow::Borrowed(
            self.joints.get(i).context("Equilibrium joint is absent")?,
        ))
    }
    fn contact(&self, i: usize) -> Result<Contact> {
        self.contacts
            .get(i)
            .copied()
            .context("Equilibrium contact is absent")
    }
    fn support(&self, i: usize) -> Result<(usize, DVec3)> {
        self.supports
            .get(i)
            .copied()
            .context("Equilibrium support is absent")
    }
    fn anchor(&self, i: usize) -> Result<usize> {
        self.anchored
            .get(i)
            .copied()
            .context("Equilibrium anchor is absent")
    }
}
pub struct StoredProblem {
    state: Box<dyn ValidationState>,
    size: ProblemSize,
    tolerances: (f64, f64),
}
impl StoredProblem {
    pub fn new(state: Box<dyn ValidationState>, force: f64, torque: f64) -> Self {
        Self {
            state,
            size: ProblemSize::default(),
            tolerances: (force, torque),
        }
    }
    pub fn push_body(&mut self, brick: Brick, mass: f64) -> Result<()> {
        let i = self.size.bodies;
        if self.state.get(&key(1, &[brick.id as usize]))?.is_some() {
            return Err(unavailable("duplicate canonical equilibrium body"));
        }
        self.state
            .put(key(1, &[brick.id as usize]), Record::Index(i))?;
        self.state.put(
            key(0, &[i]),
            Record::Body {
                brick: Box::new(brick),
                mass,
            },
        )?;
        self.size.bodies += 1;
        Ok(())
    }
    pub fn push_joint(&mut self, joint: JointSpec) -> Result<()> {
        self.state
            .put(key(2, &[self.size.joints]), Record::Joint(Box::new(joint)))?;
        self.size.joints += 1;
        Ok(())
    }
    pub fn push_contact(&mut self, c: Contact) -> Result<()> {
        self.state.put(
            key(3, &[self.size.contacts]),
            Record::Contact {
                lower: c.lower,
                upper: c.upper,
                patch: c.patch,
            },
        )?;
        self.size.contacts += 1;
        Ok(())
    }
    pub fn push_support(&mut self, body: usize, point: DVec3) -> Result<()> {
        self.state.put(
            key(4, &[self.size.supports]),
            Record::Support {
                body,
                point: point.to_array(),
            },
        )?;
        self.size.supports += 1;
        Ok(())
    }
    pub fn push_anchor(&mut self, body: usize) -> Result<()> {
        self.state
            .put(key(5, &[self.size.anchors]), Record::Index(body))?;
        self.size.anchors += 1;
        Ok(())
    }
}
impl ProblemInput for StoredProblem {
    fn size(&self) -> ProblemSize {
        self.size
    }
    fn tolerances(&self) -> (f64, f64) {
        self.tolerances
    }
    fn body(&self, i: usize) -> Result<Cow<'_, Brick>> {
        match required(self.state.as_ref(), &key(0, &[i]))? {
            Record::Body { brick, .. } if i < self.size.bodies => Ok(Cow::Owned(*brick)),
            _ => Err(unavailable("canonical body is absent or invalid")),
        }
    }
    fn body_index(&self, id: u32) -> Result<Option<usize>> {
        match self.state.get(&key(1, &[id as usize]))? {
            Some(Record::Index(i)) if i < self.size.bodies => Ok(Some(i)),
            None => Ok(None),
            _ => Err(unavailable("invalid canonical body identity")),
        }
    }
    fn mass(&self, i: usize) -> Result<f64> {
        match required(self.state.as_ref(), &key(0, &[i]))? {
            Record::Body { mass, .. } if i < self.size.bodies => Ok(mass),
            _ => Err(unavailable("canonical mass is absent or invalid")),
        }
    }
    fn joint(&self, i: usize) -> Result<Cow<'_, JointSpec>> {
        match required(self.state.as_ref(), &key(2, &[i]))? {
            Record::Joint(joint) if i < self.size.joints => Ok(Cow::Owned(*joint)),
            _ => Err(unavailable("canonical joint is absent or invalid")),
        }
    }
    fn contact(&self, i: usize) -> Result<Contact> {
        match required(self.state.as_ref(), &key(3, &[i]))? {
            Record::Contact {
                lower,
                upper,
                patch,
            } if i < self.size.contacts => Ok(Contact {
                lower,
                upper,
                patch,
            }),
            _ => Err(unavailable("canonical contact is absent or invalid")),
        }
    }
    fn support(&self, i: usize) -> Result<(usize, DVec3)> {
        match required(self.state.as_ref(), &key(4, &[i]))? {
            Record::Support { body, point } if i < self.size.supports => {
                Ok((body, DVec3::from_array(point)))
            }
            _ => Err(unavailable("canonical support is absent or invalid")),
        }
    }
    fn anchor(&self, i: usize) -> Result<usize> {
        match required(self.state.as_ref(), &key(5, &[i]))? {
            Record::Index(body) if i < self.size.anchors => Ok(body),
            _ => Err(unavailable("canonical anchor is absent or invalid")),
        }
    }
}
pub(super) fn body_index(problem: &dyn ProblemInput, id: u32) -> Result<usize> {
    problem
        .body_index(id)?
        .context("Equilibrium joint body is absent")
}
pub(super) fn weight(problem: &dyn ProblemInput) -> Result<f64> {
    let mut sum = 0.;
    for i in 0..problem.size().bodies {
        sum += problem.mass(i)?;
    }
    let weight = sum * crate::physical::GRAVITY_M_S2;
    ensure!(weight.is_finite(), "Equilibrium weight overflow");
    Ok(weight)
}
pub(super) fn force_unit(problem: &dyn ProblemInput) -> Result<f64> {
    Ok((weight(problem)? / problem.size().bodies.max(1) as f64).max(1e-3))
}

/// Owned local input is bounded by the local body/column/KKT budgets before
/// allocation. A whole physical component is never a hidden fallback.
pub(super) struct LocalInput {
    pub bricks: Vec<Brick>,
    pub joints: Vec<JointSpec>,
    pub contacts: Vec<Contact>,
    pub supports: Vec<(usize, DVec3)>,
    pub anchors: Vec<usize>,
    pub masses: Vec<f64>,
    pub tolerances: (f64, f64),
}
impl LocalInput {
    pub fn problem(&self) -> Problem<'_> {
        Problem {
            bricks: Bodies::all(&self.bricks),
            joints: &self.joints,
            contacts: self.contacts.clone(),
            supports: self.supports.clone(),
            anchored: self.anchors.clone(),
            masses: self.masses.clone(),
            force_tolerance: self.tolerances.0,
            torque_tolerance: self.tolerances.1,
        }
    }
}
pub(super) fn materialize(
    problem: &dyn ProblemInput,
    bodies: &[usize],
    joints: &[usize],
    contacts: &[usize],
    supports: &[usize],
    anchors: &[usize],
) -> Result<LocalInput> {
    let index: HashMap<_, _> = bodies.iter().enumerate().map(|(i, &g)| (g, i)).collect();
    Ok(LocalInput {
        bricks: bodies
            .iter()
            .map(|&i| problem.body(i).map(Cow::into_owned))
            .collect::<Result<_>>()?,
        joints: joints
            .iter()
            .map(|&i| problem.joint(i).map(Cow::into_owned))
            .collect::<Result<_>>()?,
        contacts: contacts
            .iter()
            .map(|&i| {
                let c = problem.contact(i)?;
                Ok(Contact {
                    lower: *index
                        .get(&c.lower)
                        .context("Local contact endpoint is absent")?,
                    upper: *index
                        .get(&c.upper)
                        .context("Local contact endpoint is absent")?,
                    patch: c.patch,
                })
            })
            .collect::<Result<_>>()?,
        supports: supports
            .iter()
            .map(|&i| {
                let (b, p) = problem.support(i)?;
                Ok((*index.get(&b).context("Local support is absent")?, p))
            })
            .collect::<Result<_>>()?,
        anchors: anchors
            .iter()
            .map(|&i| {
                index
                    .get(&problem.anchor(i)?)
                    .copied()
                    .context("Local anchor is absent")
            })
            .collect::<Result<_>>()?,
        masses: bodies
            .iter()
            .map(|&i| problem.mass(i))
            .collect::<Result<_>>()?,
        tolerances: problem.tolerances(),
    })
}
