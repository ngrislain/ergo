use ergo_transactions::{CommitOptions, ContentId, Error, OctreePath, Repository, canonical_cbor};
use futures::TryStreamExt;
use object_store::{ObjectStore, memory::InMemory, path::Path};
use std::sync::Arc;

#[tokio::test]
async fn deep_hash_paths_use_packed_edges_and_virtual_region_dependencies() {
    let backend = Arc::new(InMemory::new());
    let repo = Repository::new(backend.clone(), "packed").unwrap();
    repo.create("main").await.unwrap();
    let a = OctreePath::new(vec![1; 87]).unwrap();
    let mut b = vec![1; 87];
    b[40] = 2;
    let b = OctreePath::new(b).unwrap();
    let mut tx = repo.begin("main").await.unwrap();
    tx.put(&a, &1u32).await.unwrap();
    tx.put(&b, &2u32).await.unwrap();
    tx.commit("a", "deep paths", CommitOptions::default())
        .await
        .unwrap();
    let objects: Vec<_> = backend
        .list(Some(&Path::from("packed/objects")))
        .try_collect()
        .await
        .unwrap();
    assert!(
        objects.len() < 50,
        "unary paths must not allocate hundreds of S3 objects"
    );
    let virtual_region = OctreePath::new(vec![1; 60]).unwrap();
    let mut reader = repo.begin("main").await.unwrap();
    reader.watch(&virtual_region).await.unwrap();
    reader.put(&"7".parse().unwrap(), &7u32).await.unwrap();
    let mut independent = repo.begin("main").await.unwrap();
    independent.put(&b, &20u32).await.unwrap();
    independent
        .commit("b", "other packed branch", CommitOptions::default())
        .await
        .unwrap();
    reader
        .commit("a", "virtual region unchanged", CommitOptions::default())
        .await
        .unwrap();
    let snapshot = repo.checkout("main").await.unwrap();
    assert_eq!(snapshot.get::<u32>(&a).await.unwrap(), Some(1));
    assert_eq!(snapshot.get::<u32>(&b).await.unwrap(), Some(20));
}

#[tokio::test]
async fn noncurrent_heads_and_commits_are_rejected() {
    for format in [1, 2] {
        let backend = Arc::new(InMemory::new());
        let repo = Repository::new(backend.clone(), "unsupported").unwrap();
        let snapshot = repo.create("main").await.unwrap();
        let commit_id = snapshot.id().clone();
        backend
            .put(
                &Path::from("unsupported/refs/main.cbor"),
                canonical_cbor(&serde_json::json!({
                    "format":format,"commit":commit_id,"history":{"root":null,"retained_from":0}
                }))
                .unwrap()
                .into(),
            )
            .await
            .unwrap();
        assert!(repo.checkout("main").await.is_err());
        let mut old = serde_json::to_value(snapshot.commit()).unwrap();
        old["format"] = serde_json::json!(format);
        let bytes = canonical_cbor(&old).unwrap();
        let id = ContentId::of(&bytes);
        backend
            .put(
                &Path::from(format!("unsupported/objects/{id}.cbor")),
                bytes.into(),
            )
            .await
            .unwrap();
        assert!(repo.snapshot(&id).await.is_err());
    }
}

#[tokio::test]
async fn limited_reads_reject_large_objects_including_cache_hits() {
    let repo = Repository::new(Arc::new(InMemory::new()), "limits")
        .unwrap()
        .with_cache_capacity(4096);
    repo.create("main").await.unwrap();
    let mut tx = repo.begin("main").await.unwrap();
    tx.put(&"1".parse().unwrap(), &vec![9u8; 1024])
        .await
        .unwrap();
    let snapshot = tx
        .commit("a", "chunk", CommitOptions::default())
        .await
        .unwrap()
        .snapshot;
    snapshot
        .get::<Vec<u8>>(&"1".parse().unwrap())
        .await
        .unwrap();
    assert!(matches!(
        snapshot
            .get_limited::<Vec<u8>>(&"1".parse().unwrap(), 32)
            .await,
        Err(Error::ObjectTooLarge { .. })
    ));
}
