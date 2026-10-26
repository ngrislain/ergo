//! Check an explicit route against the same collision/step/gravity engine as the viewer.
use anyhow::{Context, Result, ensure};
use ergo_client::walking::{Scene, Walker};
use ergo_world::{coordinates, spatial::Octree};
use glam::DVec3;
use serde::Deserialize;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Route {
    format: String,
    schema: u32,
    origin_world: [f64; 3],
    start: [f64; 3],
    eye_height: f64,
    min_clearance: f64,
    speed: f64,
    waypoints: Vec<Waypoint>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Waypoint {
    name: String,
    feet: [f64; 3],
}

fn main() -> Result<()> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    ensure!(
        args.len() == 2,
        "Usage: cargo run -p ergo-client --example check_walk -- CONSTRUCTION ROUTE.json"
    );
    let model = ergo_world::storage::decode(&std::fs::read(&args[0])?)?;
    let route: Route = serde_json::from_slice(&std::fs::read(&args[1])?)?;
    ensure!(
        route.format == "ergo-walk-route-1" && route.schema == 2,
        "Unsupported walking route"
    );
    ensure!(
        route.eye_height > 0. && route.min_clearance >= route.eye_height + 0.25 && route.speed > 0.,
        "Invalid body/clearance/speed"
    );
    ensure!(!route.waypoints.is_empty(), "The route is empty");
    let origin = DVec3::from(route.origin_world);
    let tree = Octree::new(model.bricks.iter().map(|b| b.bounds).collect());
    let scene = Scene::new(&model.bricks, &tree, route.eye_height, &|_| false);
    let clearance = Scene::new(&model.bricks, &tree, route.min_clearance - 0.25, &|_| false);
    let mut feet = coordinates::internal(origin + DVec3::from(route.start));
    ensure!(
        clearance.clear(feet),
        "Starting body/headroom intersects geometry"
    );
    let mut walker = Walker::default();
    let mut visited = Vec::new();
    for waypoint in route.waypoints {
        let target = coordinates::internal(origin + DVec3::from(waypoint.feet));
        ensure!(target.is_finite(), "Invalid waypoint {}", waypoint.name);
        let mut reached = false;
        for _ in 0..12000 {
            let delta = DVec3::new(target.x - feet.x, 0., target.z - feet.z);
            if delta.length() < 0.02 {
                reached = true;
                break;
            }
            let velocity = delta.normalize() * (delta.length() * 120.).min(route.speed);
            feet = walker.advance(&scene, feet, velocity, 1. / 120., false);
            ensure!(
                clearance.clear(feet),
                "Insufficient headroom on {} at {:?}",
                waypoint.name,
                coordinates::public(feet) - origin
            );
        }
        for _ in 0..120 {
            feet = walker.advance(&scene, feet, DVec3::ZERO, 1. / 120., false);
        }
        let relative = coordinates::public(feet) - origin;
        ensure!(reached, "Blocked on {} at {:?}", waypoint.name, relative);
        ensure!(
            (feet.y - target.y).abs() < 0.01,
            "Wrong floor on {} at {:?}",
            waypoint.name,
            relative
        );
        ensure!(
            clearance.clear(feet),
            "Body/headroom intersects geometry on {}",
            waypoint.name
        );
        visited.push(
            serde_json::json!({"waypoint":waypoint.name,"feet_relative":relative.to_array()}),
        );
    }
    println!(
        "{}",
        serde_json::to_string_pretty(&serde_json::json!({
            "passed":true,"pieces":model.bricks.len(),"engine":"ergo-client::walking::Walker",
            "eye_height":route.eye_height,"minimum_clearance_checked":route.min_clearance,
            "jump_used":false,"teleport_used":false,"waypoints":visited
        }))
        .context("Cannot encode walking receipt")?
    );
    Ok(())
}
