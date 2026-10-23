use anyhow::{Context, Result, ensure};
use ergo_world::{
    WorldExport,
    geometry::{Mesh, fallback_mesh},
    ldraw::{self, Document, brick_from_reference},
};
use std::{
    collections::BTreeMap,
    io::{Cursor, Read},
    path::PathBuf,
};

const MAX_UNCOMPRESSED: u64 = 128 * 1024 * 1024;

pub struct ImportedLdraw {
    pub bricks: Vec<ergo_world::Brick>,
    pub meshes: BTreeMap<String, Mesh>,
    pub notes: Vec<String>,
}

pub fn import_ldraw(
    bytes: &[u8],
    filename: &str,
    library: Option<PathBuf>,
) -> Result<ImportedLdraw> {
    let mut embedded = BTreeMap::new();
    let text = if filename.to_lowercase().ends_with(".io") || bytes.starts_with(b"PK") {
        let mut archive = zip::ZipArchive::new(Cursor::new(bytes)).context("Cannot open Studio archive; export LDraw from Studio if this .io version is unsupported")?;
        ensure!(archive.len() <= 10_000, "Too many archive entries");
        let mut total = 0;
        let mut root = None;
        for i in 0..archive.len() {
            let raw = archive.by_index_raw(i)?;
            let name = raw.name().to_owned();
            let size = raw.size();
            drop(raw);
            total += size;
            ensure!(
                size <= 32 * 1024 * 1024 && total <= MAX_UNCOMPRESSED,
                "Studio archive expands beyond the 128 MiB limit"
            );
            let lower = name.to_lowercase();
            if ![".ldr", ".mpd", ".dat"]
                .iter()
                .any(|ext| lower.ends_with(ext))
            {
                continue;
            }
            let name = ldraw::normalize_name(&name)?;
            // Studio's legacy ZIP encryption uses this format password. No files
            // are extracted; custom archive paths never become local disk paths.
            let mut file = archive
                .by_index_decrypt(i, b"soho0909")
                .context("Unsupported Studio encryption. In Studio use File > Export As > LDraw")?;
            let mut text = String::new();
            (&mut file)
                .take(32 * 1024 * 1024 + 1)
                .read_to_string(&mut text)
                .context("Model is not UTF-8 LDraw text")?;
            ensure!(text.len() <= 32 * 1024 * 1024, "LDraw file is too large");
            if name == "model.ldr"
                || (root.is_none() && (name.ends_with(".ldr") || name.ends_with(".mpd")))
            {
                root = Some(name.clone());
            }
            embedded.insert(name, text);
        }
        let name = root.context("Studio archive has no model.ldr; export as LDraw")?;
        embedded[&name].clone()
    } else {
        std::str::from_utf8(bytes)
            .context("Expected a Studio .io or UTF-8 LDraw .ldr/.mpd file")?
            .to_owned()
    };
    let mut doc = Document::parse(&text, "model.ldr")?;
    for (name, text) in embedded {
        doc.files.entry(name).or_insert(text);
    }
    let mut refs = doc.instances()?;
    for r in &mut refs {
        r.name = ergo_world::catalog::canonical_part(&r.name);
        ensure!(
            ergo_world::catalog::known_color(r.color) || doc.colors.contains_key(&r.color),
            "Unknown color {}; include its !COLOUR definition in the Studio/LDraw export",
            r.color
        );
    }
    let mut meshes = BTreeMap::new();
    let mut notes = Vec::new();
    let library = library
        .map(|path| path.canonicalize())
        .transpose()
        .context("LDRAW_DIR does not exist")?;
    let resolver = |name: &str| -> Option<String> {
        if let Some(text) = doc.files.get(name) {
            return Some(text.clone());
        }
        if let Some(root) = &library {
            for dir in ["parts", "p", "unofficial/parts", "unofficial/p", ""] {
                let Ok(path) = root.join(dir).join(name).canonicalize() else {
                    continue;
                };
                if !path.starts_with(root) {
                    continue;
                }
                if let Ok(metadata) = path.metadata() {
                    if metadata.len() > 16 * 1024 * 1024 {
                        continue;
                    }
                    if let Ok(text) = std::fs::read_to_string(path) {
                        return Some(text);
                    }
                }
            }
        }
        None
    };
    for r in &refs {
        if meshes.contains_key(&r.name) {
            continue;
        }
        let mesh = match ldraw::bake_mesh(&r.name, &resolver, &doc.colors) {
            Ok(mut mesh) => {
                if ergo_world::geometry::part_spec(&r.name).is_some() {
                    mesh.body_bounds = fallback_mesh(&r.name).body_bounds;
                }
                mesh
            }
            Err(error) => {
                if notes.len() < 24 {
                    notes.push(format!(
                        "{}: {error:#}; using {} geometry",
                        r.name,
                        if ergo_world::geometry::part_spec(&r.name).is_some() {
                            "parametric"
                        } else {
                            "proxy"
                        }
                    ));
                }
                fallback_mesh(&r.name)
            }
        };
        meshes.insert(r.name.clone(), mesh);
    }
    let bricks: Vec<_> = refs
        .iter()
        .enumerate()
        .map(|(i, r)| brick_from_reference(i as u32 + 1, r, &meshes[&r.name], &doc.colors))
        .collect();
    ergo_world::catalog::validate(&bricks)?;
    for brick in &bricks {
        ergo_world::physical::validate_pose(brick)?;
    }
    Ok(ImportedLdraw {
        bricks,
        meshes,
        notes,
    })
}

