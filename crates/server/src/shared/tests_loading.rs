use super::*;
use async_trait::async_trait;
use axum::{
    body::{Body, to_bytes},
    http::Request,
};
use futures::{TryStreamExt, stream::BoxStream};
use object_store::{
    GetOptions, GetResult, ListResult, MultipartUpload, ObjectMeta, ObjectStore,
    PutMultipartOptions, PutOptions, PutPayload, PutResult, path::Path,
};
use std::sync::{
    Mutex,
    atomic::{AtomicBool, Ordering},
};
use tower::ServiceExt;

async fn request(
    app: &Router,
    headers: &HeaderMap,
    method: &str,
    url: &str,
    etag: Option<&str>,
) -> Response {
    let mut request = Request::builder().method(method).uri(url);
    for (key, value) in headers {
        request = request.header(key, value);
    }
    if let Some(etag) = etag {
        request = request.header("If-None-Match", etag);
    }
    app.clone()
        .oneshot(request.body(Body::empty()).unwrap())
        .await
        .unwrap()
}

#[tokio::test]
async fn merkle_freshness_heads_do_not_materialize_views_and_recheck_limits_and_permissions() {
    let mut service = service();
    let (head, imported) = seeded(&service, scene(10.)).await;
    let (app, alice) = http(&service);
    let (_, bob) = crate::auth::Auth::test_session("bob");
    for url in [
        format!("/api/shared-worlds/{}/view?format=cbor", head.id),
        format!(
            "/api/shared-worlds/{}/assemblies/{}/workspace",
            head.id, imported.assembly
        ),
    ] {
        let first = request(&app, &alice, "HEAD", &url, None).await;
        assert_eq!(first.status(), StatusCode::OK);
        let etag = first.headers()["etag"].to_str().unwrap().to_owned();
        assert!(to_bytes(first.into_body(), 1024).await.unwrap().is_empty());
        assert_eq!(
            request(&app, &alice, "HEAD", &url, Some(&etag))
                .await
                .status(),
            StatusCode::NOT_MODIFIED
        );
        assert_eq!(
            request(&app, &bob, "HEAD", &url, Some(&etag))
                .await
                .status(),
            StatusCode::FORBIDDEN
        );
        let prefixes = service
            .store
            .backend
            .list(Some(&service.store.path("cache/read-loads")))
            .try_collect::<Vec<_>>()
            .await
            .unwrap();
        assert!(
            prefixes.is_empty(),
            "A freshness probe must never start or checkpoint a model build"
        );
    }
    let url = format!(
        "/api/shared-worlds/{}/assemblies/{}/workspace",
        head.id, imported.assembly
    );
    let first = request(&app, &alice, "HEAD", &url, None).await;
    let tag = first.headers()["etag"].to_str().unwrap().to_owned();
    let mut change = edit(&service, &head, &imported.pieces[&1], glam::DVec3::ZERO).await;
    change.pieces[0].geometry.as_mut().unwrap().color = 14;
    change.pieces[0].geometry.as_mut().unwrap().rgb = Some(ergo_world::color_rgb(14));
    service
        .prepare(&head.id, "alice", change)
        .await
        .unwrap()
        .publish()
        .await
        .unwrap();
    let changed = request(&app, &alice, "HEAD", &url, Some(&tag)).await;
    assert_eq!(changed.status(), StatusCode::OK);
    assert_ne!(changed.headers()["etag"].to_str().unwrap(), tag);
    let tag = changed.headers()["etag"].to_str().unwrap().to_owned();
    service.limits.pieces = 1;
    let (limited, alice) = http(&service);
    assert_eq!(
        request(&limited, &alice, "HEAD", &url, Some(&tag))
            .await
            .status(),
        StatusCode::OK,
        "A changed authoring cap must invalidate the token"
    );
}

