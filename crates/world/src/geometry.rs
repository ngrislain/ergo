use crate::Aabb;
use glam::{DVec3, Vec3};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct GeometryCredit {
    pub file: String,
    pub source_url: String,
    pub author: String,
    pub license: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Mesh {
    pub schema: u32,
    pub part: String,
    pub quality: String,
    /// Unindexed triangles: position.xyz, normal.xyz, color.rgb (-1 inherits instance).
    #[serde(with = "crate::coordinates::mesh_vertices")]
    pub vertices: Vec<f32>,
    pub bounds: Aabb,
    pub body_bounds: Aabb,
    #[serde(default)]
    pub credits: Vec<GeometryCredit>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub physical: Option<crate::physical::PartMeasurement>,
}

#[derive(Clone, Copy, Debug)]
pub struct PartSpec {
    pub width: u32,
    pub depth: u32,
    pub height: f64,
    pub studs: bool,
}

pub fn part_spec(part: &str) -> Option<PartSpec> {
    let (width, depth, height, studs) = match part.trim_end_matches(".dat") {
        "3005" => (1, 1, 1.0, true),
        "3004" => (1, 2, 1.0, true),
        "3010" => (1, 4, 1.0, true),
        "3009" => (1, 6, 1.0, true),
        "3008" => (1, 8, 1.0, true),
        "3003" => (2, 2, 1.0, true),
        "3002" => (2, 3, 1.0, true),
        "3001" => (2, 4, 1.0, true),
        "2456" => (2, 6, 1.0, true),
        "3007" => (2, 8, 1.0, true),
        "4201" => (8, 8, 1.0, true),
        "4204" => (8, 16, 1.0, true),
        "3024" => (1, 1, 1.0 / 3.0, true),
        "3023" => (1, 2, 1.0 / 3.0, true),
        "3710" => (1, 4, 1.0 / 3.0, true),
        "3666" => (1, 6, 1.0 / 3.0, true),
        "3022" => (2, 2, 1.0 / 3.0, true),
        "3021" => (2, 3, 1.0 / 3.0, true),
        "3020" => (2, 4, 1.0 / 3.0, true),
        "3795" => (2, 6, 1.0 / 3.0, true),
        "3034" => (2, 8, 1.0 / 3.0, true),
        "3832" => (2, 10, 1.0 / 3.0, true),
        "4282" => (2, 16, 1.0 / 3.0, true),
        "3031" => (4, 4, 1.0 / 3.0, true),
        "3032" => (4, 6, 1.0 / 3.0, true),
        "3035" => (4, 8, 1.0 / 3.0, true),
        "41539" => (8, 8, 1.0 / 3.0, true),
        "3070b" | "3070" => (1, 1, 1.0 / 3.0, false),
        "3069b" | "3069" => (1, 2, 1.0 / 3.0, false),
        "3068b" | "3068" => (2, 2, 1.0 / 3.0, false),
        "2431" => (1, 4, 1.0 / 3.0, false),
        _ => return None,
    };
    Some(PartSpec {
        // LDraw's basic rectangular parts run their long dimension along X.
        width: depth,
        depth: width,
        height,
        studs,
    })
}

pub fn fallback_mesh(part: &str) -> Mesh {
    let spec = part_spec(part).unwrap_or(PartSpec {
        width: 2,
        depth: 2,
        height: 1.0,
        studs: false,
    });
    let half_w = spec.width as f64 / 2.0;
    let half_d = spec.depth as f64 / 2.0;
    let body = Aabb::new(
        DVec3::new(-half_w, -spec.height, -half_d),
        DVec3::new(half_w, 0.0, half_d),
    );
    let mut mesh = Mesh {
        schema: crate::SCHEMA_VERSION,
        part: part.to_owned(),
        quality: if part_spec(part).is_some() {
            "parametric"
        } else {
            "proxy"
        }
        .into(),
        vertices: Vec::new(),
        bounds: body,
        body_bounds: body,
        credits: Vec::new(),
        physical: None,
    };
    let gap = 0.012;
    add_box(
        &mut mesh.vertices,
        Vec3::new(
            -half_w as f32 + gap,
            -spec.height as f32 + gap,
            -half_d as f32 + gap,
        ),
        Vec3::new(half_w as f32 - gap, 0.0, half_d as f32 - gap),
    );
    if spec.studs {
        for x in 0..spec.width {
            for z in 0..spec.depth {
                add_stud(
                    &mut mesh.vertices,
                    Vec3::new(
                        x as f32 + 0.5 - half_w as f32,
                        0.0,
                        z as f32 + 0.5 - half_d as f32,
                    ),
                );
            }
        }
        mesh.bounds.max.y = 1.0 / 6.0;
    }
    mesh
}

/// Trusted nominal collision/support box, independent of visual source extrema.
pub fn nominal_body_bounds(part: &str) -> Option<Aabb> {
    if let Some(spec) = part_spec(part) {
        return Some(Aabb::new(
            DVec3::new(
                -f64::from(spec.width) / 2.,
                -spec.height,
                -f64::from(spec.depth) / 2.,
            ),
            DVec3::new(f64::from(spec.width) / 2., 0., f64::from(spec.depth) / 2.),
        ));
    }
    crate::part_profiles::profile(part).map(|profile| profile.bounds)
}

pub fn ground_mesh() -> Mesh {
    let body = Aabb::new(DVec3::new(-0.5, -1.0, -0.5), DVec3::new(0.5, 0.0, 0.5));
    let mut vertices = Vec::new();
    add_box(&mut vertices, body.min.as_vec3(), body.max.as_vec3());
    Mesh {
        schema: crate::SCHEMA_VERSION,
        part: "__ground__".into(),
        quality: "virtual_baseplate".into(),
        vertices,
        bounds: body,
        body_bounds: body,
        credits: Vec::new(),
        physical: None,
    }
}
/// One reusable decorative stud; the renderer instances only nearby visible
/// studs rather than allocating all 1,048,576 studs of a storage baseplate.
pub fn ground_stud_mesh() -> Mesh {
    ground_stud_mesh_with_segments(12)
}
pub fn ground_stud_mesh_with_segments(segments: u32) -> Mesh {
    let mut vertices = Vec::new();
    add_stud_segments(&mut vertices, Vec3::ZERO, segments.clamp(6, 48));
    let bounds = Aabb::new(DVec3::new(-0.3, 0., -0.3), DVec3::new(0.3, 1. / 6., 0.3));
    Mesh {
        schema: crate::SCHEMA_VERSION,
        part: "__ground_stud__".into(),
        quality: "decorative_ground_stud".into(),
        vertices,
        bounds,
        body_bounds: bounds,
        credits: Vec::new(),
        physical: None,
    }
}
pub fn body_mesh(part: &str, bounds: Aabb) -> Mesh {
    let mut vertices = Vec::new();
    add_box(&mut vertices, bounds.min.as_vec3(), bounds.max.as_vec3());
    Mesh {
        schema: crate::SCHEMA_VERSION,
        part: part.into(),
        quality: "body_lod".into(),
        vertices,
        bounds,
        body_bounds: bounds,
        credits: Vec::new(),
        physical: None,
    }
}

pub fn triangle(out: &mut Vec<f32>, a: Vec3, b: Vec3, c: Vec3, color: Option<u32>) {
    let normal = (b - a).cross(c - a).normalize_or_zero();
    let rgb = color
        .map(|c| {
            [
                ((c >> 16) & 255) as f32 / 255.0,
                ((c >> 8) & 255) as f32 / 255.0,
                (c & 255) as f32 / 255.0,
            ]
        })
        .unwrap_or([-1.0; 3]);
    for p in [a, b, c] {
        out.extend_from_slice(&p.to_array());
        out.extend_from_slice(&normal.to_array());
        out.extend_from_slice(&rgb);
    }
}

fn add_box(out: &mut Vec<f32>, min: Vec3, max: Vec3) {
    let p = [
        Vec3::new(min.x, min.y, min.z),
        Vec3::new(max.x, min.y, min.z),
        Vec3::new(max.x, max.y, min.z),
        Vec3::new(min.x, max.y, min.z),
        Vec3::new(min.x, min.y, max.z),
        Vec3::new(max.x, min.y, max.z),
        Vec3::new(max.x, max.y, max.z),
        Vec3::new(min.x, max.y, max.z),
    ];
    for [a, b, c, d] in [
        [0, 3, 2, 1],
        [4, 5, 6, 7],
        [0, 4, 7, 3],
        [1, 2, 6, 5],
        [3, 7, 6, 2],
        [0, 1, 5, 4],
    ] {
        triangle(out, p[a], p[b], p[c], None);
        triangle(out, p[a], p[c], p[d], None);
    }
}

fn add_stud(out: &mut Vec<f32>, center: Vec3) {
    add_stud_segments(out, center, 12);
}
fn add_stud_segments(out: &mut Vec<f32>, center: Vec3, segments: u32) {
    let height = 1.0 / 6.0;
    let top = center + Vec3::Y * height;
    for i in 0..segments {
        let a = i as f32 * std::f32::consts::TAU / segments as f32;
        let b = (i + 1) as f32 * std::f32::consts::TAU / segments as f32;
        let p = center + Vec3::new(a.cos() * 0.3, 0.0, a.sin() * 0.3);
        let q = center + Vec3::new(b.cos() * 0.3, 0.0, b.sin() * 0.3);
        triangle(out, p, p + Vec3::Y * height, q + Vec3::Y * height, None);
        triangle(out, p, q + Vec3::Y * height, q, None);
        triangle(out, top, q + Vec3::Y * height, p + Vec3::Y * height, None);
    }
}

pub fn mesh_ray(mesh: &Mesh, origin: DVec3, direction: DVec3) -> Option<f64> {
    let mut nearest = f64::INFINITY;
    for tri in mesh.vertices.chunks_exact(27) {
        let point = |i| DVec3::new(tri[i] as f64, tri[i + 1] as f64, tri[i + 2] as f64);
        let a = point(0);
        let e1 = point(9) - a;
        let e2 = point(18) - a;
        let h = direction.cross(e2);
        let determinant = e1.dot(h);
        if determinant.abs() < 1e-10 {
            continue;
        }
        let inverse = determinant.recip();
        let s = origin - a;
        let u = inverse * s.dot(h);
        if !(0.0..=1.0).contains(&u) {
            continue;
        }
        let q = s.cross(e1);
        let v = inverse * direction.dot(q);
        if v < 0.0 || u + v > 1.0 {
            continue;
        }
        let t = inverse * e2.dot(q);
        if t > 0.0 {
            nearest = nearest.min(t);
        }
    }
    nearest.is_finite().then_some(nearest)
}
