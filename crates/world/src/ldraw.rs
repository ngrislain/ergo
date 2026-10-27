//! LDraw type-1 assembly flattening and type-3/4 mesh baking. No filesystem or
//! network access: callers supply a bounded, sandboxed part resolver.
use crate::{
    Aabb, Brick, color_rgb,
    geometry::{Mesh, triangle},
};
use anyhow::{Context, Result, bail, ensure};
use glam::{DMat3, DVec3};
use std::collections::{BTreeMap, HashSet};

pub const MAX_PARTS: usize = 200_000;
const MAX_DEPTH: usize = 64;
const MAX_VERTICES: usize = 1_000_000;

pub fn normalize_name(name: &str) -> Result<String> {
    let name = name.trim().replace('\\', "/").to_lowercase();
    ensure!(
        !name.is_empty()
            && name.len() <= 240
            && !name.starts_with('/')
            && !name.chars().any(char::is_control)
            && !name.contains(':')
            && !name
                .split('/')
                .any(|s| s == ".." || s == "." || s.is_empty()),
        "Invalid LDraw part path"
    );
    Ok(name)
}

pub fn conversion() -> DMat3 {
    DMat3::from_diagonal(DVec3::new(1.0 / 20.0, -1.0 / 24.0, -1.0 / 20.0))
}

/// Primitive-only DAT geometry remains one physical part. A DAT model needs an
/// explicit Model/Submodel header; LDR/MPD files are models by convention.
pub fn is_assembly(name: &str, text: &str) -> bool {
    if !name.ends_with(".dat") {
        return true;
    }
    text.lines().any(|line| {
        let tokens = line.split_whitespace().collect::<Vec<_>>();
        tokens.first() == Some(&"0")
            && tokens
                .get(1)
                .is_some_and(|s| s.eq_ignore_ascii_case("!LDRAW_ORG"))
            && tokens.get(2).is_some_and(|s| {
                [
                    "model",
                    "submodel",
                    "unofficial_model",
                    "unofficial_submodel",
                ]
                .iter()
                .any(|kind| s.eq_ignore_ascii_case(kind))
            })
    })
}

#[derive(Clone, Debug)]
pub struct Reference {
    pub color: u32,
    pub position: DVec3,
    pub transform: DMat3,
    pub name: String,
}

fn number(s: &str) -> Result<f64> {
    let n: f64 = s.parse().context("Invalid LDraw number")?;
    ensure!(
        n.is_finite() && n.abs() < 1e12,
        "Non-finite or excessive LDraw number"
    );
    Ok(n)
}

fn color(s: &str) -> Result<u32> {
    if let Some(hex) = s.strip_prefix("0x").or_else(|| s.strip_prefix("0X")) {
        Ok(u32::from_str_radix(hex, 16)?)
    } else {
        Ok(s.parse()?)
    }
}

pub fn reference(tokens: &[&str]) -> Result<Reference> {
    ensure!(tokens.len() >= 15, "Truncated LDraw type-1 line");
    let n: Vec<_> = tokens[2..14]
        .iter()
        .map(|s| number(s))
        .collect::<Result<_>>()?;
    let matrix = DMat3::from_cols_array(&[n[3], n[6], n[9], n[4], n[7], n[10], n[5], n[8], n[11]]);
    ensure!(
        matrix.determinant().abs() > 1e-10,
        "Singular LDraw transform"
    );
    Ok(Reference {
        color: color(tokens[1])?,
        position: DVec3::new(n[0], n[1], n[2]),
        transform: matrix,
        name: normalize_name(&tokens[14..].join(" "))?,
    })
}

#[derive(Default)]
pub struct Document {
    pub files: BTreeMap<String, String>,
    pub root: String,
    pub colors: BTreeMap<u32, u32>,
}

impl Document {
    pub fn parse(text: &str, filename: &str) -> Result<Self> {
        let mut doc = Self::default();
        let fallback = normalize_name(filename)?;
        let mut current = fallback.clone();
        let mut saw_file = false;
        let mut defined = HashSet::new();
        doc.files.insert(current.clone(), String::new());
        for line in text.lines() {
            let tokens: Vec<_> = line.split_whitespace().collect();
            if tokens.len() >= 3 && tokens[0] == "0" && tokens[1].eq_ignore_ascii_case("FILE") {
                current = normalize_name(&tokens[2..].join(" "))?;
                if !saw_file {
                    doc.root = current.clone();
                    saw_file = true;
                }
                ensure!(
                    defined.insert(current.clone()),
                    "Duplicate MPD subfile: {current}"
                );
                doc.files.entry(current.clone()).or_default();
                continue;
            }
            if tokens
                .get(1)
                .is_some_and(|s| s.eq_ignore_ascii_case("NOFILE"))
            {
                current = fallback.clone();
                continue;
            }
            if tokens.get(1) == Some(&"!COLOUR") {
                let get = |key| {
                    tokens
                        .iter()
                        .position(|s| *s == key)
                        .and_then(|i| tokens.get(i + 1))
                        .copied()
                };
                if let (Some(code), Some(value)) = (get("CODE"), get("VALUE")) {
                    doc.colors.insert(
                        color(code)?,
                        u32::from_str_radix(value.trim_start_matches('#'), 16)?,
                    );
                }
            }
            doc.files.entry(current.clone()).or_default().push_str(line);
            doc.files.get_mut(&current).unwrap().push('\n');
        }
        if !saw_file {
            doc.root = fallback;
        }
        Ok(doc)
    }

