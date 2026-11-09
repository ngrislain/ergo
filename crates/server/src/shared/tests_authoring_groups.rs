use super::*;
use ergo_world::groups::{Group, Member};

fn nested() -> WorldExport {
    let mut source = scene(10.);
    source.groups = vec![
        Group {
            id: 7,
            name: "Child".into(),
            members: vec![Member::Piece { id: 2 }, Member::Piece { id: 1 }],
            pivot_world: glam::DVec3::ZERO,
        },
        Group {
            id: 500,
            name: "Root".into(),
            members: vec![Member::Group { id: 7 }],
            pivot_world: glam::DVec3::ZERO,
        },
    ];
    source
}

#[tokio::test]
async fn removing_an_empty_group_keeps_exports_and_group_discovery_readable() {
    let service = service();
    let mut source = nested();
    source.groups.push(Group {
        id: 501,
        name: "Temporary".into(),
        members: vec![],
        pivot_world: glam::DVec3::ZERO,
    });
    let (head, imported) = seeded(&service, source.clone()).await;
    let loaded = service
        .authoring_group(&head.id, "alice", &imported.assembly, 501, &head.revision)
        .await
        .unwrap();
    source.groups = serde_json::from_value(loaded["groups"].clone()).unwrap();
    let mut after = source.clone();
    after.groups.retain(|g| g.id != 501);
    let delta =
        ergo_world::authoring::Delta::between(&source, &after, &Default::default()).unwrap();
    let result = service
        .edit_assembly(
            &head.id,
            "alice",
            &imported.assembly,
            edits::EditRequest {
                base: head.revision,
                delta,
            },
        )
        .await
        .unwrap();
    let (app, headers) = http(&service);
    let root = format!(
        "/api/shared-worlds/{}/assemblies/{}",
        head.id, imported.assembly
    );
    let (status, export) = json(
        &app,
        "GET",
        &format!(
            "{root}/export?format=ergo&revision={}",
            result.accepted.revision
        ),
        &headers,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{export}");
    assert_eq!(export["groups"].as_array().unwrap().len(), 2);
    let (status, page) = json(
        &app,
        "GET",
        &format!(
            "{root}/workspace/groups?revision={}",
            result.accepted.revision
        ),
        &headers,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{page}");
    assert_eq!(page["groups"].as_array().unwrap().len(), 2);
}

#[tokio::test]
async fn empty_tombstoned_group_prefixes_are_absent_but_orphan_members_are_not_hidden() {
    let service = service();
    let mut source = nested();
    source.groups.push(Group {
        id: 501,
        name: "Empty".into(),
        members: vec![],
        pivot_world: glam::DVec3::ZERO,
    });
    let (head, imported) = seeded(&service, source).await;
    let repo = service.repo(&head.id).unwrap();
    let mut tx = repo.begin("main").await.unwrap();
    tx.remove(&paged::row(&imported.assembly, 5, 501).child(0).unwrap())
        .await
        .unwrap();
    let committed = tx
        .commit(
            "alice",
            "Record-level group tombstone fixture",
            Default::default(),
        )
        .await
        .unwrap();
    let revision = committed.snapshot.id().clone();
    committed.snapshot.close().await.unwrap();
    let (app, headers) = http(&service);
    let root = format!(
        "/api/shared-worlds/{}/assemblies/{}",
        head.id, imported.assembly
    );
    let (status, export) = json(
        &app,
        "GET",
        &format!("{root}/export?format=ergo&revision={revision}"),
        &headers,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{export}");
    assert_eq!(export["groups"].as_array().unwrap().len(), 2);
    let (status, page) = json(
        &app,
        "GET",
        &format!("{root}/workspace/groups?revision={revision}"),
        &headers,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{page}");
    assert_eq!(page["groups"].as_array().unwrap().len(), 2);
    let mut tx = repo.begin("main").await.unwrap();
    tx.remove(&paged::row(&imported.assembly, 5, 7).child(0).unwrap())
        .await
        .unwrap();
    let committed = tx
        .commit("alice", "Orphan group fixture", Default::default())
        .await
        .unwrap();
    let revision = committed.snapshot.id().clone();
    committed.snapshot.close().await.unwrap();
    let (status, error) = json(
        &app,
        "GET",
        &format!("{root}/export?format=ergo&revision={revision}"),
        &headers,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{error}");
    assert!(
        error["error"]
            .as_str()
            .unwrap()
            .contains("Missing group header")
    );
}

#[tokio::test]
async fn group_headers_are_paged_and_complete_child_loads_preserve_unloaded_parents() {
    let service = service();
    let source = nested();
    let (head, imported) = seeded(&service, source.clone()).await;
    let (app, headers) = http(&service);
    let root = format!(
        "/api/shared-worlds/{}/assemblies/{}/workspace",
        head.id, imported.assembly
    );
    let (_, manifest) = json(&app, "GET", &root, &headers, None).await;
    assert_eq!(manifest["next_group_id"], 501);
    let url = format!("{root}/groups?revision={}&limit=1", head.revision);
    let (status, page) = json(&app, "GET", &url, &headers, None).await;
    assert_eq!(status, StatusCode::OK, "{page}");
    assert_eq!(page["groups"][0]["id"], 7);
    assert_eq!(page["groups"][0]["members_loaded"], false);
    let encoded = url::form_urlencoded::Serializer::new(String::new())
        .append_pair("cursor", page["next"].as_str().unwrap())
        .finish();
    let (_, second) = json(&app, "GET", &format!("{url}&{encoded}"), &headers, None).await;
    assert_eq!(second["groups"][0]["id"], 500);
    let loaded = service
        .authoring_group(&head.id, "alice", &imported.assembly, 7, &head.revision)
        .await
        .unwrap();
    assert_eq!(loaded["group_complete"], true);
    assert_eq!(loaded["physical_domain_complete"], false);
    assert_eq!(loaded["groups"].as_array().unwrap().len(), 1);
    assert_eq!(loaded["piece_ids"].as_array().unwrap().len(), 2);
    let mut before = source.clone();
    before.groups = serde_json::from_value(loaded["groups"].clone()).unwrap();
    let mut after = before.clone();
    after.groups.clear();
    let delta =
        ergo_world::authoring::Delta::between(&before, &after, &Default::default()).unwrap();
    let result = service
        .edit_assembly(
            &head.id,
            "alice",
            &imported.assembly,
            edits::EditRequest {
                base: head.revision,
                delta,
            },
        )
        .await
        .unwrap();
    let parent = service
        .authoring_group(
            &head.id,
            "alice",
            &imported.assembly,
            500,
            &result.accepted.revision,
        )
        .await
        .unwrap();
    assert_eq!(parent["groups"].as_array().unwrap().len(), 1);
    assert_eq!(parent["groups"][0]["members"].as_array().unwrap().len(), 2);
    assert_eq!(parent["piece_ids"], serde_json::json!([1, 2]));
}

#[tokio::test]
async fn group_loading_discloses_and_fences_outside_explicit_constraints() {
    let service = service();
    let mut demo = ergo_world::demo::Demo::new();
    demo.add("3003", 4, 0., 0., 0.);
    demo.add("3003", 4, 0., 1., 0.);
    let mut source = scene(0.);
    source.bricks = demo.bricks;
    source.meshes = demo.meshes;
    source.joints = ergo_world::constraints::inferred_joints(&source.bricks);
    assert!(!source.joints.is_empty());
    source.groups = vec![Group {
        id: 10,
        name: "Foot only".into(),
        members: vec![Member::Piece { id: 1 }],
        pivot_world: glam::DVec3::ZERO,
    }];
    let (head, imported) = seeded(&service, source).await;
    let loaded = service
        .authoring_group(&head.id, "alice", &imported.assembly, 10, &head.revision)
        .await
        .unwrap();
    assert_eq!(loaded["blocked_transform_groups"], serde_json::json!([10]));
    assert!(loaded["joints"].as_array().unwrap().is_empty());
    assert_eq!(loaded["piece_ids"], serde_json::json!([1]));
    assert!(
        service
            .authoring_group(&head.id, "bob", &imported.assembly, 10, &head.revision)
            .await
            .is_err()
    );
}

#[tokio::test]
async fn oversized_group_closures_stop_without_partial_claims_or_publication() {
    let service = service();
    let mut source = scene(10.);
    source.groups = (1..=513)
        .map(|id| Group {
            id,
            name: format!("Empty {id}"),
            members: vec![],
            pivot_world: glam::DVec3::ZERO,
        })
        .collect();
    source.groups.push(Group {
        id: 1000,
        name: "Too many descendants".into(),
        members: (1..=513).map(|id| Member::Group { id }).collect(),
        pivot_world: glam::DVec3::ZERO,
    });
    let (head, imported) = seeded(&service, source).await;
    let error = service
        .authoring_group(&head.id, "alice", &imported.assembly, 1000, &head.revision)
        .await
        .unwrap_err();
    assert_eq!(error.status, StatusCode::PAYLOAD_TOO_LARGE);
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

#[tokio::test]
async fn read_only_region_grants_cannot_change_assembly_wide_metadata() {
    let service = service();
    let (head, imported) = seeded(&service, scene(10.)).await;
    let first = piece(&service, &head.id, &imported.pieces[&1]).await;
    let coord = (*first.replicas.iter().next().unwrap()).into();
    let base = service
        .grants(
            &head.id,
            "alice",
            GrantRequest {
                base: head.revision,
                grants: vec![Grant {
                    user: "bob".into(),
                    regions: vec![OctreePath::for_block(coord)],
                }],
                authenticated_editing: None,
            },
        )
        .await
        .unwrap();
    let mut before = scene(10.);
    let mut after = before.clone();
    after.name = "Forbidden rename".into();
    before.bricks.clear();
    after.bricks.clear();
    let delta =
        ergo_world::authoring::Delta::between(&before, &after, &Default::default()).unwrap();
    let error = service
        .edit_assembly(
            &head.id,
            "bob",
            &imported.assembly,
            edits::EditRequest {
                base: base.clone(),
                delta,
            },
        )
        .await
        .err()
        .unwrap();
    assert_eq!(error.status, StatusCode::FORBIDDEN);
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
        base
    );
}

#[tokio::test]
async fn numerical_inconclusive_errors_are_not_reported_as_published_or_unstable() {
    use axum::body::to_bytes;
    let error = SharedError::from(anyhow::Error::new(ergo_world::equilibrium::Inconclusive(
        "budget".into(),
    )));
    let response = error.into_response();
    assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
    let value: serde_json::Value =
        serde_json::from_slice(&to_bytes(response.into_body(), 4096).await.unwrap()).unwrap();
    assert_eq!(value["validation_state"], "inconclusive");
    assert_eq!(value["published"], false);
    assert!(value["candidate"].is_null());
}
