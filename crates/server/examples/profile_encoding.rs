//! Read-only reproduction of the shared replacement's canonical encoding pass.
use anyhow::{Context, Result, ensure};

fn main() -> Result<()> {
    let path = std::env::args()
        .nth(1)
        .context("Usage: profile_encoding CONSTRUCTION.json")?;
    let bytes = std::fs::read(path)?;
    let scene = ergo_world::storage::decode(&bytes)?;
    drop(bytes);
    let started = std::time::Instant::now();
    let encoded = ergo_transactions::canonical_cbor(&scene)?;
    ensure!(!encoded.is_empty(), "Empty encoding");
    println!(
        "{}",
        serde_json::json!({
            "read_only":true,"published":false,"pieces":scene.bricks.len(),
            "encoded_bytes":encoded.len(),"content_id":ergo_transactions::ContentId::of(&encoded),
            "encoding_milliseconds":started.elapsed().as_millis()
        })
    );
    Ok(())
}
