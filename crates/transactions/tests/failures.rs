use async_trait::async_trait;
use ergo_transactions::{CommitOptions, Error, Repository};
use futures::stream::BoxStream;
use object_store::{
    GetOptions, GetResult, ListResult, MultipartUpload, ObjectMeta, ObjectStore, PutMode,
    PutMultipartOptions, PutOptions, PutPayload, PutResult, memory::InMemory, path::Path,
};
use std::{
    fmt,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU8, AtomicUsize, Ordering},
    },
};

/// Inject failures at protocol boundaries, rather than duplicating the tree algorithm.
#[derive(Debug, Default)]
struct FaultStore {
    inner: InMemory,
    // 1 = fail immutable upload, 2 = lose successful CAS response,
    // 3 = SDK returns 412 after a hidden successful CAS, 4 = fail CAS before write.
    fault: AtomicU8,
    head_started: tokio::sync::Notify,
    head_updates: AtomicUsize,
    object_reads: AtomicUsize,
    delayed_reads: AtomicBool,
}
impl fmt::Display for FaultStore {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("fault-injecting memory store")
    }
}
fn failure() -> object_store::Error {
    object_store::Error::Generic {
        store: "fault-test",
        source: std::io::Error::other("injected I/O failure").into(),
    }
}

#[async_trait]
impl ObjectStore for FaultStore {
    async fn put_opts(
        &self,
        path: &Path,
        payload: PutPayload,
        opts: PutOptions,
    ) -> object_store::Result<PutResult> {
        if path.as_ref().contains("/objects/")
            && self
                .fault
                .compare_exchange(1, 0, Ordering::SeqCst, Ordering::SeqCst)
                .is_ok()
        {
            return Err(failure());
        }
        if matches!(opts.mode, PutMode::Update(_)) && path.as_ref().contains("/refs/") {
            self.head_updates.fetch_add(1, Ordering::SeqCst);
            let fault = self.fault.swap(0, Ordering::SeqCst);
            if fault == 7 {
                return Err(object_store::Error::AlreadyExists {
                    path: path.to_string(),
                    source: std::io::Error::other("conditional conflict with unchanged head")
                        .into(),
                });
            }
            if fault == 4 {
                return Err(failure());
            }
            let result = self.inner.put_opts(path, payload, opts).await?;
            match fault {
                5 => {
                    self.head_started.notify_one();
                    std::future::pending::<()>().await;
                    unreachable!()
                }
                2 => return Err(failure()),
                3 => {
                    return Err(object_store::Error::Precondition {
                        path: path.to_string(),
                        source: std::io::Error::other("lost success then retried CAS").into(),
                    });
                }
                _ => return Ok(result),
            }
        }
        self.inner.put_opts(path, payload, opts).await
    }
    async fn put_multipart_opts(
        &self,
        p: &Path,
        o: PutMultipartOptions,
    ) -> object_store::Result<Box<dyn MultipartUpload>> {
        self.inner.put_multipart_opts(p, o).await
    }
    async fn get_opts(&self, p: &Path, o: GetOptions) -> object_store::Result<GetResult> {
        if p.as_ref().contains("/objects/") {
            self.object_reads.fetch_add(1, Ordering::SeqCst);
            if self.delayed_reads.load(Ordering::SeqCst) {
                tokio::task::yield_now().await;
            }
        }
        self.inner.get_opts(p, o).await
    }
    async fn delete(&self, p: &Path) -> object_store::Result<()> {
        if self
            .fault
            .compare_exchange(6, 0, Ordering::SeqCst, Ordering::SeqCst)
            .is_ok()
        {
            return Err(failure());
        }
        self.inner.delete(p).await
    }
    fn list(&self, p: Option<&Path>) -> BoxStream<'static, object_store::Result<ObjectMeta>> {
        self.inner.list(p)
    }
    async fn list_with_delimiter(&self, p: Option<&Path>) -> object_store::Result<ListResult> {
        self.inner.list_with_delimiter(p).await
    }
    async fn copy(&self, a: &Path, b: &Path) -> object_store::Result<()> {
        self.inner.copy(a, b).await
    }
    async fn copy_if_not_exists(&self, a: &Path, b: &Path) -> object_store::Result<()> {
        self.inner.copy_if_not_exists(a, b).await
    }
}

