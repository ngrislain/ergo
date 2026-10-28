//! Cached, render-only greedy surfaces of upright standard body boxes.
//! Near pieces, specialty parts, picking and physics never use this geometry.
use super::{DISPLAY_SCALE, Mesh};
use crate::part_lod::RenderPart;
use ergo_world::{Aabb, Brick, geometry};
use glam::DVec3;
use std::collections::BTreeMap;

const AIR: u32 = 0x1000000;
const MAX_VOXELS: usize = 1024 * 1024;
const MAX_MESH_BYTES: usize = 64 * 1024 * 1024;
const CHUNK: f64 = 32.;

pub struct Chunk {
    pub members: Vec<usize>,
    pub bounds: Aabb,
    pub origin: DVec3,
    pub max_diameter: f64,
    pub mesh: Mesh,
}
#[derive(Default)]
pub struct Surfaces {
    pub chunks: Vec<Chunk>,
    pub membership: Vec<Option<usize>>,
}
struct Body {
    index: usize,
    min: [i64; 3],
    max: [i64; 3],
    rgb: u32,
}

impl Surfaces {
    pub fn build(bricks: &[Brick], meshes: &BTreeMap<String, Mesh>) -> Self {
        let parts: BTreeMap<_, _> = meshes
            .iter()
            .map(|(id, mesh)| {
                (
                    id.as_str(),
                    RenderPart::resolve(mesh).and_then(|part| part.rectangular()),
                )
            })
            .collect();
        let mut groups = BTreeMap::<[i64; 3], Vec<Body>>::new();
        for (index, brick) in bricks.iter().enumerate() {
            if parts.get(brick.part.as_str()).is_none_or(Option::is_none)
                || (brick.transform.y_axis - DVec3::Y).length() > 1e-6
                || (brick.transform.x_axis.length_squared() - 1.).abs() > 1e-6
                || (brick.transform.z_axis.length_squared() - 1.).abs() > 1e-6
                || brick.transform.x_axis.dot(brick.transform.z_axis).abs() > 1e-6
                || brick.transform.x_axis.y.abs() > 1e-6
                || brick.transform.z_axis.y.abs() > 1e-6
                || brick
                    .transform
                    .to_cols_array()
                    .iter()
                    .any(|value| value.abs() > 1e-6 && (value.abs() - 1.).abs() > 1e-6)
            {
                continue;
            }
            let scale = DVec3::new(1., 3., 1.);
            let lo = brick.bounds.min * scale;
            let hi = brick.bounds.max * scale;
            if (lo - lo.round()).abs().max_element() > 1e-5
                || (hi - hi.round()).abs().max_element() > 1e-5
            {
                continue;
            }
            let min = lo.round().as_i64vec3().to_array();
            let max = hi.round().as_i64vec3().to_array();
            if (0..3).any(|axis| max[axis] <= min[axis]) {
                continue;
            }
            let key = (brick.bounds.center() / CHUNK)
                .floor()
                .as_i64vec3()
                .to_array();
            groups.entry(key).or_default().push(Body {
                index,
                min,
                max,
                rgb: brick.rgb,
            });
        }
        let mut result = Self {
            chunks: Vec::new(),
            membership: vec![None; bricks.len()],
        };
        let mut bytes = 0;
        for bodies in groups.into_values() {
            if bodies.len() < 8 {
                continue;
            }
            if let Some(chunk) = build(&bodies, bricks) {
                let size = chunk.mesh.vertices.len() * 4;
                if bytes + size > MAX_MESH_BYTES {
                    continue;
                }
                bytes += size;
                for &index in &chunk.members {
                    result.membership[index] = Some(result.chunks.len());
                }
                result.chunks.push(chunk);
            }
        }
        result
    }
}