#[derive(Debug)]
struct InterruptedStore {
    inner: Arc<dyn ObjectStore>,
    interrupt: AtomicBool,
    saved: tokio::sync::Notify,
    blocked: tokio::sync::Notify,
    gets: Mutex<Vec<String>>,
    stage: &'static str,
}
impl std::fmt::Display for InterruptedStore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("interrupted checkpoint store")
    }
}
#[async_trait]
impl ObjectStore for InterruptedStore {
    async fn put_opts(
        &self,
        path: &Path,
        payload: PutPayload,
        options: PutOptions,
    ) -> object_store::Result<PutResult> {
        #[derive(Deserialize)]
        struct Header {
            mark: super::super::load_checkpoint::Mark,
        }
        let pause = if path.as_ref().contains("cache/read-loads/") {
            let bytes = payload
                .iter()
                .flat_map(|part| part.iter().copied())
                .collect::<Vec<_>>();
            ciborium::from_reader::<Header, _>(bytes.as_slice())
                .ok()
                .is_some_and(|header| {
                    header.mark.stage == self.stage
                        && if self.stage == "membership" {
                            header.mark.rows >= 128
                        } else {
                            header.mark.pieces >= 64
                        }
                })
        } else {
            false
        };
        let result = self.inner.put_opts(path, payload, options).await?;
        if pause && self.interrupt.swap(false, Ordering::SeqCst) {
            self.saved.notify_one();
            // Simulate a connection/process disappearing after durable storage,
            // before the acknowledgement reaches the loader.
            self.blocked.notified().await;
        }
        Ok(result)
    }
    async fn put_multipart_opts(
        &self,
        p: &Path,
        o: PutMultipartOptions,
    ) -> object_store::Result<Box<dyn MultipartUpload>> {
        self.inner.put_multipart_opts(p, o).await
    }
    async fn get_opts(&self, p: &Path, o: GetOptions) -> object_store::Result<GetResult> {
        self.gets.lock().unwrap().push(p.to_string());
        self.inner.get_opts(p, o).await
    }
    async fn delete(&self, p: &Path) -> object_store::Result<()> {
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

async fn exercise_restart(stage: &'static str) {
    let original = service();
    let mut demo = ergo_world::demo::Demo::new();
    for index in 0..130 {
        demo.add(
            "3005",
            71,
            (index % 20) as f64 * 3.,
            0.,
            (index / 20) as f64 * 3.,
        );
    }
    let scene = WorldExport {
        schema: SCHEMA_VERSION,
        name: "Interrupted view".into(),
        geo: Default::default(),
        bricks: demo.bricks,
        meshes: demo.meshes,
        joints: vec![],
        fixed_bricks: vec![],
        groups: vec![],
        ground: vec![],
        model: None,
    };
    let (mut head, imported) = seeded(&original, scene).await;
    if stage == "membership" {
        let repo = original.repo(&head.id).unwrap();
        let mut tx = repo.begin("main").await.unwrap();
        let path = paged::rule_path(&imported.assembly, 0);
        let mut header: paged::RulesHeader = tx.get(&path).await.unwrap().unwrap();
        header.piece_count = None;
        tx.put(&path, &header).await.unwrap();
        let accepted = tx
            .commit("fixture", "Membership-page fixture", Default::default())
            .await
            .unwrap();
        head.revision = accepted.snapshot.id().clone();
        accepted.snapshot.close().await.unwrap();
    }
    let snapshot = original
        .repo(&head.id)
        .unwrap()
        .checkout("main")
        .await
        .unwrap();
    let first_ids = imported
        .pieces
        .values()
        .cloned()
        .collect::<BTreeSet<_>>()
        .into_iter()
        .take(64)
        .collect::<Vec<_>>();
    let mut completed_payloads = Vec::new();
    for id in &first_ids {
        completed_payloads.push(
            snapshot
                .value_id(&entity_path(3, id))
                .await
                .unwrap()
                .unwrap()
                .to_string(),
        );
    }
    let key = super::super::read_cache::ViewKey {
        world: head.id.clone(),
        revision: head.revision.clone(),
        coord: [0; 3],
        radius: 1,
        pieces: original.view_limits.pieces,
        read_bytes: original.view_limits.bytes,
    };
    let (reference, logical) = super::super::view::reference_packet(&snapshot, &key)
        .await
        .unwrap();
    snapshot.close().await.unwrap();
    let gate = Arc::new(InterruptedStore {
        inner: original.store.backend.clone(),
        interrupt: AtomicBool::new(true),
        saved: Default::default(),
        blocked: Default::default(),
        gets: Default::default(),
        stage,
    });
    let mut store = original.store.clone();
    store.backend = gate.clone();
    let active = SharedWorlds::new(store.clone()).unwrap();
    let (app, alice) = http(&active);
    let url = format!(
        "/api/shared-worlds/{}/view?format=cbor&revision={}",
        head.id, head.revision
    );
    let task = tokio::spawn({
        let app = app.clone();
        let alice = alice.clone();
        let url = url.clone();
        async move { request(&app, &alice, "GET", &url, None).await }
    });
    tokio::time::timeout(std::time::Duration::from_secs(30), gate.saved.notified())
        .await
        .unwrap();
    task.abort();
    assert!(task.await.unwrap_err().is_cancelled());
    drop(app);
    drop(active);
    let newer = original
        .import(
            &head.id,
            "alice",
            ImportRequest {
                base: head.revision.clone(),
                scene: super::scene(20_000.),
            },
        )
        .await
        .unwrap();
    // A fresh service/cache represents a restarted process. Only the persisted
    // source/checkpoint store survives; no verified-object cache is carried over.
    let cold = Arc::new(InterruptedStore {
        inner: original.store.backend.clone(),
        interrupt: AtomicBool::new(false),
        saved: Default::default(),
        blocked: Default::default(),
        gets: Default::default(),
        stage,
    });
    store.backend = cold.clone();
    let restarted = SharedWorlds::new(store).unwrap();
    let (app, alice) = http(&restarted);
    let (_, bob) = crate::auth::Auth::test_session("bob");
    assert_eq!(
        request(&app, &bob, "GET", &url, None).await.status(),
        StatusCode::FORBIDDEN,
        "A saved checkpoint cannot authorize another reader"
    );
    let response = request(&app, &alice, "GET", &url, None).await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.headers()["x-ergo-load-resumed"], "true");
    let raw = to_bytes(response.into_body(), 64 * 1024 * 1024)
        .await
        .unwrap();
    let actual: super::super::view::ViewPacket = ciborium::from_reader(raw.as_ref()).unwrap();
    assert_eq!(
        canonical_cbor(&actual).unwrap(),
        canonical_cbor(&reference).unwrap()
    );
    assert_eq!(
        restarted
            .read_cache
            .get(&key.memory_key().unwrap())
            .unwrap()
            .logical_read_bytes,
        logical
    );
    for cid in completed_payloads {
        assert!(
            !cold
                .gets
                .lock()
                .unwrap()
                .iter()
                .any(|path| path.ends_with(&cid)),
            "Completed piece payload {cid} was fetched again"
        );
    }
    assert_eq!(
        restarted
            .repo(&head.id)
            .unwrap()
            .checkout("main")
            .await
            .unwrap()
            .id(),
        &newer.accepted.revision
    );
}

#[tokio::test]
async fn an_interrupted_view_resumes_after_restart_without_fetching_completed_piece_payloads() {
    exercise_restart("pieces").await;
}

#[tokio::test]
async fn a_membership_page_checkpoint_resumes_without_double_charging_the_header() {
    exercise_restart("membership").await;
}
