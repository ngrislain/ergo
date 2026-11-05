//! Read-only preview of one map source/display tile with the configured style.
use anyhow::{Result, ensure};
use std::sync::Arc;

#[tokio::main]
async fn main() -> Result<()> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    ensure!(
        args.len() == 4,
        "Usage: preview_map LATITUDE LONGITUDE ZOOM OUTPUT"
    );
    let latitude = args[0].parse()?;
    let longitude = args[1].parse()?;
    let zoom = args[2].parse()?;
    ensure!(
        !std::path::Path::new(&args[3]).exists(),
        "Choose a fresh preview output"
    );
    let cache = ergo_assets::ground::GroundCache::new(
        Arc::new(object_store::memory::InMemory::new()),
        String::new(),
    )?;
    let (x, y, _, _) = ergo_assets::ground::mercator(latitude, longitude, zoom)?;
    let bytes = cache.preview_tile(zoom, x, y).await?;
    std::fs::write(&args[3], bytes)?;
    println!(
        "{}",
        serde_json::json!({"provider":cache.provider(),"zoom":zoom,"x":x,"y":y,"output":args[3],"published":false})
    );
    Ok(())
}
