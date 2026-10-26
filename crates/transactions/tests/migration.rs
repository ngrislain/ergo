use ergo_transactions::{CommitOptions, OctreePath, Repository};
use object_store::ObjectStore;
use std::sync::Arc;

#[tokio::test]
async fn current_history_install_is_fenced_validated_and_preserves_state() {
    let store = Arc::new(object_store::memory::InMemory::new());
    let repo = Repository::new(store, "world")
        .unwrap()
        .with_coordinated_gc();
    let initial = repo.create("main").await.unwrap();
    let first = initial.id().clone();
    initial.close().await.unwrap();
    let mut tx = repo.begin("main").await.unwrap();
    tx.put(&"1/2".parse().unwrap(), &42u32).await.unwrap();
    let old = tx
        .commit("alice", "shape", CommitOptions::default())
        .await
        .unwrap()
        .snapshot;
    let expected = old.id().clone();
    let mut converted = old.commit().clone();
    old.close().await.unwrap();
    converted.transaction_id = "format-migration".into();
    let current = repo.put_blob(&converted).await.unwrap();
    for _ in 0..100 {
        if repo.active_pins().await.unwrap().is_empty() {
            break;
        }
        tokio::task::yield_now().await;
    }
    let mut maintenance = repo.maintenance().await.unwrap();
    assert!(
        maintenance
            .replace_history("main", &first, &[first.clone(), current.clone()])
            .await
            .is_err()
    );
    assert!(
        maintenance
            .replace_history("main", &expected, &[current.clone(), first.clone()])
            .await
            .is_err()
    );
    maintenance
        .replace_history("main", &expected, &[first.clone(), current.clone()])
        .await
        .unwrap();
    assert!(
        repo.checkout("main").await.is_err(),
        "Install must retain the barrier until explicitly finished"
    );
    maintenance.finish().await.unwrap();
    let snapshot = repo.checkout("main").await.unwrap();
    assert_eq!(snapshot.id(), &current);
    assert_eq!(
        snapshot.get::<u32>(&"1/2".parse().unwrap()).await.unwrap(),
        Some(42)
    );
    assert_eq!(snapshot.commit().actor, "alice");
    snapshot.close().await.unwrap();
    assert!(repo.accepted_snapshot("main", &expected).await.is_err());
    repo.accepted_snapshot("main", &first)
        .await
        .unwrap()
        .close()
        .await
        .unwrap();
}

#[tokio::test]
async fn fenced_prefix_migration_preserves_accepted_history_and_keeps_source_closed() {
    let store = Arc::new(object_store::memory::InMemory::new());
    let source = Repository::new(store.clone(), "old")
        .unwrap()
        .with_coordinated_gc();
    source.create("main").await.unwrap().close().await.unwrap();
    let mut tx = source.begin("main").await.unwrap();
    tx.put(&"1/2".parse::<OctreePath>().unwrap(), &vec![1, 2, 3])
        .await
        .unwrap();
    let accepted = tx
        .commit("alice", "shape", CommitOptions::default())
        .await
        .unwrap();
    let revision = accepted.snapshot.id().clone();
    accepted.snapshot.close().await.unwrap();
    let target = Repository::new(store, "world")
        .unwrap()
        .with_coordinated_gc();
    let mut maintenance = source.maintenance().await.unwrap();
    let report = maintenance.migrate_to(&target, 1024 * 1024).await.unwrap();
    assert!(report.objects > 0);
    assert_eq!(report.branches, 1);
    assert!(source.checkout("main").await.is_err());
    let snapshot = target.checkout("main").await.unwrap();
    assert_eq!(snapshot.id(), &revision);
    assert_eq!(
        snapshot
            .get::<Vec<i32>>(&"1/2".parse().unwrap())
            .await
            .unwrap(),
        Some(vec![1, 2, 3])
    );
    snapshot.close().await.unwrap();
    maintenance.finish().await.unwrap();
    source
        .checkout("main")
        .await
        .unwrap()
        .close()
        .await
        .unwrap();
}

#[tokio::test]
async fn corrupt_source_never_publishes_a_target_head_and_keeps_both_fenced() {
    let store = Arc::new(object_store::memory::InMemory::new());
    let source = Repository::new(store.clone(), "old")
        .unwrap()
        .with_coordinated_gc();
    source.create("main").await.unwrap().close().await.unwrap();
    store
        .put(
            &object_store::path::Path::from(format!("old/objects/{}.cbor", "0".repeat(64))),
            bytes::Bytes::from_static(b"corrupt").into(),
        )
        .await
        .unwrap();
    let target = Repository::new(store.clone(), "world")
        .unwrap()
        .with_coordinated_gc();
    let mut maintenance = source.maintenance().await.unwrap();
    assert!(maintenance.migrate_to(&target, 1024 * 1024).await.is_err());
    assert!(source.checkout("main").await.is_err());
    assert!(target.checkout("main").await.is_err());
    assert!(
        store
            .get(&object_store::path::Path::from("world/refs/main.cbor"))
            .await
            .is_err()
    );
}
