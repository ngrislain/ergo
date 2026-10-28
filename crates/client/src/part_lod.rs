//! Reviewed render-only part library. Never used for physical admission or picking.
use ergo_world::{
    Aabb,
    geometry::{self, GeometryCredit, Mesh, PartSpec},
};
use glam::{DVec3, Vec3};

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
        let id = ergo_world::catalog::PARTS
            .iter()
            .find(|(id, _)| *id == canonical)?
            .0;
        let shape = if let Some(spec) = geometry::part_spec(id) {
            // An alias string or its nominal box alone is not proof of shape.
            // Only exact known parametric geometry may recover a standard LOD.
            if alias {
                let expected = geometry::fallback_mesh(id);
                if mesh.quality != "parametric"
                    || mesh.vertices != expected.vertices
                    || mesh.body_bounds.min != expected.body_bounds.min
                    || mesh.body_bounds.max != expected.body_bounds.max
                {
                    return None;
                }
            }
            Shape::Rectangular(spec)
        } else {
            if alias || mesh.quality != "source" {
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
