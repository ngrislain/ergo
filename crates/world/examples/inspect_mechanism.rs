//! Read-only registered-joint inventory; optional explicit diagnostic-joint export.
use anyhow::{Result, ensure};
use ergo_world::{JointKind, physical::ConnectionMeasurements};
use std::collections::{BTreeMap, BTreeSet};

fn main() -> Result<()> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    ensure!(
        (1..=2).contains(&args.len()),
        "Usage: inspect_mechanism INPUT.native.json [DIAGNOSTIC.native.json]"
    );
    let mut scene = ergo_world::storage::decode(&std::fs::read(&args[0])?)?;
    ConnectionMeasurements::default().normalize_scene(&mut scene)?;
    for brick in &scene.bricks {
        ergo_world::physical::validate_pose(brick)?;
    }
    scene.joints = ergo_world::constraints::inferred_joints(&scene.bricks);
    let free = scene
        .joints
        .iter()
        .filter(|j| matches!(j.kind, JointKind::Revolute { limits: None, .. }))
        .count();
    let fixed = scene
        .joints
        .iter()
        .filter(|j| matches!(j.kind, JointKind::Fixed))
        .count();
    let mut graph: BTreeMap<u32, BTreeSet<u32>> = scene
        .bricks
        .iter()
        .map(|b| (b.id, BTreeSet::new()))
        .collect();
    for joint in &scene.joints {
        graph.get_mut(&joint.a).unwrap().insert(joint.b);
        graph.get_mut(&joint.b).unwrap().insert(joint.a);
    }
    let mut remaining: BTreeSet<_> = graph.keys().copied().collect();
    let mut components = Vec::new();
    while let Some(first) = remaining.pop_first() {
        let mut stack = vec![first];
        let mut count = 0;
        while let Some(id) = stack.pop() {
            count += 1;
            for peer in &graph[&id] {
                if remaining.remove(peer) {
                    stack.push(*peer);
                }
            }
        }
        components.push(count);
    }
    components.sort_unstable_by(|a, b| b.cmp(a));
    let calibration = ConnectionMeasurements::default()
        .apply(&scene.bricks, &mut scene.joints)
        .err()
        .map(|e| e.to_string());
    let static_issue = ergo_world::validation::validate_static(&mut scene.clone())
        .err()
        .map(|e| e.to_string());
    if args.len() == 2 {
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&args[1])?;
        serde_json::to_writer(&mut file, &ergo_world::storage::Construction::from(&scene))?;
    }
    println!(
        "{}",
        serde_json::json!({"pieces":scene.bricks.len(),"registered_joints":scene.joints.len(),"free_revolute_joints":free,"fixed_keyed_or_stud_joints":fixed,"limited_revolute_joints":scene.joints.len()-free-fixed,"joint_component_sizes":components,"calibration_issue":calibration,"static_issue":static_issue,"brakes_or_motors_added":false,"published":false})
    );
    Ok(())
}
