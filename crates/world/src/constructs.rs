//! Immutable, versioned common constructs, referenced without copying their parts.
use crate::assembly::{Model, Reference, Transform};
use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::LazyLock,
};

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Construct {
    pub id: String,
    pub name: String,
    pub description: String,
    pub credits: Vec<crate::geometry::GeometryCredit>,
    pub parts: Vec<PlacedPart>,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PlacedPart {
    pub part: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color: Option<u32>,
    pub position: [f64; 3],
    #[serde(default)]
    pub quarter_turns: i32,
}

pub fn all() -> &'static [Construct] {
    static ENTRIES: LazyLock<Vec<Construct>> = LazyLock::new(|| {
        serde_json::from_str(include_str!("../library/constructs.json"))
            .expect("Checked-in construct library must be valid")
    });
    &ENTRIES
}

pub fn get(id: &str) -> Option<&'static Construct> {
    all().iter().find(|entry| entry.id == id)
}

#[derive(Serialize)]
pub struct ConstructInfo {
    pub id: &'static str,
    pub name: &'static str,
    pub description: &'static str,
    pub part_count: usize,
    pub credits: &'static [crate::geometry::GeometryCredit],
}

pub fn catalog() -> Vec<ConstructInfo> {
    all()
        .iter()
        .map(|entry| ConstructInfo {
            id: &entry.id,
            name: &entry.name,
            description: &entry.description,
            part_count: entry.parts.len(),
            credits: &entry.credits,
        })
        .collect()
}

impl Construct {
    pub fn root(&self) -> String {
        format!("library/{}/main", self.id)
    }

    pub fn references(&self) -> Vec<Reference> {
        self.parts
            .iter()
            .map(|part| {
                let (c, s) = match part.quarter_turns.rem_euclid(4) {
                    0 => (1., 0.),
                    1 => (0., 1.),
                    2 => (-1., 0.),
                    _ => (0., -1.),
                };
                Reference {
                    file: crate::catalog::canonical_part(&part.part),
                    color: part.color.unwrap_or(16),
                    matrix: Transform::from_world([
                        [c, -s, 0., part.position[0]],
                        [s, c, 0., part.position[1]],
                        [0., 0., 1., part.position[2]],
                        [0., 0., 0., 1.],
                    ])
                    .expect("Checked-in construct poses must be valid"),
                }
            })
            .collect()
    }
}

pub(crate) fn install(
    id: &str,
    definitions: &mut BTreeMap<String, Vec<Reference>>,
) -> Result<String> {
    let entry = get(id).context("Unknown versioned construct ID")?;
    let root = entry.root();
    let references = entry.references();
    if let Some(previous) = definitions.get(&root) {
        ensure!(
            *previous == references,
            "Conflicting common construct definition"
        );
    } else {
        definitions.insert(root.clone(), references);
    }
    Ok(root)
}

/// Exact matches only: an independently edited definition is serialized inline.
pub(crate) fn installed(model: &Model) -> (BTreeMap<String, String>, BTreeSet<String>) {
    let roots: BTreeMap<_, _> = all()
        .iter()
        .filter(|entry| {
            model
                .subparts
                .get(&entry.root())
                .is_some_and(|parts| *parts == entry.references())
        })
        .map(|entry| (entry.root(), entry.id.clone()))
        .collect();
    let names = roots.keys().cloned().collect();
    (roots, names)
}

