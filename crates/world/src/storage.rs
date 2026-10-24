//! Canonical construction files: LDraw subparts + per-occurrence Ergo metadata.
//! Runtime/REST scene messages and spatial indexes keep materialized leaf bricks.
use crate::{WorldExport, assembly::Model, geometry::Mesh, ldraw};
use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

pub const FORMAT: &str = "ergo-ldraw-1";

/// The REST/WASM boundary accepts canonical files as well as materialized scenes.
/// Serialization of WorldExport remains materialized for streaming/local jobs.
#[derive(Deserialize)]
pub struct SceneWire {
    #[serde(default)]
    format: Option<String>,
    schema: u32,
    name: String,
    geo: crate::GeoReference,
    #[serde(default)]
    model: Option<Model>,
    #[serde(default)]
    bricks: Option<Vec<crate::Brick>>,
    #[serde(default)]
    pieces: Vec<PieceProperties>,
    meshes: BTreeMap<String, Mesh>,
    #[serde(default)]
    joints: Vec<crate::JointSpec>,
    #[serde(default)]
    fixed_bricks: Vec<u32>,
    #[serde(default)]
    ground: Vec<crate::terrain::GroundBlock>,
    #[serde(default)]
    groups: Vec<crate::groups::Group>,
}
impl TryFrom<SceneWire> for WorldExport {
    type Error = anyhow::Error;
    fn try_from(w: SceneWire) -> Result<Self> {
        if let Some(format) = w.format {
            ensure!(
                w.bricks.is_none(),
                "Construction cannot contain both model and materialized bricks"
            );
            Construction {
                format,
                schema: w.schema,
                name: w.name,
                geo: w.geo,
                model: w.model.context("Missing construction model")?,
                pieces: w.pieces,
                meshes: w.meshes,
                joints: w.joints,
                fixed_bricks: w.fixed_bricks,
                ground: w.ground,
                groups: w.groups,
            }
            .into_scene()
        } else {
            Ok(Self {
                schema: w.schema,
                name: w.name,
                geo: w.geo,
                bricks: w
                    .bricks
                    .context("Missing materialized bricks or explicit construction format")?,
                meshes: w.meshes,
                model: w.model,
                joints: w.joints,
                fixed_bricks: w.fixed_bricks,
                ground: w.ground,
                groups: w.groups,
            })
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PieceProperties {
    pub id: u32,
    pub rgb: u32,
    #[serde(default)]
    pub step: u32,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Construction {
    pub format: String,
    pub schema: u32,
    pub name: String,
    pub geo: crate::GeoReference,
    pub model: Model,
    /// Depth-first leaf order. IDs belong to occurrences, never shared templates.
    #[serde(default)]
    pub pieces: Vec<PieceProperties>,
    pub meshes: BTreeMap<String, Mesh>,
    #[serde(default)]
    pub joints: Vec<crate::JointSpec>,
    #[serde(default)]
    pub fixed_bricks: Vec<u32>,
    #[serde(default)]
    pub ground: Vec<crate::terrain::GroundBlock>,
    #[serde(default)]
    pub groups: Vec<crate::groups::Group>,
}

impl From<&WorldExport> for Construction {
    fn from(scene: &WorldExport) -> Self {
        Self {
            format: FORMAT.into(),
            schema: scene.schema,
            name: scene.name.clone(),
            geo: scene.geo,
            model: Model::for_scene(scene),
            pieces: scene
                .bricks
                .iter()
                .map(|b| PieceProperties {
                    id: b.id,
                    rgb: b.rgb,
                    step: b.step,
                })
                .collect(),
            meshes: scene.meshes.clone(),
            joints: scene.joints.clone(),
            fixed_bricks: scene.fixed_bricks.clone(),
            ground: scene.ground.clone(),
            groups: scene.groups.clone(),
        }
    }
}
impl Construction {
    pub fn into_scene(self) -> Result<WorldExport> {
        ensure!(
            self.format == FORMAT && self.schema == crate::SCHEMA_VERSION,
            "Unsupported construction format or coordinate schema"
        );
        let refs = self.model.references()?;
        ensure!(
            self.pieces.is_empty() || self.pieces.len() == refs.len(),
            "Piece properties must match expanded subpart occurrences"
        );
        let mut bricks = Vec::with_capacity(refs.len());
        let mut ids = std::collections::HashSet::new();
        for (i, r) in refs.iter().enumerate() {
            let mesh = self.meshes.get(&r.name).context("Missing subpart mesh")?;
            let properties = self.pieces.get(i);
            let id = properties.map_or(i as u32 + 1, |p| p.id);
            ensure!(id > 0 && ids.insert(id), "Invalid or duplicate piece ID");
            let mut brick = ldraw::brick_from_reference(id, r, mesh, &BTreeMap::new());
            if let Some(p) = properties {
                brick.rgb = p.rgb;
                brick.step = p.step;
            }
            bricks.push(brick);
        }
        Ok(WorldExport {
            schema: self.schema,
            name: self.name,
            geo: self.geo,
            bricks,
            meshes: self.meshes,
            joints: self.joints,
            fixed_bricks: self.fixed_bricks,
            ground: self.ground,
            groups: self.groups,
            model: Some(self.model),
        })
    }
}

/// Read old schema-2 leaf files as well as new construction envelopes. An unknown
/// explicit format is an error, never silently interpreted as a legacy file.
pub fn decode(bytes: &[u8]) -> Result<WorldExport> {
    Ok(serde_json::from_slice(bytes)?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::assembly::{Reference, Transform};
    use glam::{DMat3, DVec3};

    fn construction() -> Construction {
        Construction {
            format: FORMAT.into(),
            schema: crate::SCHEMA_VERSION,
            name: "Repeated pillar".into(),
            geo: Default::default(),
            model: Model {
                root: "main.ldr".into(),
                subparts: BTreeMap::from([
                    (
                        "main.ldr".into(),
                        vec![
                            Reference {
                                file: "pillar.ldr".into(),
                                color: 4,
                                matrix: Transform::default(),
                            },
                            Reference {
                                file: "pillar.ldr".into(),
                                color: 1,
                                matrix: Transform::from_pose(
                                    DMat3::from_rotation_y(std::f64::consts::FRAC_PI_2),
                                    DVec3::new(100., 0., 40.),
                                ),
                            },
                        ],
                    ),
                    (
                        "pillar.ldr".into(),
                        vec![Reference {
                            file: "3003.dat".into(),
                            color: 16,
                            matrix: Transform::from_pose(
                                DMat3::IDENTITY,
                                DVec3::new(20., -24., 0.),
                            ),
                        }],
                    ),
                ]),
            },
            pieces: vec![],
            meshes: BTreeMap::from([(
                "3003.dat".into(),
                crate::geometry::fallback_mesh("3003.dat"),
            )]),
            joints: vec![],
            fixed_bricks: vec![],
            ground: vec![],
            groups: vec![],
        }
    }

    #[test]
    fn shared_subparts_compose_in_ldu_inherit_colors_and_roundtrip_metadata() {
        let mut scene = construction().into_scene().unwrap();
        assert_eq!(scene.bricks.len(), 2);
        assert_eq!(scene.bricks[0].color, 4);
        assert_eq!(scene.bricks[1].color, 1);
        assert!((scene.bricks[1].position - DVec3::new(5., 1., -1.)).length() < 1e-12);
        scene.bricks[0].id = 91;
        scene.bricks[0].rgb = 0x123456;
        scene.bricks[0].step = 8;
        scene.fixed_bricks.push(91);
        let stored = Construction::from(&scene);
        assert_eq!(stored.model.subparts.len(), 2);
        let encoded = serde_json::to_vec(&stored).unwrap();
        let value: serde_json::Value = serde_json::from_slice(&encoded).unwrap();
        assert!(value.get("bricks").is_none());
        assert_eq!(
            value["model"]["subparts"]["main.ldr"][1]["matrix"][0][3],
            100.
        );
        let decoded = decode(&encoded).unwrap();
        assert_eq!(decoded.bricks[0].id, 91);
        assert_eq!(decoded.bricks[0].rgb, 0x123456);
        assert_eq!(decoded.bricks[0].step, 8);
        assert_eq!(decoded.fixed_bricks, vec![91]);
        let mpd = stored.model.to_mpd().unwrap();
        let parsed = ldraw::Document::parse(&mpd, "model.mpd")
            .unwrap()
            .instances()
            .unwrap();
        assert_eq!(parsed.len(), 2);
        assert!((parsed[1].position - DVec3::new(100., -24., 20.)).length() < 1e-12);
        let mut cbor = Vec::new();
        ciborium::into_writer(&stored, &mut cbor).unwrap();
        let cbor: Construction = ciborium::from_reader(cbor.as_slice()).unwrap();
        assert_eq!(cbor.into_scene().unwrap().bricks[0].id, 91);
    }

    #[test]
    fn old_files_load_and_independent_edits_cannot_restore_stale_template_poses() {
        let mut scene = construction().into_scene().unwrap();
        let old = serde_json::to_vec(&scene).unwrap();
        assert_eq!(decode(&old).unwrap().bricks.len(), 2);
        scene.bricks[1].translated(DVec3::new(7., 0., 0.));
        let stored = Construction::from(&scene);
        assert_eq!(stored.model.subparts.len(), 1);
        let restored = stored.into_scene().unwrap();
        assert!((restored.bricks[1].position - scene.bricks[1].position).length() < 1e-12);
    }

    #[test]
    fn rejects_invalid_matrices_cycles_paths_missing_parts_and_duplicate_ids() {
        let mut rows: [[f64; 4]; 4] = Transform::default().into();
        rows[3][0] = 1.;
        assert!(Transform::try_from(rows).is_err());
        rows[3][0] = 0.;
        rows[0][0] = 0.;
        assert!(Transform::try_from(rows).is_err());
        rows[0][0] = f64::NAN;
        assert!(Transform::try_from(rows).is_err());
        let mut c = construction();
        c.model.subparts.get_mut("pillar.ldr").unwrap()[0].file = "main.ldr".into();
        assert!(c.into_scene().is_err());
        let mut c = construction();
        c.model.subparts.get_mut("pillar.ldr").unwrap()[0].file = "../part.dat".into();
        assert!(c.into_scene().is_err());
        let mut c = construction();
        c.meshes.clear();
        assert!(c.into_scene().is_err());
        let mut c = construction();
        c.pieces = vec![
            PieceProperties {
                id: 1,
                rgb: 0,
                step: 0
            };
            2
        ];
        assert!(c.into_scene().is_err());
    }

    #[test]
    fn nested_dat_subparts_and_namespaced_instances_preserve_hierarchy() {
        let doc = ldraw::Document::parse("0 FILE main.ldr\n1 4 0 0 0 1 0 0 0 1 0 0 0 1 sub.dat\n0 FILE sub.dat\n0 !LDRAW_ORG Model\n1 16 0 -24 0 1 0 0 0 1 0 0 0 1 3003.dat", "model.mpd").unwrap();
        assert_eq!(doc.instances().unwrap()[0].name, "3003.dat");
        let mut a = Model::from_document(&doc).unwrap();
        let b = a.clone();
        a.append(
            &b,
            Transform::from_pose(DMat3::IDENTITY, DVec3::X * 100.),
            1,
        )
        .unwrap();
        let refs = a.references().unwrap();
        assert_eq!(refs.len(), 2);
        assert_eq!(refs[1].position.x, 100.);
        assert!(a.subparts.contains_key("instance-1/sub.dat"));
    }
}
