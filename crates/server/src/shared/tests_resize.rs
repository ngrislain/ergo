use super::*;
use std::collections::BTreeSet;

#[tokio::test]
async fn old_blocks_migrate_atomically_without_changing_poses_meshes_ids_or_permissions() {
    let service = service();
    let mut input = scene(255.);
    input.bricks.truncate(1);
    let (head, imported) = seeded(&service, input).await;
    let id = &imported.pieces[&1];
    let repo = service.repo(&head.id).unwrap();
    let before = piece(&service, &head.id, id).await;
    let mesh: ergo_world::geometry::Mesh = repo.blob(&before.mesh).await.unwrap();
    let brick = before.geometry.brick(1, &mesh).unwrap();
    let old_coord: ergo_world::BlockCoord = [0, -1, 0].into();
    let mut tx = repo.begin("main").await.unwrap();
    let mut metadata: serde_json::Value = tx.get(&root(2)).await.unwrap().unwrap();
    metadata["layout"] = serde_json::json!(3);
    metadata.as_object_mut().unwrap().remove("cell_size");
    tx.put(&root(2), &metadata).await.unwrap();
    let mut policy: Policy = tx.get(&root(0)).await.unwrap().unwrap();
    policy.grants.push(Grant {
        user: "bob".into(),
        regions: vec![OctreePath::for_block(old_coord.into())],
    });
    tx.put(&root(0), &policy).await.unwrap();
    tx.remove(&root(1)).await.unwrap();
    tx.remove(&root(5)).await.unwrap();
    tx.remove(&OctreePath::new(vec![3, 2]).unwrap())
        .await
        .unwrap();
    let mut legacy_piece = before.clone();
    legacy_piece.replicas = BTreeSet::from([old_coord]);
    tx.put(&entity_path(3, id), &legacy_piece).await.unwrap();
    tx.put(
        &spatial::directory(old_coord),
        &spatial::BlockHeader {
            coord: old_coord,
            paged: true,
            packed: false,
        },
    )
    .await
    .unwrap();
    let path = paged::append(
        &block_path(old_coord).child(0).unwrap(),
        &entity_path(0, id).octants()[1..],
    );
    tx.put(
        &path,
        &spatial::SpatialRef {
            coord: old_coord,
            piece: id.clone(),
            bounds: brick.render_bounds,
        },
    )
    .await
    .unwrap();
    let old = tx
        .commit(
            "fixture",
            "Synthetic old-cell snapshot",
            CommitOptions::default(),
        )
        .await
        .unwrap();
    let old_revision = old.snapshot.id().clone();
    old.snapshot.close().await.unwrap();
    assert!(
        service
            .head(
                &head.id,
                &Principal {
                    id: Some("alice".into())
                },
                None
            )
            .await
            .is_err(),
        "Old layouts must not run in compatibility mode"
    );
    let report = service.resize_cells(&head.id).await.unwrap();
    assert_eq!(report["cell_size"], 128);
    assert_eq!(report["height_plates"], 384);
    let current = service
        .head(
            &head.id,
            &Principal {
                id: Some("alice".into()),
            },
            None,
        )
        .await
        .unwrap();
    assert_eq!(current.metadata.layout, 4);
    let after = piece(&service, &head.id, id).await;
    assert_eq!(after.id, before.id);
    assert_eq!(after.mesh, before.mesh);
    assert_eq!(
        canonical_cbor(&after.geometry).unwrap(),
        canonical_cbor(&before.geometry).unwrap()
    );
    assert_eq!(
        after.replicas,
        BTreeSet::from([[1, -1, 0].into(), [2, -1, 0].into()])
    );
    assert!(current.policy.can_write("bob", [7, -1, 7].into()));
    assert!(!current.policy.can_write("bob", [8, -1, 7].into()));
    let snapshot = repo.checkout("main").await.unwrap();
    assert!(
        snapshot
            .get::<spatial::BlockHeader>(&spatial::directory(old_coord))
            .await
            .unwrap()
            .is_none()
    );
    for coord in &after.replicas {
        assert!(
            snapshot
                .get::<spatial::BlockHeader>(&spatial::directory(*coord))
                .await
                .unwrap()
                .is_some()
        );
    }
    snapshot.close().await.unwrap();
    let historical = repo.snapshot(&old_revision).await.unwrap();
    let retained: Piece = historical.get(&entity_path(3, id)).await.unwrap().unwrap();
    assert_eq!(retained.replicas, legacy_piece.replicas);
    historical.close().await.unwrap();
    assert_eq!(
        service.resize_cells(&head.id).await.unwrap()["already_current"],
        true
    );
}
