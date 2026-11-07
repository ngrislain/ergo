//! Exercise authoritative import/replacement, not just client-side inference.
use super::*;
use ergo_world::{JointKind, physical::ConnectionMeasurement};
use glam::DVec3;
use std::collections::BTreeSet;

async fn construct(id: &str) -> WorldExport {
    let model: ergo_world::assembly::Model = serde_json::from_value(serde_json::json!({
        "format":"ergo-model-1", "schema":2, "root":"fixture", "definitions":{"fixture":[{
            "target":{"kind":"construct","id":id}, "color":1,
            "transform":[[1,0,0,0],[0,1,0,0],[0,0,1,0],[0,0,0,1]]
        }]}
    }))
    .unwrap();
    let cache = ergo_assets::PartCache::new(ergo_assets::fixture_store(), String::new()).unwrap();
    let mut result = scene(0.);
    result.bricks.clear();
    result.meshes.clear();
    for (i, reference) in model.references().unwrap().iter().enumerate() {
        let mesh = cache.resolve(&reference.name).await.unwrap().mesh;
        result.bricks.push(ergo_world::ldraw::brick_from_reference(
            i as u32 + 1,
            reference,
            &mesh,
            &Default::default(),
        ));
        result.meshes.insert(reference.name.clone(), mesh);
    }
    result.model = Some(model);
    result.joints = ergo_world::constraints::inferred_joints_with_tolerance(&result.bricks, 1e-4);
    result
}

fn calibrated_service(scene: &WorldExport) -> SharedWorlds {
    let mut service = service();
    let mut keys = BTreeSet::new();
    let entries = scene.joints.iter().filter_map(|joint| {
        let a = scene.bricks.iter().find(|b| b.id == joint.a).unwrap();
        let b = scene.bricks.iter().find(|b| b.id == joint.b).unwrap();
        let overlap = a.bounds.max.min(b.bounds.max) - a.bounds.min.max(b.bounds.min);
        let footprint = [overlap.x.max(0.).round() as u32, overlap.z.max(0.).round() as u32];
        let kind = if matches!(joint.kind, JointKind::Fixed) { "fixed" } else { "revolute" };
        let pair = if a.part < b.part { (a.part.clone(), b.part.clone()) } else { (b.part.clone(), a.part.clone()) };
        keys.insert((pair.0, pair.1, kind, footprint)).then(|| ConnectionMeasurement {
            part_a:a.part.clone(), part_b:b.part.clone(), kind:kind.into(), overlap_studs:footprint,
            conservative_force_limit_n:1000., conservative_torque_limit_nm:1000.,
            source:"Synthetic geometry-gate regression fixture; not real connector measurements".into(), samples:1,
        })
    }).collect();
    service.store.measurements = Arc::new(ergo_world::physical::ConnectionMeasurements {
        parts: vec![],
        entries,
        idealized_fixture: false,
    });
    service
}

