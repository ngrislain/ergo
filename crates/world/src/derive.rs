//! Automatic, **unreviewed** physics profiles derived from official LDraw
//! geometry for the full-library index. Reviewed library entries never use
//! this module; their hand-checked `part_profiles` remain authoritative.
//!
//! Everything is computed in source LDU (LDraw axes, Y down):
//! - top studs are upright placements of the standard stud primitives;
//! - the body envelope is the baked mesh bounds without the stud protrusion;
//! - receiving grids are stud-lattice positions whose underside leaves room for
//!   a 4-LDU stud, grouped into rectangles;
//! - solid volume is a three-axis majority vote of ray-parity voxel fills.
//!
//! None of this measures connector strength or exact contact geometry.
use crate::{
    geometry::Mesh,
    part_profiles::{Nominal, NominalGrid},
};
use anyhow::{Result, ensure};
use glam::{DMat3, DVec3};
use std::collections::{BTreeMap, BTreeSet};

pub const DERIVATION: &str = "auto-v1";
/// ABS density, g/cm³. One LDU is 0.4 mm.
pub const ABS_DENSITY: f64 = 1.05;
const LDU_MM: f64 = 0.4;
const STUD_HEIGHT: f64 = 4.;
const PITCH: f64 = 20.;
const MAX_VOXELS: f64 = 8e6;

/// Upward-facing top-stud primitives (solid, hollow and open variants).
const TOP_STUDS: &[&str] = &[
    "stud.dat",
    "stud2.dat",
    "stud2a.dat",
    "stud6.dat",
    "stud6a.dat",
    "stud10.dat",
    "stud15.dat",
    "stud17a.dat",
];

pub struct Derived {
    pub mesh: Mesh,
    pub nominal: Nominal,
    /// Majority-vote solid volume in LDU³.
    pub solid_ldu3: f64,
}

impl Derived {
    /// Uncalibrated ABS mass estimate in kilograms.
    pub fn abs_mass_kg(&self) -> f64 {
        self.solid_ldu3 * LDU_MM.powi(3) * 1e-3 * ABS_DENSITY * 1e-3
    }
}

pub fn derive(part: &str, resolver: &impl Fn(&str) -> Option<String>) -> Result<Derived> {
    let mut studs = BTreeSet::new();
    let mut mesh = crate::ldraw::bake_mesh_observed(
        part,
        resolver,
        &BTreeMap::new(),
        &mut |name, matrix, position| {
            let base = name.rsplit('/').next().unwrap_or(name);
            if TOP_STUDS.contains(&base) && upright_unit(matrix) {
                studs.insert(key(position));
            }
        },
    )?;
    let triangles = source_triangles(&mesh);
    let mut lo = DVec3::splat(f64::INFINITY);
    let mut hi = DVec3::splat(f64::NEG_INFINITY);
    for p in triangles.iter().flatten() {
        lo = lo.min(*p);
        hi = hi.max(*p);
    }
    ensure!(
        lo.is_finite() && hi.is_finite(),
        "Empty geometry for {part}"
    );
    let studs: Vec<DVec3> = studs.into_iter().map(unkey).collect();
    let mut top = lo.y;
    if let Some(surface) = studs.iter().map(|s| s.y).reduce(f64::min)
        && lo.y >= surface - STUD_HEIGHT - 0.05
    {
        top = surface;
    }
    let body = [[lo.x, top, lo.z], [hi.x, hi.y, hi.z]].map(|p| p.map(quantize));
    ensure!(
        body[1][1] - body[0][1] > 0.05,
        "Degenerate body envelope for {part}"
    );
    let grids = receiving(&Bins::new(&triangles, lo, hi), lo, hi, &studs);
    let mut grids = grids.into_iter();
    let nominal = Nominal {
        bounds_ldu: body,
        studs_ldu: studs.iter().map(|s| s.to_array()).collect(),
        cavity: grids.next(),
        additional_cavities: grids.collect(),
        collision_boxes_ldu: vec![],
    };
    let solid_ldu3 = solid_volume(&triangles, lo, hi);
    let c = crate::ldraw::conversion();
    let bounds = crate::Aabb::new(
        DVec3::from_array(nominal.bounds_ldu[0]),
        DVec3::from_array(nominal.bounds_ldu[1]),
    )
    .transformed(c, DVec3::ZERO);
    mesh.body_bounds = bounds;
    Ok(Derived {
        mesh,
        nominal,
        solid_ldu3,
    })
}

