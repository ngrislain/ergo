use crate::{
    Brick,
    geometry::{Mesh, fallback_mesh},
    ldraw::{Reference, brick_from_reference},
};
use glam::{DMat3, DVec3};
use std::collections::BTreeMap;

pub struct Demo {
    pub bricks: Vec<Brick>,
    pub meshes: BTreeMap<String, Mesh>,
}

impl Default for Demo {
    fn default() -> Self {
        Self::new()
    }
}

impl Demo {
    pub fn new() -> Self {
        Self {
            bricks: Vec::new(),
            meshes: BTreeMap::new(),
        }
    }

    /// Place a catalog part by its lower southwest corner (x east, z north).
    pub fn add(&mut self, part: &str, color: u32, x: f64, y: f64, z: f64) -> u32 {
        let name = format!("{part}.dat");
        let mesh = self
            .meshes
            .entry(name.clone())
            .or_insert_with(|| fallback_mesh(&name));
        let p = DVec3::new(
            x - mesh.body_bounds.min.x,
            y - mesh.body_bounds.min.y,
            z - mesh.body_bounds.min.z,
        );
        let c = crate::ldraw::conversion();
        let reference = Reference {
            color,
            position: c.inverse() * p,
            transform: DMat3::IDENTITY,
            name,
        };
        let id = self.bricks.len() as u32 + 1;
        self.bricks
            .push(brick_from_reference(id, &reference, mesh, &BTreeMap::new()));
        id
    }
}

pub fn pavilion() -> Demo {
    let mut d = Demo::new();
    // Quiet tiled square, intentionally assembled from ordinary catalog bricks.
    for x in (0..48).step_by(4) {
        for z in (0..48).step_by(4) {
            d.add(
                "3031",
                if (x + z) % 8 == 0 { 71 } else { 7 },
                x as f64,
                0.0,
                z as f64,
            );
        }
    }
    for x in (8..40).step_by(4) {
        for z in (8..40).step_by(2) {
            d.add("3020", 19, x as f64, 1.0 / 3.0, z as f64);
        }
    }
    // Perimeter walls, piers and an open central gallery.
    for y in 0..11 {
        for x in 10..38 {
            for z in 10..38 {
                let edge = !(12..36).contains(&x) || !(12..36).contains(&z);
                let pier = (x - 10) % 8 < 2 || (z - 10) % 8 < 2;
                let opening = y > 1 && y < 8 && !pier;
                let tower = !(12..36).contains(&x) && !(12..36).contains(&z);
                if edge && !opening && !tower && x % 2 == 0 && z % 2 == 0 {
                    d.add(
                        "3003",
                        if y == 0 || y == 9 { 28 } else { 19 },
                        x as f64,
                        2.0 / 3.0 + y as f64,
                        z as f64,
                    );
                }
            }
        }
    }
    // Cornice and a stepped teal roof, with a skylight cutout.
    for x in (8..40).step_by(2) {
        for z in (8..40).step_by(2) {
            let tower = !(12..36).contains(&x) && !(12..36).contains(&z);
            if !tower && (!(12..36).contains(&x) || !(12..36).contains(&z)) {
                d.add("3003", 15, x as f64, 11.0 + 2.0 / 3.0, z as f64);
            }
        }
    }
    for level in 0..6 {
        let lo = 10 + level * 2;
        let hi = 38 - level * 2;
        for x in (lo..hi).step_by(2) {
            for z in (lo..hi).step_by(2) {
                let tower = !(12..36).contains(&x) && !(12..36).contains(&z) && level < 4;
                if !tower {
                    d.add(
                        "3003",
                        if level % 2 == 0 { 3 } else { 321 },
                        x as f64,
                        12.0 + 2.0 / 3.0 + level as f64,
                        z as f64,
                    );
                }
            }
        }
    }
    // Corner towers: limestone shafts and warm red caps.
    for tx in [8, 36] {
        for tz in [8, 36] {
            for y in 0..16 {
                for x in (tx..tx + 4).step_by(2) {
                    for z in (tz..tz + 4).step_by(2) {
                        d.add(
                            "3003",
                            if y % 5 == 4 { 15 } else { 19 },
                            x as f64,
                            2.0 / 3.0 + y as f64,
                            z as f64,
                        );
                    }
                }
            }
            for y in 0..3 {
                for x in (tx..tx + 4).step_by(2) {
                    for z in (tz..tz + 4).step_by(2) {
                        if y < 2 || (x == tx && z == tz) {
                            d.add("3003", 12, x as f64, 16.0 + 2.0 / 3.0 + y as f64, z as f64);
                        }
                    }
                }
            }
        }
    }
    // Garden planters, trees and small yellow benches around the plaza.
    for (tx, tz) in [(3, 3), (41, 3), (3, 41), (41, 41)] {
        for y in 0..2 {
            d.add("3003", 15, tx as f64, 1.0 / 3.0 + y as f64, tz as f64);
        }
        for y in 2..6 {
            d.add("3005", 70, tx as f64, 1.0 / 3.0 + y as f64, tz as f64);
        }
        for y in 0..4 {
            for x in 0_i32..4 {
                for z in 0_i32..4 {
                    if (x - 1).pow(2) + (z - 1).pow(2) < 7 - y {
                        d.add(
                            "3005",
                            if y % 2 == 0 { 2 } else { 288 },
                            tx as f64 - 1.0 + x as f64,
                            6.0 + 1.0 / 3.0 + y as f64,
                            tz as f64 - 1.0 + z as f64,
                        );
                    }
                }
            }
        }
    }
    for z in [4.0, 42.0] {
        for x in [16.0, 28.0] {
            d.add("3004", 0, x, 1.0 / 3.0, z);
            d.add("3004", 0, x + 3.0, 1.0 / 3.0, z);
            for dx in [0, 2] {
                d.add("3023", 14, x + dx as f64, 1.0 + 1.0 / 3.0, z);
            }
        }
    }
    d
}

