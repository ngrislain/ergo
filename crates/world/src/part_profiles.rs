//! Sourced catalogue averages and nominal architectural collision interfaces.
//! Visual geometry must still be resolved from the real catalogue asset. These
//! boxes/grids are model approximations, not connector-strength measurements.
use crate::Aabb;
use glam::DVec3;
use serde::{Deserialize, Serialize};

/// Reviewed source-local proxy data. Coordinates are LDU, not public axes.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Nominal {
    pub bounds_ldu: [[f64; 3]; 2],
    pub studs_ldu: Vec<[f64; 3]>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cavity: Option<NominalGrid>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub additional_cavities: Vec<NominalGrid>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub collision_boxes_ldu: Vec<[[f64; 3]; 2]>,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct NominalGrid {
    pub width: u32,
    pub depth: u32,
    pub first: [f64; 2],
    pub bottom: f64,
}

#[derive(Clone, Copy)]
pub struct Grid {
    pub width: u32,
    pub depth: u32,
    /// First receiving stud centre, in internal local X/Z coordinates.
    pub first: [f64; 2],
    pub bottom: f64,
}

pub struct Profile {
    pub bounds: Aabb,
    pub grams: f64,
    pub source: String,
    pub cavity: Option<Grid>,
    pub additional_cavities: Vec<Grid>,
    pub studs: Vec<DVec3>,
}

/// Shared interface lookup for collision clearance and inferred connectivity.
pub fn stud_centres(part: &str) -> Vec<DVec3> {
    if let Some(spec) = crate::geometry::part_spec(part) {
        return if spec.studs {
            centres(
                spec.width,
                spec.depth,
                [
                    0.5 - f64::from(spec.width) / 2.,
                    0.5 - f64::from(spec.depth) / 2.,
                ],
            )
        } else {
            vec![]
        };
    }
    profile(part).map_or_else(Vec::new, |p| p.studs)
}
pub fn receiving_grids(part: &str) -> Vec<Grid> {
    if let Some(spec) = crate::geometry::part_spec(part) {
        return vec![grid(
            spec.width,
            spec.depth,
            [
                0.5 - f64::from(spec.width) / 2.,
                0.5 - f64::from(spec.depth) / 2.,
            ],
            -spec.height,
        )];
    }
    profile(part).map_or_else(Vec::new, |p| {
        p.cavity.into_iter().chain(p.additional_cavities).collect()
    })
}

/// Horizontal bearing planes on the source-reviewed stepped 4444 mould. The
/// broad mass proxy deliberately excludes its hanging wings and thin rear shelf;
/// that does not make their real stud-landing faces disappear.
pub(crate) fn upper_bearing_regions(brick: &crate::Brick) -> Vec<Aabb> {
    let b = brick.bounds;
    let mut faces = vec![Aabb::new(DVec3::new(b.min.x, b.max.y, b.min.z), b.max)];
    if crate::catalog::canonical_id(&brick.part) == "4444.dat"
        && (crate::physical::metric_transform(brick.transform).y_axis - DVec3::Y).length() < 1e-8
    {
        let c = crate::ldraw::conversion();
        // s/4444s01.dat: the end studs land at source Y=24, and the
        // three-stud rear shelf at Y=8. These are rigid mould faces, not bonds.
        for (lo, hi) in [
            ([-50., 24., -10.], [-30., 24., 10.]),
            ([30., 24., -10.], [50., 24., 10.]),
            ([-30., 8., 10.], [30., 8., 30.]),
        ] {
            faces.push(
                Aabb::new(DVec3::from_array(lo), DVec3::from_array(hi))
                    .transformed(c, DVec3::ZERO)
                    .transformed(brick.transform, brick.position),
            );
        }
    }
    faces
}

