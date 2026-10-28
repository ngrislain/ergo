//! Read-only comparison of per-brick and scene-budgeted rendering policies.
use anyhow::{Context, Result, ensure};
use ergo_client::scene::{
    Camera, DISPLAY_SCALE, FramePlan, GeometryKey, RenderPolicy, SceneOptions, ScenePlanner,
    Visibility,
};
use ergo_world::{WorldExport, spatial::Octree};
use glam::{DMat4, DVec3};
use std::{
    collections::{BTreeMap, HashSet},
    time::Instant,
};

fn triangles(
    plan: &BTreeMap<GeometryKey, Vec<f32>>,
    scene: &WorldExport,
    planner: &ScenePlanner,
) -> usize {
    plan.iter()
        .map(|(key, instances)| {
            let per_instance = match key {
                GeometryKey::Part(part, 0) => scene.meshes[part].vertices.len() / 27,
                GeometryKey::Part(part, level) => {
                    ergo_client::part_lod::RenderPart::resolve(&scene.meshes[part])
                        .unwrap()
                        .mesh(*level)
                        .vertices
                        .len()
                        / 27
                }
                GeometryKey::Surface(index) => planner.surface_mesh(*index).vertices.len() / 27,
                _ => 12,
            };
            per_instance * (instances.len() / 20)
        })
        .sum()
}
fn summarize(
    plan: FramePlan,
    scene: &WorldExport,
    planner: &ScenePlanner,
    ms: f64,
    cold_ms: f64,
) -> serde_json::Value {
    serde_json::json!({"color_triangles":triangles(&plan.color,scene,planner),"shadow_triangles":triangles(&plan.shadow,scene,planner),
        "color_instances":plan.color.values().map(|list|list.len()/20).sum::<usize>(),
        "shadow_instances":plan.shadow.values().map(|list|list.len()/20).sum::<usize>(),
        "lod_counts":plan.lod_counts,"shadow_lod_counts":plan.shadow_lod_counts,"global_detail_scale":plan.detail_scale,
        "conservative_triangle_estimate_exceeds_target":plan.budget_exceeded,"shadow_detail_scale":plan.shadow_detail_scale,
        "surface_pieces":plan.surface_pieces,"warm_plan_cpu_ms":ms,"cold_plan_cpu_ms":cold_ms})
}
fn main() -> Result<()> {
    let mut args: Vec<_> = std::env::args().skip(1).collect();
    let view_packet = args.iter().any(|arg| arg == "--view-packet");
    args.retain(|arg| arg != "--view-packet");
    let materialize = args.first().is_some_and(|arg| arg == "--scene-json");
    let inventory = args.first().is_some_and(|arg| arg == "--inventory");
    let option = materialize || inventory;
    ensure!(
        args.len() == if option { 2 } else { 1 },
        "Usage: profile_scene [--view-packet] [--scene-json|--inventory] INPUT.json"
    );
    let bytes = std::fs::read(&args[usize::from(option)])?;
    let scene = if view_packet {
        #[derive(serde::Deserialize)]
        struct Packet {
            scene: WorldExport,
        }
        serde_json::from_slice::<Packet>(&bytes)?.scene
    } else {
        ergo_world::storage::decode(&bytes)?
    };
    if materialize {
        // Local diagnostic transport, not an import or publication certificate.
        serde_json::to_writer(std::io::stdout().lock(), &scene)?;
        return Ok(());
    }
    if inventory {
        let mut counts = BTreeMap::<&str, usize>::new();
        for brick in &scene.bricks {
            *counts.entry(&brick.part).or_default() += 1;
        }
        let mut parts: Vec<_> = counts
            .into_iter()
            .map(|(part, count)| {
                let mesh = &scene.meshes[part];
                serde_json::json!({"part":part,"instances":count,"quality":mesh.quality,
                "source_triangles":mesh.vertices.len()/27*count,
                "rectangular_lod":ergo_world::geometry::part_spec(part).is_some(),
                "reviewed_display_lod":ergo_client::part_lod::RenderPart::resolve(mesh).is_some(),
                "vertex_sha256":ergo_client::part_lod::vertex_hash(mesh),
                "recommended":ergo_world::catalog::PARTS.iter().any(|(id,_)|*id==part)})
            })
            .collect();
        parts.sort_by_key(|part| std::cmp::Reverse(part["source_triangles"].as_u64().unwrap_or(0)));
        println!("{}", serde_json::to_string_pretty(&parts)?);
        return Ok(());
    }
    let bounds = scene
        .bricks
        .iter()
        .map(|brick| brick.render_bounds)
        .reduce(ergo_world::Aabb::union)
        .context("Empty construction")?;
    let tree = Octree::new(
        scene
            .bricks
            .iter()
            .map(|brick| brick.render_bounds)
            .collect(),
    );
    let centre = bounds.center();
    let extent = (bounds.size() * DISPLAY_SCALE).length();
    let light_target = centre * DISPLAY_SCALE;
    let light_position = light_target + DVec3::new(-0.6, 1., 0.8).normalize() * 360.;
    let shadow = Visibility::new(
        DMat4::orthographic_rh_gl(-180., 180., -180., 180., 0.1, 720.)
            * DMat4::look_at_rh(light_position, light_target, DVec3::Y)
            * DMat4::from_scale(DISPLAY_SCALE),
        DVec3::ZERO,
    );
    let highlighted = HashSet::new();
    let mut cases = Vec::new();
    for multiplier in [1., 3.] {
        let eye = centre + DVec3::new(0.8, 0.65, 1.).normalize() * extent * multiplier;
        let camera = Camera::new(eye, centre, 1280., 720., 45., extent * multiplier * 5.);
        let mut results = serde_json::Map::new();
        for (name, budgeted, merge_surfaces) in [
            ("per_brick", false, false),
            ("budgeted_unmerged", true, false),
            ("budgeted", true, true),
        ] {
            let mut planner = ScenePlanner::default();
            let options = || SceneOptions {
                policy: if budgeted {
                    RenderPolicy::INTERACTIVE
                } else {
                    RenderPolicy {
                        triangle_budget: None,
                        ..RenderPolicy::INTERACTIVE
                    }
                },
                selected: None,
                highlighted: &highlighted,
                explode: 0.,
                shadow_resolution: budgeted.then_some(2048.),
                reuse_shadow: budgeted,
                merge_surfaces,
            };
            let cold = Instant::now();
            planner.plan(
                &scene.bricks,
                &tree,
                &scene.meshes,
                &camera,
                &shadow,
                options(),
            );
            let cold_ms = cold.elapsed().as_secs_f64() * 1000.;
            let start = Instant::now();
            let plan = planner.plan(
                &scene.bricks,
                &tree,
                &scene.meshes,
                &camera,
                &shadow,
                options(),
            );
            results.insert(
                name.into(),
                summarize(
                    plan,
                    &scene,
                    &planner,
                    start.elapsed().as_secs_f64() * 1000.,
                    cold_ms,
                ),
            );
        }
        cases.push(serde_json::json!({"camera_distance_multiplier":multiplier,"results":results}));
    }
    println!(
        "{}",
        serde_json::to_string_pretty(
            &serde_json::json!({"pieces":scene.bricks.len(),"execution":"native CPU planning and submitted triangle counts; not a GPU/FPS benchmark","cases":cases})
        )?
    );
    Ok(())
}