#[tokio::test]
async fn server_rejects_forged_spear_shield_and_bow_joints_before_publication() {
    for (id, equipment) in [
        ("medieval-guard-v3", vec!["4497.dat", "3846p4h.dat"]),
        ("medieval-archer-v1", vec!["4499.dat"]),
    ] {
        let valid = construct(id).await;
        let service = calibrated_service(&valid);
        let (head, accepted) = seeded(&service, valid.clone()).await;
        assert_eq!(
            accepted
                .accepted
                .audit
                .validation
                .as_ref()
                .unwrap()
                .joint_strength_required,
            Some(true)
        );
        let (app, headers) = http(&service);
        let app = app.layer(axum::extract::DefaultBodyLimit::max(128 * 1024 * 1024));
        let path = format!(
            "/api/shared-worlds/{}/assemblies/{}/workspace",
            head.id, accepted.assembly
        );
        for part in equipment {
            let piece_id = valid.bricks.iter().find(|b| b.part == part).unwrap().id;
            let joint_index = valid
                .joints
                .iter()
                .position(|j| {
                    if j.a != piece_id && j.b != piece_id {
                        return false;
                    }
                    let other = if j.a == piece_id { j.b } else { j.a };
                    valid
                        .bricks
                        .iter()
                        .any(|b| b.id == other && b.part == "3820.dat")
                })
                .unwrap();
            for attack in ["shifted", "tilted", "anchor", "kind", "mesh", "duplicate"] {
                let mut invalid = valid.clone();
                invalid.model = None;
                for joint in &mut invalid.joints {
                    joint.break_force = Some(1e20);
                    joint.break_torque = Some(1e20);
                    joint.strength_source = Some("untrusted client".into());
                }
                match attack {
                    "shifted" => invalid
                        .bricks
                        .iter_mut()
                        .find(|b| b.id == piece_id)
                        .unwrap()
                        .translated(DVec3::X * 0.02),
                    "tilted" => {
                        let brick = invalid
                            .bricks
                            .iter_mut()
                            .find(|b| b.id == piece_id)
                            .unwrap();
                        brick.transform = ergo_world::physical::world_transform(
                            glam::DMat3::from_rotation_z(0.1),
                        ) * brick.transform;
                    }
                    "anchor" => invalid.joints[joint_index].anchor += DVec3::X,
                    // Claim the opposite of the registered behaviour.
                    "kind" => {
                        let joint = &mut invalid.joints[joint_index];
                        joint.kind = match joint.kind {
                            JointKind::Fixed => JointKind::Revolute {
                                axis: DVec3::Y,
                                limits: None,
                            },
                            JointKind::Revolute { .. } => JointKind::Fixed,
                        }
                    }
                    "mesh" => invalid.meshes.get_mut(part).unwrap().vertices[0] += 0.1,
                    "duplicate" => invalid.joints.push(invalid.joints[joint_index].clone()),
                    _ => unreachable!(),
                }
                let (status, response) = json(
                    &app,
                    "PUT",
                    &path,
                    &headers,
                    Some(serde_json::json!({"base":head.revision,"scene":invalid})),
                )
                .await;
                assert_eq!(
                    status,
                    StatusCode::UNPROCESSABLE_ENTITY,
                    "{part} / {attack}: {response}"
                );
                let error = response["error"].as_str().unwrap();
                assert!(
                    error.contains("attachment") || error.contains("Catalogue geometry mismatch"),
                    "{part} / {attack}: {response}"
                );
                assert_eq!(
                    service
                        .head(
                            &head.id,
                            &Principal {
                                id: Some("alice".into())
                            },
                            None
                        )
                        .await
                        .unwrap()
                        .revision,
                    head.revision
                );
            }
        }
        // The lower-level immutable mesh lookup rejects the same forged stream.
        let mut ws = service
            .working(&head.id, "alice", &head.revision)
            .await
            .unwrap();
        let mut forged = valid.meshes["3820.dat"].clone();
        forged.vertices[0] += 0.1;
        let cid = ws.tx.stage_blob(&forged).unwrap();
        assert!(
            ws.mesh(&cid)
                .await
                .err()
                .unwrap()
                .to_string()
                .contains("Catalogue geometry mismatch")
        );
    }
}

#[tokio::test]
async fn server_rejects_wrong_wheel_axis_fixed_bearing_and_invented_stops() {
    let valid = construct("classic-car-v2").await;
    let service = calibrated_service(&valid);
    let (head, accepted) = seeded(&service, valid.clone()).await;
    let (app, headers) = http(&service);
    let app = app.layer(axum::extract::DefaultBodyLimit::max(128 * 1024 * 1024));
    let path = format!(
        "/api/shared-worlds/{}/assemblies/{}/workspace",
        head.id, accepted.assembly
    );
    let index = valid
        .joints
        .iter()
        .position(|j| matches!(j.kind, JointKind::Revolute { .. }))
        .unwrap();
    for attack in ["axis", "fixed", "stops"] {
        let mut invalid = valid.clone();
        invalid.model = None;
        match attack {
            "axis" => {
                invalid.joints[index].kind = JointKind::Revolute {
                    axis: DVec3::Y,
                    limits: None,
                }
            }
            "fixed" => invalid.joints[index].kind = JointKind::Fixed,
            "stops" => {
                if let JointKind::Revolute { limits, .. } = &mut invalid.joints[index].kind {
                    *limits = Some([-0.01, 0.01]);
                }
            }
            _ => unreachable!(),
        }
        let (status, response) = json(
            &app,
            "PUT",
            &path,
            &headers,
            Some(serde_json::json!({"base":head.revision,"scene":invalid})),
        )
        .await;
        assert_eq!(
            status,
            StatusCode::UNPROCESSABLE_ENTITY,
            "{attack}: {response}"
        );
        assert!(
            response["error"].as_str().unwrap().contains("attachment"),
            "{response}"
        );
        assert_eq!(
            service
                .head(
                    &head.id,
                    &Principal {
                        id: Some("alice".into())
                    },
                    None
                )
                .await
                .unwrap()
                .revision,
            head.revision
        );
    }
}