/// Compound arches and horses bear only on their declared end feet/hooves,
/// never on empty space inside the overall envelope.
pub(crate) fn lower_bearing_regions(brick: &crate::Brick) -> Vec<Aabb> {
    let explicit = crate::part_library::get(&brick.part)
        .and_then(|p| p.catalogue.nominal.as_ref())
        .is_some_and(|n| !n.additional_cavities.is_empty());
    if explicit
        && (crate::physical::metric_transform(brick.transform).y_axis - DVec3::Y).length() < 1e-8
    {
        return receiving_grids(&brick.part)
            .into_iter()
            .map(|g| {
                let lo = DVec3::new(g.first[0] - 0.5, g.bottom, g.first[1] - 0.5);
                let hi = lo + DVec3::new(f64::from(g.width), 0., f64::from(g.depth));
                Aabb::new(lo, hi).transformed(brick.transform, brick.position)
            })
            .collect();
    }
    let b = brick.bounds;
    vec![Aabb::new(b.min, DVec3::new(b.max.x, b.min.y, b.max.z))]
}

pub(crate) fn bearing_bounds(brick: &crate::Brick) -> Aabb {
    upper_bearing_regions(brick)
        .into_iter()
        .fold(brick.bounds, Aabb::union)
}

fn grid(width: u32, depth: u32, first: [f64; 2], bottom: f64) -> Grid {
    Grid {
        width,
        depth,
        first,
        bottom,
    }
}
fn centres(width: u32, depth: u32, first: [f64; 2]) -> Vec<DVec3> {
    (0..width)
        .flat_map(|x| {
            (0..depth)
                .map(move |z| DVec3::new(first[0] + f64::from(x), 0., first[1] + f64::from(z)))
        })
        .collect()
}

/// Source-defined shield face and two grip bars. Their union leaves the empty
/// space behind a held shield free, unlike its overall mass/support envelope.
pub fn collision_boxes(part: &str) -> Vec<Aabb> {
    if let Some(nominal) = crate::part_library::get(part).and_then(|p| p.catalogue.nominal.as_ref())
        && !nominal.collision_boxes_ldu.is_empty()
    {
        let c = crate::ldraw::conversion();
        return nominal
            .collision_boxes_ldu
            .iter()
            .map(|bounds| {
                Aabb::new(DVec3::from_array(bounds[0]), DVec3::from_array(bounds[1]))
                    .transformed(c, DVec3::ZERO)
            })
            .collect();
    }
    let id = crate::catalog::canonical_id(part);
    let boxes = if ["3846.dat", "3846p4h.dat"].contains(&id) {
        vec![
            ([-20., -14., -10.], [20., 32.37, -8.]),
            ([-4., 0., -4.], [4., 18., 4.]),
            ([-4., -4., -8.], [4., 4., 14.]),
        ]
    } else if id == "4497.dat" {
        vec![
            ([-4., 0., -4.], [4., 98., 4.]),
            ([-7., -48., -3.], [7., 0., 3.]),
        ]
    } else if id == "4499.dat" {
        let mut boxes = vec![
            ([-4., -27.3, -4.], [4., 2.6, 4.]),
            ([-2.5, -14.85, -14.], [2.5, -9.85, 44.5]),
            ([-1.25, -17.35, -24.], [1.25, -7.35, -13.]),
        ];
        // Source cylindrical bow limbs and strings. Do not fill the empty
        // space between the grip, curved limbs and string with one solid box.
        let segments = [
            ([0., 2.6, -0.25], [0., 13.1, 1.75], 3.),
            ([0., 13.1, 1.75], [0., 22.1, 6.75], 3.),
            ([0., 22.1, 6.75], [0., 26.9, 12.75], 3.),
            ([0., 26.9, 12.75], [0., 32.0, 20.3], 3.),
            ([0., -12.35, 33.], [0., 30.65, 17.5], 1.75),
        ];
        for (start, end, radius) in segments {
            for mirror in [false, true] {
                let mut start = DVec3::from_array(start);
                let mut end = DVec3::from_array(end);
                if mirror {
                    start.y = -24.7 - start.y;
                    end.y = -24.7 - end.y;
                }
                for i in 0..8 {
                    let a = start.lerp(end, f64::from(i) / 8.);
                    let b = start.lerp(end, f64::from(i + 1) / 8.);
                    let r = DVec3::splat(radius);
                    boxes.push(((a.min(b) - r).to_array(), (a.max(b) + r).to_array()));
                }
            }
        }
        boxes
    } else {
        vec![]
    };
    if !boxes.is_empty() {
        let c = crate::ldraw::conversion();
        return boxes
            .into_iter()
            .map(|(min, max)| {
                Aabb::new(DVec3::from_array(min), DVec3::from_array(max))
                    .transformed(c, DVec3::ZERO)
            })
            .collect();
    }
    crate::geometry::nominal_body_bounds(part)
        .into_iter()
        .collect()
}

