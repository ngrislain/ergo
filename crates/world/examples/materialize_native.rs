//! Convert a native proposal to public WorldExport JSON for ordinary API validation.
use anyhow::{Result, ensure};
use std::{fs, path::Path};

fn main() -> Result<()> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    ensure!(
        args.len() == 2,
        "Usage: materialize_native INPUT.native.json OUTPUT.scene.json"
    );
    ensure!(
        !Path::new(&args[1]).exists(),
        "Preserve an existing materialized proposal"
    );
    ensure!(
        fs::metadata(&args[0])?.len() <= 128 * 1024 * 1024,
        "Native input exceeds 128 MiB"
    );
    let scene = ergo_world::storage::decode(&fs::read(&args[0])?)?;
    let mut output = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&args[1])?;
    serde_json::to_writer(&mut output, &scene)?;
    println!(
        "{}",
        serde_json::json!({"materialized_pieces":scene.bricks.len(),"published":false,"output":args[1]})
    );
    Ok(())
}