pub fn credits(model: &Model) -> Vec<&'static crate::geometry::GeometryCredit> {
    let (roots, _) = installed(model);
    let mut stack = vec![model.root.clone()];
    let mut visited = BTreeSet::new();
    let mut entries = BTreeSet::new();
    while let Some(name) = stack.pop() {
        if !visited.insert(name.clone()) {
            continue;
        }
        if let Some(id) = roots.get(&name) {
            entries.insert(id.clone());
        }
        if let Some(references) = model.subparts.get(&name) {
            stack.extend(
                references
                    .iter()
                    .filter(|reference| model.subparts.contains_key(&reference.file))
                    .map(|reference| reference.file.clone()),
            );
        }
    }
    entries
        .iter()
        .flat_map(|id| &get(id).unwrap().credits)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn common_constructs_are_versioned_proper_models_with_canonical_parts() {
        let mut ids = BTreeSet::new();
        for entry in all() {
            assert!(ids.insert(&entry.id));
            assert!(entry.id.ends_with("-v1"));
            let references = entry.references();
            assert_eq!(references.len(), entry.parts.len());
            let model = Model {
                root: entry.root(),
                subparts: BTreeMap::from([(entry.root(), references)]),
            };
            model.validate_isometries().unwrap();
            for part in &entry.parts {
                assert_eq!(crate::catalog::canonical_part(&part.part), part.part);
                assert!(crate::physical::mass_kg(&part.part).unwrap() > 0.);
                assert!(
                    part.color
                        .is_none_or(|color| color != 16 && crate::catalog::known_color(color))
                );
            }
        }
    }

    fn instance(id: &str, color: u32) -> Model {
        let mut value = serde_json::json!({
            "format":"ergo-model-1","schema":2,"root":"main","definitions":{"main":[{
                "target":{"kind":"construct","id":id},"color":color,
                "transform":[[1,0,0,0],[0,1,0,0],[0,0,1,0],[0,0,0,1]]
            }]}
        });
        if color == 16 {
            value["definitions"]["main"][0]
                .as_object_mut()
                .unwrap()
                .remove("color");
        }
        serde_json::from_value(value).unwrap()
    }

    #[test]
    fn hundred_cars_share_one_definition_and_roundtrip_distinct_poses_and_colours() {
        let prototype = instance("classic-car-v1", 16);
        let mut model = Model {
            root: "main".into(),
            subparts: BTreeMap::from([("main".into(), vec![])]),
        };
        for index in 0..100 {
            model
                .append(
                    &prototype,
                    Transform::from_world([
                        [1., 0., 0., f64::from(index) * 12.],
                        [0., 1., 0., 0.],
                        [0., 0., 1., 0.],
                        [0., 0., 0., 1.],
                    ])
                    .unwrap(),
                    if index % 2 == 0 { 4 } else { 1 },
                )
                .unwrap();
        }
        model.validate_isometries().unwrap();
        let leaves = model.references().unwrap();
        assert_eq!(leaves.len(), 2100);
        assert_eq!(leaves[5].color, 4);
        assert_eq!(leaves[26].color, 1);
        let wire = serde_json::to_vec(&model).unwrap();
        let decoded: Model = serde_json::from_slice(&wire).unwrap();
        assert_eq!(
            serde_json::to_value(&model).unwrap(),
            serde_json::to_value(&decoded).unwrap()
        );
        let flat = Model {
            root: "flat".into(),
            subparts: BTreeMap::from([(
                "flat".into(),
                leaves
                    .iter()
                    .map(|leaf| Reference {
                        file: leaf.name.clone(),
                        color: leaf.color,
                        matrix: Transform::from_pose(leaf.transform, leaf.position),
                    })
                    .collect(),
            )]),
        };
        let flat_bytes = serde_json::to_vec(&flat).unwrap().len();
        assert!(
            wire.len() * 8 < flat_bytes,
            "{} compact vs {} flat model bytes",
            wire.len(),
            flat_bytes
        );
        assert!(model.subparts.len() < 6);
        eprintln!(
            "100 cars: {} compact model bytes vs {} flat model bytes",
            wire.len(),
            flat_bytes
        );
        let value = serde_json::to_value(&model).unwrap();
        assert!(
            !value["definitions"]
                .as_object()
                .unwrap()
                .contains_key("library/classic-car-v1/main")
        );
        assert_eq!(decoded.references().unwrap().len(), 2100);
    }

    #[test]
    fn edits_to_a_common_definition_remain_explicit_and_cannot_shadow_the_library() {
        let mut model = instance("minifigure-v1", 4);
        model
            .subparts
            .get_mut(&get("minifigure-v1").unwrap().root())
            .unwrap()[0]
            .color = 14;
        let value = serde_json::to_value(&model).unwrap();
        assert!(
            value["definitions"]
                .as_object()
                .unwrap()
                .contains_key("library/minifigure-v1/main")
        );
        let decoded: Model = serde_json::from_value(value).unwrap();
        assert_eq!(decoded.references().unwrap()[0].color, 14);
        let mut invalid = serde_json::to_value(instance("minifigure-v1", 4)).unwrap();
        invalid["definitions"]["library/minifigure-v1/main"] = serde_json::json!([]);
        assert!(serde_json::from_value::<Model>(invalid).is_err());
        let mut unknown = serde_json::to_value(instance("minifigure-v1", 4)).unwrap();
        unknown["definitions"]["main"][0]["target"]["id"] = serde_json::json!("minifigure-v999");
        assert!(serde_json::from_value::<Model>(unknown).is_err());
    }
    #[test]
    fn an_existing_coloured_car_becomes_a_reference_without_moving_its_parts() {
        let entry = get("classic-car-v1").unwrap();
        let mut parts = entry.references();
        for part in &mut parts {
            if part.color == 16 {
                part.color = 4;
            }
        }
        let mut model = Model {
            root: "existing-car".into(),
            subparts: BTreeMap::from([("existing-car".into(), parts)]),
        };
        let before = model.references().unwrap();
        assert_eq!(model.use_common_constructs().unwrap(), 1);
        let after = model.references().unwrap();
        assert_eq!(before.len(), after.len());
        for (old, new) in before.iter().zip(&after) {
            assert_eq!(old.name, new.name);
            assert_eq!(old.color, new.color);
            assert_eq!(old.position, new.position);
            assert_eq!(old.transform, new.transform);
        }
        let wire = serde_json::to_value(&model).unwrap();
        assert_eq!(
            wire["definitions"]["existing-car"][0]["target"]["kind"],
            "construct"
        );
        let mpd = model.to_mpd().unwrap();
        assert!(mpd.contains("Robert Paciorek [bercik]"));
        assert!(mpd.contains("https://library.ldraw.org/omr/sets/610"));
    }
}
