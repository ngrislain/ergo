//! Read-only nominal compression diagnostic for a complete native construction.
use anyhow::{Context, Result, ensure};

fn main() -> Result<()> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    ensure!(args.len() == 1, "Usage: support_issue CONSTRUCTION.json");
    let mut scene = ergo_world::storage::decode(&std::fs::read(&args[0])?)?;
    let measurements = ergo_world::physical::ConnectionMeasurements::default();
    measurements.normalize_scene(&mut scene)?;
    let issue = ergo_world::validation::compression_issue(
        &scene.bricks,
        Default::default(),
        &measurements,
    )?;
    if let Some(issue) = issue {
        let brick = scene
            .bricks
            .iter()
            .find(|b| b.id == issue.brick_id)
            .context("Issue brick missing")?;
        let below: Vec<_> = scene
            .bricks
            .iter()
            .filter(|b| {
                (b.bounds.max.y - brick.bounds.min.y).abs() < 1e-5
                    && b.bounds.max.x >= brick.bounds.min.x
                    && b.bounds.min.x <= brick.bounds.max.x
                    && b.bounds.max.z >= brick.bounds.min.z
                    && b.bounds.min.z <= brick.bounds.max.z
            })
            .map(|b| serde_json::json!({"id":b.id,"part":b.part,"bounds":b.bounds}))
            .collect();
        println!(
            "{}",
            serde_json::to_string_pretty(
                &serde_json::json!({"issue":issue,"bounds":brick.bounds,"below":below})
            )?
        );
    } else {
        println!(
            "{}",
            serde_json::json!({"contact_only_compression_verified":true,"pieces":scene.bricks.len()})
        );
    }
    Ok(())
}
