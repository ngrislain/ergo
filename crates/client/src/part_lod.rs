//! Reviewed render-only part library. Never used for physical admission or picking.
use ergo_world::{
    Aabb,
    geometry::{self, GeometryCredit, Mesh, PartSpec},
};
use glam::{DVec3, Vec3};
use sha2::{Digest, Sha256};

/// Platform-independent identity of the complete internal float32 vertex stream.
pub fn vertex_hash(mesh: &Mesh) -> String {
    let mut hash = Sha256::new();
    for value in &mesh.vertices {
        hash.update(ergo_world::geometry::vertex_bytes(*value));
    }
    format!("{:x}", hash.finalize())
}

#[derive(Clone, Copy)]
pub struct RenderPart {
    pub id: &'static str,
    shape: Shape,
}
#[derive(Clone, Copy)]
enum Shape {
    Rectangular(PartSpec),
    Frame {
        half_width: f64,
        height: f64,
        aperture_x: f64,
        aperture_top: f64,
        aperture_bottom: f64,
    },
    Round4,
    TechnicBrick(u32),
}

impl RenderPart {
    pub fn resolve(mesh: &Mesh) -> Option<Self> {
        let alias = mesh.part.starts_with("view/") || mesh.part.starts_with("readback-variants/");
        let candidate = if alias {
            mesh.part.rsplit('/').next()?
        } else {
            &mesh.part
        };
        let canonical = ergo_world::catalog::canonical_part(candidate);
        let id = ergo_world::part_library::get(&canonical)
            .filter(|entry| entry.catalogue.derivation.is_none())?
            .catalogue
            .id
            .as_str();
        let shape = if let Some(spec) = geometry::part_spec(id) {
            // An alias string, attribution or nominal box alone is not proof of
            // shape. Match complete parametric vertices or a reviewed exact
            // source fingerprint, independently of physical-record variants.
            if alias {
                let expected = geometry::fallback_mesh(id);
                let known = if mesh.quality == "parametric" {
                    mesh.vertices == expected.vertices
                } else if mesh.quality == "source" {
                    let fingerprint =
                        ergo_world::part_library::get(id).map(|entry| entry.vertex_hash.as_str());
                    fingerprint.is_some_and(|expected| vertex_hash(mesh) == expected)
                } else {
                    false
                };
                if !known
                    || mesh.body_bounds.min != expected.body_bounds.min
                    || mesh.body_bounds.max != expected.body_bounds.max
                {
                    return None;
                }
            }
            Shape::Rectangular(spec)
        } else {
            if mesh.quality != "source" {
                return None;
            }
            let technic = match id {
                "6541.dat" => Some(1),
                "3700.dat" => Some(2),
                "3701.dat" => Some(4),
                "3894.dat" => Some(6),
                "3702.dat" => Some(8),
                "3895.dat" => Some(12),
                "3703.dat" => Some(16),
                _ => None,
            };
            if let Some(length) = technic {
                let known = ergo_world::part_library::get(id)
                    .map(|entry| entry.vertex_hash.as_str())
                    .is_some_and(|expected| vertex_hash(mesh) == expected);
                let bounds = geometry::nominal_body_bounds(id)?;
                if !known
                    || mesh.body_bounds.min != bounds.min
                    || mesh.body_bounds.max != bounds.max
                {
                    return None;
                }
                Shape::TechnicBrick(length)
            } else {
                if alias {
                    return None;
                }
                match id {
                    "60594.dat" => Shape::Frame {
                        half_width: 2.,
                        height: 3.,
                        aperture_x: 1.8,
                        aperture_top: -4. / 24.,
                        aperture_bottom: -64. / 24.,
                    },
                    "60592.dat" => Shape::Frame {
                        half_width: 1.,
                        height: 2.,
                        aperture_x: 0.75,
                        aperture_top: -5. / 24.,
                        aperture_bottom: -40. / 24.,
                    },
                    "87081.dat" => Shape::Round4,
                    _ => return None,
                }
            }
        };
        Some(Self { id, shape })
    }
    pub fn rectangular(self) -> Option<PartSpec> {
        match self.shape {
            Shape::Rectangular(spec) => Some(spec),
            _ => None,
        }
    }
    pub fn studded(self) -> bool {
        self.rectangular().is_none_or(|spec| spec.studs)
    }
    pub fn mesh(self, level: u8) -> Mesh {
        assert!((1..=2).contains(&level));
        if let Shape::TechnicBrick(length) = self.shape {
            return technic_brick(self.id, length, level);
        }
        if let Shape::Rectangular(spec) = self.shape {
            let bounds = geometry::nominal_body_bounds(self.id).unwrap();
            if level == 2 {
                return geometry::body_mesh(self.id, bounds);
            }
            let mut mesh = geometry::body_mesh(
                self.id,
                Aabb::new(
                    bounds.min + DVec3::splat(0.012),
                    bounds.max - DVec3::new(0.012, 0., 0.012),
                ),
            );
            mesh.quality = "render_lod".into();
            mesh.body_bounds = bounds;
            mesh.bounds = bounds;
            if spec.studs {
                for x in 0..spec.width {
                    for z in 0..spec.depth {
                        cylinder(
                            &mut mesh.vertices,
                            Vec3::new(
                                x as f32 + 0.5 - spec.width as f32 / 2.,
                                0.,
                                z as f32 + 0.5 - spec.depth as f32 / 2.,
                            ),
                            0.3,
                            0.,
                            0.,
                            1. / 6.,
                            8,
                        );
                    }
                }
                mesh.bounds.max.y = 1. / 6.;
            }
            return mesh;
        }
        let bounds = geometry::nominal_body_bounds(self.id).unwrap();
        let mut mesh = geometry::body_mesh(self.id, bounds);
        mesh.quality = "render_lod".into();
        mesh.vertices.clear();
        let author = match self.shape {
            Shape::Frame {
                half_width,
                height,
                aperture_x,
                aperture_top,
                aperture_bottom,
            } => {
                // Front aperture extrema, in official LDraw units converted by
                // X/Z=20 and Y=-24. Retain the opening through the whole depth;
                // glazing recesses, clip pockets and underside tubes are omitted.
                for (min, max) in [
                    ([-half_width, -height, -0.5], [-aperture_x, 0., 0.5]),
                    ([aperture_x, -height, -0.5], [half_width, 0., 0.5]),
                    ([-aperture_x, aperture_top, -0.5], [aperture_x, 0., 0.5]),
                    (
                        [-aperture_x, -height, -0.5],
                        [aperture_x, aperture_bottom, 0.5],
                    ),
                ] {
                    mesh.vertices.extend(
                        geometry::body_mesh(
                            self.id,
                            Aabb::new(DVec3::from_array(min), DVec3::from_array(max)),
                        )
                        .vertices,
                    );
                }
                if level == 1 {
                    for x in [-0.5, 0.5] {
                        cylinder(
                            &mut mesh.vertices,
                            Vec3::new(x, 0., 0.),
                            0.3,
                            0.,
                            0.,
                            1. / 6.,
                            8,
                        );
                    }
                }
                if self.id == "60594.dat" {
                    "J.C. Tchang [tchang]"
                } else {
                    "Michael Heidemann [mikeheide]; LEGO Universe Team"
                }
            }
            Shape::Round4 => {
                // 87081s01: radius 40 LDU, body Y=0..24, central
                // opening radius 7 LDU. Retain the round outline and pinhole.
                cylinder(
                    &mut mesh.vertices,
                    Vec3::ZERO,
                    2.,
                    0.35,
                    -1.,
                    0.,
                    if level == 1 { 24 } else { 12 },
                );
                if level == 1 {
                    // Source stug-2x2 groups at X=+/-20 plus four Z=+/-30 studs.
                    for x in [-1.5, -0.5, 0.5, 1.5] {
                        for z in [-0.5, 0.5] {
                            cylinder(
                                &mut mesh.vertices,
                                Vec3::new(x, 0., z),
                                0.3,
                                0.,
                                0.,
                                1. / 6.,
                                8,
                            );
                        }
                    }
                    for x in [-0.5, 0.5] {
                        for z in [-1.5, 1.5] {
                            cylinder(
                                &mut mesh.vertices,
                                Vec3::new(x, 0., z),
                                0.3,
                                0.,
                                0.,
                                1. / 6.,
                                8,
                            );
                        }
                    }
                }
                "Magnus Forsberg [MagFors]; Orion Pobursky [OrionP]"
            }
            Shape::Rectangular(_) => unreachable!(),
            Shape::TechnicBrick(_) => unreachable!(),
        };
        mesh.bounds.max.y = if level == 1 { 1. / 6. } else { 0. };
        mesh.credits.push(GeometryCredit {
            file: self.id.into(),
            source_url: format!(
                "https://library.ldraw.org/library/official/parts/{}",
                self.id
            ),
            author: author.into(),
            license:
                "CC BY 4.0; simplified display geometry derived from official LDraw dimensions"
                    .into(),
        });
        if matches!(self.shape, Shape::Round4) {
            mesh.credits.push(GeometryCredit {
                file: "s/87081s01.dat".into(),
                source_url: "https://library.ldraw.org/library/official/parts/s/87081s01.dat"
                    .into(),
                author: author.into(),
                license: "CC BY 4.0".into(),
            });
        }
        mesh
    }
}

