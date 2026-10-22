//! Synthetic, repeatable concurrency benchmark. No network access unless --s3.
//! Emits JSON, checks final leaf counts, and retains its unique S3 prefix.
use ergo_transactions::{CommitOptions, OctreePath, Repository};
use object_store::{
    ObjectStore,
    aws::{AmazonS3Builder, S3ConditionalPut},
    memory::InMemory,
};
use serde::Serialize;
use std::{env, sync::Arc, time::Instant};

#[derive(Serialize)]
struct Payload {
    worker: usize,
    edit: usize,
    geometry: ciborium::value::Value,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let mut writers = 64usize;
    let mut shards = 8usize;
    let mut edits = 4usize;
    let mut payload_bytes = 16 * 1024usize;
    let mut s3 = false;
    let mut coordinated = false;
    let mut group_size = 1usize;
    let mut args = env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--s3" => s3 = true,
            "--coordinated-gc" => coordinated = true,
            "--group-size" => group_size = args.next().ok_or("missing group size")?.parse()?,
            "--writers" => writers = args.next().ok_or("missing writers")?.parse()?,
            "--shards" => shards = args.next().ok_or("missing shards")?.parse()?,
            "--edits" => edits = args.next().ok_or("missing edits")?.parse()?,
            "--payload-bytes" => {
                payload_bytes = args.next().ok_or("missing payload bytes")?.parse()?
            }
            "--help" => {
                println!(
                    "concurrent_bench [--writers 64] [--shards 8] [--edits 4] [--payload-bytes 16384] [--group-size 1] [--s3] [--coordinated-gc]"
                );
                return Ok(());
            }
            _ => return Err(format!("unknown argument: {arg}").into()),
        }
    }
    if !(1..=1024).contains(&writers)
        || !(1..=writers).contains(&shards)
        || edits == 0
        || writers.checked_mul(edits).is_none_or(|n| n > 262_144)
        || payload_bytes > 4 * 1024 * 1024
        || !(1..=128).contains(&group_size)
    {
        return Err("invalid dimensions: writers 1..1024, shards 1..writers, edits > 0, total edits <= 262144, payload <= 4 MiB".into());
    }
    let backend: Arc<dyn ObjectStore> = if s3 {
        Arc::new(
            AmazonS3Builder::from_env()
                .with_bucket_name(env::var("S3_BUCKET")?)
                .with_region(env::var("S3_REGION").unwrap_or_else(|_| "fr-par".into()))
                .with_endpoint(
                    env::var("S3_ENDPOINT")
                        .unwrap_or_else(|_| "https://s3.fr-par.scw.cloud".into()),
                )
                .with_virtual_hosted_style_request(false)
                .with_conditional_put(S3ConditionalPut::ETagMatch)
                .build()?,
        )
    } else {
        Arc::new(InMemory::new())
    };
    let prefix = format!("ergo-transaction-bench/{}", uuid::Uuid::new_v4());
    let mut repo = Repository::new(backend, &prefix)?.with_cache_capacity(64 * 1024 * 1024);
    if coordinated {
        repo = repo.with_coordinated_gc();
    }
    for shard in 0..shards {
        repo.create(&format!("shard-{shard}"))
            .await?
            .close()
            .await?;
    }
    let barrier = Arc::new(tokio::sync::Barrier::new(writers));
    let started = Instant::now();
    let mut samples = Vec::new();
    let mut attempts = 0;
    let mut rebases = 0;
    let mut head_cas_attempts = 0;
    if group_size == 1 {
        let mut jobs = Vec::new();
        for worker in 0..writers {
            let repo = repo.clone();
            let barrier = barrier.clone();
            jobs.push(tokio::spawn(async move {
                let mut results = Vec::new();
                for edit in 0..edits {
                    let begin = Instant::now();
                    let branch = format!("shard-{}", worker % shards);
                    let mut tx = repo.begin(&branch).await?;
                    let key = worker * edits + edit;
                    let path = OctreePath::new(
                        (0..6)
                            .rev()
                            .map(|q| ((key >> (q * 3)) & 7) as u8)
                            .collect::<Vec<_>>(),
                    )?;
                    tx.put(
                        &path,
                        &Payload {
                            worker,
                            edit,
                            geometry: ciborium::value::Value::Bytes(vec![7; payload_bytes]),
                        },
                    )
                    .await?;
                    if edit == 0 {
                        tokio::time::timeout(std::time::Duration::from_secs(60), barrier.wait())
                            .await?;
                    }
                    let options = CommitOptions {
                        max_attempts: 256,
                        ..Default::default()
                    };
                    let accepted = tx
                        .commit("benchmark", "synthetic independent edit", options)
                        .await?;
                    results.push((
                        begin.elapsed().as_secs_f64() * 1000.0,
                        accepted.attempts,
                        accepted.rebases,
                    ));
                    accepted.snapshot.close().await?;
                }
                Ok::<_, Box<dyn std::error::Error + Send + Sync>>(results)
            }));
        }
        for job in jobs {
            for (ms, a, r) in job.await?? {
                samples.push(ms);
                attempts += a;
                head_cas_attempts += a;
                rebases += r;
            }
        }
    } else {
        // Group mode stages one wave before publication. It measures publication
        // batching, not the same arrival schedule as the independent-writer mode.
        for edit in 0..edits {
            let mut groups: Vec<Vec<(ergo_transactions::Submission, Instant)>> =
                (0..shards).map(|_| Vec::new()).collect();
            for worker in 0..writers {
                let begin = Instant::now();
                let mut tx = repo.begin(&format!("shard-{}", worker % shards)).await?;
                let key = worker * edits + edit;
                let path = OctreePath::new(
                    (0..6)
                        .rev()
                        .map(|q| ((key >> (q * 3)) & 7) as u8)
                        .collect::<Vec<_>>(),
                )?;
                tx.put(
                    &path,
                    &Payload {
                        worker,
                        edit,
                        geometry: ciborium::value::Value::Bytes(vec![7; payload_bytes]),
                    },
                )
                .await?;
                groups[worker % shards].push((
                    ergo_transactions::Submission {
                        transaction: tx,
                        actor: "benchmark".into(),
                        message: "grouped independent edit".into(),
                    },
                    begin,
                ));
            }
            for group in groups {
                let mut pending = group.into_iter();
                loop {
                    let batch: Vec<_> = pending.by_ref().take(group_size).collect();
                    if batch.is_empty() {
                        break;
                    }
                    let (submissions, begins): (Vec<_>, Vec<_>) = batch.into_iter().unzip();
                    let results = repo
                        .commit_batch(
                            submissions,
                            CommitOptions {
                                max_attempts: 256,
                                ..Default::default()
                            },
                        )
                        .await?;
                    let mut batch_attempts = 0;
                    for (result, begin) in results.into_iter().zip(begins) {
                        let accepted = result?;
                        batch_attempts = batch_attempts.max(accepted.attempts);
                        attempts += accepted.attempts;
                        rebases += accepted.rebases;
                        samples.push(begin.elapsed().as_secs_f64() * 1000.);
                        accepted.snapshot.close().await?;
                    }
                    head_cas_attempts += batch_attempts;
                }
            }
        }
    }
    let elapsed = started.elapsed().as_secs_f64();
    let mut count = 0;
    for shard in 0..shards {
        let snapshot = repo.checkout(&format!("shard-{shard}")).await?;
        let mut after = None;
        loop {
            let page = snapshot
                .scan_page(&OctreePath::root(), after.as_ref(), 1000)
                .await?;
            count += page.leaves.len();
            after = page.next;
            if after.is_none() {
                break;
            }
        }
        snapshot.close().await?;
    }
    if count != writers * edits {
        return Err(format!("lost edits: expected {}, found {count}", writers * edits).into());
    }
    samples.sort_by(f64::total_cmp);
    let percentile = |q: f64| samples[((samples.len() - 1) as f64 * q).ceil() as usize];
    println!(
        "{}",
        serde_json::to_string_pretty(&serde_json::json!({
            "backend": if s3 { "s3" } else { "memory" }, "prefix": prefix, "coordinated_gc": coordinated,
            "writers": writers, "shards": shards, "edits_per_writer": edits, "payload_bytes": payload_bytes,
            "group_size":group_size,"head_cas_attempts":head_cas_attempts,
            "commits": count, "seconds": elapsed, "commits_per_second": count as f64 / elapsed,
            "latency_ms": {"p50": percentile(0.5), "p95": percentile(0.95), "p99": percentile(0.99)},
            "publication_attempts": attempts, "rebases": rebases, "cache": repo.cache_stats()
        }))?
    );
    Ok(())
}
