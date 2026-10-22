use ergo_transactions::{CommitOptions, Error, OctreePath, Repository};
use object_store::memory::InMemory;
use std::sync::Arc;

async fn repo() -> Repository {
    let repo = Repository::new(Arc::new(InMemory::new()), "managed")
        .unwrap()
        .with_coordinated_gc()
        .with_cache_capacity(1024 * 1024);
    repo.create("main").await.unwrap().close().await.unwrap();
    repo
}
async fn no_pins(repo: &Repository) {
    for _ in 0..256 {
        if repo.active_pins().await.unwrap().is_empty() {
            return;
        }
        tokio::task::yield_now().await;
    }
    panic!("pins did not close");
}

#[tokio::test]
async fn reader_clones_and_active_writers_exclude_collection() {
    let repo = repo().await;
    let reader = repo.checkout("main").await.unwrap();
    let clone = reader.clone();
    reader.close().await.unwrap();
    assert!(matches!(
        repo.maintenance().await,
        Err(Error::ActivePins { .. })
    ));
    clone.close().await.unwrap();
    let writer = repo.begin("main").await.unwrap();
    assert!(matches!(
        repo.maintenance().await,
        Err(Error::ActivePins { .. })
    ));
    drop(writer);
    no_pins(&repo).await;
    let maintenance = repo.maintenance().await.unwrap();
    assert!(matches!(
        repo.begin("main").await,
        Err(Error::MaintenanceBusy)
    ));
    assert!(matches!(
        repo.checkout("main").await,
        Err(Error::MaintenanceBusy)
    ));
    maintenance.finish().await.unwrap();
    repo.checkout("main").await.unwrap().close().await.unwrap();
}

#[tokio::test]
async fn collector_keeps_history_audit_and_attachments_but_deletes_orphans() {
    let repo = repo().await;
    let orphan = repo.put_blob(&"unused mesh").await.unwrap();
    no_pins(&repo).await;
    let mut tx = repo.begin("main").await.unwrap();
    let attachment = tx.stage_blob(&vec![1u32, 2, 3]).unwrap();
    let audit = tx.set_metadata(&"server certificate").unwrap();
    tx.put(&"2/1".parse().unwrap(), &attachment).await.unwrap();
    let first = tx
        .commit("a", "mesh", CommitOptions::default())
        .await
        .unwrap()
        .snapshot;
    let original = first.id().clone();
    first.close().await.unwrap();
    let mut tx = repo.begin("main").await.unwrap();
    tx.remove(&"2".parse().unwrap()).await.unwrap();
    tx.commit("a", "remove", CommitOptions::default())
        .await
        .unwrap()
        .snapshot
        .close()
        .await
        .unwrap();
    no_pins(&repo).await;
    let mut maintenance = repo.maintenance().await.unwrap();
    maintenance.compact("main").await.unwrap();
    let report = maintenance.collect(|_| Ok(Vec::new())).await.unwrap();
    assert!(report.deleted_objects > 0);
    maintenance.finish().await.unwrap();
    assert!(repo.blob::<String>(&orphan).await.is_err());
    assert_eq!(
        repo.blob::<Vec<u32>>(&attachment).await.unwrap(),
        vec![1, 2, 3]
    );
    assert_eq!(
        repo.blob::<String>(&audit).await.unwrap(),
        "server certificate"
    );
    let old = repo.snapshot(&original).await.unwrap();
    assert_eq!(
        old.get::<ergo_transactions::ContentId>(&"2/1".parse().unwrap())
            .await
            .unwrap(),
        Some(attachment)
    );
    old.close().await.unwrap();
    let current = repo.checkout("main").await.unwrap();
    assert!(current.scan(&OctreePath::root()).await.unwrap().is_empty());
    assert_eq!(current.commit().root, None);
    current.close().await.unwrap();
}

#[tokio::test]
async fn maintenance_cancellation_stays_fenced_until_explicit_operator_recovery() {
    let repo = repo().await;
    let maintenance = repo.maintenance().await.unwrap();
    let token = maintenance.token().to_owned();
    drop(maintenance);
    assert!(matches!(
        repo.begin("main").await,
        Err(Error::MaintenanceBusy)
    ));
    assert_eq!(
        repo.maintenance_status().await.unwrap().exclusive,
        Some(token.clone())
    );
    repo.recover_maintenance(&token).await.unwrap();
    repo.checkout("main").await.unwrap().close().await.unwrap();
}

#[tokio::test]
async fn cached_orphan_cannot_be_reused_after_collection() {
    let repo = repo().await;
    let orphan = repo.put_blob(&55u32).await.unwrap();
    assert_eq!(repo.blob::<u32>(&orphan).await.unwrap(), 55);
    no_pins(&repo).await;
    let mut maintenance = repo.maintenance().await.unwrap();
    maintenance.collect(|_| Ok(Vec::new())).await.unwrap();
    maintenance.finish().await.unwrap();
    let mut tx = repo.begin("main").await.unwrap();
    assert!(tx.retain_blob(&orphan).await.is_err());
}

#[tokio::test]
async fn compaction_is_region_bounded_and_does_not_change_other_geometry() {
    let repo = repo().await;
    let mut tx = repo.begin("main").await.unwrap();
    for q in 0..8u8 {
        tx.put(&OctreePath::new(vec![1, q]).unwrap(), &u32::from(q))
            .await
            .unwrap();
    }
    tx.put(&"7".parse().unwrap(), &7u32).await.unwrap();
    tx.remove(&"1/3".parse().unwrap()).await.unwrap();
    tx.commit("a", "subtrees", CommitOptions::default())
        .await
        .unwrap()
        .snapshot
        .close()
        .await
        .unwrap();
    no_pins(&repo).await;
    let mut maintenance = repo.maintenance().await.unwrap();
    assert!(matches!(
        maintenance
            .compact_region("main", &OctreePath::root(), 1)
            .await,
        Err(Error::CompactionLimit { .. })
    ));
    maintenance
        .compact_region("main", &"1/3".parse().unwrap(), 4)
        .await
        .unwrap();
    maintenance.finish().await.unwrap();
    let snapshot = repo.checkout("main").await.unwrap();
    assert_eq!(
        snapshot.get::<u32>(&"7".parse().unwrap()).await.unwrap(),
        Some(7)
    );
    assert_eq!(snapshot.scan(&"1".parse().unwrap()).await.unwrap().len(), 7);
    snapshot.close().await.unwrap();
}

#[tokio::test]
async fn failed_streaming_rewrite_never_publishes_an_intermediate_batch() {
    let repo = repo().await;
    let before = repo.checkout("main").await.unwrap();
    let id = before.id().clone();
    before.close().await.unwrap();
    let mut maintenance = repo.maintenance().await.unwrap();
    let result = maintenance
        .rewrite("main", |_| {
            futures::stream::iter((0..256u32).map(|i| {
                if i == 200 {
                    return Err(Error::Encoding("injected migration failure".into()));
                }
                ergo_transactions::RecordEdit::put(
                    OctreePath::new(vec![(i / 64) as u8, ((i / 8) % 8) as u8, (i % 8) as u8])
                        .unwrap(),
                    &i,
                )
            }))
        })
        .await;
    assert!(result.is_err());
    maintenance.finish().await.unwrap();
    let current = repo.checkout("main").await.unwrap();
    assert_eq!(current.id(), &id);
    assert!(
        current
            .scan_page(&OctreePath::root(), None, 1)
            .await
            .unwrap()
            .leaves
            .is_empty()
    );
    current.close().await.unwrap();
}