fn upright_unit(m: DMat3) -> bool {
    (m.y_axis - DVec3::Y).length() < 1e-6
        && (m.x_axis.length() - 1.).abs() < 1e-6
        && (m.z_axis.length() - 1.).abs() < 1e-6
        && m.x_axis.y.abs() < 1e-6
        && m.z_axis.y.abs() < 1e-6
}

/// Derived LDU values are quantized to 1/1000 LDU: far below any physical
/// tolerance, and free of float32 conversion noise in the published library.
fn quantize(v: f64) -> f64 {
    let q = (v * 1000.).round() / 1000.;
    if q == 0. { 0. } else { q }
}

/// Positions rounded to 1/1000 LDU for deterministic de-duplication.
fn key(p: DVec3) -> [i64; 3] {
    [p.x, p.y, p.z].map(|v| (v * 1000.).round() as i64)
}
fn unkey(k: [i64; 3]) -> DVec3 {
    DVec3::new(k[0] as f64, k[1] as f64, k[2] as f64) / 1000. + DVec3::ZERO // `-0.` normalizes to `0.`
}

fn source_triangles(mesh: &Mesh) -> Vec<[DVec3; 3]> {
    let inverse = crate::ldraw::conversion().inverse();
    mesh.vertices
        .chunks_exact(27)
        .map(|t| {
            [0, 9, 18].map(|i| {
                inverse * DVec3::new(f64::from(t[i]), f64::from(t[i + 1]), f64::from(t[i + 2]))
            })
        })
        .collect()
}

/// Source triangles binned on a 10-LDU X/Z grid, so socket probes only test
/// nearby faces (a 48 x 48 baseplate has hundreds of thousands).
struct Bins<'a> {
    triangles: &'a [[DVec3; 3]],
    lo: (f64, f64),
    n: (usize, usize),
    bins: Vec<Vec<u32>>,
}
const BIN: f64 = 10.;
impl<'a> Bins<'a> {
    fn new(triangles: &'a [[DVec3; 3]], lo: DVec3, hi: DVec3) -> Self {
        let n = (
            ((hi.x - lo.x) / BIN).ceil().max(1.) as usize,
            ((hi.z - lo.z) / BIN).ceil().max(1.) as usize,
        );
        let mut bins = Self {
            triangles,
            lo: (lo.x, lo.z),
            n,
            bins: vec![Vec::new(); n.0 * n.1],
        };
        for (t, tri) in triangles.iter().enumerate() {
            let min = tri[0].min(tri[1]).min(tri[2]);
            let max = tri[0].max(tri[1]).max(tri[2]);
            let ((x0, x1), (z0, z1)) = bins.range(min.x, max.x, min.z, max.z);
            for x in x0..=x1 {
                for z in z0..=z1 {
                    bins.bins[x * n.1 + z].push(t as u32);
                }
            }
        }
        bins
    }
    fn range(&self, x0: f64, x1: f64, z0: f64, z1: f64) -> ((usize, usize), (usize, usize)) {
        let cell =
            |v: f64, lo: f64, n: usize| (((v - lo) / BIN).floor().max(0.) as usize).min(n - 1);
        (
            (cell(x0, self.lo.0, self.n.0), cell(x1, self.lo.0, self.n.0)),
            (cell(z0, self.lo.1, self.n.1), cell(z1, self.lo.1, self.n.1)),
        )
    }
    /// Nearest hit within `reach` (Möller–Trumbore) among binned candidates.
    fn ray(&self, origin: DVec3, direction: DVec3, reach: f64) -> Option<f64> {
        let end = origin + direction * if reach.is_finite() { reach } else { 0. };
        let ((x0, x1), (z0, z1)) = self.range(
            origin.x.min(end.x),
            origin.x.max(end.x),
            origin.z.min(end.z),
            origin.z.max(end.z),
        );
        let mut candidates: Vec<u32> = (x0..=x1)
            .flat_map(|x| (z0..=z1).flat_map(move |z| self.bins[x * self.n.1 + z].iter().copied()))
            .collect();
        candidates.sort_unstable();
        candidates.dedup();
        let mut nearest = f64::INFINITY;
        for t in candidates {
            let [a, b, c] = self.triangles[t as usize];
            let (e1, e2) = (b - a, c - a);
            let h = direction.cross(e2);
            let det = e1.dot(h);
            if det.abs() < 1e-12 {
                continue;
            }
            let s = origin - a;
            let u = s.dot(h) / det;
            if !(0. ..=1.).contains(&u) {
                continue;
            }
            let q = s.cross(e1);
            let v = direction.dot(q) / det;
            if v < 0. || u + v > 1. {
                continue;
            }
            let t = e2.dot(q) / det;
            if t > 1e-9 && t <= reach {
                nearest = nearest.min(t);
            }
        }
        nearest.is_finite().then_some(nearest)
    }
}

