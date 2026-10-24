//! LDraw-style reusable subfiles. Matrices are row-major, column-vector affine
//! transforms in LDU, with -Y up. Runtime/indexed bricks are derived leaves.
use crate::{Brick, WorldExport, ldraw};
use anyhow::{Context, Result, ensure};
use glam::{DMat3, DMat4, DVec3};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap};

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

#[derive(Clone, Debug, Serialize, Deserialize)]
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

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Model {
    pub root: String,
    pub subparts: BTreeMap<String, Vec<Reference>>,
}

impl Model {
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
            // Polygon-bearing .dat files are mesh definitions, not assemblies.
            if name != &document.root
                && text
                    .lines()
                    .any(|line| matches!(line.split_whitespace().next(), Some("3" | "4")))
            {
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

    /// Append an independent instance without copying its leaf geometry. Local
    /// definition names are namespaced to avoid collisions with existing files.
    pub fn append(&mut self, other: &Self, matrix: Transform, color: u32) -> Result<()> {
        self.references()?;
        other.references()?;
        let prefix = (1..)
            .map(|i| format!("instance-{i}/"))
            .find(|p| self.subparts.keys().all(|name| !name.starts_with(p)))
            .unwrap();
        let rename = |name: &str| {
            if other.subparts.contains_key(name) {
                format!("{prefix}{name}")
            } else {
                name.into()
            }
        };
        for (name, references) in &other.subparts {
            self.subparts.insert(
                rename(name),
                references
                    .iter()
                    .map(|r| Reference {
                        file: rename(&r.file),
                        color: r.color,
                        matrix: r.matrix,
                    })
                    .collect(),
            );
        }
        let root = (1..)
            .map(|i| format!("combined-{i}.ldr"))
            .find(|name| !self.subparts.contains_key(name))
            .unwrap();
        self.subparts.insert(
            root.clone(),
            vec![
                Reference {
                    file: self.root.clone(),
                    matrix: Transform::default(),
                    color: 16,
                },
                Reference {
                    file: rename(&other.root),
                    matrix,
                    color,
                },
            ],
        );
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
