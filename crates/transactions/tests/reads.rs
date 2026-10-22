use ergo_transactions::{CommitOptions, Error, OctreePath, Repository};
use object_store::memory::InMemory;
use std::sync::Arc;
fn p(s: &str) -> OctreePath {
    s.parse().unwrap()
}

#[tokio::test]
async fn pagination_is_complete_ordered_and_protects_empty_predicates() {
    let repo = Repository::new(Arc::new(InMemory::new()), "pages")
        .unwrap()
        .with_cache_capacity(64 * 1024);
    repo.create("main").await.unwrap();
    let mut tx = repo.begin("main").await.unwrap();
    for i in 0..40u32 {
        tx.put(
            &OctreePath::new(vec![(i / 8) as u8, (i % 8) as u8]).unwrap(),
            &i,
        )
        .await
        .unwrap();
    }
    tx.remove(&p("1/1")).await.unwrap();
    let snapshot = tx
        .commit("a", "many", CommitOptions::default())
        .await
        .unwrap()
        .snapshot;
    let expected = snapshot.scan(&p("/")).await.unwrap();
    let mut leaves = Vec::new();
    let mut cursor = None;
    loop {
        let page = snapshot
            .scan_page(&p("/"), cursor.as_ref(), 3)
            .await
            .unwrap();
        assert!(page.leaves.len() <= 3);
        leaves.extend(page.leaves);
        cursor = page.next;
        if cursor.is_none() {
            break;
        }
    }
    assert_eq!(leaves, expected);
    assert!(matches!(
        snapshot.scan_page(&p("2"), Some(&p("3/1")), 1).await,
        Err(Error::InvalidCursor)
    ));
    assert!(snapshot.scan_page(&p("/"), None, 0).await.is_err());
    let mut observed = repo.begin("main").await.unwrap();
    observed.scan_page(&p("7"), None, 1).await.unwrap();
    observed.put(&p("5"), &5u32).await.unwrap();
    let mut phantom = repo.begin("main").await.unwrap();
    phantom.put(&p("7/1"), &71u32).await.unwrap();
    phantom
        .commit("b", "phantom", CommitOptions::default())
        .await
        .unwrap();
    assert!(matches!(
        observed
            .commit("a", "stale predicate", CommitOptions::default())
            .await,
        Err(Error::Conflict { .. })
    ));
}

#[tokio::test]
async fn shared_lru_obeys_byte_budget_and_never_caches_heads_or_crosses_namespaces() {
    let root = Repository::new(Arc::new(InMemory::new()), "cache")
        .unwrap()
        .with_cache_capacity(512);
    let a = root.child("a").unwrap();
    let b = root.child("b").unwrap();
    a.create("main").await.unwrap();
    b.create("main").await.unwrap();
    let id = a.put_blob(&vec![99u32; 100]).await.unwrap();
    a.blob::<Vec<u32>>(&id).await.unwrap();
    let hits = a.cache_stats().hits;
    a.blob::<Vec<u32>>(&id).await.unwrap();
    assert!(a.cache_stats().hits > hits);
    assert!(b.blob::<Vec<u32>>(&id).await.is_err());
    for i in 0..10u32 {
        let id = a.put_blob(&vec![i; 100]).await.unwrap();
        a.blob::<Vec<u32>>(&id).await.unwrap();
    }
    let stats = a.cache_stats();
    assert!(stats.resident_bytes <= 512 && stats.evictions > 0);
    let mut tx = a.begin("main").await.unwrap();
    tx.put(&p("1"), &1u32).await.unwrap();
    let accepted = tx
        .commit("a", "new head", CommitOptions::default())
        .await
        .unwrap();
    assert_eq!(
        a.checkout("main").await.unwrap().id(),
        accepted.snapshot.id()
    );
}

