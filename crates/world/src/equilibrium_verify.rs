//! Independent, body-at-a-time reaction checking. Incidence is derived from
//! the canonical problem, never from solver partition plans or their masks.
use super::*;
use crate::validation::{
    ValidationRecord as Record, ValidationState,
    working_state::{key, required, unavailable},
};

#[derive(Clone, Copy, Default)]
struct Sum {
    value: f64,
    correction: f64,
    absolute: f64,
    count: usize,
}
impl Sum {
    fn add(&mut self, value: f64) {
        let next = self.value + value;
        self.correction += if self.value.abs() >= value.abs() {
            (self.value - next) + value
        } else {
            (value - next) + self.value
        };
        self.value = next;
        self.absolute += value.abs();
        self.count += 1;
    }
    fn checked(self) -> Result<(f64, f64)> {
        let value = self.value + self.correction;
        let roundoff = f64::EPSILON * (4. * self.count as f64 + 32.) * self.absolute;
        if !value.is_finite() || !roundoff.is_finite() {
            return Err(Inconclusive("nonfinite/overflowed witness accumulation".into()).into());
        }
        Ok((value, roundoff))
    }
}

pub(super) struct WitnessVerifier {
    bodies: usize,
}
impl WitnessVerifier {
    pub(super) fn new(problem: &dyn ProblemInput, state: &mut dyn ValidationState) -> Result<Self> {
        let n = problem.size().bodies;
        state.clear()?;
        for i in 0..n {
            let id = problem.body(i)?.id;
            ensure!(
                state.get(&key(0, &[id as usize]))?.is_none(),
                "Duplicate witness body IDs"
            );
            state.put(key(0, &[id as usize]), Record::Index(i))?;
            state.put(key(2, &[i]), Record::Index(0))?;
        }
        for i in 0..problem.size().joints {
            let j = problem.joint(i)?;
            let a = Self::index(state, j.a)?;
            let b = Self::index(state, j.b)?;
            Self::add(state, a, 0, i)?;
            Self::add(state, b, 1, i)?;
        }
        for i in 0..problem.size().contacts {
            let c = problem.contact(i)?;
            ensure!(c.lower < n && c.upper < n, "Missing contact body");
            Self::add(state, c.lower, 2, i)?;
            Self::add(state, c.upper, 3, i)?;
        }
        for i in 0..problem.size().supports {
            let body = problem.support(i)?.0;
            ensure!(body < n, "Missing support body");
            Self::add(state, body, 4, i)?;
        }
        for i in 0..problem.size().anchors {
            let body = problem.anchor(i)?;
            ensure!(body < n, "Missing anchor body");
            Self::add(state, body, 5, i)?;
        }
        Ok(Self { bodies: n })
    }
    fn index(state: &dyn ValidationState, id: u32) -> Result<usize> {
        match required(state, &key(0, &[id as usize]))? {
            Record::Index(i) => Ok(i),
            _ => Err(unavailable("witness body identity is absent")),
        }
    }
    fn count(state: &dyn ValidationState, body: usize) -> Result<usize> {
        match required(state, &key(2, &[body]))? {
            Record::Index(i) => Ok(i),
            _ => Err(unavailable("witness incidence count is absent")),
        }
    }
    fn add(state: &mut dyn ValidationState, body: usize, kind: usize, i: usize) -> Result<()> {
        let count = Self::count(state, body)?
            .checked_add(1)
            .ok_or_else(|| unavailable("witness incidence overflow"))?;
        let group = match kind {
            0 | 1 => 0,
            2 | 3 => 1,
            4 => 2,
            _ => 3,
        };
        state.put(key(1, &[body, group, i]), Record::Index((i << 3) | kind))?;
        state.put(key(2, &[body]), Record::Index(count))
    }

