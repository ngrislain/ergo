use crate::{Aabb, Brick, geometry::part_spec};
use glam::DVec3;
use serde::Serialize;

const LEAF_CAPACITY: usize = 24;
const MAX_DEPTH: usize = 16;

pub struct Octree {
    root: Option<Node>,
    bounds: Vec<Aabb>,
}

struct Node {
    bounds: Aabb,
    items: Vec<usize>,
    children: Vec<Node>,
}

impl Octree {
    pub fn frustum(&self, planes: &[[f64; 4]; 6], origin: DVec3) -> Vec<usize> {
        fn visible(bounds: Aabb, planes: &[[f64; 4]; 6], origin: DVec3) -> bool {
            let c = bounds.center() - origin;
            let h = bounds.size() * 0.5;
            planes.iter().all(|p| {
                let n = DVec3::new(p[0], p[1], p[2]);
                n.dot(c) + p[3] + n.abs().dot(h) >= 0.0
            })
        }
        fn visit(
            node: &Node,
            all: &[Aabb],
            planes: &[[f64; 4]; 6],
            origin: DVec3,
            out: &mut Vec<usize>,
        ) {
            if !visible(node.bounds, planes, origin) {
                return;
            }
            out.extend(
                node.items
                    .iter()
                    .copied()
                    .filter(|i| visible(all[*i], planes, origin)),
            );
            for child in &node.children {
                visit(child, all, planes, origin, out);
            }
        }
        let mut out = Vec::new();
        if let Some(root) = &self.root {
            visit(root, &self.bounds, planes, origin, &mut out);
        }
        out
    }
    pub fn new(bounds: Vec<Aabb>) -> Self {
        let root = bounds.iter().copied().reduce(Aabb::union).map(|b| {
            let side = b.size().max_element().max(0.001);
            Node::build(
                Aabb::new(b.min, b.min + DVec3::splat(side)),
                (0..bounds.len()).collect(),
                &bounds,
                0,
            )
        });
        Self { root, bounds }
    }

    pub fn query(&self, bounds: Aabb) -> Vec<usize> {
        let mut out = Vec::new();
        if let Some(root) = &self.root {
            root.query(bounds, &self.bounds, &mut out);
        }
        out
    }

    pub fn ray(&self, origin: DVec3, direction: DVec3) -> Vec<(usize, f64)> {
        let mut out = Vec::new();
        if let Some(root) = &self.root {
            root.ray(origin, direction, &self.bounds, &mut out);
        }
        out.sort_unstable_by(|a, b| a.1.total_cmp(&b.1));
        out
    }

    pub fn node_count(&self) -> usize {
        fn count(n: &Node) -> usize {
            1 + n.children.iter().map(count).sum::<usize>()
        }
        self.root.as_ref().map(count).unwrap_or(0)
    }
}

impl Node {
    fn build(bounds: Aabb, items: Vec<usize>, all: &[Aabb], depth: usize) -> Self {
        if items.len() <= LEAF_CAPACITY || depth >= MAX_DEPTH {
            return Self {
                bounds,
                items,
                children: Vec::new(),
            };
        }
        let half = bounds.size() * 0.5;
        let boxes: Vec<_> = (0..8)
            .map(|i| {
                let min = bounds.min
                    + half
                        * DVec3::new((i & 1) as f64, ((i >> 1) & 1) as f64, ((i >> 2) & 1) as f64);
                Aabb::new(min, min + half)
            })
            .collect();
        let mut buckets: [Vec<usize>; 8] = Default::default();
        let mut parent_items = Vec::new();
        for index in items {
            if let Some(child) = boxes.iter().position(|b| b.contains(all[index])) {
                buckets[child].push(index);
            } else {
                parent_items.push(index);
            }
        }
        let children = buckets
            .into_iter()
            .enumerate()
            .filter(|(_, ids)| !ids.is_empty())
            .map(|(i, ids)| Node::build(boxes[i], ids, all, depth + 1))
            .collect();
        Self {
            bounds,
            items: parent_items,
            children,
        }
    }

    fn query(&self, b: Aabb, all: &[Aabb], out: &mut Vec<usize>) {
        if !self.bounds.intersects(b) {
            return;
        }
        out.extend(self.items.iter().copied().filter(|&i| all[i].intersects(b)));
        for child in &self.children {
            child.query(b, all, out);
        }
    }

    fn ray(&self, p: DVec3, d: DVec3, all: &[Aabb], out: &mut Vec<(usize, f64)>) {
        if self.bounds.ray(p, d).is_none() {
            return;
        }
        for &i in &self.items {
            if let Some(t) = all[i].ray(p, d) {
                out.push((i, t));
            }
        }
        for child in &self.children {
            child.ray(p, d, all, out);
        }
    }
}

