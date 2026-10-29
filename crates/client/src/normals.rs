//! Render-only, angle-weighted normal fans. Positions, colours and source hashes
//! stay unchanged; legacy frozen triangle soups benefit at GPU upload too.
use crate::scene::DISPLAY_SCALE;
use ergo_world::geometry::Mesh;
use glam::DVec3;
use std::collections::HashMap;

const WELD: f64 = 100_000.;
const MAX_CORNERS: usize = 600_000;
type Point = [i64; 3];

#[derive(Clone, Copy)]
struct Occurrence {
    face: usize,
    low: usize,
    high: usize,
    direction: i8,
}
struct Edge {
    first: Occurrence,
    second: Option<Occurrence>,
    non_manifold: bool,
}
struct Face {
    normal: DVec3,
    angles: [f64; 3],
    authored: bool,
}
struct Fans {
    parent: Vec<usize>,
    sign: Vec<i8>,
    rank: Vec<u8>,
}
impl Fans {
    fn new(count: usize) -> Self {
        Self {
            parent: (0..count).collect(),
            sign: vec![1; count],
            rank: vec![0; count],
        }
    }
    fn find(&mut self, index: usize) -> (usize, i8) {
        let parent = self.parent[index];
        if parent == index {
            return (index, 1);
        }
        let (root, sign) = self.find(parent);
        self.sign[index] *= sign;
        self.parent[index] = root;
        (root, self.sign[index])
    }
    fn join(&mut self, a: usize, b: usize, orientation: i8) {
        let (mut a, sa) = self.find(a);
        let (mut b, sb) = self.find(b);
        if a == b {
            return;
        }
        if self.rank[a] < self.rank[b] {
            std::mem::swap(&mut a, &mut b);
        }
        self.parent[b] = a;
        self.sign[b] = orientation * sa * sb;
        if self.rank[a] == self.rank[b] {
            self.rank[a] += 1;
        }
    }
}

/// Crease-limited smoothing only across two-face shared edges, never point-only
/// contact or non-manifold seams. Existing non-flat artist normals are retained.
pub fn display_vertices(mesh: &Mesh) -> Vec<f32> {
    let mut vertices = mesh.vertices.clone();
    let corners = vertices.len() / 9;
    if !vertices.len().is_multiple_of(27) || corners > MAX_CORNERS {
        return vertices;
    }
    let crease = if matches!(
        mesh.quality.as_str(),
        "render_lod" | "decorative_ground_stud"
    ) {
        65_f64
    } else {
        40_f64
    };
    let cosine = crease.to_radians().cos();
    let mut faces = Vec::with_capacity(corners / 3);
    let mut keys = Vec::<[Point; 3]>::with_capacity(corners / 3);
    for triangle in vertices.chunks_exact(27) {
        let points: [DVec3; 3] = std::array::from_fn(|i| {
            DVec3::new(
                triangle[i * 9] as f64,
                triangle[i * 9 + 1] as f64,
                triangle[i * 9 + 2] as f64,
            )
        });
        let physical = points.map(|point| point * DISPLAY_SCALE);
        let normal = (physical[1] - physical[0])
            .cross(physical[2] - physical[0])
            .normalize_or_zero();
        let angles = std::array::from_fn(|i| {
            let a = physical[(i + 1) % 3] - physical[i];
            let b = physical[(i + 2) % 3] - physical[i];
            a.cross(b).length().atan2(a.dot(b))
        });
        let authored = normal == DVec3::ZERO
            || (0..3).any(|i| {
                let stored = DVec3::new(
                    triangle[i * 9 + 3] as f64,
                    triangle[i * 9 + 4] as f64,
                    triangle[i * 9 + 5] as f64,
                ) / DISPLAY_SCALE;
                stored.normalize_or_zero().dot(normal).abs() < 0.9999
            });
        faces.push(Face {
            normal,
            angles,
            authored,
        });
        keys.push(points.map(|point| (point * WELD).round().as_i64vec3().to_array()));
    }
    // Insertion-ordered edges make both union and accumulation deterministic;
    // the hash map is used only for lookup, never iteration order.
    let mut edge_index = HashMap::new();
    let mut edges: Vec<Edge> = Vec::new();
    for (face, points) in keys.iter().enumerate() {
        if faces[face].normal == DVec3::ZERO {
            continue;
        }
        for i in 0..3 {
            let j = (i + 1) % 3;
            if points[i] == points[j] {
                continue;
            }
            let (key, low, high, direction) = if points[i] < points[j] {
                ((points[i], points[j]), face * 3 + i, face * 3 + j, 1)
            } else {
                ((points[j], points[i]), face * 3 + j, face * 3 + i, -1)
            };
            let occurrence = Occurrence {
                face,
                low,
                high,
                direction,
            };
            if let Some(&index) = edge_index.get(&key) {
                let edge: &mut Edge = &mut edges[index];
                if edge.second.is_some() {
                    edge.non_manifold = true;
                } else {
                    edge.second = Some(occurrence);
                }
            } else {
                edge_index.insert(key, edges.len());
                edges.push(Edge {
                    first: occurrence,
                    second: None,
                    non_manifold: false,
                });
            }
        }
    }
    let mut fans = Fans::new(corners);
    for edge in edges {
        let Some(b) = edge.second else {
            continue;
        };
        if edge.non_manifold || faces[edge.first.face].authored || faces[b.face].authored {
            continue;
        }
        let a = edge.first;
        // Mirrored LDraw subparts can reverse winding. Keep each side's winding
        // sign while averaging in a consistently oriented physical normal fan.
        let orientation = -a.direction * b.direction;
        if faces[a.face].normal.dot(faces[b.face].normal) * f64::from(orientation) + 1e-9 < cosine {
            continue;
        }
        fans.join(a.low, b.low, orientation);
        fans.join(a.high, b.high, orientation);
    }
    let mut sums = vec![DVec3::ZERO; corners];
    let mut counts = vec![0_u32; corners];
    for index in 0..corners {
        let face = &faces[index / 3];
        if face.authored {
            continue;
        }
        let (root, sign) = fans.find(index);
        sums[root] += face.normal * (face.angles[index % 3] * f64::from(sign));
        counts[root] += 1;
    }
    for index in 0..corners {
        if faces[index / 3].authored {
            continue;
        }
        let (root, sign) = fans.find(index);
        if counts[root] < 2 {
            continue;
        }
        let physical = sums[root].normalize_or_zero() * f64::from(sign);
        if physical == DVec3::ZERO {
            continue;
        }
        let normal = (physical * DISPLAY_SCALE).normalize().as_vec3().to_array();
        vertices[index * 9 + 3..index * 9 + 6].copy_from_slice(&normal);
    }
    vertices
}

