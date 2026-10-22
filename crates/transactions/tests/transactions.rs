use ciborium::value::Value;
use ergo_transactions::{CommitOptions, ContentId, Error, OctreePath, Repository, canonical_cbor};
use object_store::{ObjectStore, memory::InMemory, path::Path};
use serde::{Deserialize, Serialize};
use std::{collections::HashMap, sync::Arc, time::Duration};

fn path(s: &str) -> OctreePath {
    s.parse().unwrap()
}
fn options() -> CommitOptions {
    CommitOptions {
        max_attempts: 128,
        initial_backoff: Duration::ZERO,
        max_backoff: Duration::ZERO,
        ..Default::default()
    }
}
async fn repository() -> Repository {
    let repo = Repository::new(Arc::new(InMemory::new()), "test").unwrap();
    repo.create("main").await.unwrap();
    repo
}
async fn put(repo: &Repository, p: &str, value: u32) {
    let mut tx = repo.begin("main").await.unwrap();
    tx.put(&path(p), &value).await.unwrap();
    tx.commit("test", "put", options()).await.unwrap();
}

#[tokio::test]
async fn disjoint_siblings_rebase_and_old_snapshots_stay_readable() {
    let repo = repository().await;
    let original = repo.checkout("main").await.unwrap();
    let mut a = repo.begin("main").await.unwrap();
    let mut b = repo.begin("main").await.unwrap();
    a.put(&path("2/1"), &11u32).await.unwrap();
    b.put(&path("2/3"), &33u32).await.unwrap();
    assert_eq!(b.get::<u32>(&path("2/3")).await.unwrap(), Some(33));
    let first = a.commit("a", "left", options()).await.unwrap();
    let second = b.commit("b", "right", options()).await.unwrap();
    assert_eq!(second.rebases, 1);
    assert_eq!(second.attempts, 2);
    assert_eq!(
        second.snapshot.commit().parent.as_ref(),
        Some(first.snapshot.id())
    );
    assert_eq!(second.snapshot.commit().base.as_ref(), Some(original.id()));
    assert_eq!(
        second.snapshot.get::<u32>(&path("2/1")).await.unwrap(),
        Some(11)
    );
    assert_eq!(
        second.snapshot.get::<u32>(&path("2/3")).await.unwrap(),
        Some(33)
    );
    assert!(original.scan(&path("/")).await.unwrap().is_empty());
    assert_eq!(first.snapshot.get::<u32>(&path("2/3")).await.unwrap(), None);
    assert_eq!(
        repo.snapshot(first.snapshot.id())
            .await
            .unwrap()
            .get::<u32>(&path("2/1"))
            .await
            .unwrap(),
        Some(11)
    );
}

#[tokio::test]
async fn independent_substitutions_commute_in_both_commit_orders() {
    for reverse in [false, true] {
        let repo = repository().await;
        put(&repo, "3/1", 1).await;
        put(&repo, "3/2", 2).await;
        let mut deletion = repo.begin("main").await.unwrap();
        let mut replacement = repo.begin("main").await.unwrap();
        deletion.remove(&path("3/1")).await.unwrap();
        replacement.put(&path("3/2"), &20u32).await.unwrap();
        let (first, second) = if reverse {
            (replacement, deletion)
        } else {
            (deletion, replacement)
        };
        first.commit("a", "first", options()).await.unwrap();
        second.commit("b", "second", options()).await.unwrap();
        let snapshot = repo.checkout("main").await.unwrap();
        assert_eq!(snapshot.get::<u32>(&path("3/1")).await.unwrap(), None);
        assert_eq!(snapshot.get::<u32>(&path("3/2")).await.unwrap(), Some(20));
    }
}

#[tokio::test]
async fn same_leaf_conflicts_even_when_payloads_are_identical() {
    let repo = repository().await;
    let mut a = repo.begin("main").await.unwrap();
    let mut b = repo.begin("main").await.unwrap();
    let a_id = a.put(&path("1/2"), &7u32).await.unwrap();
    let b_id = b.put(&path("1/2"), &7u32).await.unwrap();
    assert_eq!(a_id, b_id); // geometry deduplicates; edit generations do not
    a.commit("a", "first", options()).await.unwrap();
    assert!(
        matches!(b.commit("b", "stale", options()).await, Err(Error::Conflict { paths }) if paths == vec![path("1/2")])
    );
}

