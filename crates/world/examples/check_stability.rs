//! Developer-native execution of the same bounded cases used by the WASM tool.
use anyhow::{Result, ensure};
use ergo_world::{
    WorldExport,
    physical::{ConnectionMeasurements, GravityProbeSettings},
    solidity::{self, LoadCase, SmallEarthquake},
};
use serde_json::json;

fn main() -> Result<()> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    ensure!(
        args.len() == 3 && args[2] == "--estimated-connectors",
        "Usage: check_stability CONSTRUCTION OUTPUT.json --estimated-connectors"
    );
    let output = std::path::Path::new(&args[1]);
    ensure!(!output.exists(), "Choose a fresh diagnostic output");
    let mut scene: WorldExport = ergo_world::storage::decode(&std::fs::read(&args[0])?)?;
    ConnectionMeasurements::default().normalize_scene(&mut scene)?;
    scene.joints = ergo_world::constraints::inferred_joints(&scene.bricks);
    let estimates = ergo_world::estimates::apply_stud_estimates(&scene.bricks, &mut scene.joints);
    let start = std::time::Instant::now();
    let (_, _, gravity) = solidity::test(
        &scene,
        LoadCase::Gravity(GravityProbeSettings {
            steps: 120,
            position_tolerance: 0.01,
            rotation_tolerance_radians: 0.01,
        }),
    )?;
    eprintln!(
        "Gravity diagnostic completed in {:.1}s",
        start.elapsed().as_secs_f64()
    );
    let (_, _, shake) = solidity::test(
        &scene,
        LoadCase::SmallEarthquake(SmallEarthquake::default()),
    )?;
    let passed = gravity.survived_declared_case
        && shake.survived_declared_case
        && estimates.unsupported_piece_pairs.is_empty();
    let result = json!({"execution":"native-ergo-world","estimate_model":estimates,"gravity":gravity,"shake":shake,
        "stable_under_declared_estimates":passed,"calibrated_stability_verified":false,"elapsed_s":start.elapsed().as_secs_f64()});
    std::fs::write(output, serde_json::to_vec_pretty(&result)?)?;
    println!(
        "{}",
        json!({"output":args[1],"stable_under_declared_estimates":passed,"calibrated_stability_verified":false})
    );
    Ok(())
}
