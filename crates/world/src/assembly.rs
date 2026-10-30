//! Reusable Ergo model definitions. The native wire format is in format.rs;
//! source-import transforms remain an internal adapter for geometry conversion.
use crate::{Brick, WorldExport, ldraw};
use anyhow::{Context, Result, ensure};
use glam::{DMat3, DMat4, DVec3};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap};
mod format;
pub const FORMAT: &str = format::FORMAT;

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "[[f64; 4]; 4]", into = "[[f64; 4]; 4]")]
pub struct Transform(pub DMat4);

impl Default for Transform {
    fn default() -> Self {
        Self(DMat4::IDENTITY)
    }
}
impl TryFrom<[[f64; 4]; 4]> for Transform {
    type Error = anyhow::Error;
    fn try_from(rows: [[f64; 4]; 4]) -> Result<Self> {
        let matrix = DMat4::from_cols_array_2d(&rows).transpose();
        ensure!(
            matrix.is_finite() && rows[3] == [0., 0., 0., 1.],
            "Expected a finite affine 4x4 matrix with last row [0,0,0,1]"
        );
        ensure!(
            DMat3::from_mat4(matrix).determinant().abs() > 1e-10,
            "Singular subpart transform"
        );
        Ok(Self(matrix))
    }
}
impl From<Transform> for [[f64; 4]; 4] {
    fn from(value: Transform) -> Self {
        value.0.transpose().to_cols_array_2d()
    }
}
impl Transform {
    /// Assembly adapters store linear matrices in uniform LDraw units. Native
    /// anisotropic construction units are converted at the format boundary.
    pub fn validate_isometry(&self) -> Result<()> {
        Transform::try_from(<[[f64; 4]; 4]>::from(*self))?;
        ensure!(
            crate::physical::is_proper_rotation(DMat3::from_mat4(self.0)),
            "Model transform must be a proper isometry (translation + rotation); no scale, shear or reflection"
        );
        Ok(())
    }
    pub fn from_world(rows: [[f64; 4]; 4]) -> Result<Self> {
        format::world_transform(rows)
    }
    pub fn from_pose(matrix: DMat3, position: DVec3) -> Self {
        Self(DMat4::from_cols(
            matrix.x_axis.extend(0.),
            matrix.y_axis.extend(0.),
            matrix.z_axis.extend(0.),
            position.extend(1.),
        ))
    }
    pub fn from_brick(brick: &Brick) -> Self {
        let c = ldraw::conversion();
        Self::from_pose(
            c.inverse() * brick.transform * c,
            c.inverse() * brick.position,
        )
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Reference {
    pub file: String,
    #[serde(default = "current_color")]
    pub color: u32,
    #[serde(default)]
    pub matrix: Transform,
}
fn current_color() -> u32 {
    16
}

#[derive(Clone, Debug)]
pub struct Model {
    pub root: String,
    pub subparts: BTreeMap<String, Vec<Reference>>,
}

impl Model {
    /// Normalize only catalogue leaves; local definitions keep their identity.
    pub fn canonicalize_parts(&mut self) -> Result<()> {
        let definitions = self
            .subparts
            .keys()
            .cloned()
            .collect::<std::collections::BTreeSet<_>>();
        for references in self.subparts.values_mut() {
            for reference in references {
                if !definitions.contains(&reference.file) {
                    let canonical = crate::catalog::canonical_part(&reference.file);
                    ensure!(
                        !definitions.contains(&canonical),
                        "Canonical part conflicts with a local definition"
                    );
                    reference.file = canonical;
                }
            }
        }
        self.references()?;
        Ok(())
    }

    /// Replace matching local prototypes with a versioned library reference.
    /// Fixed colours and every pose must match; only the inherited body colour
    /// may become an instance parameter. Independent edits remain explicit.
    pub fn use_common_constructs(&mut self) -> Result<usize> {
        self.references()?;
        let mut replacements = Vec::new();
        for (name, references) in &self.subparts {
            for entry in crate::constructs::all() {
                if *name == entry.root() {
                    continue;
                }
                let expected = entry.references();
                if references.len() != expected.len() {
                    continue;
                }
                let mut color = None;
                let matches = references.iter().zip(&expected).all(|(actual, expected)| {
                    if actual.file != expected.file
                        || actual
                            .matrix
                            .0
                            .to_cols_array()
                            .iter()
                            .zip(expected.matrix.0.to_cols_array())
                            .any(|(a, b)| (a - b).abs() > 1e-9)
                    {
                        return false;
                    }
                    if expected.color == 16 {
                        match color {
                            Some(previous) => previous == actual.color,
                            None => {
                                color = Some(actual.color);
                                true
                            }
                        }
                    } else {
                        actual.color == expected.color
                    }
                });
                if matches {
                    replacements.push((name.clone(), entry.id.clone(), color.unwrap_or(16)));
                    break;
                }
            }
        }
        for (name, id, color) in &replacements {
            let root = crate::constructs::install(id, &mut self.subparts)?;
            self.subparts.insert(
                name.clone(),
                vec![Reference {
                    file: root,
                    color: *color,
                    matrix: Transform::default(),
                }],
            );
        }
        self.references()?;
        Ok(replacements.len())
    }
    pub fn validate_isometries(&self) -> Result<()> {
        self.references()?;
        for (name, refs) in &self.subparts {
            for reference in refs {
                reference
                    .matrix
                    .validate_isometry()
                    .with_context(|| format!("Invalid instance {name} -> {}", reference.file))?;
            }
        }
        Ok(())
    }
    pub fn from_bricks(bricks: &[Brick]) -> Self {
        Self {
            root: "main.ldr".into(),
            subparts: BTreeMap::from([(
                "main.ldr".into(),
                bricks
                    .iter()
                    .map(|b| Reference {
                        file: b.part.clone(),
                        color: b.color,
                        matrix: Transform::from_brick(b),
                    })
                    .collect(),
            )]),
        }
    }

    pub fn from_document(document: &ldraw::Document) -> Result<Self> {
        let mut subparts = BTreeMap::new();
        for (name, text) in &document.files {
            // DAT part/primitive geometry is baked, not counted as individual
            // physical bricks merely because its faces are in other subfiles.
            if name != &document.root && !ldraw::is_assembly(name, text) {
                continue;
            }
            let mut references = Vec::new();
            for line in text.lines() {
                let tokens: Vec<_> = line.split_whitespace().collect();
                if tokens.first() == Some(&"1") {
                    let r = ldraw::reference(&tokens)?;
                    references.push(Reference {
                        file: if document.files.contains_key(&r.name) {
                            r.name
                        } else {
                            crate::catalog::canonical_part(&r.name)
                        },
                        color: r.color,
                        matrix: Transform::from_pose(r.transform, r.position),
                    });
                }
            }
            subparts.insert(name.clone(), references);
        }
        let model = Self {
            root: document.root.clone(),
            subparts,
        };
        model.references()?;
        Ok(model)
    }

    pub fn references(&self) -> Result<Vec<ldraw::Reference>> {
        ensure!(
            self.subparts.len() <= ldraw::MAX_PARTS,
            "Too many subpart definitions"
        );
        let mut total = 0;
        for (name, refs) in &self.subparts {
            ensure!(
                ldraw::normalize_name(name)? == *name,
                "Subpart names must be normalized"
            );
            total += refs.len();
            ensure!(
                total <= ldraw::MAX_PARTS,
                "Too many stored subpart references"
            );
            for r in refs {
                ensure!(
                    ldraw::normalize_name(&r.file)? == r.file && r.color != 24,
                    "Invalid subpart reference or edge color 24"
                );
                Transform::try_from(<[[f64; 4]; 4]>::from(r.matrix))?;
                ensure!(
                    self.subparts.contains_key(&r.file) || r.file.ends_with(".dat"),
                    "Missing subpart {}",
                    r.file
                );
            }
        }
        // Validate every definition, including unused ones, in linear graph time.
        fn visit(
            name: &str,
            model: &Model,
            state: &mut HashMap<String, usize>,
            depth: usize,
        ) -> Result<usize> {
            ensure!(depth < 64, "Subpart nesting exceeds 64 levels");
            if let Some(&height) = state.get(name) {
                ensure!(height != usize::MAX, "Cyclic subpart reference");
                ensure!(depth + height <= 64, "Subpart nesting exceeds 64 levels");
                return Ok(height);
            }
            state.insert(name.into(), usize::MAX);
            let mut height = 1;
            for r in model.subparts.get(name).context("Missing root subpart")? {
                if model.subparts.contains_key(&r.file) {
                    height = height.max(1 + visit(&r.file, model, state, depth + 1)?);
                }
            }
            state.insert(name.into(), height);
            Ok(height)
        }
        let mut state = HashMap::new();
        for name in self.subparts.keys() {
            visit(name, self, &mut state, 0)?;
        }
        let mut out = Vec::new();
        self.expand(&self.root, DMat4::IDENTITY, 16, &mut 0, &mut out)?;
        Ok(out)
    }

    fn expand(
        &self,
        name: &str,
        parent: DMat4,
        color: u32,
        visits: &mut usize,
        out: &mut Vec<ldraw::Reference>,
    ) -> Result<()> {
        *visits += 1;
        ensure!(*visits <= 1_000_000, "Subpart expansion exceeds work limit");
        for r in self.subparts.get(name).context("Missing root subpart")? {
            let matrix = parent * r.matrix.0;
            ensure!(matrix.is_finite(), "Non-finite composed subpart transform");
            let color = if r.color == 16 { color } else { r.color };
            if self.subparts.contains_key(&r.file) {
                self.expand(&r.file, matrix, color, visits, out)?;
            } else {
                ensure!(out.len() < ldraw::MAX_PARTS, "Model exceeds 200,000 parts");
                out.push(ldraw::Reference {
                    name: r.file.clone(),
                    color,
                    position: matrix.w_axis.truncate(),
                    transform: DMat3::from_mat4(matrix),
                });
            }
        }
        Ok(())
    }

    /// Preserve the hierarchy only while it describes the current leaf poses.
    /// Independent CAD edits must never save a stale template over edited bricks.
    pub fn matches(&self, bricks: &[Brick]) -> bool {
        self.references().is_ok_and(|refs| {
            refs.len() == bricks.len()
                && refs.iter().zip(bricks).all(|(r, b)| {
                    let c = ldraw::conversion();
                    r.name == b.part
                        && r.color == b.color
                        && (c * r.position - b.position).abs().max_element() < 1e-9
                        && (c * r.transform * c.inverse() - b.transform)
                            .to_cols_array()
                            .iter()
                            .all(|v| v.abs() < 1e-9)
                })
        })
    }

    pub fn for_scene(scene: &WorldExport) -> Self {
        scene
            .model
            .as_ref()
            .filter(|m| m.matches(&scene.bricks))
            .cloned()
            .unwrap_or_else(|| Self::from_bricks(&scene.bricks))
    }

    pub fn apply_transform(&mut self, matrix: Transform) -> Result<()> {
        self.references()?;
        Transform::try_from(<[[f64; 4]; 4]>::from(matrix))?;
        let root = (1..)
            .map(|i| format!("transformed-{i}.ldr"))
            .find(|name| !self.subparts.contains_key(name))
            .unwrap();
        self.subparts.insert(
            root.clone(),
            vec![Reference {
                file: self.root.clone(),
                color: 16,
                matrix,
            }],
        );
        self.root = root;
        self.references()?;
        Ok(())
    }

    /// Append an independent instance, interning identical definitions across
    /// placements. Colour inheritance and distinct leaf occurrences are retained.
    pub fn append(&mut self, other: &Self, matrix: Transform, color: u32) -> Result<()> {
        self.references()?;
        other.references()?;
        for references in other.subparts.values() {
            for reference in references {
                ensure!(
                    other.subparts.contains_key(&reference.file)
                        || !self.subparts.contains_key(&reference.file),
                    "Incoming part ID conflicts with an existing model definition"
                );
            }
        }
        let prefix = (1..)
            .map(|i| format!("instance-{i}/"))
            .find(|p| self.subparts.keys().all(|name| !name.starts_with(p)))
            .unwrap();
        fn intern(
            name: &str,
            other: &Model,
            target: &mut Model,
            names: &mut BTreeMap<String, String>,
            prefix: &str,
        ) -> String {
            if let Some(id) = names.get(name) {
                return id.clone();
            }
            let references = other.subparts[name]
                .iter()
                .map(|reference| Reference {
                    file: if other.subparts.contains_key(&reference.file) {
                        intern(&reference.file, other, target, names, prefix)
                    } else {
                        reference.file.clone()
                    },
                    color: reference.color,
                    matrix: reference.matrix,
                })
                .collect::<Vec<_>>();
            let id = if let Some((id, _)) = target
                .subparts
                .iter()
                .find(|(_, value)| **value == references)
            {
                id.clone()
            } else {
                let collides_with_part = target
                    .subparts
                    .values()
                    .any(|references| references.iter().any(|reference| reference.file == name));
                let id = if !target.subparts.contains_key(name) && !collides_with_part {
                    name.to_owned()
                } else {
                    format!("{prefix}{name}")
                };
                target.subparts.insert(id.clone(), references);
                id
            };
            names.insert(name.into(), id.clone());
            id
        }
        let mut names = BTreeMap::new();
        for name in other.subparts.keys() {
            intern(name, other, self, &mut names, &prefix);
        }
        let root = (1..)
            .map(|i| format!("combined-{i}.ldr"))
            .find(|name| !self.subparts.contains_key(name))
            .unwrap();
        let previous = self.root.clone();
        let mut instances = self.subparts[&previous].clone();
        instances.push(Reference {
            file: names[&other.root].clone(),
            matrix,
            color,
        });
        self.subparts.insert(root.clone(), instances);
        if !self.subparts.values().any(|references| {
            references
                .iter()
                .any(|reference| reference.file == previous)
        }) {
            self.subparts.remove(&previous);
        }
        self.root = root;
        self.references()?;
        Ok(())
    }

    /// Standard MPD syntax; Ergo-only IDs, physics and geographic metadata live
    /// in the construction envelope, not in invented LDraw command lines.
    pub fn to_mpd(&self) -> Result<String> {
        use std::fmt::Write;
        self.references()?;
        let mut text = String::new();
        for name in
            std::iter::once(&self.root).chain(self.subparts.keys().filter(|n| *n != &self.root))
        {
            writeln!(text, "0 FILE {name}\r")?;
            if name == &self.root {
                for credit in crate::constructs::credits(self) {
                    writeln!(
                        text,
                        "0 // Common construct source: {} | Author: {} | License: {}\r",
                        credit.source_url.replace(['\r', '\n'], " "),
                        credit.author.replace(['\r', '\n'], " "),
                        credit.license.replace(['\r', '\n'], " ")
                    )?;
                }
            }
            for r in &self.subparts[name] {
                let a: [[f64; 4]; 4] = r.matrix.into();
                writeln!(
                    text,
                    "1 {} {} {} {} {} {} {} {} {} {} {} {} {} {}\r",
                    r.color,
                    a[0][3],
                    a[1][3],
                    a[2][3],
                    a[0][0],
                    a[0][1],
                    a[0][2],
                    a[1][0],
                    a[1][1],
                    a[1][2],
                    a[2][0],
                    a[2][1],
                    a[2][2],
                    r.file
                )?;
            }
        }
        Ok(text)
    }
}

#[cfg(test)]
mod isometry_tests {
    use super::*;
    #[test]
    fn nested_cancelling_scales_and_unused_invalid_instances_are_rejected() {
        let leaf = Reference {
            file: "3003.dat".into(),
            color: 19,
            matrix: Transform::from_pose(DMat3::from_diagonal(DVec3::splat(0.5)), DVec3::ZERO),
        };
        let mut model = Model {
            root: "main".into(),
            subparts: BTreeMap::from([
                (
                    "main".into(),
                    vec![Reference {
                        file: "child".into(),
                        color: 16,
                        matrix: Transform::from_pose(
                            DMat3::from_diagonal(DVec3::splat(2.)),
                            DVec3::ZERO,
                        ),
                    }],
                ),
                ("child".into(), vec![leaf]),
            ]),
        };
        assert!(crate::physical::is_proper_rotation(
            model.references().unwrap()[0].transform
        ));
        assert!(model.validate_isometries().is_err());
        model = Model::from_bricks(&crate::demo::physics_lab().bricks);
        model.validate_isometries().unwrap();
        model.subparts.insert(
            "unused".into(),
            vec![Reference {
                file: "3003.dat".into(),
                color: 19,
                matrix: Transform::from_pose(
                    DMat3::from_diagonal(DVec3::new(-1., 1., 1.)),
                    DVec3::ZERO,
                ),
            }],
        );
        assert!(model.validate_isometries().is_err());
    }
}