#[tokio::test]
async fn parallel_verified_reads_share_one_download_and_failures_remain_retryable() {
    let store = Arc::new(FaultStore::default());
    let repo = Repository::new(store.clone(), "coalesced")
        .unwrap()
        .with_cache_capacity(64 * 1024);
    let value = vec![27u32; 100];
    let id = repo.put_blob(&value).await.unwrap();
    let repo = Repository::new(store.clone(), "coalesced")
        .unwrap()
        .with_cache_capacity(64 * 1024);
    store.object_reads.store(0, Ordering::SeqCst);
    store.delayed_reads.store(true, Ordering::SeqCst);
    let results = futures::future::join_all((0..16).map(|_| repo.blob::<Vec<u32>>(&id))).await;
    for result in results {
        assert_eq!(result.unwrap(), value);
    }
    assert_eq!(
        store.object_reads.load(Ordering::SeqCst),
        1,
        "Concurrent verified reads must not duplicate storage GETs"
    );
    let next = vec![71u32; 10];
    let missing =
        ergo_transactions::ContentId::of(&ergo_transactions::canonical_cbor(&next).unwrap());
    assert!(repo.blob::<Vec<u32>>(&missing).await.is_err());
    // External arrival must retry the failed download, not be masked by the
    // acknowledged-upload LRU population on this Repository instance.
    store
        .inner
        .put(
            &Path::from(format!("coalesced/objects/{missing}.cbor")),
            ergo_transactions::canonical_cbor(&next).unwrap().into(),
        )
        .await
        .unwrap();
    assert_eq!(repo.blob::<Vec<u32>>(&missing).await.unwrap(), next);
    assert_eq!(store.object_reads.load(Ordering::SeqCst), 3);
}