/// Stud-lattice positions whose underside leaves a 4-LDU stud's height free
/// above the bottom plane and is surrounded by gripping material (walls,
/// tubes or pins within reach on every side), grouped into rectangles. The
/// lattice follows the top studs when present, else the body footprint.
fn receiving(bins: &Bins, lo: DVec3, hi: DVec3, studs: &[DVec3]) -> Vec<NominalGrid> {
    // LDraw parts are modelled on 0- or 10-LDU lattice phases; top studs
    // usually share the socket lattice but not always (a single centre stud
    // on a 2 x 2 plate). Sockets open on a downward-facing horizontal plane,
    // which need not be the lowest point (hanging wheel pins). Keep the
    // candidate with the most sockets; ties prefer the lower plane.
    let mut phases: Vec<(f64, f64)> = studs
        .first()
        .map(|s| (s.x.rem_euclid(PITCH), s.z.rem_euclid(PITCH)))
        .into_iter()
        .collect();
    for phase in [(10., 10.), (0., 10.), (10., 0.), (0., 0.)] {
        if !phases
            .iter()
            .any(|p| (p.0 - phase.0).abs() < 0.01 && (p.1 - phase.1).abs() < 0.01)
        {
            phases.push(phase);
        }
    }
    let cells = |grids: &[NominalGrid]| grids.iter().map(|g| g.width * g.depth).sum::<u32>();
    let mut best: Vec<NominalGrid> = Vec::new();
    for level in downward_planes(bins.triangles, hi.y) {
        for &phase in &phases {
            let grids = receiving_phase(bins, lo, DVec3::new(hi.x, level, hi.z), phase);
            if cells(&grids) > cells(&best) {
                best = grids;
            }
        }
    }
    best
}

/// Horizontal planes within one plate (8 LDU) of the lowest point, with at
/// least 10 LDU² of face area, lowest first. Hanging pins or clips may sit
/// slightly below the true socket plane; interior shelves higher up do not
/// open downward and are never socket mouths.
fn downward_planes(triangles: &[[DVec3; 3]], lowest: f64) -> Vec<f64> {
    let mut areas: BTreeMap<i64, f64> = BTreeMap::new();
    for [a, b, c] in triangles {
        if (a.y - b.y).abs() < 0.01 && (a.y - c.y).abs() < 0.01 && a.y >= lowest - 8.1 {
            let area = (*b - *a).cross(*c - *a).length() / 2.;
            *areas.entry((a.y * 100.).round() as i64).or_default() += area;
        }
    }
    areas
        .into_iter()
        .rev()
        .filter(|(_, area)| *area >= 10.)
        .map(|(y, _)| y as f64 / 100.)
        .collect()
}

fn receiving_phase(bins: &Bins, lo: DVec3, hi: DVec3, phase: (f64, f64)) -> Vec<NominalGrid> {
    let bottom = hi.y;
    let height = bottom - lo.y;
    if height < STUD_HEIGHT {
        return vec![];
    }
    let axis = |min: f64, max: f64, phase: f64| {
        let first = ((min + PITCH / 2. - phase - 0.1) / PITCH).ceil();
        let last = ((max - PITCH / 2. - phase + 0.1) / PITCH).floor();
        (first <= last).then(|| (phase + first * PITCH, (last - first) as u32 + 1))
    };
    let (Some((x0, width)), Some((z0, depth))) =
        (axis(lo.x, hi.x, phase.0), axis(lo.z, hi.z, phase.1))
    else {
        return vec![];
    };
    let centre = |i: u32, j: u32| (x0 + f64::from(i) * PITCH, z0 + f64::from(j) * PITCH);
    let mut open = vec![vec![false; depth as usize]; width as usize];
    for i in 0..width {
        for j in 0..depth {
            let (x, z) = centre(i, j);
            // Probe slightly off the exact centre, avoiding coincident edges.
            let origin = DVec3::new(x + 0.013, bottom + 1., z + 0.007);
            let clear = bins
                .ray(origin, -DVec3::Y, f64::INFINITY)
                .is_some_and(|t| t - 1. >= STUD_HEIGHT - 0.5 && t - 1. < height)
                && bins
                    .ray(origin - DVec3::Y * 0.5, DVec3::Y, f64::INFINITY)
                    .is_none();
            open[i as usize][j as usize] = clear && gripped(bins, x, bottom - 2., z);
        }
    }
    let mut grids = Vec::new();
    loop {
        // Largest remaining rectangle of open positions, deterministic order.
        let mut best: Option<(u32, u32, u32, u32)> = None;
        for i0 in 0..width {
            for j0 in 0..depth {
                if !open[i0 as usize][j0 as usize] {
                    continue;
                }
                let mut max_j = depth;
                for i1 in i0..width {
                    let mut j1 = j0;
                    while j1 < max_j && open[i1 as usize][j1 as usize] {
                        j1 += 1;
                    }
                    max_j = j1;
                    if max_j == j0 {
                        break;
                    }
                    let area = (i1 - i0 + 1) * (max_j - j0);
                    if best.is_none_or(|(a, b, c, d)| area > (c - a) * (d - b)) {
                        best = Some((i0, j0, i1 + 1, max_j));
                    }
                }
            }
        }
        let Some((i0, j0, i1, j1)) = best else { break };
        for row in &mut open[i0 as usize..i1 as usize] {
            for cell in &mut row[j0 as usize..j1 as usize] {
                *cell = false;
            }
        }
        let (x, z) = centre(i0, j0);
        grids.push(NominalGrid {
            width: i1 - i0,
            depth: j1 - j0,
            first: [x, z].map(quantize),
            bottom: quantize(bottom),
        });
    }
    grids
}

