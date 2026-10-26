//! Ergo-native model wire format. Import coordinates remain an internal adapter.
use super::{Model, Reference, Transform};
use glam::{DMat3, DMat4, DVec3};
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::collections::BTreeMap;

pub(super) const FORMAT: &str = "ergo-model-1";
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ModelWire {
    format: String,
    schema: u32,
    root: String,
    definitions: BTreeMap<String, Vec<InstanceWire>>,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct InstanceWire {
    target: Target,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    color: Option<u32>,
    transform: [[f64; 4]; 4],
}
#[derive(Serialize, Deserialize)]
#[serde(tag = "kind", content = "id", rename_all = "snake_case")]
enum Target {
    Part(String),
    Definition(String),
}

fn import_to_world() -> DMat4 {
    let public = DMat3::from_cols(DVec3::X, DVec3::Z, DVec3::NEG_Y);
    DMat4::from_mat3(public * crate::ldraw::conversion())
}
pub(super) fn world_transform(rows: [[f64; 4]; 4]) -> anyhow::Result<Transform> {
    anyhow::ensure!(
        rows[3] == [0., 0., 0., 1.] && rows.iter().flatten().all(|n| n.is_finite()),
        "Expected a finite affine Ergo transform"
    );
    let units = import_to_world();
    let native = DMat4::from_cols_array_2d(&rows).transpose();
    Transform::try_from(
        (units.inverse() * native * units)
            .transpose()
            .to_cols_array_2d(),
    )
}
impl Serialize for Model {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let units = import_to_world();
        let definitions = self
            .subparts
            .iter()
            .map(|(id, references)| {
                let instances = references
                    .iter()
                    .map(|reference| InstanceWire {
                        target: if self.subparts.contains_key(&reference.file) {
                            Target::Definition(reference.file.clone())
                        } else {
                            Target::Part(reference.file.clone())
                        },
                        color: (reference.color != 16).then_some(reference.color),
                        transform: (units * reference.matrix.0 * units.inverse())
                            .transpose()
                            .to_cols_array_2d(),
                    })
                    .collect();
                (id.clone(), instances)
            })
            .collect();
        ModelWire {
            format: FORMAT.into(),
            schema: crate::SCHEMA_VERSION,
            root: self.root.clone(),
            definitions,
        }
        .serialize(serializer)
    }
}
impl<'de> Deserialize<'de> for Model {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        use serde::de::Error;
        let wire = ModelWire::deserialize(deserializer)?;
        if wire.format != FORMAT || wire.schema != crate::SCHEMA_VERSION {
            return Err(D::Error::custom(
                "Unsupported Ergo model format or coordinate schema",
            ));
        }
        let mut subparts = BTreeMap::new();
        for (id, instances) in &wire.definitions {
            let mut references = Vec::with_capacity(instances.len());
            for instance in instances {
                let file = match &instance.target {
                    Target::Part(id) if !wire.definitions.contains_key(id) => id,
                    Target::Definition(id) if wire.definitions.contains_key(id) => id,
                    _ => return Err(D::Error::custom("Part/definition target mismatch")),
                };
                if instance.color == Some(16) {
                    return Err(D::Error::custom("Omit color to inherit it"));
                }
                let matrix = world_transform(instance.transform).map_err(D::Error::custom)?;
                references.push(Reference {
                    file: file.clone(),
                    color: instance.color.unwrap_or(16),
                    matrix,
                });
            }
            subparts.insert(id.clone(), references);
        }
        let model = Self {
            root: wire.root,
            subparts,
        };
        model.references().map_err(D::Error::custom)?;
        Ok(model)
    }
}
