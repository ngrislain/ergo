//! Orthographic line art from the same catalog mesh and SI dimensions as WebGL.
//! This is a projection, not a separately hand-drawn approximation of a brick.
use crate::{
    geometry::{fallback_mesh, mesh_ray, part_spec},
    physical,
};
use glam::{DVec2, DVec3};
use std::{collections::BTreeMap, fmt::Write};

pub fn brick_outline(part: &str) -> Option<String> {
    part_spec(part)?;
    let mesh = fallback_mesh(part);
    let eye = DVec3::new(1., 0.9, 1.4).normalize();
    let right = DVec3::Y.cross(eye).normalize();
    let up = eye.cross(right);
    let project = |p: DVec3| {
        let p = physical::to_meters(p) * 1000.;
        DVec2::new(p.dot(right), -p.dot(up))
    };
    // Quantized vertex keys merge the duplicate vertices of triangle soup.
    let key = |p: DVec3| p.to_array().map(|n| (n * 1_000_000.).round() as i64);
    type Edge = (DVec3, DVec3, Vec<DVec3>);
    let mut edges: BTreeMap<([i64; 3], [i64; 3]), Edge> = BTreeMap::new();
    let mut min = DVec2::splat(f64::INFINITY);
    let mut max = DVec2::splat(f64::NEG_INFINITY);
    for tri in mesh.vertices.chunks_exact(27) {
        let points =
            [0, 9, 18].map(|i| DVec3::new(tri[i] as f64, tri[i + 1] as f64, tri[i + 2] as f64));
        let normal = (physical::to_meters(points[1] - points[0]))
            .cross(physical::to_meters(points[2] - points[0]))
            .normalize_or_zero();
        for i in 0..3 {
            let a = points[i];
            let b = points[(i + 1) % 3];
            min = min.min(project(a));
            max = max.max(project(a));
            let (a, b) = if key(a) <= key(b) { (a, b) } else { (b, a) };
            edges
                .entry((key(a), key(b)))
                .or_insert_with(|| (a, b, vec![]))
                .2
                .push(normal);
        }
    }
    let margin = 1.4;
    let size = max - min + DVec2::splat(margin * 2.);
    let spec = part_spec(part)?;
    let mut svg = format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"{:.4} {:.4} {:.4} {:.4}\" preserveAspectRatio=\"xMidYMid meet\" data-part=\"{part}\" data-body-mm=\"{},{},{}\" aria-hidden=\"true\"><g fill=\"none\" stroke=\"currentColor\" stroke-width=\"0.28\" stroke-linecap=\"round\" stroke-linejoin=\"round\">",
        min.x - margin,
        min.y - margin,
        size.x,
        size.y,
        spec.width as f64 * 8.,
        spec.height * 9.6,
        spec.depth as f64 * 8.
    );
    for (_, (a, b, normals)) in edges {
        let front = normals.iter().any(|n| n.dot(eye) > 1e-6);
        let silhouette = normals.iter().any(|n| n.dot(eye) <= 1e-6);
        let crease = normals.iter().any(|n| normals[0].dot(*n) < 0.65);
        if !front || !(normals.len() == 1 || silhouette || crease) {
            continue;
        }
        // Hidden-line removal against actual triangles. Small segments also
        // correctly hide body edges behind studs, without a hand-drawn mask.
        for i in 0..12 {
            let p = a.lerp(b, i as f64 / 12.);
            let q = a.lerp(b, (i + 1) as f64 / 12.);
            let midpoint = (p + q) * 0.5;
            let origin = physical::to_world(physical::to_meters(midpoint) + eye);
            let direction = physical::to_world(-eye);
            if mesh_ray(&mesh, origin, direction).is_some_and(|t| t < 1. - 1e-7) {
                continue;
            }
            let p = project(p);
            let q = project(q);
            write!(
                svg,
                "<path d=\"M{:.4} {:.4}L{:.4} {:.4}\"/>",
                p.x, p.y, q.x, q.y
            )
            .ok()?;
        }
    }
    svg.push_str("</g></svg>");
    Some(svg)
}

#[cfg(test)]
mod tests {
    #[test]
    fn illustration_uses_the_catalog_dimensions_and_is_an_actual_projection() {
        let svg = super::brick_outline("3003.dat").unwrap();
        assert!(svg.contains("data-body-mm=\"16,9.6,16\""));
        assert!(svg.contains("preserveAspectRatio=\"xMidYMid meet\""));
        assert!(svg.matches("<path").count() > 20);
        assert_ne!(svg, super::brick_outline("3022.dat").unwrap());
        assert!(super::brick_outline("unknown.dat").is_none());
    }
}