/// Material within reach of an inserted stud on all sides: the directions of
/// horizontal rays hitting within 11 LDU leave no angular gap over 180°.
/// Empty space under an arch opening or beside a lone pin is not a socket.
fn gripped(bins: &Bins, x: f64, y: f64, z: f64) -> bool {
    let origin = DVec3::new(x + 0.013, y, z + 0.007);
    let hits: Vec<usize> = (0..16)
        .filter(|k| {
            let angle = std::f64::consts::TAU * *k as f64 / 16.;
            let direction = DVec3::new(angle.cos(), 0., angle.sin());
            bins.ray(origin, direction, 11.).is_some()
        })
        .collect();
    hits.len() >= 3
        && hits
            .iter()
            .zip(hits.iter().cycle().skip(1))
            .all(|(a, b)| (b + 15 - a) % 16 < 8)
}

/// Three-axis majority vote of ray-parity voxel fills, robust to the small
/// gaps and coincident faces common in LDraw primitives.
fn solid_volume(triangles: &[[DVec3; 3]], lo: DVec3, hi: DVec3) -> f64 {
    let size = hi - lo;
    let voxel = (size.x * size.y * size.z / MAX_VOXELS).cbrt().max(1.);
    let n = (size / voxel).ceil().max(DVec3::ONE).as_uvec3();
    let (nx, ny, nz) = (n.x as usize, n.y as usize, n.z as usize);
    let index = |x: usize, y: usize, z: usize| (x * ny + y) * nz + z;
    let mut votes = vec![0u8; nx * ny * nz];
    for axis in 0..3 {
        // Columns span the two other axes; fill along `axis`.
        let (u, v) = [(1, 2), (0, 2), (0, 1)][axis];
        let counts = [nx, ny, nz];
        let (nu, nv, na) = (counts[u], counts[v], counts[axis]);
        let mut bins: Vec<Vec<u32>> = vec![Vec::new(); nu * nv];
        for (t, tri) in triangles.iter().enumerate() {
            let cell = |p: DVec3, k: usize, count: usize| {
                (((p[k] - lo[k]) / voxel - 0.5).floor().max(0.) as usize).min(count - 1)
            };
            let min = tri[0].min(tri[1]).min(tri[2]);
            let max = tri[0].max(tri[1]).max(tri[2]);
            for a in cell(min, u, nu)..=(cell(max, u, nu) + 1).min(nu - 1) {
                for b in cell(min, v, nv)..=(cell(max, v, nv) + 1).min(nv - 1) {
                    bins[a * nv + b].push(t as u32);
                }
            }
        }
        let mut hits = Vec::new();
        for a in 0..nu {
            for b in 0..nv {
                let pu = lo[u] + (a as f64 + 0.5) * voxel + 0.00113;
                let pv = lo[v] + (b as f64 + 0.5) * voxel + 0.00071;
                hits.clear();
                for &t in &bins[a * nv + b] {
                    if let Some(h) = column_hit(&triangles[t as usize], u, v, axis, pu, pv) {
                        hits.push(h);
                    }
                }
                hits.sort_by(f64::total_cmp);
                hits.dedup_by(|x, y| (*x - *y).abs() < 1e-6);
                for pair in hits.chunks_exact(2) {
                    let start = ((pair[0] - lo[axis]) / voxel - 0.5).ceil().max(0.) as usize;
                    let end = ((pair[1] - lo[axis]) / voxel - 0.5).floor();
                    if end < 0. {
                        continue;
                    }
                    for c in start..=(end as usize).min(na - 1) {
                        let mut p = [0; 3];
                        p[u] = a;
                        p[v] = b;
                        p[axis] = c;
                        votes[index(p[0], p[1], p[2])] += 1;
                    }
                }
            }
        }
    }
    votes.iter().filter(|&&v| v >= 2).count() as f64 * voxel.powi(3)
}