fn build(bodies: &[Body], bricks: &[Brick]) -> Option<Chunk> {
    let min = std::array::from_fn::<_, 3, _>(|axis| {
        bodies.iter().map(|body| body.min[axis]).min().unwrap()
    });
    let max = std::array::from_fn::<_, 3, _>(|axis| {
        bodies.iter().map(|body| body.max[axis]).max().unwrap()
    });
    let dimensions: [usize; 3] = std::array::from_fn(|axis| (max[axis] - min[axis]) as usize);
    let count = dimensions
        .iter()
        .try_fold(1usize, |count, &dimension| count.checked_mul(dimension))?;
    if count > MAX_VOXELS {
        return None;
    }
    let mut cells = vec![AIR; count];
    let address = |p: [usize; 3]| p[0] + dimensions[0] * (p[1] + dimensions[1] * p[2]);
    for body in bodies {
        for z in body.min[2]..body.max[2] {
            for y in body.min[1]..body.max[1] {
                for x in body.min[0]..body.max[0] {
                    let index = address([
                        (x - min[0]) as usize,
                        (y - min[1]) as usize,
                        (z - min[2]) as usize,
                    ]);
                    if cells[index] != AIR {
                        return None;
                    }
                    cells[index] = body.rgb;
                }
            }
        }
    }
    let mut vertices = Vec::new();
    for axis in 0..3 {
        let u = (axis + 1) % 3;
        let v = (axis + 2) % 3;
        let mut mask = vec![0u32; dimensions[u] * dimensions[v]];
        for plane in 0..=dimensions[axis] {
            for row in 0..dimensions[v] {
                for column in 0..dimensions[u] {
                    let mut p = [0; 3];
                    p[u] = column;
                    p[v] = row;
                    let a = if plane == 0 {
                        AIR
                    } else {
                        p[axis] = plane - 1;
                        cells[address(p)]
                    };
                    let b = if plane == dimensions[axis] {
                        AIR
                    } else {
                        p[axis] = plane;
                        cells[address(p)]
                    };
                    mask[column + row * dimensions[u]] = if a != AIR && b == AIR {
                        (a + 1) | (1 << 25)
                    } else if a == AIR && b != AIR {
                        b + 1
                    } else {
                        0
                    };
                }
            }
            for row in 0..dimensions[v] {
                let mut column = 0;
                while column < dimensions[u] {
                    let value = mask[column + row * dimensions[u]];
                    if value == 0 {
                        column += 1;
                        continue;
                    }
                    let mut width = 1;
                    while column + width < dimensions[u]
                        && mask[column + width + row * dimensions[u]] == value
                    {
                        width += 1;
                    }
                    let mut height = 1;
                    while row + height < dimensions[v]
                        && (0..width)
                            .all(|x| mask[column + x + (row + height) * dimensions[u]] == value)
                    {
                        height += 1;
                    }
                    let mut p = [0f64; 3];
                    p[axis] = plane as f64;
                    p[u] = column as f64;
                    p[v] = row as f64;
                    let mut du = [0.; 3];
                    du[u] = width as f64;
                    let mut dv = [0.; 3];
                    dv[v] = height as f64;
                    let convert = |p: DVec3| (p / DVec3::new(1., 3., 1.)).as_vec3();
                    let p = DVec3::from_array(p);
                    let du = DVec3::from_array(du);
                    let dv = DVec3::from_array(dv);
                    let points = [
                        convert(p),
                        convert(p + du),
                        convert(p + du + dv),
                        convert(p + dv),
                    ];
                    let order = if value & (1 << 25) != 0 {
                        [0, 1, 2, 3]
                    } else {
                        [0, 3, 2, 1]
                    };
                    let rgb = Some((value & !(1 << 25)) - 1);
                    if vertices.len() + 54 > bodies.len() * 12 * 27
                        || (vertices.len() + 54) * 4 > MAX_MESH_BYTES
                    {
                        return None;
                    }
                    geometry::triangle(
                        &mut vertices,
                        points[order[0]],
                        points[order[1]],
                        points[order[2]],
                        rgb,
                    );
                    geometry::triangle(
                        &mut vertices,
                        points[order[0]],
                        points[order[2]],
                        points[order[3]],
                        rgb,
                    );
                    for y in 0..height {
                        for x in 0..width {
                            mask[column + x + (row + y) * dimensions[u]] = 0;
                        }
                    }
                    column += width;
                }
            }
        }
    }
    if vertices.len() / 27 >= bodies.len() * 12 {
        return None;
    }
    let origin = DVec3::new(min[0] as f64, min[1] as f64 / 3., min[2] as f64);
    let size = DVec3::new(
        dimensions[0] as f64,
        dimensions[1] as f64 / 3.,
        dimensions[2] as f64,
    );
    let bounds = bodies
        .iter()
        .map(|body| bricks[body.index].render_bounds)
        .reduce(Aabb::union)
        .unwrap();
    let mesh = Mesh {
        schema: ergo_world::SCHEMA_VERSION,
        part: "__surface__".into(),
        quality: "surface_lod".into(),
        vertices,
        bounds: Aabb::new(DVec3::ZERO, size),
        body_bounds: Aabb::new(DVec3::ZERO, size),
        credits: Vec::new(),
        physical: None,
    };
    Some(Chunk {
        members: bodies.iter().map(|body| body.index).collect(),
        bounds,
        origin,
        max_diameter: bodies
            .iter()
            .map(|body| (bricks[body.index].render_bounds.size() * DISPLAY_SCALE).length())
            .fold(0., f64::max),
        mesh,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use ergo_world::demo::Demo;
    #[test]
    fn a_solid_wall_merges_to_six_quads_and_preserves_surface_colors() {
        let mut demo = Demo::new();
        for x in 0..8 {
            demo.add("3003", 4, x as f64 * 2., 0., 0.);
        }
        let surfaces = Surfaces::build(&demo.bricks, &demo.meshes);
        assert_eq!(surfaces.chunks.len(), 1);
        assert_eq!(surfaces.chunks[0].mesh.vertices.len() / 27, 12);
        assert_eq!(surfaces.chunks[0].members.len(), 8);
        assert!(
            surfaces.chunks[0]
                .mesh
                .vertices
                .chunks_exact(9)
                .all(|vertex| vertex[6] > 0.5)
        );
    }
    #[test]
    fn greedy_surfaces_leave_a_window_open_and_keep_material_boundaries() {
        let mut demo = Demo::new();
        for x in 0..4 {
            for y in 0..4 {
                if x == 1 && y == 1 {
                    continue;
                }
                demo.add(
                    "3003",
                    if x < 2 { 4 } else { 2 },
                    x as f64 * 2.,
                    y as f64,
                    0.,
                );
            }
        }
        let surfaces = Surfaces::build(&demo.bricks, &demo.meshes);
        let chunk = &surfaces.chunks[0];
        let ray = DVec3::new(3., 1.5, 5.) - chunk.origin;
        assert!(geometry::mesh_ray(&chunk.mesh, ray, -DVec3::Z).is_none());
        assert!(
            geometry::mesh_ray(
                &chunk.mesh,
                DVec3::new(0., 1.5, 5.) - chunk.origin,
                -DVec3::Z
            )
            .is_some()
        );
        let colors: std::collections::BTreeSet<_> = chunk
            .mesh
            .vertices
            .chunks_exact(9)
            .map(|v| [v[6].to_bits(), v[7].to_bits(), v[8].to_bits()])
            .collect();
        assert_eq!(colors.len(), 2);
    }
    #[test]
    fn unsupported_or_off_grid_bodies_are_not_changed() {
        let mut demo = Demo::new();
        for x in 0..8 {
            demo.add("3003", 4, x as f64 * 2. + 0.25, 0., 0.);
        }
        assert!(
            Surfaces::build(&demo.bricks, &demo.meshes)
                .chunks
                .is_empty()
        );
        let mut sheared = Demo::new();
        for x in 0..8 {
            sheared.add("3003", 4, x as f64 * 2., 0., 0.);
        }
        for brick in &mut sheared.bricks {
            brick.transform.x_axis += DVec3::Z;
            let mesh = &sheared.meshes[&brick.part];
            brick.bounds = mesh
                .body_bounds
                .transformed(brick.transform, brick.position);
            brick.render_bounds = mesh.bounds.transformed(brick.transform, brick.position);
        }
        assert!(
            Surfaces::build(&sheared.bricks, &sheared.meshes)
                .chunks
                .is_empty(),
            "Affine visual imports must not be filled as axis-aligned bodies"
        );
    }
}