pub fn validate_export(scene: &mut WorldExport) -> Result<()> {
    ensure!(
        scene.schema == ergo_world::SCHEMA_VERSION,
        "Unsupported scene schema"
    );
    scene.geo.validate()?;
    ergo_world::groups::validate(&scene.bricks, &scene.groups)?;
    ensure!(
        scene.ground.len() <= 128,
        "Too many ground blocks in a snapshot"
    );
    for block in &scene.ground {
        block.validate()?;
    }
    ensure!(
        !scene.bricks.is_empty() && scene.bricks.len() <= ldraw::MAX_PARTS,
        "Scene must contain 1–200,000 bricks"
    );
    let mut vertices = 0;
    for (name, mesh) in &mut scene.meshes {
        ldraw::normalize_name(name)?;
        ensure!(
            ergo_world::catalog::valid_part(name),
            "Invalid geometry part ID {name}"
        );
        ensure!(
            mesh.schema == ergo_world::SCHEMA_VERSION && mesh.part == *name,
            "Invalid geometry schema or part ID"
        );
        ensure!(
            ergo_world::geometry::part_spec(name).is_some() || mesh.quality == "ldraw",
            "Non-standard parts require actual LDraw/Studio geometry, not placeholders"
        );
        mesh.physical = None;
        ensure!(
            mesh.vertices.len() % 27 == 0 && !mesh.vertices.is_empty(),
            "Geometry must contain complete triangles"
        );
        vertices += mesh.vertices.len() / 9;
        ensure!(
            vertices <= 4_000_000 && mesh.vertices.iter().all(|n| n.is_finite()),
            "Invalid or excessive geometry"
        );
        let bounds = mesh
            .vertices
            .chunks_exact(9)
            .map(|v| {
                let p = glam::DVec3::new(v[0] as f64, v[1] as f64, v[2] as f64);
                ergo_world::Aabb::new(p, p)
            })
            .reduce(ergo_world::Aabb::union)
            .unwrap();
        mesh.bounds = bounds;
        mesh.body_bounds = if ergo_world::geometry::part_spec(name).is_some() {
            fallback_mesh(name).body_bounds
        } else {
            bounds
        };
    }
    for brick in &mut scene.bricks {
        ergo_world::physical::validate_pose(brick)?;
        let mesh = scene
            .meshes
            .get(&brick.part)
            .context("Scene references missing geometry")?;
        ensure!(
            brick.position.is_finite()
                && brick.transform.is_finite()
                && brick.transform.determinant().abs() > 1e-10,
            "Invalid brick transform"
        );
        let body_bounds = mesh.body_bounds;
        brick.bounds = body_bounds.transformed(brick.transform, brick.position);
        brick.render_bounds = mesh.bounds.transformed(brick.transform, brick.position);
    }
    ergo_world::constraints::validate_joints(&scene.bricks, &scene.joints, &scene.fixed_bricks)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn nonstandard_geometry_and_custom_studio_colors_import_without_fabricated_physics() {
        let model = "0 FILE model.ldr\n0 !COLOUR Studio_Custom CODE 100000 VALUE #123456 EDGE #333333\n1 100000 0 -24 0 1 0 0 0 1 0 0 0 1 custom.dat\n0 FILE custom.dat\n4 16 -20 0 -20 20 0 -20 20 0 20 -20 0 20\n4 16 -20 24 -20 -20 24 20 20 24 20 20 24 -20\n";
        let imported = import_ldraw(model.as_bytes(), "custom.mpd", None).unwrap();
        assert_eq!(imported.bricks[0].part, "custom.dat");
        assert_eq!(imported.bricks[0].rgb, 0x123456);
        assert_eq!(imported.meshes["custom.dat"].quality, "ldraw");
        let mut scene = WorldExport {
            schema: ergo_world::SCHEMA_VERSION,
            name: "Measured custom part".into(),
            geo: Default::default(),
            bricks: imported.bricks,
            meshes: imported.meshes,
            joints: Vec::new(),
            fixed_bricks: Vec::new(),
            groups: Vec::new(),
            ground: Vec::new(),
        };
        validate_export(&mut scene).unwrap();
        assert!(
            ergo_world::validation::validate_static(&mut scene)
                .unwrap_err()
                .to_string()
                .contains("sourced")
        );
        let record = ergo_world::physical::PartMeasurement {
            part: "custom.dat".into(),
            mass_kg: 0.003,
            body_bounds: scene.meshes["custom.dat"].body_bounds,
            source: "synthetic test-only mass and dimension fixture".into(),
            samples: 1,
        };
        let measurements = ergo_world::physical::ConnectionMeasurements {
            entries: Vec::new(),
            parts: vec![record],
        };
        ergo_world::validation::validate_static_with_measurements(
            &mut scene,
            Default::default(),
            &measurements,
        )
        .unwrap();
    }

    #[test]
    fn studio_archive_imports_model_and_preserves_supported_parts_colors() {
        let text = "0 Studio export\n1 4 0 0 0 1 0 0 0 1 0 0 0 1 3003.dat\n1 1 0 -24 0 1 0 0 0 1 0 0 0 1 3003.dat\n";
        let mut archive = zip::ZipWriter::new(Cursor::new(Vec::new()));
        archive
            .start_file("model.ldr", zip::write::SimpleFileOptions::default())
            .unwrap();
        archive.write_all(text.as_bytes()).unwrap();
        let bytes = archive.finish().unwrap().into_inner();
        let model = import_ldraw(&bytes, "construction.io", None).unwrap();
        assert_eq!(model.bricks.len(), 2);
        assert_eq!(model.meshes.len(), 1);
        assert_eq!(model.bricks[0].color, 4);
        assert_eq!(model.bricks[1].color, 1);
        assert_eq!(model.bricks[1].position.y, 1.0);
    }

    #[test]
    fn invalid_part_paths_undefined_colors_and_scaled_real_bricks_are_rejected() {
        for text in [
            "1 4 0 0 0 1 0 0 0 1 0 0 0 1 ../unknown.dat",
            "1 9999 0 0 0 1 0 0 0 1 0 0 0 1 3003.dat",
            "1 4 0 0 0 2 0 0 0 2 0 0 0 2 3003.dat",
        ] {
            assert!(import_ldraw(text.as_bytes(), "test.ldr", None).is_err());
        }
    }
}