#[tokio::test]
async fn ancestor_deletion_and_previously_absent_descendant_conflict_in_both_orders() {
    for reverse in [false, true] {
        let repo = repository().await;
        put(&repo, "2/4", 4).await;
        let mut ancestor = repo.begin("main").await.unwrap();
        let mut descendant = repo.begin("main").await.unwrap();
        ancestor.remove(&path("2")).await.unwrap();
        descendant.put(&path("2/5"), &5u32).await.unwrap();
        let (first, second) = if reverse {
            (descendant, ancestor)
        } else {
            (ancestor, descendant)
        };
        first.commit("a", "first", options()).await.unwrap();
        assert!(matches!(
            second.commit("b", "overlap", options()).await,
            Err(Error::Conflict { .. })
        ));
    }
    // Even deleting an already-empty ancestor establishes a replacement fence.
    let repo = repository().await;
    let mut child = repo.begin("main").await.unwrap();
    child.put(&path("6/1"), &1u32).await.unwrap();
    let mut clear = repo.begin("main").await.unwrap();
    clear.remove(&path("6")).await.unwrap();
    clear
        .commit("a", "clear empty region", options())
        .await
        .unwrap();
    assert!(matches!(
        child.commit("b", "stale insert", options()).await,
        Err(Error::Conflict { .. })
    ));
}

#[tokio::test]
async fn tombstone_fences_survive_subdivision_but_do_not_block_new_siblings() {
    let repo = repository().await;
    let mut clear = repo.begin("main").await.unwrap();
    clear.remove(&path("2")).await.unwrap();
    clear.commit("a", "clear", options()).await.unwrap();
    let mut a = repo.begin("main").await.unwrap();
    let mut b = repo.begin("main").await.unwrap();
    a.put(&path("2/1"), &1u32).await.unwrap();
    b.put(&path("2/3"), &3u32).await.unwrap();
    a.commit("a", "subdivide", options()).await.unwrap();
    b.commit("b", "sibling", options()).await.unwrap();
    assert_eq!(
        repo.checkout("main")
            .await
            .unwrap()
            .scan(&path("2"))
            .await
            .unwrap()
            .len(),
        2
    );
}

#[tokio::test]
async fn restored_geometry_does_not_hide_an_intervening_dependency_edit() {
    let repo = repository().await;
    put(&repo, "2/1", 10).await;
    let mut stale = repo.begin("main").await.unwrap();
    assert_eq!(stale.get::<u32>(&path("2/1")).await.unwrap(), Some(10));
    stale.put(&path("7"), &7u32).await.unwrap();
    put(&repo, "2/1", 20).await;
    put(&repo, "2/1", 10).await;
    assert!(
        matches!(stale.commit("a", "ABA dependency", options()).await, Err(Error::Conflict { paths }) if paths == vec![path("2/1")])
    );
}

#[tokio::test]
async fn predicate_reads_detect_phantoms_and_prevent_cross_branch_write_skew() {
    let repo = repository().await;
    let mut predicate = repo.begin("main").await.unwrap();
    assert!(predicate.scan(&path("4")).await.unwrap().is_empty());
    predicate.put(&path("1"), &10u32).await.unwrap();
    put(&repo, "4/7", 47).await;
    assert!(
        matches!(predicate.commit("a", "predicate", options()).await, Err(Error::Conflict { paths }) if paths.contains(&path("4")))
    );
    assert_eq!(
        repo.checkout("main")
            .await
            .unwrap()
            .get::<u32>(&path("1"))
            .await
            .unwrap(),
        None
    );

    put(&repo, "0", 1).await;
    put(&repo, "7", 1).await;
    let mut a = repo.begin("main").await.unwrap();
    let mut b = repo.begin("main").await.unwrap();
    assert_eq!(a.get::<u32>(&path("7")).await.unwrap(), Some(1));
    assert_eq!(b.get::<u32>(&path("0")).await.unwrap(), Some(1));
    a.put(&path("0"), &0u32).await.unwrap();
    b.put(&path("7"), &0u32).await.unwrap();
    a.commit("a", "preserve at least one support", options())
        .await
        .unwrap();
    assert!(matches!(
        b.commit("b", "would remove last support", options()).await,
        Err(Error::Conflict { .. })
    ));
}

#[tokio::test]
async fn watched_support_region_invalidates_an_otherwise_disjoint_edit() {
    let repo = repository().await;
    let mut tx = repo.begin("main").await.unwrap();
    tx.watch(&path("5/2")).await.unwrap();
    tx.put(&path("1/3"), &13u32).await.unwrap();
    put(&repo, "5/2/0", 520).await;
    assert!(
        matches!(tx.commit("a", "support changed", options()).await, Err(Error::Conflict { paths }) if paths == vec![path("5/2")])
    );
}

