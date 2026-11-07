//! Offline synthesis from a local official archive, for development checks.
//! Usage: synthesize COMPLETE_ZIP [--rebrickable DIR] [--library OUT.json]
//!        [--fixture LIBRARY.json OBJECTS.cbor.gz]
use ergo_assets::library;
use std::time::Instant;

fn main() -> anyhow::Result<()> {
    let mut args = std::env::args().skip(1);
    let archive = args.next().expect("Path to the official complete.zip");
    let mut inputs = library::Inputs::default();
    inputs
        .files
        .insert(library::LDRAW_INPUT.into(), std::fs::read(archive)?);
    inputs.files.insert(
        library::REVIEWS_INPUT.into(),
        library::REVIEWS.as_bytes().to_vec(),
    );
    let (mut fixture, mut output) = (None, None);
    while let Some(flag) = args.next() {
        match flag.as_str() {
            "--fixture" => fixture = args.next().zip(args.next()),
            "--library" => output = args.next(),
            "--rebrickable" => {
                let dir = std::path::PathBuf::from(args.next().expect("Rebrickable snapshot dir"));
                inputs.files.insert(
                    library::REBRICKABLE_PARTS.into(),
                    std::fs::read(dir.join("parts.json"))?,
                );
                inputs.files.insert(
                    library::REBRICKABLE_CATEGORIES.into(),
                    std::fs::read(dir.join("part_categories.json"))?,
                );
            }
            other => anyhow::bail!("Unknown flag {other}"),
        }
    }
    let started = Instant::now();
    let workers = std::thread::available_parallelism().map_or(4, |n| n.get());
    let objects = std::sync::Mutex::new(std::collections::BTreeMap::new());
    let bytes = std::sync::atomic::AtomicUsize::new(0);
    let synthesis = library::synthesize(&inputs, workers, &|entry, cbor| {
        bytes.fetch_add(cbor.len(), std::sync::atomic::Ordering::Relaxed);
        if fixture.is_some() && library::in_fixture(entry) {
            objects
                .lock()
                .unwrap()
                .insert(entry.mesh_hash.clone(), cbor);
        }
        Ok(())
    })?;
    let objects = objects.into_inner().unwrap();
    eprintln!("synthesized in {:.1}s", started.elapsed().as_secs_f64());
    if let Ok(path) = std::env::var("ERGO_EXCLUDED_OUT") {
        std::fs::write(path, serde_json::to_vec_pretty(&synthesis.report.excluded)?)?;
    }
    let mut report = serde_json::to_value(&synthesis.report)?;
    let excluded = report["excluded"].as_object().map_or(0, |e| e.len());
    report["excluded_count"] = excluded.into();
    report["excluded"] = serde_json::Value::Object(
        report["excluded"]
            .as_object()
            .cloned()
            .unwrap_or_default()
            .into_iter()
            .take(15)
            .collect(),
    );
    report["library_json_bytes"] = synthesis.json.len().into();
    report["object_bytes"] = bytes.load(std::sync::atomic::Ordering::Relaxed).into();
    println!("{}", serde_json::to_string_pretty(&report)?);
    if let Some((library_path, objects_path)) = fixture {
        let (library, objects) = library::fixture(&synthesis, &objects)?;
        std::fs::write(library_path, library)?;
        std::fs::write(objects_path, objects)?;
    }
    if let Some(path) = output {
        std::fs::write(path, &synthesis.json)?;
    }
    Ok(())
}