#[tokio::test]
async fn unfenced_conditional_failure_stops_before_blind_retry() {
    let store = Arc::new(FaultStore::default());
    let repo = Repository::new(store.clone(), "unfenced").unwrap();
    repo.create("main").await.unwrap();
    let mut tx = repo.begin("main").await.unwrap();
    tx.put(&"1".parse().unwrap(), &1u32).await.unwrap();
    store.fault.store(7, Ordering::SeqCst);
    assert!(matches!(
        tx.commit("a", "uncertain", CommitOptions::default()).await,
        Err(Error::PublicationUnknown { .. })
    ));
    assert_eq!(store.head_updates.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn grouped_lost_acknowledgements_report_each_members_own_candidate() {
    let store = Arc::new(FaultStore::default());
    let repo = Repository::new(store.clone(), "group-fault").unwrap();
    repo.create("main").await.unwrap();
    let mut submissions = Vec::new();
    for q in [1u8, 2] {
        let mut tx = repo.begin("main").await.unwrap();
        tx.put(
            &ergo_transactions::OctreePath::new(vec![q]).unwrap(),
            &u32::from(q),
        )
        .await
        .unwrap();
        submissions.push(ergo_transactions::Submission {
            transaction: tx,
            actor: "a".into(),
            message: "group".into(),
        });
    }
    store.fault.store(2, Ordering::SeqCst);
    let results = repo
        .commit_batch(submissions, CommitOptions::default())
        .await
        .unwrap();
    assert_eq!(store.head_updates.load(Ordering::SeqCst), 1);
    let mut ids = Vec::new();
    for result in results {
        let Error::PublicationUnknown { candidate, .. } = result.unwrap_err() else {
            panic!("expected unknown")
        };
        assert!(repo.was_committed("main", &candidate).await.unwrap());
        ids.push(candidate);
    }
    assert_ne!(ids[0], ids[1]);
}

#[tokio::test]
async fn uncertain_sweep_error_keeps_the_maintenance_barrier() {
    let store = Arc::new(FaultStore::default());
    let repo = Repository::new(store.clone(), "sweep")
        .unwrap()
        .with_coordinated_gc();
    repo.create("main").await.unwrap().close().await.unwrap();
    repo.put_blob(&"orphan").await.unwrap();
    for _ in 0..256 {
        if repo.active_pins().await.unwrap().is_empty() {
            break;
        }
        tokio::task::yield_now().await;
    }
    let mut maintenance = repo.maintenance().await.unwrap();
    let token = maintenance.token().to_owned();
    store.fault.store(6, Ordering::SeqCst);
    assert!(maintenance.collect(|_| Ok(Vec::new())).await.is_err());
    assert!(matches!(
        maintenance.finish().await,
        Err(Error::MaintenancePoisoned { .. })
    ));
    assert!(matches!(
        repo.begin("main").await,
        Err(Error::MaintenanceBusy)
    ));
    // The controlled failing delete returned before issuing a remote request.
    repo.recover_maintenance(&token).await.unwrap();
}

#[tokio::test]
async fn cancelled_publication_keeps_its_durable_pin_until_operator_recovery() {
    let store = Arc::new(FaultStore::default());
    let repo = Repository::new(store.clone(), "cancel")
        .unwrap()
        .with_coordinated_gc();
    repo.create("main").await.unwrap().close().await.unwrap();
    let mut tx = repo.begin("main").await.unwrap();
    let pin = tx.pin_id().unwrap().to_owned();
    tx.put(&"1".parse().unwrap(), &1u32).await.unwrap();
    store.fault.store(5, Ordering::SeqCst);
    let job = tokio::spawn(tx.commit("a", "cancelled", CommitOptions::default()));
    store.head_started.notified().await;
    job.abort();
    let _ = job.await;
    assert!(repo.active_pins().await.unwrap().contains(&pin));
    assert!(matches!(
        repo.maintenance().await,
        Err(Error::ActivePins { .. })
    ));
    // The controlled fault has already completed its write; no request is pending now.
    repo.recover_abandoned_pin(&pin).await.unwrap();
    let maintenance = repo.maintenance().await.unwrap();
    maintenance.finish().await.unwrap();
}

#[tokio::test]
async fn failed_staging_never_advances_the_head() {
    let store = Arc::new(FaultStore::default());
    let repo = Repository::new(store.clone(), "faults").unwrap();
    let initial = repo.create("main").await.unwrap();
    let mut tx = repo.begin("main").await.unwrap();
    tx.put(&"1".parse().unwrap(), &1u32).await.unwrap();
    tx.put(&"2".parse().unwrap(), &2u32).await.unwrap();
    store.fault.store(1, Ordering::SeqCst);
    assert!(matches!(
        tx.commit("a", "partial upload", CommitOptions::default())
            .await,
        Err(Error::Store(_))
    ));
    let current = repo.checkout("main").await.unwrap();
    assert_eq!(current.id(), initial.id());
    assert!(
        current
            .scan(&"/".parse().unwrap())
            .await
            .unwrap()
            .is_empty()
    );
}

#[tokio::test]
async fn lost_acknowledgement_is_resolved_using_history_after_a_later_commit() {
    let store = Arc::new(FaultStore::default());
    let repo = Repository::new(store.clone(), "faults").unwrap();
    repo.create("main").await.unwrap();
    let mut tx = repo.begin("main").await.unwrap();
    tx.put(&"1".parse().unwrap(), &1u32).await.unwrap();
    store.fault.store(2, Ordering::SeqCst);
    let Error::PublicationUnknown { candidate, .. } = tx
        .commit("a", "lost ack", CommitOptions::default())
        .await
        .unwrap_err()
    else {
        panic!("expected ambiguous outcome");
    };
    let mut later = repo.begin("main").await.unwrap();
    later.put(&"2".parse().unwrap(), &2u32).await.unwrap();
    later
        .commit("b", "later", CommitOptions::default())
        .await
        .unwrap();
    assert!(repo.was_committed("main", &candidate).await.unwrap());
    assert_eq!(repo.checkout("main").await.unwrap().commit().sequence, 2);
}

#[tokio::test]
async fn a_staged_commit_is_not_proof_of_publication() {
    let store = Arc::new(FaultStore::default());
    let repo = Repository::new(store.clone(), "faults").unwrap();
    repo.create("main").await.unwrap();
    let mut tx = repo.begin("main").await.unwrap();
    tx.put(&"1".parse().unwrap(), &1u32).await.unwrap();
    store.fault.store(4, Ordering::SeqCst);
    let Error::PublicationUnknown { candidate, .. } = tx
        .commit("a", "failed CAS", CommitOptions::default())
        .await
        .unwrap_err()
    else {
        panic!("expected ambiguous outcome");
    };
    assert!(!repo.was_committed("main", &candidate).await.unwrap());
    assert_eq!(repo.checkout("main").await.unwrap().commit().sequence, 0);
}

#[tokio::test]
async fn sdk_precondition_after_hidden_success_does_not_duplicate_the_transaction() {
    let store = Arc::new(FaultStore::default());
    let repo = Repository::new(store.clone(), "faults").unwrap();
    repo.create("main").await.unwrap();
    let mut tx = repo.begin("main").await.unwrap();
    tx.put(&"1".parse().unwrap(), &1u32).await.unwrap();
    store.fault.store(3, Ordering::SeqCst);
    let options = CommitOptions {
        max_attempts: 1,
        ..Default::default()
    };
    let result = tx.commit("a", "hidden success", options).await.unwrap();
    assert_eq!(result.attempts, 1);
    assert_eq!(repo.checkout("main").await.unwrap().commit().sequence, 1);
}

#[tokio::test]
async fn unsupported_cas_never_falls_back_to_an_unconditional_overwrite() {
    let dir = tempfile::tempdir().unwrap();
    let store =
        Arc::new(object_store::local::LocalFileSystem::new_with_prefix(dir.path()).unwrap());
    let repo = Repository::new(store, "local").unwrap();
    let initial = repo.create("main").await.unwrap();
    let mut tx = repo.begin("main").await.unwrap();
    tx.put(&"1".parse().unwrap(), &1u32).await.unwrap();
    assert!(
        tx.commit("a", "unsupported", CommitOptions::default())
            .await
            .is_err()
    );
    assert_eq!(repo.checkout("main").await.unwrap().id(), initial.id());
}
