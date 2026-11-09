//! Bounded matrix-free balance correction of a candidate reaction witness.
//! This produces candidates only: capacities and unilaterality remain the
//! independent verifier's responsibility, never a linear-solver assumption.
use super::*;

const MAX_WORKSPACE_BYTES: usize = 32 * 1024 * 1024;
const MAX_ITERATIONS: usize = 1024;
const MAX_ACTIVE_PASSES: usize = 4;

struct Operator<'a, 'b> {
    problem: &'a Problem<'b>,
    layout: &'a Layout,
    index: HashMap<u32, usize>,
    active: Vec<bool>,
}
impl Operator<'_, '_> {
    fn lever(&self, body: usize, point: DVec3) -> DVec3 {
        crate::physical::to_meters(point - self.problem.bricks[body].bounds.center()) / 0.008
    }
    fn vector(rows: &[f64], body: usize, offset: usize) -> DVec3 {
        let k = body * 6 + offset;
        DVec3::new(rows[k], rows[k + 1], rows[k + 2])
    }
    fn points(p: crate::Aabb) -> [DVec3; 4] {
        [
            (p.min.x, p.min.z),
            (p.max.x, p.min.z),
            (p.max.x, p.max.z),
            (p.min.x, p.max.z),
        ]
        .map(|(x, z)| DVec3::new(x, p.min.y, z))
    }
    fn add(rows: &mut [f64], body: usize, f: DVec3, m: DVec3) {
        for k in 0..3 {
            rows[body * 6 + k] += f[k];
            rows[body * 6 + 3 + k] += m[k];
        }
    }
    fn permitted(spec: &JointSpec, mut f: DVec3, mut m: DVec3) -> (DVec3, DVec3) {
        if !spec.break_force.is_some_and(|v| v > 0.) {
            f = DVec3::ZERO;
        }
        if let Some(u) = metric_axis(spec) {
            if spec.axial_force == Some(0.) {
                f -= u * f.dot(u);
            }
            let about = u * m.dot(u);
            m = if spec.break_torque.is_some_and(|v| v > 0.) {
                m - about
            } else {
                DVec3::ZERO
            };
            if spec.break_force.is_some_and(|v| v > 0.)
                && spec.friction_torque.is_some_and(|v| v > 0.)
            {
                m += about;
            }
        } else if !spec.break_torque.is_some_and(|v| v > 0.) {
            m = DVec3::ZERO;
        }
        (f, m)
    }
    fn diagonal(&self, diagonal: &mut [f64]) {
        diagonal.fill(0.);
        for spec in self.problem.joints {
            for body in [self.index[&spec.a], self.index[&spec.b]] {
                let r = self.lever(body, spec.anchor);
                for k in 0..3 {
                    let mut e = DVec3::ZERO;
                    e[k] = 1.;
                    let v = e.cross(r);
                    let (f, m) = Self::permitted(spec, v, e);
                    diagonal[body * 6 + k] += Self::permitted(spec, e, DVec3::ZERO).0[k];
                    diagonal[body * 6 + 3 + k] += v.dot(f) + m[k];
                }
            }
        }
        let mut normal = |body: usize, point: DVec3| {
            let m = self.lever(body, point).cross(DVec3::Y);
            diagonal[body * 6 + 1] += 1.;
            for k in 0..3 {
                diagonal[body * 6 + 3 + k] += m[k] * m[k];
            }
        };
        for (i, c) in self.problem.contacts.iter().enumerate() {
            for (k, p) in Self::points(c.patch).into_iter().enumerate() {
                if self.active[self.layout.contact + i * 4 + k] {
                    normal(c.lower, p);
                    normal(c.upper, p);
                }
            }
        }
        for (i, &(body, p)) in self.problem.supports.iter().enumerate() {
            if self.active[self.layout.support + i] {
                normal(body, p);
            }
        }
        for &body in &self.problem.anchored {
            for k in 0..6 {
                diagonal[body * 6 + k] += 1.;
            }
        }
        for value in diagonal {
            *value = value.max(1e-12);
        }
    }
    /// B times dimensionless reactions (force / weight, moment / weight-stud).
    fn apply(&self, x: &[f64], rows: &mut [f64]) {
        rows.fill(0.);
        for (j, spec) in self.problem.joints.iter().enumerate() {
            let (f, m) = wrench(x, self.layout, j);
            for (body, sign) in [(self.index[&spec.a], -1.), (self.index[&spec.b], 1.)] {
                Self::add(
                    rows,
                    body,
                    f * sign,
                    (m + self.lever(body, spec.anchor).cross(f)) * sign,
                );
            }
        }
        for (i, c) in self.problem.contacts.iter().enumerate() {
            for (k, point) in Self::points(c.patch).into_iter().enumerate() {
                let f = DVec3::Y * x[self.layout.contact + i * 4 + k];
                Self::add(rows, c.lower, -f, -self.lever(c.lower, point).cross(f));
                Self::add(rows, c.upper, f, self.lever(c.upper, point).cross(f));
            }
        }
        for (i, &(body, point)) in self.problem.supports.iter().enumerate() {
            let f = DVec3::Y * x[self.layout.support + i];
            Self::add(rows, body, f, self.lever(body, point).cross(f));
        }
        for (i, &body) in self.problem.anchored.iter().enumerate() {
            let c = self.layout.anchor + i * 6;
            Self::add(
                rows,
                body,
                DVec3::new(x[c], x[c + 1], x[c + 2]),
                DVec3::new(x[c + 3], x[c + 4], x[c + 5]),
            );
        }
    }
    /// Q B^T: Q excludes zero-capacity joint directions and inactive normals.
    fn transpose(&self, rows: &[f64], x: &mut [f64]) {
        x.fill(0.);
        for (j, spec) in self.problem.joints.iter().enumerate() {
            let (a, b) = (self.index[&spec.a], self.index[&spec.b]);
            let ma = Self::vector(rows, a, 3);
            let mb = Self::vector(rows, b, 3);
            let f = Self::vector(rows, b, 0) - Self::vector(rows, a, 0)
                + mb.cross(self.lever(b, spec.anchor))
                - ma.cross(self.lever(a, spec.anchor));
            let (f, m) = Self::permitted(spec, f, mb - ma);
            for k in 0..3 {
                x[j * 6 + k] = f[k];
                x[j * 6 + 3 + k] = m[k];
            }
        }
        let normal = |body: usize, point: DVec3| {
            Self::vector(rows, body, 0).y
                + Self::vector(rows, body, 3).dot(self.lever(body, point).cross(DVec3::Y))
        };
        for (i, c) in self.problem.contacts.iter().enumerate() {
            for (k, point) in Self::points(c.patch).into_iter().enumerate() {
                let col = self.layout.contact + i * 4 + k;
                if self.active[col] {
                    x[col] = normal(c.upper, point) - normal(c.lower, point);
                }
            }
        }
        for (i, &(body, point)) in self.problem.supports.iter().enumerate() {
            let col = self.layout.support + i;
            if self.active[col] {
                x[col] = normal(body, point);
            }
        }
        for (i, &body) in self.problem.anchored.iter().enumerate() {
            x[self.layout.anchor + i * 6..self.layout.anchor + (i + 1) * 6]
                .copy_from_slice(&rows[body * 6..(body + 1) * 6]);
        }
    }
}