/// Native receiving grid of a source-LDU nominal record.
pub fn nominal_grid(g: &NominalGrid) -> Grid {
    // LDraw conversion flips source Z into native south. Start at the
    // opposite end so native grid indices still increase positively.
    grid(
        g.width,
        g.depth,
        [g.first[0] / 20., -g.first[1] / 20. - f64::from(g.depth - 1)],
        -g.bottom / 24.,
    )
}

/// All native receiving grids of a source-LDU nominal record.
pub fn nominal_grids(nominal: &Nominal) -> Vec<Grid> {
    nominal
        .cavity
        .iter()
        .chain(&nominal.additional_cavities)
        .map(nominal_grid)
        .collect()
}

/// Nominal profile of a reviewed or derived library part, in native units.
/// Source meshes and credits are retained separately.
pub fn profile(part: &str) -> Option<Profile> {
    let entry = &crate::part_library::get(part)?.catalogue;
    let nominal = entry.nominal.as_ref()?;
    let c = crate::ldraw::conversion();
    Some(Profile {
        bounds: Aabb::new(
            DVec3::from_array(nominal.bounds_ldu[0]),
            DVec3::from_array(nominal.bounds_ldu[1]),
        )
        .transformed(c, DVec3::ZERO),
        grams: entry.mass_kg * 1000.,
        source: entry.mass_source.clone(),
        studs: nominal
            .studs_ldu
            .iter()
            .map(|p| c * DVec3::from_array(*p))
            .collect(),
        cavity: nominal.cavity.as_ref().map(nominal_grid),
        additional_cavities: nominal
            .additional_cavities
            .iter()
            .map(nominal_grid)
            .collect(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn profiles_preserve_source_datums_and_do_not_turn_curves_into_box_assets() {
        let slope = profile("3037.dat").unwrap();
        assert_eq!(slope.bounds.min.z, -0.5);
        assert_eq!(slope.bounds.max.z, 1.5);
        assert_eq!(slope.studs.len(), 4);
        assert!(slope.studs.iter().all(|p| p.z == 0.));
        assert!(crate::geometry::part_spec("3037.dat").is_none());
        assert_eq!(crate::geometry::fallback_mesh("3037.dat").quality, "proxy");
        assert!((profile("76382p00.dat").unwrap().grams - 1.30).abs() < 1e-12);
        assert!(profile("invented.dat").is_none());
    }
    #[test]
    fn factory_figure_interfaces_are_inferred_without_invented_strength() {
        let mut demo = crate::demo::Demo::new();
        for (part, position) in [
            ("3815c01", DVec3::new(0., 5. / 3., 0.)),
            ("76382p00", DVec3::new(0., 3., 0.)),
            ("3626ap01", DVec3::new(0., 4., 0.)),
        ] {
            demo.add(part, 14, 0., 0., 0.);
            let brick = demo.bricks.last_mut().unwrap();
            brick.position = position;
            brick.bounds = profile(&brick.part)
                .unwrap()
                .bounds
                .transformed(brick.transform, position);
        }
        let mut joints = crate::constraints::inferred_joints(&demo.bricks);
        assert_eq!(joints.len(), 2);
        assert!(
            crate::physical::ConnectionMeasurements::default()
                .apply(&demo.bricks, &mut joints)
                .is_err()
        );
        assert_eq!(
            crate::estimates::apply_stud_estimates(&demo.bricks, &mut joints).estimated_connections,
            0
        );
    }
}