#[derive(Clone, Debug, Serialize)]
pub struct Relation {
    pub brick_id: u32,
    pub kind: &'static str,
    pub certainty: &'static str,
}

/// Broad-phase contacts are deliberately not called physical attachment. Only
/// upright, unscaled catalogue parts with registered interfaces infer a connection.
/// Inferred catalogue factory clips still have no assumed strength.
pub fn relation(a: &Brick, b: &Brick) -> Option<Relation> {
    let overlap = a.bounds.max.min(b.bounds.max) - a.bounds.min.max(b.bounds.min);
    let eps = 0.002;
    let touching_axes = (0..3).filter(|&i| overlap[i].abs() < eps).count();
    if touching_axes != 1 || (0..3).any(|i| overlap[i] < -eps) {
        return None;
    }
    let mut inferred = false;
    let mut factory_clip = false;
    if overlap.y.abs() < eps && overlap.x > 0.05 && overlap.z > 0.05 {
        let (lower, upper) = if a.position.y < b.position.y {
            (a, b)
        } else {
            (b, a)
        };
        if let (Some(lo), Some(_hi)) = (part_spec(&lower.part), part_spec(&upper.part)) {
            let upright = |brick: &Brick| {
                let m = brick.transform;
                (m.y_axis - DVec3::Y).length() < 1e-5
                    && [m.x_axis, m.z_axis].iter().all(|v| {
                        (v.length() - 1.0).abs() < 1e-5
                            && v.y.abs() < 1e-5
                            && (v.x.abs().max(v.z.abs()) - 1.0).abs() < 1e-5
                    })
            };
            let aligned = [
                a.bounds.min.x - b.bounds.min.x,
                a.bounds.min.z - b.bounds.min.z,
            ]
            .iter()
            .all(|v| (v - v.round()).abs() < eps);
            inferred = lo.studs && upright(a) && upright(b) && aligned;
        } else if crate::interference::axis_aligned(lower)
            && crate::interference::axis_aligned(upper)
            && (lower.transform.y_axis - DVec3::Y).length() < 1e-5
            && (upper.transform.y_axis - DVec3::Y).length() < 1e-5
        {
            if let Some(grid) = crate::interference::receiving_grid(&upper.part) {
                inferred = crate::interference::top_studs(&lower.part)
                    .into_iter()
                    .any(|stud| {
                        let point = upper.transform.inverse()
                            * (lower.position + lower.transform * stud - upper.position);
                        let u = point.x - grid.first[0];
                        let v = point.z - grid.first[1];
                        (point.y - grid.bottom).abs() < eps
                            && (u - u.round()).abs() < eps
                            && (v - v.round()).abs() < eps
                            && u >= -eps
                            && u <= f64::from(grid.width - 1) + eps
                            && v >= -eps
                            && v <= f64::from(grid.depth - 1) + eps
                    });
            }
            factory_clip = ((lower.part == "970c00.dat" && upper.part == "76382p00.dat")
                || (lower.part == "76382p00.dat"
                    && ["3626a.dat", "3626ap01.dat"].contains(&upper.part.as_str())))
                && (lower.position.x - upper.position.x).abs() < eps
                && (lower.position.z - upper.position.z).abs() < eps;
            inferred |= factory_clip;
        }
    }
    Some(Relation {
        brick_id: b.id,
        kind: if factory_clip {
            "factory clip connection"
        } else if inferred {
            "stud connection"
        } else {
            "bounding-box contact"
        },
        certainty: if inferred { "inferred" } else { "candidate" },
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn octree_keeps_split_crossing_objects_and_matches_brute_force() {
        let bounds: Vec<_> = (0..800)
            .map(|i| {
                let p = DVec3::new((i % 20) as f64, ((i / 20) % 20) as f64, (i / 400) as f64);
                Aabb::new(p - DVec3::splat(0.1), p + DVec3::splat(1.1))
            })
            .collect();
        let tree = Octree::new(bounds.clone());
        for i in 0..30 {
            let p = DVec3::new(i as f64 * 0.7, i as f64 * 0.3, 0.0);
            let q = Aabb::new(p, p + DVec3::splat(2.0));
            let mut actual = tree.query(q);
            actual.sort_unstable();
            let expected: Vec<_> = bounds
                .iter()
                .enumerate()
                .filter(|(_, b)| b.intersects(q))
                .map(|(i, _)| i)
                .collect();
            assert_eq!(actual, expected);
        }
    }
}