    pub fn instances(&self) -> Result<Vec<Reference>> {
        let mut out = Vec::new();
        self.flatten(
            &self.root,
            DMat3::IDENTITY,
            DVec3::ZERO,
            16,
            &mut HashSet::new(),
            &mut out,
        )?;
        ensure!(!out.is_empty(), "The model contains no part references");
        Ok(out)
    }

    fn flatten(
        &self,
        name: &str,
        matrix: DMat3,
        position: DVec3,
        inherited: u32,
        stack: &mut HashSet<String>,
        out: &mut Vec<Reference>,
    ) -> Result<()> {
        ensure!(
            stack.len() < MAX_DEPTH && stack.insert(name.into()),
            "Cyclic or overly deep MPD submodels"
        );
        let text = self.files.get(name).context("Missing MPD root")?;
        for (line_number, line) in text.lines().enumerate() {
            let tokens: Vec<_> = line.split_whitespace().collect();
            if tokens.first() != Some(&"1") {
                continue;
            }
            let r = reference(&tokens).with_context(|| format!("{name}:{}", line_number + 1))?;
            let transform = matrix * r.transform;
            let p = matrix * r.position + position;
            let code = if r.color == 16 { inherited } else { r.color };
            if self
                .files
                .get(&r.name)
                .is_some_and(|text| is_assembly(&r.name, text))
            {
                self.flatten(&r.name, transform, p, code, stack, out)?;
            } else {
                ensure!(
                    !r.name.ends_with(".ldr") && !r.name.ends_with(".mpd"),
                    "Missing submodel {}",
                    r.name
                );
                ensure!(out.len() < MAX_PARTS, "Model exceeds {MAX_PARTS} parts");
                out.push(Reference {
                    name: r.name,
                    color: code,
                    transform,
                    position: p,
                });
            }
        }
        stack.remove(name);
        Ok(())
    }
}

pub fn brick_from_reference(
    id: u32,
    r: &Reference,
    mesh: &Mesh,
    colors: &BTreeMap<u32, u32>,
) -> Brick {
    let c = conversion();
    let transform = c * r.transform * c.inverse();
    let position = c * r.position;
    Brick {
        id,
        part: r.name.clone(),
        color: r.color,
        rgb: colors
            .get(&r.color)
            .copied()
            .unwrap_or_else(|| color_rgb(r.color)),
        position,
        transform,
        bounds: crate::placement::body_bounds(mesh).transformed(transform, position),
        render_bounds: mesh.bounds.transformed(transform, position),
        step: 0,
    }
}

pub fn bake_mesh(
    part: &str,
    resolver: &impl Fn(&str) -> Option<String>,
    colors: &BTreeMap<u32, u32>,
) -> Result<Mesh> {
    let mut vertices = Vec::new();
    bake_recursive(
        part,
        resolver,
        colors,
        DMat3::IDENTITY,
        DVec3::ZERO,
        16,
        &mut HashSet::new(),
        &mut vertices,
    )?;
    ensure!(!vertices.is_empty(), "No triangles found for {part}");
    let bounds = vertices
        .chunks_exact(9)
        .map(|v| {
            let p = DVec3::new(v[0] as f64, v[1] as f64, v[2] as f64);
            Aabb::new(p, p)
        })
        .reduce(Aabb::union)
        .unwrap();
    Ok(Mesh {
        schema: crate::SCHEMA_VERSION,
        part: part.into(),
        quality: "source".into(),
        vertices,
        bounds,
        body_bounds: bounds,
        credits: Vec::new(),
        physical: None,
    })
}