    pub(super) fn verify<R: ReactionRead + ?Sized>(
        &self,
        problem: &dyn ProblemInput,
        x: &R,
        layout: &Layout,
        owned: Option<&[bool]>,
        state: &dyn ValidationState,
    ) -> Result<Certificate> {
        use crate::physical::{GRAVITY_M_S2, to_meters};
        ensure!(
            layout == &Layout::of_input(problem)
                && x.reaction_len() == layout.columns
                && self.bodies == problem.size().bodies,
            "Invalid/nonfinite equilibrium witness"
        );
        let mut values = [0.; 128];
        for start in (0..layout.columns).step_by(values.len()) {
            let count = (layout.columns - start).min(values.len());
            x.read_reactions(start, &mut values[..count])?;
            ensure!(
                values[..count].iter().all(|v| v.is_finite()),
                "Invalid/nonfinite equilibrium witness"
            );
        }
        ensure!(
            owned.is_none_or(|v| v.len() == problem.size().bodies),
            "Invalid witness ownership mask"
        );
        let total = input::weight(problem)?;
        ensure!(total.is_finite(), "Invalid witness weight");
        let force_tol = problem.tolerances().0.max(1e-7 * total);
        let torque_tol = problem.tolerances().1.max(1e-9 * total);
        let origin = to_meters(problem.body(0)?.bounds.center());
        let at = |p: DVec3| to_meters(p) - origin;
        let mut certificate = Certificate::default();
        for j in 0..problem.size().joints {
            let joint = problem.joint(j)?;
            let (f, m) = reaction_wrench(x, layout, j)?;
            let (a, b) = (Self::index(state, joint.a)?, Self::index(state, joint.b)?);
            let (part_a, part_b) = (problem.body(a)?.part.clone(), problem.body(b)?.part.clone());
            if f.length() > force_tol || m.length() > torque_tol {
                certificate.loaded_joints += 1;
            }
            let capped = |load: f64, capacity: Option<f64>, tolerance: f64| -> Result<f64> {
                let capacity = capacity.unwrap_or(0.);
                let bound = capacity * (1. + 1e-6) + tolerance;
                if !load.is_finite() || !bound.is_finite() {
                    return Err(Inconclusive("ill-conditioned joint capacity check".into()).into());
                }
                ensure!(
                    load <= bound,
                    "Construction is not static: joint {} ({}) ↔ {} ({}) carries {load:.3e} with capacity {capacity:.3e} (tolerance {tolerance:.1e})",
                    joint.a,
                    part_a,
                    joint.b,
                    part_b
                );
                Ok(if capacity > 0. { load / capacity } else { 0. })
            };
            let mut utilisation = capped(f.length(), joint.break_force, force_tol)?;
            match metric_axis(&joint) {
                None => {
                    utilisation =
                        utilisation.max(capped(m.length(), joint.break_torque, torque_tol)?)
                }
                Some(u) => {
                    let axial = joint
                        .break_force
                        .map(|cap| joint.axial_force.unwrap_or(cap).min(cap));
                    utilisation = utilisation
                        .max(capped(f.dot(u).abs(), axial, force_tol)?)
                        .max(capped(
                            (m - u * m.dot(u)).length(),
                            joint.break_torque,
                            torque_tol,
                        )?)
                        .max(capped(
                            m.dot(u).abs(),
                            joint
                                .friction_torque
                                .filter(|_| joint.break_force.is_some_and(|f| f > 0.)),
                            torque_tol,
                        )?);
                }
            }
            certificate.max_utilisation = certificate.max_utilisation.max(utilisation);
        }
        for k in layout.contact..layout.anchor {
            ensure!(
                x.reaction(k)? >= -force_tol,
                "Construction is not static: tensile contact/ground support"
            );
        }
        for body in 0..problem.size().bodies {
            let brick = problem.body(body)?;
            if owned.is_some_and(|mask| !mask[body]) {
                continue;
            }
            let mut force = [Sum::default(); 3];
            let mut moment = [Sum::default(); 3];
            let mut add = |f: DVec3, p: DVec3, m: DVec3| {
                for k in 0..3 {
                    force[k].add(f[k]);
                    moment[k].add(m[k]);
                    let a = (k + 1) % 3;
                    let b = (k + 2) % 3;
                    moment[k].add(p[a] * f[b]);
                    moment[k].add(-p[b] * f[a]);
                }
            };
            add(
                DVec3::NEG_Y * problem.mass(body)? * GRAVITY_M_S2,
                at(brick.bounds.center()),
                DVec3::ZERO,
            );
            let prefix = key(1, &[body]);
            let mut after = None;
            let mut seen = 0;
            loop {
                let page = state.page(&prefix, after.as_deref(), 128)?;
                if page.is_empty() {
                    break;
                }
                for (key, record) in page {
                    let Record::Index(entry) = record else {
                        return Err(unavailable("invalid witness incidence"));
                    };
                    after = Some(key);
                    seen += 1;
                    let i = entry >> 3;
                    match entry & 7 {
                        0 | 1 => {
                            let (f, m) = reaction_wrench(x, layout, i)?;
                            let sign = if entry & 7 == 0 { -1. } else { 1. };
                            add(f * sign, at(problem.joint(i)?.anchor), m * sign);
                        }
                        2 | 3 => {
                            let c = problem.contact(i)?;
                            let p = c.patch;
                            let sign = if entry & 7 == 2 { -1. } else { 1. };
                            for (corner, (cx, cz)) in [
                                (p.min.x, p.min.z),
                                (p.max.x, p.min.z),
                                (p.max.x, p.max.z),
                                (p.min.x, p.max.z),
                            ]
                            .into_iter()
                            .enumerate()
                            {
                                add(
                                    DVec3::Y * x.reaction(layout.contact + i * 4 + corner)? * sign,
                                    at(DVec3::new(cx, p.min.y, cz)),
                                    DVec3::ZERO,
                                );
                            }
                        }
                        4 => add(
                            DVec3::Y * x.reaction(layout.support + i)?,
                            at(problem.support(i)?.1),
                            DVec3::ZERO,
                        ),
                        5 => {
                            let c = layout.anchor + i * 6;
                            let mut values = [0.; 6];
                            x.read_reactions(c, &mut values)?;
                            add(
                                DVec3::new(values[0], values[1], values[2]),
                                at(brick.bounds.center()),
                                DVec3::new(values[3], values[4], values[5]),
                            );
                        }
                        _ => return Err(unavailable("invalid witness incidence kind")),
                    }
                }
            }
            if seen != Self::count(state, body)? {
                return Err(unavailable("witness incidence lost or gained records"));
            }
            let mut f = DVec3::ZERO;
            let mut m = DVec3::ZERO;
            let mut ef = DVec3::ZERO;
            let mut em = DVec3::ZERO;
            for k in 0..3 {
                (f[k], ef[k]) = force[k].checked()?;
                (m[k], em[k]) = moment[k].checked()?;
            }
            if ef.length() > force_tol || em.length() > torque_tol {
                return Err(Inconclusive(format!(
                    "witness roundoff budget exceeded at brick {}",
                    brick.id
                ))
                .into());
            }
            ensure!(
                f.length() + ef.length() <= force_tol && m.length() + em.length() <= torque_tol,
                "Construction is not static: brick {} ({}) is not in equilibrium (force residual {:.3e} N / {:.3e}, moment residual {:.3e} N m / {:.3e})",
                brick.id,
                brick.part,
                f.length() + ef.length(),
                force_tol,
                m.length() + em.length(),
                torque_tol
            );
        }
        Ok(certificate)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn cancellation_retains_the_small_load_and_exposes_its_roundoff_budget() {
        let mut sum = Sum::default();
        sum.add(0.01);
        sum.add(1e16);
        sum.add(-1e16);
        let (value, budget) = sum.checked().unwrap();
        assert_eq!(value, 0.01);
        assert!(budget > 1e-6);
        let mut overflow = Sum::default();
        overflow.add(f64::MAX);
        overflow.add(f64::MAX);
        assert!(overflow.checked().is_err());
    }
}
