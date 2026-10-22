//! S3_BUCKET and AWS credentials are required. S3_ENDPOINT/S3_REGION/S3_PREFIX
//! follow Ergo's server conventions. Every run creates an isolated branch.
use ergo_transactions::{CommitOptions, OctreePath, Repository};
use object_store::aws::{AmazonS3Builder, S3ConditionalPut};
use serde::{Deserialize, Serialize};
use std::{env, sync::Arc};

#[derive(Debug, Serialize, Deserialize)]
struct GeometryBlock {
    positions: Vec<[f32; 3]>,
    triangles: Vec<[u32; 3]>,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let store = AmazonS3Builder::from_env()
        .with_bucket_name(env::var("S3_BUCKET")?)
        .with_region(env::var("S3_REGION").unwrap_or_else(|_| "fr-par".into()))
        .with_endpoint(
            env::var("S3_ENDPOINT").unwrap_or_else(|_| "https://s3.fr-par.scw.cloud".into()),
        )
        .with_virtual_hosted_style_request(false)
        .with_conditional_put(S3ConditionalPut::ETagMatch)
        .build()?;
    let prefix = env::var("S3_PREFIX").unwrap_or_else(|_| "ergo-transactions".into());
    let repo = Repository::new(Arc::new(store), &prefix)?;
    let branch = format!("example-{}", uuid::Uuid::new_v4());
    repo.create(&branch).await?;
    let mut left = repo.begin(&branch).await?;
    let mut right = repo.begin(&branch).await?;
    let triangle = GeometryBlock {
        positions: vec![[0., 0., 0.], [1., 0., 0.], [0., 1., 0.]],
        triangles: vec![[0, 1, 2]],
    };
    // A shard-local octree keeps paths short. for_block([x,y,z]) is also available.
    let a: OctreePath = "2/1".parse()?;
    let b: OctreePath = "2/6".parse()?;
    left.put(&a, &triangle).await?;
    right.put(&b, &triangle).await?;
    // Real geometry validation must read/watch collision and support dependencies
    // here, before commit. Separate branches alone do not imply physical independence.
    let (a_result, b_result) = tokio::join!(
        left.commit("editor-a", "left block", CommitOptions::default()),
        right.commit("editor-b", "right block", CommitOptions::default()),
    );
    let a_result = a_result?;
    let b_result = b_result?;
    println!("branch: {branch}");
    println!(
        "commits: {} and {}",
        a_result.snapshot.id(),
        b_result.snapshot.id()
    );
    println!("rebases: {}", a_result.rebases + b_result.rebases);
    let snapshot = repo.checkout(&branch).await?;
    println!(
        "current geometry blocks: {}",
        snapshot.scan(&OctreePath::root()).await?.len()
    );
    Ok(())
}