fn technic_brick(id: &str, length: u32, level: u8) -> Mesh {
    let bounds = geometry::nominal_body_bounds(id).unwrap();
    let mut mesh = geometry::body_mesh(id, bounds);
    mesh.quality = "render_lod".into();
    mesh.vertices.clear();
    let half = length as f32 / 2.;
    // Source pegholes: centre Y=10 LDU, inner radius=6 LDU. Counterbores,
    // underside tubes and mould detail are omitted, never the through-bores.
    let centers: Vec<f32> = if length == 1 {
        vec![0.]
    } else {
        (1..length).map(|x| x as f32 - half).collect()
    };
    let cy: f32 = -10. / 24.;
    let rx: f32 = 6. / 20.;
    let ry: f32 = 6. / 24.;
    let segments = if level == 1 { 12 } else { 6 };
    for &x in &centers {
        let mut angles: Vec<f32> = (0..segments)
            .map(|i| i as f32 * std::f32::consts::TAU / segments as f32)
            .collect();
        for dx in [-0.5, 0.5] {
            for y in [-1., 0.] {
                angles.push(
                    ((y - cy) / ry)
                        .atan2(dx / rx)
                        .rem_euclid(std::f32::consts::TAU),
                );
            }
        }
        angles.sort_by(f32::total_cmp);
        angles.dedup_by(|a, b| (*a - *b).abs() < 1e-5);
        let points = |angle: f32, z: f32| {
            let dx = rx * angle.cos();
            let dy = ry * angle.sin();
            let tx = if dx.abs() < 1e-7 {
                f32::INFINITY
            } else {
                0.5 / dx.abs()
            };
            let ty = if dy.abs() < 1e-7 {
                f32::INFINITY
            } else {
                (if dy > 0. { -cy } else { -1. - cy }) / dy
            };
            let t = tx.min(ty);
            (
                Vec3::new(x + dx * t, cy + dy * t, z),
                Vec3::new(x + dx, cy + dy, z),
            )
        };
        for i in 0..angles.len() {
            let a = angles[i];
            let b = angles[(i + 1) % angles.len()];
            let (oa, ia) = points(a, 0.5);
            let (ob, ib) = points(b, 0.5);
            quad(&mut mesh.vertices, oa, ob, ib, ia);
            let (oa, ia) = points(a, -0.5);
            let (ob, ib) = points(b, -0.5);
            quad(&mut mesh.vertices, oa, ia, ib, ob);
            quad(
                &mut mesh.vertices,
                ia,
                Vec3::new(ia.x, ia.y, 0.5),
                Vec3::new(ib.x, ib.y, 0.5),
                ib,
            );
        }
    }
    let end = if length == 1 { half } else { half - 0.5 };
    quad(
        &mut mesh.vertices,
        Vec3::new(-end, 0., -0.5),
        Vec3::new(-end, 0., 0.5),
        Vec3::new(end, 0., 0.5),
        Vec3::new(end, 0., -0.5),
    );
    quad(
        &mut mesh.vertices,
        Vec3::new(-end, -1., -0.5),
        Vec3::new(end, -1., -0.5),
        Vec3::new(end, -1., 0.5),
        Vec3::new(-end, -1., 0.5),
    );
    if length > 1 {
        for (lo, hi) in [(-half, -half + 0.5), (half - 0.5, half)] {
            mesh.vertices.extend(
                geometry::body_mesh(
                    id,
                    Aabb::new(
                        DVec3::new(lo as f64, -1., -0.5),
                        DVec3::new(hi as f64, 0., 0.5),
                    ),
                )
                .vertices,
            );
        }
    }
    if level == 1 {
        for stud in 0..length {
            cylinder(
                &mut mesh.vertices,
                Vec3::new(stud as f32 + 0.5 - half, 0., 0.),
                0.3,
                0.2,
                0.,
                1. / 6.,
                8,
            );
        }
    }
    mesh.bounds.max.y = if level == 1 { 1. / 6. } else { 0. };
    mesh
}

