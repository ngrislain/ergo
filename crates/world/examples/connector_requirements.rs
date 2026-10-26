//! Inventory actual measurement-book keys without inventing capacity values.
use anyhow::{Context, Result, ensure};
use ergo_world::{JointKind, WorldExport, constraints, physical::ConnectionMeasurements};
use serde::Serialize;
use std::collections::{BTreeMap, HashSet};

#[derive(Serialize)]
struct Requirement {
    part_a: String,
    part_b: String,
    kind: &'static str,
    overlap_studs: [u32; 2],
    connection_count: usize,
    example_piece_pairs: Vec<[u32; 2]>,
    measurement_available: bool,
}

fn main() -> Result<()> {
    let arguments: Vec<_> = std::env::args().skip(1).collect();
    ensure!(
        (1..=2).contains(&arguments.len()),
        "Usage: connector_requirements CONSTRUCTION.json [MEASUREMENT_BOOK.json]"
    );
    let bytes = std::fs::read(&arguments[0])?;
    let raw: serde_json::Value = serde_json::from_slice(&bytes)?;
    ensure!(
        raw["external_dependencies"] != true,
        "Load the complete mechanical domain before inventorying an externally dependent workspace"
    );
    let mut scene: WorldExport = if raw["format"] == "ergo-construction-1" {
        ergo_world::storage::decode(&bytes)?
    } else if raw.get("scene").is_some() {
        serde_json::from_value(raw["scene"].clone())?
    } else {
        serde_json::from_value(raw)?
    };
    let measurements: ConnectionMeasurements = arguments
        .get(1)
        .map(|path| -> Result<_> { Ok(serde_json::from_slice(&std::fs::read(path)?)?) })
        .transpose()?
        .unwrap_or_default();
    measurements.normalize_scene(&mut scene)?;
    for brick in &scene.bricks {
        ergo_world::physical::validate_pose(brick)?;
    }
    constraints::validate_joints(&scene.bricks, &scene.joints, &scene.fixed_bricks)?;
    let mut pairs: HashSet<_> = scene
        .joints
        .iter()
        .map(|j| (j.a.min(j.b), j.a.max(j.b)))
        .collect();
    let inferred = constraints::inferred_joints(&scene.bricks);
    scene.joints.extend(
        inferred
            .into_iter()
            .filter(|j| pairs.insert((j.a.min(j.b), j.a.max(j.b)))),
    );
    let index: BTreeMap<_, _> = scene.bricks.iter().map(|b| (b.id, b)).collect();
    let mut required = BTreeMap::new();
    for joint in &scene.joints {
        let a = index.get(&joint.a).context("Missing joint endpoint")?;
        let b = index.get(&joint.b).context("Missing joint endpoint")?;
        let overlap = a.bounds.max.min(b.bounds.max) - a.bounds.min.max(b.bounds.min);
        // Deliberately match the current book's world-horizontal footprint keys.
        let footprint = [
            overlap.x.max(0.).round() as u32,
            overlap.z.max(0.).round() as u32,
        ];
        let kind = if matches!(joint.kind, JointKind::Fixed) {
            "fixed"
        } else {
            "revolute"
        };
        let (part_a, part_b) = if a.part <= b.part {
            (&a.part, &b.part)
        } else {
            (&b.part, &a.part)
        };
        let matched = measurements.entries.iter().any(|e| {
            ((e.part_a == *part_a && e.part_b == *part_b)
                || (e.part_a == *part_b && e.part_b == *part_a))
                && e.kind == kind
                && e.overlap_studs == footprint
        });
        let entry = required
            .entry((part_a.clone(), part_b.clone(), kind, footprint))
            .or_insert_with(|| Requirement {
                part_a: part_a.clone(),
                part_b: part_b.clone(),
                kind,
                overlap_studs: footprint,
                connection_count: 0,
                example_piece_pairs: vec![],
                measurement_available: matched,
            });
        entry.connection_count += 1;
        if entry.example_piece_pairs.len() < 3 {
            entry.example_piece_pairs.push([joint.a, joint.b]);
        }
    }
    let missing = required
        .values()
        .filter(|e| !e.measurement_available)
        .count();
    println!(
        "{}",
        serde_json::to_string_pretty(&serde_json::json!({
            "format":"ergo-connector-requirements-1", "schema":ergo_world::SCHEMA_VERSION,
            "pieces":scene.bricks.len(), "connections":scene.joints.len(),
            "unique_measurement_keys":required.len(), "missing_measurement_keys":missing,
            "requirements":required.into_values().collect::<Vec<_>>(),
            "interpretation":"Inventory only, not a measurement book or a stability certificate. Each missing key needs applicable sourced force/torque limits and sample provenance. A paper's per-contact fitted friction parameter is not a measured whole-joint isotropic capacity."
        }))?
    );
    Ok(())
}