#[tokio::test]
async fn historical_base_and_explicit_refresh_preserve_old_preconditions_and_new_reads() {
    let repo = Repository::new(Arc::new(InMemory::new()), "rebase").unwrap();
    let original = repo.create("main").await.unwrap();
    let mut first = repo.begin("main").await.unwrap();
    first.put(&p("1"), &1u32).await.unwrap();
    first
        .commit("a", "first", CommitOptions::default())
        .await
        .unwrap();
    let mut stale = repo.begin_at("main", original.id()).await.unwrap();
    stale.put(&p("2"), &2u32).await.unwrap();
    stale.refresh().await.unwrap();
    assert_eq!(stale.get::<u32>(&p("1")).await.unwrap(), Some(1));
    let accepted = stale
        .commit("b", "independent old base", CommitOptions::default())
        .await
        .unwrap();
    assert_eq!(
        accepted.snapshot.get::<u32>(&p("1")).await.unwrap(),
        Some(1)
    );
    let mut collision = repo.begin_at("main", original.id()).await.unwrap();
    collision.put(&p("1"), &10u32).await.unwrap();
    assert!(matches!(
        collision.refresh().await,
        Err(Error::Conflict { .. })
    ));
    // The implicit commit refresh must be just as safe as explicit refresh.
    let mut implicit = repo.begin_at("main", original.id()).await.unwrap();
    implicit.put(&p("1"), &11u32).await.unwrap();
    assert!(matches!(
        implicit
            .commit("c", "stale write", CommitOptions::default())
            .await,
        Err(Error::Conflict { .. })
    ));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn independent_local_handles_race_through_the_same_filesystem_lock() {
    let dir = tempfile::tempdir().unwrap();
    let a = Repository::new(
        Arc::new(ergo_transactions::local::LocalCasStore::new(dir.path()).unwrap()),
        "local",
    )
    .unwrap();
    let b = Repository::new(
        Arc::new(ergo_transactions::local::LocalCasStore::new(dir.path()).unwrap()),
        "local",
    )
    .unwrap();
    a.create("main").await.unwrap();
    let mut left = a.begin("main").await.unwrap();
    let mut right = b.begin("main").await.unwrap();
    left.put(&p("1"), &1u32).await.unwrap();
    right.put(&p("2"), &2u32).await.unwrap();
    let (left, right) = tokio::join!(
        left.commit("a", "left", CommitOptions::default()),
        right.commit("b", "right", CommitOptions::default())
    );
    left.unwrap();
    right.unwrap();
    assert_eq!(
        a.checkout("main")
            .await
            .unwrap()
            .scan(&p("/"))
            .await
            .unwrap()
            .len(),
        2
    );
}

#[tokio::test]
async fn directory_presence_is_a_predicate_not_a_read_of_every_descendant() {
    let repo = Repository::new(Arc::new(InMemory::new()), "presence").unwrap();
    repo.create("main").await.unwrap();
    let mut seed = repo.begin("main").await.unwrap();
    seed.put(&p("1/1"), &1u32).await.unwrap();
    seed.commit("a", "seed", CommitOptions::default())
        .await
        .unwrap();
    let mut a = repo.begin("main").await.unwrap();
    assert!(a.directory_exists(&p("1")).await.unwrap());
    a.put(&p("2"), &2u32).await.unwrap();
    let mut b = repo.begin("main").await.unwrap();
    b.put(&p("1/2"), &12u32).await.unwrap();
    b.commit("b", "unrelated child", CommitOptions::default())
        .await
        .unwrap();
    a.commit("a", "presence still true", CommitOptions::default())
        .await
        .unwrap();
    let mut a = repo.begin("main").await.unwrap();
    assert!(!a.directory_exists(&p("3")).await.unwrap());
    a.put(&p("2"), &20u32).await.unwrap();
    let mut b = repo.begin("main").await.unwrap();
    b.put(&p("3/4"), &34u32).await.unwrap();
    b.commit("b", "phantom directory", CommitOptions::default())
        .await
        .unwrap();
    assert!(matches!(
        a.commit("a", "invalid presence", CommitOptions::default())
            .await,
        Err(Error::Conflict { .. })
    ));
}
