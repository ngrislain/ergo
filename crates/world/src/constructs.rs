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
    /// Optional proper metric rotation in public east/north/up axes.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rotation: Option<[[f64; 3]; 3]>,
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
                let metric = part
                    .rotation
                    .unwrap_or([[c, -s, 0.], [s, c, 0.], [0., 0., 1.]]);
                let scale = [0.008, 0.008, 0.0096];
                let matrix: [[f64; 3]; 3] = std::array::from_fn(|i| {
                    std::array::from_fn(|j| metric[i][j] * scale[j] / scale[i])
                });
                Reference {
                    file: crate::catalog::canonical_part(&part.part),
                    color: part.color.unwrap_or(16),
                    matrix: Transform::from_world([
                        [matrix[0][0], matrix[0][1], matrix[0][2], part.position[0]],
                        [matrix[1][0], matrix[1][1], matrix[1][2], part.position[1]],
                        [matrix[2][0], matrix[2][1], matrix[2][2], part.position[2]],
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
            assert!(
                entry
                    .id
                    .rsplit_once("-v")
                    .unwrap()
                    .1
                    .parse::<u32>()
                    .unwrap()
                    > 0
            );
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

    fn nominal_scene(model: Model) -> crate::WorldExport {
        let mut meshes = BTreeMap::new();
        let bricks = model
            .references()
            .unwrap()
            .iter()
            .enumerate()
            .map(|(i, reference)| {
                let mesh = meshes.entry(reference.name.clone()).or_insert_with(|| {
                    let mut mesh = crate::geometry::fallback_mesh(&reference.name);
                    mesh.body_bounds =
                        crate::geometry::nominal_body_bounds(&reference.name).unwrap();
                    mesh.bounds = mesh.body_bounds;
                    mesh
                });
                crate::ldraw::brick_from_reference(i as u32 + 1, reference, mesh, &BTreeMap::new())
            })
            .collect();
        crate::WorldExport {
            schema: crate::SCHEMA_VERSION,
            name: "Catalogue fixture".into(),
            geo: Default::default(),
            bricks,
            meshes,
            joints: vec![],
            fixed_bricks: vec![],
            groups: vec![],
            ground: vec![],
            model: Some(model),
        }
    }

    #[test]
    fn adjacent_medieval_guards_have_printed_chests_and_real_equipment_interfaces() {
        let mut model = instance("medieval-guard-v3", 4);
        model
            .append(
                &instance("medieval-guard-v3", 1),
                Transform::from_world([
                    [1., 0., 0., 4.],
                    [0., 1., 0., 0.],
                    [0., 0., 1., 0.],
                    [0., 0., 0., 1.],
                ])
                .unwrap(),
                1,
            )
            .unwrap();
        let mut scene = nominal_scene(model);
        let report = crate::validation::validate_static(&mut scene).unwrap();
        assert_eq!(report.checked_bricks, 20);
        assert_eq!(report.joint_strength_required, Some(true));
        for part in ["3844.dat", "3846p4h.dat", "4497.dat", "973p4f.dat"] {
            assert_eq!(
                scene
                    .bricks
                    .iter()
                    .filter(|brick| brick.part == part)
                    .count(),
                2
            );
        }
        let torsos: Vec<_> = scene
            .bricks
            .iter()
            .filter(|brick| brick.part == "973p4f.dat")
            .collect();
        assert_eq!(torsos[0].color, 4);
        assert_eq!(torsos[1].color, 1);
    }

    fn standard_on_brick() -> crate::WorldExport {
        let mut model: Model = serde_json::from_value(serde_json::json!({
            "format":"ergo-model-1", "schema":2, "root":"base", "definitions":{"base":[{
                "target":{"kind":"part","id":"4201.dat"}, "color":71,
                "transform":[[1,0,0,4],[0,1,0,4],[0,0,1,1],[0,0,0,1]]
            }]}
        }))
        .unwrap();
        model
            .append(
                &instance("castle-standard-v1", 1),
                Transform::from_world([
                    [1., 0., 0., 2.5],
                    [0., 1., 0., 2.5],
                    [0., 0., 1., 1.],
                    [0., 0., 0., 1.],
                ])
                .unwrap(),
                1,
            )
            .unwrap();
        nominal_scene(model)
    }

    #[test]
    fn plate_free_models_stand_on_feet_or_tyres_and_use_real_equipment_connections() {
        for id in ["minifigure-v2", "classic-car-v2"] {
            let model = instance(id, 4);
            let mut scene = nominal_scene(model);
            assert_eq!(
                crate::placement::snap_scene(&mut scene).unwrap(),
                glam::DVec3::ZERO,
                "{id} must stay grid registered without a display plate"
            );
            let report = crate::validation::validate_static(&mut scene)
                .unwrap_or_else(|e| panic!("{id}: {e:#}"));
            let grounded: Vec<_> = scene
                .bricks
                .iter()
                .filter(|b| b.bounds.min.y.abs() < 1e-6)
                .collect();
            assert!(!grounded.is_empty());
            assert!(
                grounded
                    .iter()
                    .all(|b| ["3815c01.dat", "4624c01.dat"].contains(&b.part.as_str())),
                "{id}"
            );
            assert_eq!(report.checked_bricks, scene.bricks.len());
            if id == "classic-car-v2" {
                assert_eq!(
                    scene
                        .joints
                        .iter()
                        .filter(|j| matches!(j.kind, crate::JointKind::Revolute { .. }))
                        .count(),
                    4
                );
            }
        }
    }

    #[test]
    fn reposed_castle_guard_and_archer_use_real_wrists_and_vertical_equipment() {
        for id in ["medieval-guard-v3", "medieval-archer-v1"] {
            let mut scene = nominal_scene(instance(id, 4));
            assert_eq!(
                crate::placement::snap_scene(&mut scene).unwrap(),
                glam::DVec3::ZERO
            );
            let report = crate::validation::validate_static(&mut scene)
                .unwrap_or_else(|e| panic!("{id}: {e:#}"));
            assert_eq!(
                report.checked_bricks,
                if id == "medieval-guard-v3" { 10 } else { 9 }
            );
            let mut kinds = BTreeMap::new();
            for (i, a) in scene.bricks.iter().enumerate() {
                for b in &scene.bricks[i + 1..] {
                    if let Some(c) = crate::attachments::connection(a, b, 0.002) {
                        *kinds.entry(c.label).or_insert(0) += 1;
                    }
                }
            }
            assert_eq!(
                kinds.get("minifigure shoulder connection"),
                Some(&2),
                "{id}"
            );
            assert_eq!(kinds.get("minifigure wrist connection"), Some(&2), "{id}");
            assert_eq!(
                kinds.get("minifigure grip / bar connection"),
                Some(&if id == "medieval-guard-v3" { 2 } else { 1 }),
                "{id}"
            );
            for brick in scene
                .bricks
                .iter()
                .filter(|b| ["4497.dat", "4499.dat", "3846p4h.dat"].contains(&b.part.as_str()))
            {
                let axis = crate::physical::metric_transform(brick.transform).y_axis;
                assert!(
                    (axis - glam::DVec3::Y).length() < 1e-8,
                    "Equipment must follow the actual reposed hands, upright: {}",
                    brick.part
                );
            }
        }
    }

    #[test]
    fn a_supported_flag_passes_but_a_flying_flag_does_not_gain_invented_clutch_strength() {
        let mut supported = standard_on_brick();
        let report = crate::validation::validate_static(&mut supported).unwrap();
        assert_eq!(report.joint_strength_required, Some(false));
        let mut floating = standard_on_brick();
        let public_y = |brick: &crate::Brick| crate::coordinates::public(brick.position).y;
        floating
            .bricks
            .retain(|brick| (public_y(brick) - 4.5).abs() > 1e-6);
        floating.model = None;
        let issue = crate::validation::compression_issue(
            &floating.bricks,
            Default::default(),
            &Default::default(),
        )
        .unwrap()
        .unwrap();
        assert_eq!(issue.part, "4495a.dat");
        let report = crate::validation::validate_static(&mut floating).unwrap();
        assert_eq!(report.joint_strength_required, Some(true));
        // The server's existing admission path requires the operator book for
        // this case; the geometry-only local solver supplies no calibration.
        assert!(
            crate::physical::ConnectionMeasurements::default()
                .apply(&floating.bricks, &mut floating.joints)
                .is_err()
        );
    }

    #[test]
    fn hundred_cars_share_one_definition_and_roundtrip_distinct_poses_and_colours() {
        let prototype = instance("classic-car-v2", 16);
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
        assert_eq!(leaves.len(), 2700);
        assert_eq!(leaves[5].color, 4);
        assert_eq!(leaves[32].color, 1);
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
                .contains_key("library/classic-car-v2/main")
        );
        assert_eq!(decoded.references().unwrap().len(), 2700);
    }

    #[test]
    fn edits_to_a_common_definition_remain_explicit_and_cannot_shadow_the_library() {
        let mut model = instance("minifigure-v2", 4);
        model
            .subparts
            .get_mut(&get("minifigure-v2").unwrap().root())
            .unwrap()[0]
            .color = 14;
        let value = serde_json::to_value(&model).unwrap();
        assert!(
            value["definitions"]
                .as_object()
                .unwrap()
                .contains_key("library/minifigure-v2/main")
        );
        let decoded: Model = serde_json::from_value(value).unwrap();
        assert_eq!(decoded.references().unwrap()[0].color, 14);
        let mut invalid = serde_json::to_value(instance("minifigure-v2", 4)).unwrap();
        invalid["definitions"]["library/minifigure-v2/main"] = serde_json::json!([]);
        assert!(serde_json::from_value::<Model>(invalid).is_err());
        let mut unknown = serde_json::to_value(instance("minifigure-v2", 4)).unwrap();
        unknown["definitions"]["main"][0]["target"]["id"] = serde_json::json!("minifigure-v999");
        assert!(serde_json::from_value::<Model>(unknown).is_err());
    }
    #[test]
    fn an_existing_coloured_car_becomes_a_reference_without_moving_its_parts() {
        let entry = get("classic-car-v2").unwrap();
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
