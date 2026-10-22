use ergo_transactions::{CommitOptions, Error, Repository};
use object_store::memory::InMemory;
use std::sync::Arc;

#[tokio::test]
async fn indexed_acceptance_pruning_and_gc_preserve_current_state() {
    let repo = Repository::new(Arc::new(InMemory::new()), "history")
        .unwrap()
        .with_coordinated_gc();
    let initial = repo.create("main").await.unwrap();
    let initial_id = initial.id().clone();
    initial.close().await.unwrap();
    let mut ids = Vec::new();
    for value in 0..8u32 {
        let mut tx = repo.begin("main").await.unwrap();
        tx.put(&"1".parse().unwrap(), &value).await.unwrap();
        let snapshot = tx
            .commit("a", "edit", CommitOptions::default())
            .await
            .unwrap()
            .snapshot;
        ids.push(snapshot.id().clone());
        snapshot.close().await.unwrap();
    }
    assert!(repo.was_committed("main", &initial_id).await.unwrap());
    let page = repo.revisions("main", None, 3).await.unwrap();
    assert_eq!(
        page.revisions,
        ids[5..].iter().rev().cloned().collect::<Vec<_>>()
    );
    for _ in 0..256 {
        if repo.active_pins().await.unwrap().is_empty() {
            break;
        }
        tokio::task::yield_now().await;
    }
    let mut maintenance = repo.maintenance().await.unwrap();
    let report = maintenance.prune_history("main", 3).await.unwrap();
    assert_eq!(report.removed_revisions, 6);
    maintenance.collect(|_| Ok(Vec::new())).await.unwrap();
    maintenance.finish().await.unwrap();
    assert!(!repo.was_committed("main", &initial_id).await.unwrap());
    assert!(matches!(
        repo.accepted_snapshot("main", &ids[0]).await,
        Err(Error::RevisionUnavailable)
    ));
    let latest = repo.checkout("main").await.unwrap();
    assert_eq!(
        latest.get::<u32>(&"1".parse().unwrap()).await.unwrap(),
        Some(7)
    );
    latest.close().await.unwrap();
    assert_eq!(
        repo.revisions("main", None, 100)
            .await
            .unwrap()
            .revisions
            .len(),
        3
    );
    let mut tx = repo.begin("main").await.unwrap();
    tx.put(&"2".parse().unwrap(), &2u32).await.unwrap();
    tx.commit("a", "after pruning", CommitOptions::default())
        .await
        .unwrap()
        .snapshot
        .close()
        .await
        .unwrap();
}
