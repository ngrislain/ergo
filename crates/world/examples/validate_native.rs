//! Read-only static validation using the same kernel as server/WASM admission.
use anyhow::{Context, Result, ensure};

fn main() -> Result<()> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    ensure!(
        args.len() == 3 && args[0] == "--library",
        "Usage: validate_native --library LIBRARY.json CONSTRUCTION.native.json"
    );
    // The same synthesized part library the server installs supplies masses,
    // bounds and connectors; there is no built-in fallback.
    ergo_world::part_library::install_json(
        &std::fs::read(&args[1]).with_context(|| format!("Read {}", args[1]))?,
    )?;
    let mut scene = ergo_world::storage::decode(&std::fs::read(&args[2])?)?;
    let measurements = ergo_world::physical::ConnectionMeasurements::default();
    let result = ergo_world::validation::validate_static_with_measurements(
        &mut scene,
        Default::default(),
        &measurements,
    );
    let value = match result {
        Ok(report) => {
            serde_json::json!({"execution":"native-ergo-world-static","brick_count":scene.bricks.len(),
                "overlap_free":true,"static_under_current_joint_model":true,"publication_ready":true,
                "stability_report":report,"published":false})
        }
        Err(error) => {
            let issue = ergo_world::validation::compression_issue(
                &scene.bricks,
                Default::default(),
                &measurements,
            )
            .ok()
            .flatten();
            serde_json::json!({"execution":"native-ergo-world-static","brick_count":scene.bricks.len(),
                "failure":error.to_string(),"publication_ready":false,"compression_issue":issue,"published":false})
        }
    };
    println!("{}", value);
    Ok(())
}