#[cfg(test)]
mod tests {
    use super::*;
    use ergo_world::geometry;
    #[test]
    fn cylinders_share_radial_side_normals_without_rounding_the_flat_cap() {
        let mesh = geometry::ground_stud_mesh();
        let original = mesh.vertices.clone();
        let smoothed = display_vertices(&mesh);
        for (before, after) in original.chunks_exact(9).zip(smoothed.chunks_exact(9)) {
            assert_eq!(&before[..3], &after[..3]);
            assert_eq!(&before[6..], &after[6..]);
            let n = DVec3::new(after[3] as f64, after[4] as f64, after[5] as f64) / DISPLAY_SCALE;
            let n = n.normalize();
            if before[4] > 0.9 {
                assert!(n.dot(DVec3::Y) > 0.9999);
            } else {
                let radial = DVec3::new(after[0] as f64, 0., after[2] as f64).normalize();
                assert!(n.dot(radial) > 0.9999, "{after:?}");
            }
        }
        assert_eq!(mesh.vertices, original);
    }
    #[test]
    fn crease_threshold_is_physical_and_mirrored_winding_stays_consistent() {
        use glam::Vec3;
        for (angle, mirrored) in [(30_f64, false), (30., true), (45., false)] {
            let mut mesh = geometry::ground_mesh();
            mesh.vertices.clear();
            geometry::triangle(&mut mesh.vertices, Vec3::ZERO, Vec3::Z, Vec3::X, None);
            let d = Vec3::new(
                -angle.to_radians().cos() as f32,
                (angle.to_radians().sin() / DISPLAY_SCALE.y) as f32,
                0.,
            );
            if mirrored {
                geometry::triangle(&mut mesh.vertices, Vec3::ZERO, Vec3::Z, d, None);
            } else {
                geometry::triangle(&mut mesh.vertices, Vec3::Z, Vec3::ZERO, d, None);
            }
            let result = display_vertices(&mesh);
            if angle == 45. {
                assert_eq!(result, mesh.vertices);
                continue;
            }
            let physical = (DVec3::new(result[3] as f64, result[4] as f64, result[5] as f64)
                / DISPLAY_SCALE)
                .normalize();
            let expected = DVec3::new(15_f64.to_radians().sin(), 15_f64.to_radians().cos(), 0.);
            assert!(physical.dot(expected) > 0.9999);
        }
    }
    #[test]
    fn brick_corners_stay_sharp_and_point_contact_is_not_welded() {
        let mesh = geometry::body_mesh(
            "fixture.dat",
            ergo_world::Aabb::new(DVec3::splat(-1.), DVec3::splat(1.)),
        );
        assert_eq!(display_vertices(&mesh), mesh.vertices);
        let mut mesh = mesh;
        mesh.vertices.clear();
        geometry::triangle(
            &mut mesh.vertices,
            glam::Vec3::ZERO,
            glam::Vec3::X,
            glam::Vec3::Z,
            None,
        );
        geometry::triangle(
            &mut mesh.vertices,
            glam::Vec3::ZERO,
            glam::Vec3::new(-1., 0.1, 0.),
            glam::Vec3::NEG_Z,
            None,
        );
        assert_eq!(display_vertices(&mesh), mesh.vertices);
    }
}
