use ergo_transactions::{CommitOptions, Error, Repository, Submission};
use object_store::memory::InMemory;
use std::sync::Arc;
#[tokio::test]
async fn compatible_members_keep_distinct_commits_and_conflicts_are_not_published() {
    let repo = Repository::new(Arc::new(InMemory::new()), "batch").unwrap();
    repo.create("main").await.unwrap();
    let mut a = repo.begin("main").await.unwrap();
    let mut b = repo.begin("main").await.unwrap();
    let mut c = repo.begin("main").await.unwrap();
    a.put(&"1".parse().unwrap(), &1u32).await.unwrap();
    b.put(&"2".parse().unwrap(), &2u32).await.unwrap();
    assert_eq!(c.get::<u32>(&"1".parse().unwrap()).await.unwrap(), None);
    c.put(&"3".parse().unwrap(), &3u32).await.unwrap();
    let submissions = [a, b, c]
        .into_iter()
        .enumerate()
        .map(|(i, transaction)| Submission {
            transaction,
            actor: format!("actor-{i}"),
            message: format!("member-{i}"),
        })
        .collect();
    let mut results = repo
        .commit_batch(submissions, CommitOptions::default())
        .await
        .unwrap()
        .into_iter();
    let first = results.next().unwrap().unwrap();
    let second = results.next().unwrap().unwrap();
    assert!(matches!(
        results.next().unwrap(),
        Err(Error::Conflict { .. })
    ));
    assert_eq!(first.snapshot.commit().actor, "actor-0");
    assert_eq!(
        second.snapshot.commit().parent.as_ref(),
        Some(first.snapshot.id())
    );
    assert_eq!(
        first
            .snapshot
            .get::<u32>(&"2".parse().unwrap())
            .await
            .unwrap(),
        None
    );
    let tip = repo.checkout("main").await.unwrap();
    assert_eq!(tip.id(), second.snapshot.id());
    assert_eq!(tip.commit().sequence, 2);
    assert_eq!(tip.get::<u32>(&"3".parse().unwrap()).await.unwrap(), None);
    assert!(
        repo.was_committed("main", first.snapshot.id())
            .await
            .unwrap()
    );
}