#[allow(clippy::too_many_arguments)]
fn bake_recursive(
    name: &str,
    resolver: &impl Fn(&str) -> Option<String>,
    colors: &BTreeMap<u32, u32>,
    matrix: DMat3,
    position: DVec3,
    inherited: u32,
    stack: &mut HashSet<String>,
    vertices: &mut Vec<f32>,
) -> Result<()> {
    ensure!(
        stack.len() < MAX_DEPTH && stack.insert(name.into()),
        "Cyclic LDraw geometry: {name}"
    );
    let text = resolver(name).with_context(|| format!("Missing LDraw geometry {name}"))?;
    for line in text.lines() {
        let tokens: Vec<_> = line.split_whitespace().collect();
        match tokens.first().copied() {
            Some("1") => {
                let r = reference(&tokens)?;
                let color = if r.color == 16 { inherited } else { r.color };
                bake_recursive(
                    &r.name,
                    resolver,
                    colors,
                    matrix * r.transform,
                    matrix * r.position + position,
                    color,
                    stack,
                    vertices,
                )?;
            }
            Some("3") | Some("4") => {
                let count = if tokens[0] == "3" { 3 } else { 4 };
                ensure!(
                    tokens.len() >= 2 + count * 3,
                    "Truncated LDraw face in {name}"
                );
                let code = color(tokens[1])?;
                let code = if code == 16 { inherited } else { code };
                let rgb = (code != 16).then(|| {
                    colors
                        .get(&code)
                        .copied()
                        .unwrap_or_else(|| color_rgb(code))
                });
                let mut p = Vec::new();
                for i in 0..count {
                    let point = DVec3::new(
                        number(tokens[2 + i * 3])?,
                        number(tokens[3 + i * 3])?,
                        number(tokens[4 + i * 3])?,
                    );
                    let point = conversion() * (matrix * point + position);
                    ensure!(point.is_finite(), "Invalid geometry vertex");
                    p.push(point.as_vec3());
                }
                triangle(vertices, p[0], p[1], p[2], rgb);
                if count == 4 {
                    triangle(vertices, p[0], p[2], p[3], rgb);
                }
                ensure!(
                    vertices.len() / 9 <= MAX_VERTICES,
                    "Geometry too complex: {name}"
                );
            }
            Some("0" | "2" | "5") | None => {}
            _ => bail!("Unsupported LDraw line type in {name}"),
        }
    }
    stack.remove(name);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn primitive_only_dat_parts_are_not_flattened_into_scaled_physical_bricks() {
        let doc=Document::parse("0 FILE main.ldr\n1 4 0 0 0 1 0 0 0 1 0 0 0 1 custom.dat\n0 FILE custom.dat\n0 !LDRAW_ORG Unofficial_Part\n1 16 0 0 0 2 0 0 0 2 0 0 0 2 primitive.dat\n0 FILE primitive.dat\n3 16 0 0 0 20 0 0 0 20 0", "model.mpd").unwrap();
        let refs = doc.instances().unwrap();
        assert_eq!(refs.len(), 1);
        assert_eq!(refs[0].name, "custom.dat");
        assert_eq!(refs[0].transform, DMat3::IDENTITY);
        let model = crate::assembly::Model::from_document(&doc).unwrap();
        assert!(!model.subparts.contains_key("custom.dat"));
        assert_eq!(model.references().unwrap()[0].name, "custom.dat");
    }

    #[test]
    fn flattens_submodel_transforms_inherited_colors_and_spaced_names() {
        let doc = Document::parse("0 FILE main.ldr\n1 4 20 0 0 0 0 1 0 1 0 -1 0 0 sub model.ldr\n0 FILE sub model.ldr\n1 16 20 0 0 1 0 0 0 1 0 0 0 1 3001.dat", "test.mpd").unwrap();
        let refs = doc.instances().unwrap();
        assert_eq!(refs.len(), 1);
        assert_eq!(refs[0].color, 4);
        assert_eq!(refs[0].position, DVec3::new(20.0, 0.0, -20.0));
    }

    #[test]
    fn rejects_cycles_missing_submodels_and_nan() {
        let cyclic =
            Document::parse("0 FILE a.ldr\n1 16 0 0 0 1 0 0 0 1 0 0 0 1 a.ldr", "a.mpd").unwrap();
        assert!(cyclic.instances().is_err());
        let missing = Document::parse("1 16 0 0 0 1 0 0 0 1 0 0 0 1 absent.ldr", "a.ldr").unwrap();
        assert!(missing.instances().is_err());
        assert!(
            reference(
                &"1 4 NaN 0 0 1 0 0 0 1 0 0 0 1 a.dat"
                    .split_whitespace()
                    .collect::<Vec<_>>()
            )
            .is_err()
        );
    }

    #[test]
    fn mirrored_faces_have_recomputed_normals() {
        let mesh = bake_mesh(
            "part.dat",
            &|_| Some("4 16 0 0 0 20 0 0 20 24 0 0 24 0".into()),
            &BTreeMap::new(),
        )
        .unwrap();
        assert_eq!(mesh.vertices.len(), 54);
        assert_eq!(mesh.bounds.max.x, 1.0);
        assert_eq!(mesh.bounds.min.y, -1.0);
    }
}
