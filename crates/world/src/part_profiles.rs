//! Sourced catalogue averages and nominal architectural collision interfaces.
//! Visual geometry must still be resolved from the real catalogue asset. These
//! boxes/grids are model approximations, not connector-strength measurements.
use crate::Aabb;
use glam::DVec3;

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
pub(crate) fn receiving_grid(part: &str) -> Option<Grid> {
    if let Some(spec) = crate::geometry::part_spec(part) {
        return Some(grid(
            spec.width,
            spec.depth,
            [
                0.5 - f64::from(spec.width) / 2.,
                0.5 - f64::from(spec.depth) / 2.,
            ],
            -spec.height,
        ));
    }
    profile(part).and_then(|p| p.cavity)
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

/// Dimensions use BrickLink nominal stud sizes and LDraw's actual local datum;
/// source meshes/credits are retained separately. Reviewed 2026-10-27.
pub fn profile(part: &str) -> Option<Profile> {
    let id = part.strip_suffix(".dat")?;
    let (min, max, grams, cavity, studs, source_id) = match id {
        "60594" => (
            [-2., -3., -0.5],
            [2., 0., 0.5],
            1.85,
            Some(grid(4, 1, [-1.5, 0.], -3.)),
            centres(4, 1, [-1.5, 0.]),
            "60594",
        ),
        "60592" => (
            [-1., -2., -0.5],
            [1., 0., 0.5],
            1.00,
            Some(grid(2, 1, [-0.5, 0.], -2.)),
            centres(2, 1, [-0.5, 0.]),
            "60592",
        ),
        // Slopes have an offset footprint and only their rear stud row.
        "3037" => (
            [-2., -1., -0.5],
            [2., 0., 1.5],
            1.97,
            Some(grid(4, 2, [-1.5, 0.], -1.)),
            centres(4, 1, [-1.5, 0.]),
            "3037",
        ),
        "3039" => (
            [-1., -1., -0.5],
            [1., 0., 1.5],
            1.15,
            Some(grid(2, 2, [-0.5, 0.], -1.)),
            centres(2, 1, [-0.5, 0.]),
            "3039",
        ),
        "3633" => (
            [-2., -1., -0.5],
            [2., 0., 0.5],
            0.80,
            Some(grid(4, 1, [-1.5, 0.], -1.)),
            vec![],
            "3633",
        ),
        "3062b" | "3062" => (
            [-0.5, -1., -0.5],
            [0.5, 0., 0.5],
            0.28,
            Some(grid(1, 1, [0., 0.], -1.)),
            centres(1, 1, [0., 0.]),
            "3062",
        ),
        "3941" => (
            [-1., -1., -1.],
            [1., 0., 1.],
            1.20,
            Some(grid(2, 2, [-0.5, -0.5], -1.)),
            centres(2, 2, [-0.5, -0.5]),
            "3941",
        ),
        "87081" => (
            [-2., -1., -2.],
            [2., 0., 2.],
            3.04,
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
            "87081",
        ),
        "2039" => (
            [-1., -7., -1.],
            [1., 0., 1.],
            2.00,
            Some(grid(2, 2, [-0.5, -0.5], -7.)),
            centres(1, 1, [0., 0.]),
            "2039",
        ),
        "4073" => (
            [-0.5, -1. / 3., -0.5],
            [0.5, 0., 0.5],
            0.12,
            Some(grid(1, 1, [0., 0.], -1. / 3.)),
            centres(1, 1, [0., 0.]),
            "4073",
        ),
        "4740" => (
            [-1., -1. / 3., -1.],
            [1., 0., 1.],
            0.28,
            Some(grid(1, 1, [0., 0.], -1. / 3.)),
            centres(1, 1, [0., 0.]),
            "4740",
        ),
        "87580" => (
            [-1., -1. / 3., -1.],
            [1., 0., 1.],
            0.52,
            Some(grid(2, 2, [-0.5, -0.5], -1. / 3.)),
            centres(1, 1, [0., 0.]),
            "87580",
        ),
        // Factory minifigure units use nominal load-bearing cores. Connector
        // pegs and hanging hands are visual geometry, not certified colliders.
        "970c00" => (
            [-0.975, -5. / 3., -0.5],
            [0.975, 0., 0.55],
            1.18,
            Some(grid(2, 1, [-0.5, 0.], -5. / 3.)),
            vec![],
            "970c00",
        ),
        "76382p00" => (
            [-0.95, -4. / 3., -0.5],
            [0.95, 0., 0.5],
            0.82 + 0.15 + 0.15 + 0.09 + 0.09,
            None,
            vec![],
            "973",
        ),
        "3626a" | "3626ap01" => (
            [-0.65, -1., -0.65],
            [0.65, 0., 0.65],
            0.57,
            Some(grid(1, 1, [0., 0.], -1.)),
            centres(1, 1, [0., 0.]),
            id,
        ),
        // Factory wheel/tyre unit; its axis is horizontal in the source datum.
        "4624c01" => (
            [-0.9, -0.75, -0.4],
            [0.9, 0.75, 0.4],
            0.61,
            None,
            vec![],
            "4624c01",
        ),
        _ => return None,
    };
    let source = if id == "76382p00" {
        "Derived sum of BrickLink catalogue masses for 973, 981, 982 and 2 x 983; https://www.bricklink.com/v2/catalog/catalogitem.page?P=973 ; https://www.bricklink.com/v2/catalog/catalogitem.page?P=981 ; https://www.bricklink.com/v2/catalog/catalogitem.page?P=982 ; https://www.bricklink.com/v2/catalog/catalogitem.page?P=983".into()
    } else {
        format!("https://www.bricklink.com/v2/catalog/catalogitem.page?P={source_id}")
    };
    Some(Profile {
        bounds: Aabb::new(DVec3::from_array(min), DVec3::from_array(max)),
        grams,
        source,
        cavity,
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