/// Correct B x = gravity by Jacobi-preconditioned CG on B Q B^T, without
/// ever materializing a whole-island constraint matrix or factorization. An
/// unsuccessful/over-capacity correction is simply not a certificate. At most
/// four active-set passes freeze negative normals at zero; they do not invent
/// reactions. The caller always checks all balances again after clipping.
pub(super) fn correct_workspace<W: ReactionWrite + ?Sized>(
    problem: &Problem,
    layout: &Layout,
    witness: &mut W,
) -> Result<usize> {
    let Some(rows) = problem.bricks.len().checked_mul(6) else {
        return Ok(0);
    };
    let Some(bytes) = rows
        .checked_mul(5 * 8)
        .and_then(|b| layout.columns.checked_mul(9).and_then(|c| b.checked_add(c)))
        .and_then(|b| {
            problem
                .bricks
                .len()
                .checked_mul(64)
                .and_then(|c| b.checked_add(c))
        })
    else {
        return Ok(0);
    };
    if bytes > MAX_WORKSPACE_BYTES {
        return Ok(0);
    }
    let force = representative_force(problem);
    let torque = force * 0.008;
    let unit = |col: usize| {
        if (col < layout.contact && col % 6 >= 3)
            || (col >= layout.anchor && (col - layout.anchor) % 6 >= 3)
        {
            torque
        } else {
            force
        }
    };
    let active: Vec<_> = (0..layout.columns)
        .map(|i| {
            Ok(i < layout.contact
                || i >= layout.anchor
                || witness.reaction(i)? > problem.force_tolerance * 10.)
        })
        .collect::<Result<_>>()?;
    let mut op = Operator {
        problem,
        layout,
        index: problem
            .bricks
            .iter()
            .enumerate()
            .map(|(i, b)| (b.id, i))
            .collect(),
        active,
    };
    let mut scratch = vec![0.; layout.columns];
    let mut residual = vec![0.; rows];
    let mut direction = vec![0.; rows];
    let mut multiplier = vec![0.; rows];
    let mut product = vec![0.; rows];
    let mut diagonal = vec![0.; rows];
    let dot = |a: &[f64], b: &[f64]| a.iter().zip(b).map(|(a, b)| a * b).sum::<f64>();
    let mut iterations = 0;
    for _ in 0..MAX_ACTIVE_PASSES {
        for i in 0..layout.columns {
            scratch[i] = witness.reaction(i)? / unit(i);
        }
        op.apply(&scratch, &mut residual);
        for (body, &mass) in problem.masses.iter().enumerate() {
            for k in 0..6 {
                let target = if k == 1 {
                    mass * crate::physical::GRAVITY_M_S2 / force
                } else {
                    0.
                };
                residual[body * 6 + k] = target - residual[body * 6 + k];
            }
        }
        op.diagonal(&mut diagonal);
        for i in 0..rows {
            direction[i] = residual[i] / diagonal[i];
        }
        multiplier.fill(0.);
        let mut squared = dot(&residual, &direction);
        for _ in 0..MAX_ITERATIONS {
            if !squared.is_finite() || squared <= 0. || dot(&residual, &residual) <= 1e-22 {
                break;
            }
            op.transpose(&direction, &mut scratch);
            op.apply(&scratch, &mut product);
            let denominator = dot(&direction, &product);
            if !denominator.is_finite() || denominator <= f64::MIN_POSITIVE {
                break;
            }
            let step = squared / denominator;
            for i in 0..rows {
                multiplier[i] += step * direction[i];
                residual[i] -= step * product[i];
            }
            let next = residual
                .iter()
                .zip(&diagonal)
                .map(|(r, d)| r * r / d)
                .sum::<f64>();
            let beta = next / squared;
            for i in 0..rows {
                direction[i] = residual[i] / diagonal[i] + beta * direction[i];
            }
            squared = next;
            iterations += 1;
        }
        op.transpose(&multiplier, &mut scratch);
        if scratch.iter().any(|v| !v.is_finite()) {
            return Ok(iterations);
        }
        for i in 0..layout.columns {
            witness.set_reaction(i, witness.reaction(i)? + scratch[i] * unit(i))?;
        }
        let mut changed = false;
        for i in layout.contact..layout.anchor {
            if witness.reaction(i)? < 0. {
                witness.set_reaction(i, 0.)?;
                op.active[i] = false;
                changed = true;
            }
        }
        if !changed {
            break;
        }
    }
    Ok(iterations)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn matrix_free_transpose_matches_the_body_balance_operator() {
        let mut d = crate::demo::Demo::new();
        d.add("3003", 4, 0., 0., 0.);
        d.add("3003", 4, 0., 1., 0.);
        let joints = vec![JointSpec {
            a: 1,
            b: 2,
            anchor: DVec3::new(0.4, 1., 0.7),
            kind: JointKind::Fixed,
            break_force: Some(10.),
            break_torque: Some(1.),
            axial_force: None,
            friction_torque: None,
            strength_source: None,
        }];
        let problem = Problem {
            bricks: Bodies::all(&d.bricks),
            joints: &joints,
            contacts: vec![Contact {
                lower: 0,
                upper: 1,
                patch: crate::Aabb::new(DVec3::new(0., 1., 0.), DVec3::new(2., 1., 2.)),
            }],
            supports: vec![(0, DVec3::ZERO)],
            anchored: vec![0],
            masses: vec![0.001; 2],
            force_tolerance: 1e-6,
            torque_tolerance: 1e-8,
        };
        let layout = Layout::of(&problem);
        let op = Operator {
            problem: &problem,
            layout: &layout,
            index: HashMap::from([(1, 0), (2, 1)]),
            active: vec![true; layout.columns],
        };
        let x: Vec<_> = (0..layout.columns)
            .map(|i| ((i * 7 + 3) as f64).sin())
            .collect();
        let y: Vec<_> = (0..12).map(|i| ((i * 11 + 5) as f64).cos()).collect();
        let mut bx = vec![0.; 12];
        let mut bty = vec![0.; layout.columns];
        op.apply(&x, &mut bx);
        op.transpose(&y, &mut bty);
        let left: f64 = bx.iter().zip(&y).map(|(a, b)| a * b).sum();
        let right: f64 = x.iter().zip(&bty).map(|(a, b)| a * b).sum();
        assert!((left - right).abs() < 1e-12);
        let mut diagonal = vec![0.; 12];
        op.diagonal(&mut diagonal);
        for i in 0..12 {
            let mut e = vec![0.; 12];
            e[i] = 1.;
            op.transpose(&e, &mut bty);
            op.apply(&bty, &mut bx);
            assert!((diagonal[i] - bx[i]).abs() < 2e-12);
        }
    }

    #[test]
    fn correction_is_only_a_candidate_and_cannot_hide_weak_joints_or_missing_support() {
        let mut d = crate::demo::Demo::new();
        d.add("3005", 4, 0., 5., 0.);
        d.add("3005", 4, 1., 5., 0.);
        let joints = vec![JointSpec {
            a: 1,
            b: 2,
            anchor: DVec3::new(1., 5.5, 0.5),
            kind: JointKind::Fixed,
            break_force: Some(10.),
            break_torque: Some(1.),
            axial_force: None,
            friction_torque: None,
            strength_source: None,
        }];
        let make = |joints| Problem {
            bricks: Bodies::all(&d.bricks),
            joints,
            contacts: vec![],
            supports: vec![],
            anchored: vec![0],
            masses: vec![0.001; 2],
            force_tolerance: 1e-6,
            torque_tolerance: 1e-8,
        };
        let p = make(&joints);
        let layout = Layout::of(&p);
        let mut witness = vec![0.; layout.columns];
        assert!(correct_workspace(&p, &layout, witness.as_mut_slice()).unwrap() > 0);
        verify(&p, &witness, &layout).unwrap();
        let mut weak = joints.clone();
        weak[0].break_force = Some(1e-8);
        let p = make(&weak);
        witness.fill(0.);
        correct_workspace(&p, &layout, witness.as_mut_slice()).unwrap();
        assert!(verify(&p, &witness, &layout).is_err());
        let p = Problem {
            anchored: vec![],
            ..p
        };
        let layout = Layout::of(&p);
        let mut witness = vec![0.; layout.columns];
        correct_workspace(&p, &layout, witness.as_mut_slice()).unwrap();
        assert!(verify(&p, &witness, &layout).is_err());
    }
}