fn cylinder(
    out: &mut Vec<f32>,
    centre: Vec3,
    radius: f32,
    inner: f32,
    bottom: f32,
    top: f32,
    segments: u32,
) {
    for index in 0..segments {
        let a = index as f32 * std::f32::consts::TAU / segments as f32;
        let b = (index + 1) as f32 * std::f32::consts::TAU / segments as f32;
        let p =
            |angle: f32, r: f32, y: f32| centre + Vec3::new(angle.cos() * r, y, angle.sin() * r);
        quad(
            out,
            p(a, radius, bottom),
            p(a, radius, top),
            p(b, radius, top),
            p(b, radius, bottom),
        );
        if inner == 0. {
            // Decorative studs have a cap but no hidden bottom face.
            geometry::triangle(
                out,
                p(a, radius, top),
                centre + Vec3::Y * top,
                p(b, radius, top),
                None,
            );
            continue;
        }
        quad(
            out,
            p(a, radius, top),
            p(a, inner, top),
            p(b, inner, top),
            p(b, radius, top),
        );
        quad(
            out,
            p(a, radius, bottom),
            p(b, radius, bottom),
            p(b, inner, bottom),
            p(a, inner, bottom),
        );
        if inner > 0. {
            quad(
                out,
                p(a, inner, bottom),
                p(b, inner, bottom),
                p(b, inner, top),
                p(a, inner, top),
            );
        }
    }
}