#[tokio::test]
async fn multi_block_moves_publish_atomically_and_deleted_blocks_remain_in_history() {
    let repo = repository().await;
    put(&repo, "1/1", 42).await;
    let old = repo.checkout("main").await.unwrap();
    let mut tx = repo.begin("main").await.unwrap();
    let value: u32 = tx.get(&path("1/1")).await.unwrap().unwrap();
    tx.remove(&path("1/1")).await.unwrap();
    tx.put(&path("6/7"), &value).await.unwrap();
    assert_eq!(tx.get::<u32>(&path("1/1")).await.unwrap(), None);
    assert_eq!(old.get::<u32>(&path("6/7")).await.unwrap(), None);
    let moved = tx.commit("a", "move", options()).await.unwrap().snapshot;
    assert_eq!(moved.get::<u32>(&path("1/1")).await.unwrap(), None);
    assert_eq!(moved.get::<u32>(&path("6/7")).await.unwrap(), Some(42));
    assert_eq!(old.get::<u32>(&path("1/1")).await.unwrap(), Some(42));
}

#[tokio::test]
async fn ordered_edits_can_subdivide_and_every_history_payload_exists() {
    let repo = repository().await;
    put(&repo, "2", 2).await;
    let mut tx = repo.begin("main").await.unwrap();
    assert!(matches!(
        tx.put(&path("2/1"), &1u32).await,
        Err(Error::CoveredByLeaf(_))
    ));
    tx.remove(&path("2")).await.unwrap();
    let superseded = tx.put(&path("2/1"), &10u32).await.unwrap();
    tx.put(&path("2/1"), &11u32).await.unwrap();
    tx.put(&path("2/7"), &27u32).await.unwrap();
    let snapshot = tx
        .commit("a", "subdivide", options())
        .await
        .unwrap()
        .snapshot;
    assert_eq!(snapshot.get::<u32>(&path("2/1")).await.unwrap(), Some(11));
    assert_eq!(repo.blob::<u32>(&superseded).await.unwrap(), 10);
    assert!(matches!(
        snapshot.get::<u32>(&path("2")).await,
        Err(Error::NotLeaf(_))
    ));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn sixty_four_concurrent_transactions_preserve_all_independent_blocks() {
    let repo = repository().await;
    let barrier = Arc::new(tokio::sync::Barrier::new(64));
    let mut tasks = Vec::new();
    for i in 0..64u32 {
        let repo = repo.clone();
        let barrier = barrier.clone();
        tasks.push(tokio::spawn(async move {
            let mut tx = repo.begin("main").await.unwrap();
            let p = OctreePath::new(vec![(i / 8) as u8, (i % 8) as u8]).unwrap();
            tx.put(&p, &i).await.unwrap();
            barrier.wait().await;
            tx.commit("worker", "independent block", options())
                .await
                .unwrap()
        }));
    }
    let mut rebases = 0;
    for task in tasks {
        rebases += task.await.unwrap().rebases;
    }
    assert!(rebases > 0);
    let snapshot = repo.checkout("main").await.unwrap();
    assert_eq!(snapshot.commit().sequence, 64);
    assert_eq!(snapshot.scan(&path("/")).await.unwrap().len(), 64);
    for i in 0..64u32 {
        assert_eq!(
            snapshot
                .get::<u32>(&OctreePath::new(vec![(i / 8) as u8, (i % 8) as u8]).unwrap())
                .await
                .unwrap(),
            Some(i)
        );
    }
}

#[tokio::test]
async fn shard_heads_are_independent_and_initialization_never_overwrites() {
    let repo = repository().await;
    assert!(matches!(
        repo.create("main").await,
        Err(Error::BranchExists(_))
    ));
    repo.create("other-shard").await.unwrap();
    let mut a = repo.begin("main").await.unwrap();
    let mut b = repo.begin("other-shard").await.unwrap();
    a.put(&path("1"), &1u32).await.unwrap();
    b.put(&path("1"), &2u32).await.unwrap();
    assert_eq!(
        a.commit("a", "shard a", options()).await.unwrap().rebases,
        0
    );
    assert_eq!(
        b.commit("b", "shard b", options()).await.unwrap().rebases,
        0
    );
}

#[tokio::test]
async fn contention_is_bounded_and_read_only_transactions_do_not_publish() {
    let repo = repository().await;
    let mut tx = repo.begin("main").await.unwrap();
    tx.put(&path("1"), &1u32).await.unwrap();
    put(&repo, "2", 2).await;
    let bounded = CommitOptions {
        max_attempts: 1,
        ..options()
    };
    assert!(matches!(
        tx.commit("a", "bounded", bounded).await,
        Err(Error::Contention { attempts: 1 })
    ));
    let mut read = repo.begin("main").await.unwrap();
    assert_eq!(read.get::<u32>(&path("2")).await.unwrap(), Some(2));
    let old_id = read.base_id().clone();
    put(&repo, "3", 3).await;
    let outcome = read.commit("a", "read-only", options()).await.unwrap();
    assert_eq!(outcome.attempts, 0);
    assert_eq!(outcome.snapshot.id(), &old_id);
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
struct Geometry {
    positions: Vec<[f32; 3]>,
    triangles: Vec<[u32; 3]>,
}

#[tokio::test]
async fn cbor_geometry_roundtrips_and_downloads_verify_content_hashes() {
    let backend = Arc::new(InMemory::new());
    let repo = Repository::new(backend.clone(), "test").unwrap();
    repo.create("main").await.unwrap();
    let geometry = Geometry {
        positions: vec![[0., 0., 0.], [1., 0., 0.], [0., 1., 0.]],
        triangles: vec![[0, 1, 2]],
    };
    let mut tx = repo.begin("main").await.unwrap();
    let id = tx.put(&path("0"), &geometry).await.unwrap();
    let snapshot = tx
        .commit("a", "triangle", options())
        .await
        .unwrap()
        .snapshot;
    assert_eq!(
        snapshot.get::<Geometry>(&path("0")).await.unwrap(),
        Some(geometry)
    );
    backend
        .put(
            &Path::from(format!("test/objects/{id}.cbor")),
            "corrupt".into(),
        )
        .await
        .unwrap();
    assert!(matches!(
        snapshot.get::<Geometry>(&path("0")).await,
        Err(Error::Corrupt(_))
    ));
}

#[test]
fn deterministic_cbor_uses_core_key_order_and_rejects_ambiguous_values() {
    let mut a = HashMap::new();
    a.insert("z", 1);
    a.insert("a", 2);
    let mut b = HashMap::new();
    b.insert("a", 2);
    b.insert("z", 1);
    assert_eq!(canonical_cbor(&a).unwrap(), canonical_cbor(&b).unwrap());
    assert_eq!(
        canonical_cbor(&-0.0f64).unwrap(),
        canonical_cbor(&0.0f64).unwrap()
    );
    assert_eq!(hex::encode(canonical_cbor(&1.5f64).unwrap()), "f93e00");
    assert!(canonical_cbor(&f64::NAN).is_err());
    let duplicate = Value::Map(vec![
        (Value::Text("a".into()), Value::Null),
        (Value::Text("a".into()), Value::Null),
    ]);
    assert!(canonical_cbor(&duplicate).is_err());
    // Integer 1000 sorts before empty text by encoded bytes, despite taking more bytes.
    let mixed = Value::Map(vec![
        (Value::Text("".into()), Value::Null),
        (Value::Integer(1000.into()), Value::Null),
    ]);
    assert_eq!(
        hex::encode(canonical_cbor(&mixed).unwrap()),
        "a21903e8f660f6"
    );
}

#[test]
fn paths_validate_hierarchy_and_map_signed_block_coordinates() {
    assert!(path("2/4").is_prefix_of(&path("2/4/1")));
    assert!(path("2/4").is_disjoint_from(&path("2/5")));
    assert!(!path("/").is_disjoint_from(&path("2/5")));
    for invalid in ["8", "1//2", "01", "../0", "1/", "/1"] {
        assert!(invalid.parse::<OctreePath>().is_err());
    }
    assert!(OctreePath::new(vec![0; ergo_transactions::MAX_DEPTH + 1]).is_err());
    let a = OctreePath::for_block([i64::MIN, 0, i64::MAX]);
    let b = OctreePath::for_block([i64::MIN + 1, 0, i64::MAX]);
    assert_eq!(a.octants().len(), 64);
    assert!(a.is_disjoint_from(&b));
    assert_eq!(a.octants()[..63], b.octants()[..63]);
    assert!("../bad".parse::<ContentId>().is_err());
}
