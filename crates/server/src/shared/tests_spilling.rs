use super::*;
use ergo_world::groups::{Group, Member};

#[tokio::test]
async fn spilled_piece_cache_preserves_unique_charging_and_cached_absence() {
    let service = service();
    let (head, imported) = seeded(&service, scene(10.)).await;
    let mut working = service
        .working(&head.id, "alice", &head.revision)
        .await
        .unwrap();
    let id = &imported.pieces[&1];
    let first = working.piece(id).await.unwrap().unwrap();
    let charged = working.loaded_bytes;
    assert_eq!(working.piece(id).await.unwrap().unwrap().id, first.id);
    assert_eq!(working.loaded_bytes, charged);
    assert_eq!(working.pieces.len(), 1);
    let absent = ContentId::of(b"absent-piece");
    assert!(working.piece(&absent).await.unwrap().is_none());
    let charged = working.loaded_bytes;
    assert!(working.piece(&absent).await.unwrap().is_none());
    assert_eq!(working.loaded_bytes, charged);
    assert_eq!(working.pieces.len(), 2);
    working.tx.close().await.unwrap();
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
async fn spilled_dependency_graph_matches_paged_rows_and_replacing_it_removes_stale_edges() {
    let service = service();
    let mut source = scene(10.);
    source.groups = vec![Group {
        id: 7,
        name: "Distant authoring group".into(),
        members: vec![Member::Piece { id: 1 }, Member::Piece { id: 2 }],
        pivot_world: glam::DVec3::ZERO,
    }];
    let (head, imported) = seeded(&service, source).await;
    let seeds = BTreeSet::from([imported.pieces[&1].clone()]);
    let expected = imported.pieces.values().cloned().collect::<BTreeSet<_>>();
    let mut paged = service
        .working(&head.id, "alice", &head.revision)
        .await
        .unwrap();
    assert_eq!(paged.discover(&seeds).await.unwrap(), expected);
    paged.tx.close().await.unwrap();
    let mut indexed = service
        .working(&head.id, "alice", &head.revision)
        .await
        .unwrap();
    let rules = indexed.rules(&imported.assembly).await.unwrap();
    assert_eq!(indexed.discover(&seeds).await.unwrap(), expected);
    let mut next = rules.as_ref().clone();
    next.groups.clear();
    indexed.stage_rules(next).unwrap();
    indexed.reset_dependencies().unwrap();
    assert_eq!(indexed.discover(&seeds).await.unwrap(), seeds);
    assert!(indexed.tx.spill_database_bytes().unwrap() > 0);
    indexed.tx.close().await.unwrap();
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
async fn shared_scratch_exhaustion_is_inconclusive_and_does_not_publish_partial_rules() {
    let mut service = service();
    let before = scene(10.);
    let (head, imported) = seeded(&service, before.clone()).await;
    service.root = service
        .root
        .clone()
        .with_spill_limits(ergo_transactions::spill::SpillLimits {
            database_bytes: 64 * 1024,
            cache_kib: 4,
        })
        .unwrap();
    let mut after = before.clone();
    after.groups = (1..=128)
        .map(|id| Group {
            id,
            name: format!("Group {id}"),
            members: vec![],
            pivot_world: glam::DVec3::ZERO,
        })
        .collect();
    let delta =
        ergo_world::authoring::Delta::between(&before, &after, &Default::default()).unwrap();
    let error = service
        .edit_assembly(
            &head.id,
            "alice",
            &imported.assembly,
            edits::EditRequest {
                base: head.revision.clone(),
                delta,
            },
        )
        .await
        .err()
        .expect("a tiny disk budget must stop a large rule edit");
    assert_eq!(error.status, StatusCode::UNPROCESSABLE_ENTITY);
    assert!(error.inconclusive, "{error}");
    assert!(error.candidate.is_none());
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