fn quad(out: &mut Vec<f32>, a: Vec3, b: Vec3, c: Vec3, d: Vec3) {
    geometry::triangle(out, a, b, c, None);
    geometry::triangle(out, a, c, d, None);
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn technic_display_lods_keep_every_through_bore_and_reject_unverified_sources() {
        for (id, length) in [
            ("6541.dat", 1),
            ("3700.dat", 2),
            ("3701.dat", 4),
            ("3703.dat", 16),
        ] {
            assert!(
                RenderPart::resolve(&source(id)).is_none(),
                "An identity label alone cannot authorize Technic simplification"
            );
            let near = technic_brick(id, length, 1);
            let far = technic_brick(id, length, 2);
            assert!(far.vertices.len() < near.vertices.len());
            assert!(near.vertices.iter().all(|value| value.is_finite()));
            for mesh in [near, far] {
                let centers: Vec<_> = if length == 1 {
                    vec![0.]
                } else {
                    (1..length)
                        .map(|i| f64::from(i) - f64::from(length) / 2.)
                        .collect()
                };
                for x in centers {
                    assert!(
                        geometry::mesh_ray(&mesh, DVec3::new(x, -10. / 24., 2.), -DVec3::Z)
                            .is_none(),
                        "{id} bore must remain open"
                    );
                }
                assert!(
                    geometry::mesh_ray(
                        &mesh,
                        DVec3::new(f64::from(length) / 2. - 0.05, -10. / 24., 2.),
                        -DVec3::Z
                    )
                    .is_some(),
                    "{id} end wall must remain"
                );
                assert_eq!(
                    mesh.body_bounds.min,
                    geometry::nominal_body_bounds(id).unwrap().min
                );
                assert_eq!(
                    mesh.body_bounds.max,
                    geometry::nominal_body_bounds(id).unwrap().max
                );
            }
        }
    }
    fn source(part: &str) -> Mesh {
        let mut mesh = geometry::fallback_mesh(part);
        mesh.quality = "source".into();
        mesh
    }
    #[test]
    fn window_lods_keep_the_aperture_and_frame_datum() {
        for part in ["60594.dat", "60592.dat"] {
            let detail = RenderPart::resolve(&source(part)).unwrap();
            for level in [1, 2] {
                let mesh = detail.mesh(level);
                let middle = (mesh.body_bounds.min.y + mesh.body_bounds.max.y) / 2.;
                assert!(
                    geometry::mesh_ray(&mesh, DVec3::new(0., middle, 3.), -DVec3::Z).is_none(),
                    "Window aperture must stay open"
                );
                assert!(
                    geometry::mesh_ray(
                        &mesh,
                        DVec3::new(mesh.body_bounds.max.x - 0.05, middle, 3.),
                        -DVec3::Z
                    )
                    .is_some()
                );
                assert!(mesh.vertices.iter().all(|value| value.is_finite()));
                assert!(!mesh.credits.is_empty());
                assert!(mesh.physical.is_none());
            }
        }
    }
    #[test]
    fn round_lods_keep_a_round_outline_and_open_pinhole() {
        let detail = RenderPart::resolve(&source("87081.dat")).unwrap();
        for level in [1, 2] {
            let mesh = detail.mesh(level);
            assert!(geometry::mesh_ray(&mesh, DVec3::new(0., 2., 0.), -DVec3::Y).is_none());
            assert!(
                geometry::mesh_ray(&mesh, DVec3::new(1.9, -0.5, 1.9), -DVec3::Y).is_none(),
                "A round part must not become a bounding box"
            );
            assert!(geometry::mesh_ray(&mesh, DVec3::new(1., 2., 0.), -DVec3::Y).is_some());
            assert!(mesh.vertices.len() / 27 < 1000);
        }
    }
    #[test]
    fn aliases_require_exact_known_geometry_not_just_a_part_suffix() {
        let mut mesh = geometry::fallback_mesh("3007.dat");
        mesh.part = "view/frozen-prototype/3007.dat".into();
        assert_eq!(RenderPart::resolve(&mesh).unwrap().id, "3007.dat");
        mesh.vertices[0] += 0.05;
        assert!(RenderPart::resolve(&mesh).is_none());
        let mut mesh = source("3007.dat");
        mesh.part = "view/frozen-prototype/3007.dat".into();
        assert!(RenderPart::resolve(&mesh).is_none());
        assert!(RenderPart::resolve(&source("unreviewed.dat")).is_none());
    }
}