pub fn physics_lab() -> Demo {
    let mut d = Demo::new();
    // A stable stack, an unsupported brick and a deliberately top-heavy column.
    for y in 0..5 {
        d.add("3003", [4, 14, 1, 3, 12][y], 5.0, y as f64, 5.0);
    }
    d.add("3001", 14, 13.0, 0.0, 5.0);
    for y in 0..6 {
        d.add("3005", 19, 21.0, y as f64, 5.0);
    }
    d.add("3001", 4, 20.0, 0.0, 12.0);
    d
}

/// Authored rigid bonds in the architecture example include touching side faces,
/// so slabs behave as assembled slabs rather than independent loose columns.
pub fn authored_bonds(bricks: &[Brick]) -> Vec<crate::JointSpec> {
    let tree = crate::spatial::Octree::new(bricks.iter().map(|b| b.bounds).collect());
    let mut out = Vec::new();
    let mut parent: Vec<_> = (0..bricks.len()).collect();
    fn root(p: &mut [usize], i: usize) -> usize {
        if p[i] != i {
            p[i] = root(p, p[i]);
        }
        p[i]
    }
    for (i, b) in bricks.iter().enumerate() {
        for j in tree.query(b.bounds.expanded(0.001)) {
            if j <= i || crate::spatial::relation(b, &bricks[j]).is_none() {
                continue;
            }
            let a = root(&mut parent, i);
            let other = root(&mut parent, j);
            if a == other {
                continue;
            }
            parent[a] = other;
            out.push(crate::JointSpec {
                a: b.id,
                b: bricks[j].id,
                anchor: (b.bounds.center() + bricks[j].bounds.center()) * 0.5,
                kind: crate::JointKind::Fixed,
                break_force: None,
                break_torque: None,
                strength_source: None,
            });
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn boundary_bricks_are_available_on_both_sides() {
        let mut demo = Demo::new();
        demo.add("3001", 4, 1023.0, 0.0, 20.0);
        let blocks = crate::partition(&demo.bricks).unwrap();
        assert!(blocks.contains_key(&crate::BlockCoord(0, 0, 0)));
        assert!(blocks.contains_key(&crate::BlockCoord(1, 0, 0)));
        assert_eq!(
            blocks[&crate::BlockCoord(0, 0, 0)].bricks[0].id,
            blocks[&crate::BlockCoord(1, 0, 0)].bricks[0].id
        );
    }
}
