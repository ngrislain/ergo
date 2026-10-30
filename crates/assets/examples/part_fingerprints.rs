//! Bake the pinned official library in isolated memory and report vertex hashes.
use sha2::{Digest, Sha256};
use std::{collections::BTreeMap, io::Write, sync::Arc};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cache = ergo_assets::PartCache::new(
        Arc::new(object_store::memory::InMemory::new()),
        String::new(),
    )?
    .with_builtin_assets(false);
    let mut hashes = BTreeMap::new();
    let mut pack = Vec::new();
    for entry in ergo_world::part_library::all() {
        let mesh = cache.resolve(&entry.catalogue.id).await?.mesh;
        anyhow::ensure!(mesh.quality == "source", "Expected genuine source geometry");
        let mut hash = Sha256::new();
        for value in &mesh.vertices {
            hash.update(ergo_world::geometry::vertex_bytes(*value));
        }
        let mut raw = Vec::new();
        ciborium::into_writer(&mesh, &mut raw)?;
        let mesh_hash = format!("{:x}", Sha256::digest(&raw));
        let mut encoder = flate2::GzBuilder::new()
            .mtime(0)
            .write(Vec::new(), flate2::Compression::default());
        encoder.write_all(&raw)?;
        let compressed = encoder.finish()?;
        let asset =
            serde_json::json!({"offset":pack.len(),"bytes":compressed.len(),"mesh_hash":mesh_hash});
        pack.extend(compressed);
        hashes.insert(
            entry.catalogue.id.clone(),
            serde_json::json!({"vertex_hash":format!("{:x}",hash.finalize()),"asset":asset}),
        );
    }
    if let Some(destination) = std::env::args().nth(1) {
        std::fs::write(destination, pack)?;
    }
    println!("{}", serde_json::to_string(&hashes)?);
    Ok(())
}
