//! Read-only static validation using the same kernel as server/WASM admission.
use anyhow::{Context, Result, ensure};

fn main() -> Result<()> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    ensure!(
        args.len() == 3 && args[0] == "--library",
        "Usage: validate_native --library LIBRARY.json CONSTRUCTION.json"
    );
    // The same synthesized part library the server installs supplies masses,
    // bounds and connectors; there is no built-in fallback.
    ergo_world::part_library::install_json(
        &std::fs::read(&args[1]).with_context(|| format!("Read {}", args[1]))?,
    )?;
    // A native construction, a materialized export or a shared workspace response.
    let bytes = std::fs::read(&args[2])?;
    let raw: serde_json::Value = serde_json::from_slice(&bytes)?;
    let mut scene: ergo_world::WorldExport = if raw["format"] == "ergo-construction-1" {
        ergo_world::storage::decode(&bytes)?
    } else if raw.get("scene").is_some() {
        serde_json::from_value(raw["scene"].clone())?
    } else {
        serde_json::from_value(raw)?
    };
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
