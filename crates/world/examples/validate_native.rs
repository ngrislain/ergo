//! Read-only static validation using the same kernel as server/WASM admission.
use anyhow::{Result, ensure};

fn main() -> Result<()> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    ensure!(
        args.len() == 1,
        "Usage: validate_native CONSTRUCTION.native.json"
    );
    let mut scene = ergo_world::storage::decode(&std::fs::read(&args[0])?)?;
    let measurements = ergo_world::physical::ConnectionMeasurements::default();
    let result = ergo_world::validation::validate_static_with_measurements(
        &mut scene,
        Default::default(),
        &measurements,
    );
    let value = match result {
        Ok(report) => {
            let calibration = if report.joint_strength_required == Some(true) {
                measurements
                    .apply(&scene.bricks, &mut scene.joints)
                    .err()
                    .map(|e| e.to_string())
            } else {
                None
            };
            serde_json::json!({"execution":"native-ergo-world-static","brick_count":scene.bricks.len(),
                "overlap_free":true,"static_under_current_joint_model":true,"publication_ready":calibration.is_none(),
                "calibration_issue":calibration,"stability_report":report,"published":false})
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
