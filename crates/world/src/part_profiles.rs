//! Sourced catalogue averages and nominal architectural collision interfaces.
//! Visual geometry must still be resolved from the real catalogue asset. These
//! boxes/grids are model approximations, not connector-strength measurements.
use crate::Aabb;
use glam::DVec3;
use serde::{Deserialize, Serialize};

/// Reviewed source-local proxy data. Coordinates are LDU, not public axes.
#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Nominal {
    pub bounds_ldu: [[f64; 3]; 2],
    pub studs_ldu: Vec<[f64; 3]>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cavity: Option<NominalGrid>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub additional_cavities: Vec<NominalGrid>,
}
#[derive(Debug, Deserialize, Serialize)]
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
pub(crate) fn stud_centres(part: &str) -> Vec<DVec3> {
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
pub(crate) fn receiving_grids(part: &str) -> Vec<Grid> {
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
    let id = crate::part_library::profile_id(part);
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

/// Dimensions use BrickLink nominal stud sizes and LDraw's actual local datum;
/// source meshes/credits are retained separately. Reviewed 2026-10-27.
pub fn profile(part: &str) -> Option<Profile> {
    let entry = &crate::part_library::get(part)?.catalogue;
    if let Some(nominal) = &entry.nominal {
        let c = crate::ldraw::conversion();
        let receiving = |g: &NominalGrid| {
            grid(
                g.width,
                g.depth,
                [g.first[0] / 20., -g.first[1] / 20. - f64::from(g.depth - 1)],
                -g.bottom / 24.,
            )
        };
        return Some(Profile {
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
            cavity: nominal.cavity.as_ref().map(receiving),
            additional_cavities: nominal.additional_cavities.iter().map(receiving).collect(),
        });
    }
    let id = entry
        .profile_id
        .as_ref()
        .unwrap_or(&entry.id)
        .strip_suffix(".dat")?;
    let (min, max, cavity, studs) = match id {
        "60594" => (
            [-2., -3., -0.5],
            [2., 0., 0.5],
            Some(grid(4, 1, [-1.5, 0.], -3.)),
            centres(4, 1, [-1.5, 0.]),
        ),
        "60592" => (
            [-1., -2., -0.5],
            [1., 0., 0.5],
            Some(grid(2, 1, [-0.5, 0.], -2.)),
            centres(2, 1, [-0.5, 0.]),
        ),
        // Slopes have an offset footprint and only their rear stud row.
        "3037" => (
            [-2., -1., -0.5],
            [2., 0., 1.5],
            Some(grid(4, 2, [-1.5, 0.], -1.)),
            centres(4, 1, [-1.5, 0.]),
        ),
        "3039" => (
            [-1., -1., -0.5],
            [1., 0., 1.5],
            Some(grid(2, 2, [-0.5, 0.], -1.)),
            centres(2, 1, [-0.5, 0.]),
        ),
        "3633" => (
            [-2., -1., -0.5],
            [2., 0., 0.5],
            Some(grid(4, 1, [-1.5, 0.], -1.)),
            vec![],
        ),
        "3062b" | "3062" => (
            [-0.5, -1., -0.5],
            [0.5, 0., 0.5],
            Some(grid(1, 1, [0., 0.], -1.)),
            centres(1, 1, [0., 0.]),
        ),
        "3941" => (
            [-1., -1., -1.],
            [1., 0., 1.],
            Some(grid(2, 2, [-0.5, -0.5], -1.)),
            centres(2, 2, [-0.5, -0.5]),
        ),
        "87081" => (
            [-2., -1., -2.],
            [2., 0., 2.],
            Some(grid(4, 4, [-1.5, -1.5], -1.)),
            [
                (-1.5, -0.5),
                (-1.5, 0.5),
                (-0.5, -1.5),
                (0.5, -1.5),
                (1.5, -0.5),
                (1.5, 0.5),
                (-0.5, 1.5),
                (0.5, 1.5),
            ]
            .into_iter()
            .map(|(x, z)| DVec3::new(x, 0., z))
            .collect(),
        ),
        "2039" => (
            [-1., -7., -1.],
            [1., 0., 1.],
            Some(grid(2, 2, [-0.5, -0.5], -7.)),
            centres(1, 1, [0., 0.]),
        ),
        "4073" => (
            [-0.5, -1. / 3., -0.5],
            [0.5, 0., 0.5],
            Some(grid(1, 1, [0., 0.], -1. / 3.)),
            centres(1, 1, [0., 0.]),
        ),
        "4740" => (
            [-1., -1. / 3., -1.],
            [1., 0., 1.],
            Some(grid(1, 1, [0., 0.], -1. / 3.)),
            centres(1, 1, [0., 0.]),
        ),
        "87580" => (
            [-1., -1. / 3., -1.],
            [1., 0., 1.],
            Some(grid(2, 2, [-0.5, -0.5], -1. / 3.)),
            centres(1, 1, [0., 0.]),
        ),
        // Factory minifigure units use nominal load-bearing cores. Connector
        // pegs and hanging hands are visual geometry, not certified colliders.
        "970c00" => (
            [-0.975, -5. / 3., -0.5],
            [0.975, 0., 0.55],
            Some(grid(2, 1, [-0.5, 0.], -5. / 3.)),
            vec![],
        ),
        "76382p00" | "76382p4f" => ([-0.95, -4. / 3., -0.5], [0.95, 0., 0.5], None, vec![]),
        "3626a" | "3626ap01" => (
            [-0.65, -1., -0.65],
            [0.65, 0., 0.65],
            Some(grid(1, 1, [0., 0.], -1.)),
            centres(1, 1, [0., 0.]),
        ),
        // Factory wheel/tyre unit; its axis is horizontal in the source datum.
        "4624c01" => ([-0.9, -0.75, -0.4], [0.9, 0.75, 0.4], None, vec![]),
        // Reviewed 2026-10-29: genuine geometry remains source-only. Hair uses
        // its upper load-bearing cap; strands wrapping the head are visual,
        // like the arm/hand overhangs of the existing factory torso profile.
        "3901" => (
            [-0.65, 0., -0.65],
            [0.65, 0.25, 0.65],
            Some(grid(1, 1, [0., 0.], 0.)),
            vec![],
        ),
        // Genuine Castle parts: source datums and moulded geometry remain in
        // the pinned assets. Helmet neck protection is visual, as for hair;
        // the upper cap bears on the head without filling its hollow cavity.
        "3844" => (
            [-0.65, 0., -0.65],
            [0.65, 17. / 24., 0.65],
            Some(grid(1, 1, [0., 0.], 0.)),
            vec![],
        ),
        "4497" => ([-0.35, -49. / 12., -0.2], [0.35, 2., 0.2], None, vec![]),
        "3846p4h" => ([-1., -1.34875, -0.7], [1., 7. / 12., 0.5], None, vec![]),
        "3957a" => (
            [-0.4, -1. / 3., -0.4],
            [0.4, 11. / 3., 0.4],
            Some(grid(1, 1, [0., 0.], -1. / 3.)),
            vec![],
        ),
        // The flag's cloth hem is at 20 LDU; its lower hollow stud extends to
        // 24 LDU. Exclude connector protrusions from the bearing box, retaining
        // the complete asymmetric pennant footprint and catalogue mass. A flag
        // needs real bearing under that footprint or measured attachment loads.
        "4495a" => (
            [-0.4, -5. / 6., -3.55],
            [0.4, 0., 0.4],
            Some(grid(1, 1, [0., 0.], -1.)),
            centres(1, 1, [0., 0.]),
        ),
        // Classic vehicle shells use their lower structural floor as a nominal
        // bearing core, not filled wheel arches/passenger cavities. Upper
        // mouldings and their detailed interfaces remain visual, not certified
        // colliders or assumed connector capacities.
        "4211" => (
            [-2., -2. / 3., -2.],
            [2., -1. / 3., 3.],
            Some(grid(4, 5, [-1.5, -1.5], -2. / 3.)),
            [-30., 30.]
                .into_iter()
                .flat_map(|x| {
                    [-30., 30., 50.]
                        .into_iter()
                        .map(move |z| DVec3::new(x / 20., -1. / 3., -z / 20.))
                })
                .chain(
                    [-10., 10.]
                        .into_iter()
                        .map(|x| DVec3::new(x / 20., -1. / 3., -2.5)),
                )
                .chain([-30., 30.].into_iter().flat_map(|x| {
                    [-10., 10.]
                        .into_iter()
                        .map(move |z| DVec3::new(x / 20., 0., -z / 20.))
                }))
                .collect(),
        ),
        "3788" => (
            [-2., -2. / 3., -1.],
            [2., -1. / 3., 1.],
            Some(grid(4, 2, [-1.5, -0.5], -2. / 3.)),
            [-30., 30.]
                .into_iter()
                .flat_map(|x| {
                    [-10., 10.]
                        .into_iter()
                        .map(move |z| DVec3::new(x / 20., 0., -z / 20.))
                })
                .chain(
                    centres(2, 2, [-0.5, -0.5])
                        .into_iter()
                        .map(|p| p - DVec3::Y / 3.),
                )
                .collect(),
        ),
        "3823" => (
            [-2., -2., -1.5],
            [2., 0., 0.5],
            Some(grid(4, 2, [-1.5, -1.], -2.)),
            vec![DVec3::new(-1.5, 0., 0.), DVec3::new(1.5, 0., 0.)],
        ),
        "4083" => (
            [-2., -2., -0.5],
            [2., 0., 0.5],
            Some(grid(4, 1, [-1.5, 0.], -2.)),
            centres(4, 1, [-1.5, 0.]),
        ),
        "3829c01" => (
            [-1., -1. / 3., -0.5],
            [1., 0., 0.5],
            Some(grid(2, 1, [-0.5, 0.], -1. / 3.)),
            vec![],
        ),
        _ => return None,
    };
    Some(Profile {
        bounds: Aabb::new(DVec3::from_array(min), DVec3::from_array(max)),
        grams: entry.mass_kg * 1000.,
        source: entry.mass_source.clone(),
        cavity,
        additional_cavities: Vec::new(),
        studs,
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
            ("970c00", DVec3::new(0., 5. / 3., 0.)),
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