/// Where the line {u=pu, v=pv} crosses a triangle, as a coordinate on `axis`.
fn column_hit(tri: &[DVec3; 3], u: usize, v: usize, axis: usize, pu: f64, pv: f64) -> Option<f64> {
    let (a, b, c) = (tri[0], tri[1], tri[2]);
    let det = (b[u] - a[u]) * (c[v] - a[v]) - (c[u] - a[u]) * (b[v] - a[v]);
    if det.abs() < 1e-12 {
        return None;
    }
    let s = ((pu - a[u]) * (c[v] - a[v]) - (c[u] - a[u]) * (pv - a[v])) / det;
    let t = ((b[u] - a[u]) * (pv - a[v]) - (pu - a[u]) * (b[v] - a[v])) / det;
    (s >= 0. && t >= 0. && s + t <= 1.)
        .then(|| a[axis] + s * (b[axis] - a[axis]) + t * (c[axis] - a[axis]))
}

#[cfg(test)]
mod tests {
    use super::*;
    /// A synthetic 1 x 1 brick: an open-bottomed shell with a 4-LDU bottom rim
    /// and one stud primitive.
    fn brick(name: &str) -> Option<String> {
        Some(
            match name {
                "brick.dat" => concat!(
                    "0 Synthetic brick\n",
                    "4 16 -10 0 -10 10 0 -10 10 0 10 -10 0 10\n",
                    "4 16 -10 0 -10 -10 24 -10 10 24 -10 10 0 -10\n",
                    "4 16 -10 0 10 10 0 10 10 24 10 -10 24 10\n",
                    "4 16 -10 0 -10 -10 0 10 -10 24 10 -10 24 -10\n",
                    "4 16 10 0 -10 10 24 -10 10 24 10 10 0 10\n",
                    "4 16 -10 24 -10 10 24 -10 10 24 -6 -10 24 -6\n",
                    "4 16 -10 24 6 10 24 6 10 24 10 -10 24 10\n",
                    "4 16 -10 24 -6 -6 24 -6 -6 24 6 -10 24 6\n",
                    "4 16 6 24 -6 10 24 -6 10 24 6 6 24 6\n",
                    "1 16 0 0 0 1 0 0 0 1 0 0 0 1 stud.dat\n",
                ),
                "stud.dat" => concat!(
                    "0 Synthetic stud\n",
                    "4 16 -6 -4 -6 6 -4 -6 6 -4 6 -6 -4 6\n",
                    "4 16 -6 -4 -6 -6 0 -6 6 0 -6 6 -4 -6\n",
                    "4 16 -6 -4 6 6 -4 6 6 0 6 -6 0 6\n",
                ),
                _ => return None,
            }
            .to_owned(),
        )
    }
    #[test]
    fn studs_sockets_and_body_follow_the_source_geometry() {
        let derived = derive("brick.dat", &brick).unwrap();
        let nominal = &derived.nominal;
        assert_eq!(nominal.studs_ldu, [[0., 0., 0.]]);
        // The stud protrusion is visual; the body starts at the top surface.
        assert_eq!(nominal.bounds_ldu, [[-10., 0., -10.], [10., 24., 10.]]);
        let cavity = nominal.cavity.as_ref().expect("open underside socket");
        assert_eq!((cavity.width, cavity.depth, cavity.bottom), (1, 1, 24.));
        assert_eq!(cavity.first, [0., 0.]);
        assert!(nominal.additional_cavities.is_empty());
        assert!(derived.abs_mass_kg() > 0.);
        // A sideways stud is not a top stud.
        let sideways = |name: &str| {
            brick(name).map(|text| {
                text.replace(
                    "1 16 0 0 0 1 0 0 0 1 0 0 0 1 stud.dat",
                    "1 16 0 12 -10 1 0 0 0 0 1 0 -1 0 stud.dat",
                )
            })
        };
        assert!(
            derive("brick.dat", &sideways)
                .unwrap()
                .nominal
                .studs_ldu
                .is_empty()
        );
    }
}
