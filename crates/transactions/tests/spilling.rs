use ergo_transactions::{Error, OctreePath, Repository, spill::SpillLimits};
use object_store::memory::InMemory;
use std::sync::Arc;

#[tokio::test]
async fn spilled_execution_replays_disjoint_changes_and_retains_all_predicates() {
    let repo = Repository::new(Arc::new(InMemory::new()), "spilled-replay").unwrap();
    repo.create("main").await.unwrap();
    let mut tx = repo.begin("main").await.unwrap();
    for i in 0..700u64 {
        let path = OctreePath::new(vec![
            0,
            ((i >> 6) & 7) as u8,
            ((i >> 3) & 7) as u8,
            (i & 7) as u8,
        ])
        .unwrap();
        // The first octant plus three digits admits 512 distinct paths; longer
        // keys preserve every edit while forcing both journals and nodes to disk.
        let path = if i < 512 {
            path
        } else {
            OctreePath::new(vec![
                1,
                ((i >> 6) & 7) as u8,
                ((i >> 3) & 7) as u8,
                (i & 7) as u8,
            ])
            .unwrap()
        };
        tx.put(&path, &vec![i; 256]).await.unwrap();
    }
    assert!(tx.spill_database_bytes().unwrap() > 1024 * 1024);
    let mut other = repo.begin("main").await.unwrap();
    let remote = OctreePath::new(vec![7, 7]).unwrap();
    other.put(&remote, &999u32).await.unwrap();
    other
        .commit("bob", "disjoint", Default::default())
        .await
        .unwrap();
    let accepted = tx
        .commit("alice", "spilled", Default::default())
        .await
        .unwrap();
    assert!(accepted.rebases > 0);
    assert_eq!(accepted.snapshot.commit().edits.len(), 700);
    let cloned = accepted.snapshot.clone();
    assert!(
        std::ptr::eq(cloned.commit(), accepted.snapshot.commit()),
        "snapshot clones must share immutable history metadata"
    );
    assert_eq!(
        accepted.snapshot.get::<u32>(&remote).await.unwrap(),
        Some(999)
    );
    let mut stale = repo.begin("main").await.unwrap();
    let watched = OctreePath::new(vec![0]).unwrap();
    stale.watch(&watched).await.unwrap();
    stale
        .put(&OctreePath::new(vec![6]).unwrap(), &1u32)
        .await
        .unwrap();
    let mut change = repo.begin("main").await.unwrap();
    change
        .put(&OctreePath::new(vec![0, 7, 7, 7]).unwrap(), &2u32)
        .await
        .unwrap();
    change
        .commit("bob", "watched", Default::default())
        .await
        .unwrap();
    assert!(matches!(
        stale
            .commit("alice", "must conflict", Default::default())
            .await,
        Err(Error::Conflict { .. })
    ));
}

#[tokio::test]
async fn scratch_exhaustion_never_publishes_a_partially_staged_transaction() {
    let repo = Repository::new(Arc::new(InMemory::new()), "disk-full")
        .unwrap()
        .with_spill_limits(SpillLimits {
            database_bytes: 64 * 1024,
            cache_kib: 4,
        })
        .unwrap();
    let initial = repo.create("main").await.unwrap();
    let mut tx = repo.begin("main").await.unwrap();
    let path = OctreePath::new(vec![1]).unwrap();
    tx.put(&path, &42u32).await.unwrap();
    assert!(matches!(
        tx.stage_blob(&vec![7u8; 256 * 1024]),
        Err(Error::Scratch(_))
    ));
    assert!(matches!(
        tx.commit("alice", "must not publish", Default::default())
            .await,
        Err(Error::Scratch(_))
    ));
    assert_eq!(repo.checkout("main").await.unwrap().id(), initial.id());
    assert_eq!(
        repo.checkout("main")
            .await
            .unwrap()
            .get::<u32>(&path)
            .await
            .unwrap(),
        None
    );
}

#[tokio::test]
async fn a_group_commit_never_bypasses_the_combined_encoded_commit_budget() {
    use ergo_transactions::{CommitLimits, Submission};
    let repo = Repository::new(Arc::new(InMemory::new()), "batch-record-budget")
        .unwrap()
        .with_commit_limits(CommitLimits {
            bytes: 2500,
            item_bytes: 1024,
        })
        .unwrap();
    let initial = repo.create("main").await.unwrap();
    let mut submissions = Vec::new();
    for actor in 0..2u8 {
        let mut tx = repo.begin("main").await.unwrap();
        for i in 0..12u8 {
            tx.put(&OctreePath::new(vec![actor, i / 8, i % 8]).unwrap(), &i)
                .await
                .unwrap();
        }
        submissions.push(Submission {
            transaction: tx,
            actor: format!("actor-{actor}"),
            message: "budget".into(),
        });
    }
    assert!(matches!(
        repo.commit_batch(submissions, Default::default()).await,
        Err(Error::Scratch(_))
    ));
    assert_eq!(repo.checkout("main").await.unwrap().id(), initial.id());
}
