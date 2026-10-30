//! Deterministic metadata input for the official-source lock generator.
fn main() -> anyhow::Result<()> {
    let parts: Vec<_> = ergo_world::part_library::all()
        .map(|entry| &entry.catalogue)
        .collect();
    println!("{}", serde_json::to_string(&parts)?);
    Ok(())
}
