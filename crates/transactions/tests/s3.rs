//! Explicitly opt-in: writes a unique prefix in the configured test bucket.
use ergo_transactions::{CommitOptions, Repository};
use object_store::{
    ObjectStore, PutMode, UpdateVersion,
    aws::{AmazonS3Builder, S3ConditionalPut},
    path::Path,
};
use std::{env, sync::Arc};

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "requires S3_BUCKET and credentials; writes isolated test objects"]
async fn live_s3_conditional_race_and_independent_edits() {
    let store = Arc::new(
        AmazonS3Builder::from_env()
            .with_bucket_name(env::var("S3_BUCKET").expect("S3_BUCKET must name a test bucket"))
            .with_region(env::var("S3_REGION").unwrap_or_else(|_| "fr-par".into()))
            .with_endpoint(
                env::var("S3_ENDPOINT").unwrap_or_else(|_| "https://s3.fr-par.scw.cloud".into()),
            )
            .with_virtual_hosted_style_request(false)
            .with_conditional_put(S3ConditionalPut::ETagMatch)
            .build()
            .unwrap(),
    );
    let prefix = format!("ergo-transaction-tests/{}", uuid::Uuid::new_v4());
    println!("live test prefix: {prefix}");
    // Repeated races exercise exactly-one-winner and immediate GET visibility.
    for round in 0..5 {
        let key = Path::from(format!("{prefix}/race-{round}"));
        let initial = store
            .put_opts(&key, "initial".into(), PutMode::Create.into())
            .await
            .unwrap();
        let version: UpdateVersion = initial.into();
        let barrier = Arc::new(tokio::sync::Barrier::new(16));
        let mut tasks = Vec::new();
        for i in 0..16 {
            let store = store.clone();
            let key = key.clone();
            let version = version.clone();
            let barrier = barrier.clone();
            tasks.push(tokio::spawn(async move {
                barrier.wait().await;
                let value = format!("winner-{i}");
                match store
                    .put_opts(&key, value.clone().into(), PutMode::Update(version).into())
                    .await
                {
                    Ok(_) => Some(value),
                    Err(
                        object_store::Error::Precondition { .. }
                        | object_store::Error::AlreadyExists { .. },
                    ) => None,
                    Err(e) => panic!("unexpected conditional error: {e}"),
                }
            }));
        }
        let mut winners = Vec::new();
        for task in tasks {
            if let Some(value) = task.await.unwrap() {
                winners.push(value);
            }
        }
        assert_eq!(winners.len(), 1);
        let bytes = store.get(&key).await.unwrap().bytes().await.unwrap();
        assert_eq!(bytes.as_ref(), winners[0].as_bytes());
    }
    let repo = Repository::new(store, &prefix).unwrap();
    let original = repo.create("main").await.unwrap();
    let mut a = repo.begin("main").await.unwrap();
    let mut b = repo.begin("main").await.unwrap();
    a.put(&"1/2".parse().unwrap(), &12u32).await.unwrap();
    b.put(&"6/7".parse().unwrap(), &67u32).await.unwrap();
    let (a, b) = tokio::join!(
        a.commit("a", "left", CommitOptions::default()),
        b.commit("b", "right", CommitOptions::default())
    );
    a.unwrap();
    b.unwrap();
    let current = repo.checkout("main").await.unwrap();
    assert_eq!(
        current.get::<u32>(&"1/2".parse().unwrap()).await.unwrap(),
        Some(12)
    );
    assert_eq!(
        current.get::<u32>(&"6/7".parse().unwrap()).await.unwrap(),
        Some(67)
    );
    assert!(
        original
            .scan(&"/".parse().unwrap())
            .await
            .unwrap()
            .is_empty()
    );
    // Keep this unique prefix for inspection; clean it with bucket tooling afterwards.
}
