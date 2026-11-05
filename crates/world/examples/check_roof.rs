//! Check actual opaque source-mesh overhead coverage at explicit room stud centres.
use anyhow::{Result, ensure};
use ergo_world::{Aabb, coordinates, geometry, spatial::Octree};
use glam::DVec3;
use serde::Deserialize;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Plan {
    origin_world: [f64; 3],
    rooms: Vec<Room>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Room {
    name: String,
    bounds: [i32; 4],
    minimum_roof_height: f64,
}
impl Room {
    fn samples(&self) -> Result<i64> {
        let [x0, x1, n0, n1] = self.bounds;
        ensure!(
            x1 > x0 && n1 > n0 && self.minimum_roof_height.is_finite(),
            "Invalid room coverage bounds"
        );
        let width = i64::from(x1) - i64::from(x0);
        let depth = i64::from(n1) - i64::from(n0);
        // Check before multiplying: two full i32 ranges overflow even i64.
        ensure!(
            width <= 30000 && depth <= 30000 && width * depth <= 30000,
            "Coverage plan exceeds 30,000 samples"
        );
        Ok(width * depth)
    }
}
fn main() -> Result<()> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    ensure!(
        args.len() == 2,
        "Usage: check_roof CONSTRUCTION.native.json ROOMS.json"
    );
    let scene = ergo_world::storage::decode(&std::fs::read(&args[0])?)?;
    let plan: Plan = serde_json::from_slice(&std::fs::read(&args[1])?)?;
    let origin = DVec3::from(plan.origin_world);
    ensure!(
        origin.is_finite() && !plan.rooms.is_empty(),
        "Coverage plan needs a finite origin and at least one room"
    );
    let top = scene
        .bricks
        .iter()
        .map(|b| b.render_bounds.max.y)
        .fold(0., f64::max)
        + 2.;
    let tree = Octree::new(scene.bricks.iter().map(|b| b.render_bounds).collect());
    let mut total = 0;
    let mut missing = Vec::new();
    let mut rooms = Vec::new();
    for room in plan.rooms {
        let [x0, x1, n0, n1] = room.bounds;
        let count = room.samples()?;
        ensure!(
            count <= 30000 && total + count <= 30000,
            "Coverage plan exceeds 30,000 samples"
        );
        let mut covered = 0;
        for x in x0..x1 {
            for n in n0..n1 {
                let p = coordinates::internal(
                    origin
                        + DVec3::new(
                            f64::from(x) + 0.5,
                            f64::from(n) + 0.5,
                            room.minimum_roof_height,
                        ),
                );
                let candidates = tree.query(Aabb::new(
                    p - DVec3::new(1e-6, 0., 1e-6),
                    DVec3::new(p.x + 1e-6, top, p.z + 1e-6),
                ));
                let hit = candidates.into_iter().any(|index| {
                    let brick = &scene.bricks[index];
                    if ergo_world::catalog::color_alpha(brick.color) < 255 {
                        return false;
                    }
                    let inverse = brick.transform.inverse();
                    geometry::mesh_ray(
                        &scene.meshes[&brick.part],
                        inverse * (p - brick.position),
                        inverse * DVec3::Y,
                    )
                    .is_some()
                });
                if hit {
                    covered += 1;
                } else if missing.len() < 40 {
                    missing.push(serde_json::json!({"room":room.name,"x":x,"north":n}));
                }
            }
        }
        total += count;
        rooms.push(serde_json::json!({"room":room.name,"samples":count,"covered":covered,"closed":i64::from(covered)==count}));
    }
    let closed = rooms.iter().all(|room| room["closed"] == true);
    println!(
        "{}",
        serde_json::json!({"closed_overhead_at_all_samples":closed,"actual_source_mesh_rays":true,"samples":total,"rooms":rooms,"first_open_samples":missing,"published":false})
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extreme_integer_ranges_cannot_overflow_the_sample_budget() {
        for bounds in [
            [i32::MIN, i32::MAX, i32::MIN, i32::MAX],
            [0, 30001, 0, 1],
            [0, 200, 0, 200],
            [1, 1, 0, 1],
        ] {
            assert!(
                Room {
                    name: "Invalid bounds".into(),
                    bounds,
                    minimum_roof_height: 1.
                }
                .samples()
                .is_err()
            );
        }
        assert_eq!(
            Room {
                name: "Limit".into(),
                bounds: [0, 300, 0, 100],
                minimum_roof_height: 1.
            }
            .samples()
            .unwrap(),
            30000
        );
    }
}
