use super::*;
use async_trait::async_trait;
use futures::stream::BoxStream;
use object_store::{
    GetOptions, GetResult, ListResult, MultipartUpload, ObjectMeta, ObjectStore,
    PutMultipartOptions, PutOptions, PutPayload, PutResult, path::Path,
};
use std::sync::atomic::{AtomicUsize, Ordering};

#[derive(Debug, Default)]
struct CountStore {
    inner: object_store::memory::InMemory,
    gets: AtomicUsize,
}
impl std::fmt::Display for CountStore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("counting memory store")
    }
}
#[async_trait]
impl ObjectStore for CountStore {
    async fn put_opts(
        &self,
        p: &Path,
        v: PutPayload,
        o: PutOptions,
    ) -> object_store::Result<PutResult> {
        self.inner.put_opts(p, v, o).await
    }
    async fn put_multipart_opts(
        &self,
        p: &Path,
        o: PutMultipartOptions,
    ) -> object_store::Result<Box<dyn MultipartUpload>> {
        self.inner.put_multipart_opts(p, o).await
    }
    async fn get_opts(&self, p: &Path, o: GetOptions) -> object_store::Result<GetResult> {
        self.gets.fetch_add(1, Ordering::SeqCst);
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

async fn packed_pieces(
    service: &SharedWorlds,
    world: &str,
    revision: &ContentId,
    coord: ergo_world::BlockCoord,
) -> BTreeMap<ContentId, Piece> {
    let snapshot = service
        .repo(world)
        .unwrap()
        .accepted_snapshot("main", revision)
        .await
        .unwrap();
    let mut values = BTreeMap::new();
    assert!(
        bulk::read_block(&snapshot, coord, |page| {
            values.extend(page.pieces);
            Ok(())
        })
        .await
        .unwrap()
    );
    snapshot.close().await.unwrap();
    values
}

#[tokio::test]
async fn packed_pages_track_appearance_moves_deletions_and_historical_snapshots() {
    let service = service();
    let (head, imported) = seeded(&service, scene(100.)).await;
    let id = &imported.pieces[&1];
    let original = piece(&service, &head.id, id).await;
    let coord = *original.replicas.iter().next().unwrap();
    assert_eq!(
        canonical_cbor(&packed_pieces(&service, &head.id, &head.revision, coord).await[id])
            .unwrap(),
        canonical_cbor(&original).unwrap()
    );
    let mut request = edit(&service, &head, id, glam::DVec3::ZERO).await;
    let geometry = request.pieces[0].geometry.as_mut().unwrap();
    geometry.color = 14;
    geometry.rgb = Some(ergo_world::color_rgb(14));
    let colored = service
        .prepare(&head.id, "alice", request)
        .await
        .unwrap()
        .publish()
        .await
        .unwrap();
    assert_eq!(
        packed_pieces(&service, &head.id, &colored.revision, coord).await[id]
            .geometry
            .color,
        14
    );
    assert_eq!(
        packed_pieces(&service, &head.id, &head.revision, coord).await[id]
            .geometry
            .color,
        original.geometry.color
    );
    let mut moved = original.clone();
    moved.geometry.color = 14;
    moved.geometry.rgb = Some(ergo_world::color_rgb(14));
    moved.geometry.position.x += 1024.;
    let accepted = service
        .prepare(
            &head.id,
            "alice",
            CommitRequest {
                base: colored.revision,
                pieces: vec![PieceEdit {
                    id: id.clone(),
                    geometry: Some(moved.geometry),
                    mesh: None,
                }],
                rules: vec![],
                joints: vec![],
                message: "move packed replica".into(),
            },
        )
        .await
        .unwrap()
        .publish()
        .await
        .unwrap();
    assert!(
        !packed_pieces(&service, &head.id, &accepted.revision, coord)
            .await
            .contains_key(id)
    );
    let actual = piece(&service, &head.id, id).await;
    let new_coord = *actual.replicas.iter().next().unwrap();
    assert!(
        packed_pieces(&service, &head.id, &accepted.revision, new_coord)
            .await
            .contains_key(id)
    );
    let deleted = service
        .prepare(
            &head.id,
            "alice",
            CommitRequest {
                base: accepted.revision,
                pieces: vec![PieceEdit {
                    id: id.clone(),
                    geometry: None,
                    mesh: None,
                }],
                rules: vec![],
                joints: vec![],
                message: "remove packed replica".into(),
            },
        )
        .await
        .unwrap()
        .publish()
        .await
        .unwrap();
    assert!(
        !packed_pieces(&service, &head.id, &deleted.revision, new_coord)
            .await
            .contains_key(id)
    );
}

#[tokio::test]
async fn legacy_block_backfill_changes_only_the_read_projection() {
    let service = service();
    let (head, imported) = seeded(&service, scene(100.)).await;
    let id = &imported.pieces[&1];
    let original = piece(&service, &head.id, id).await;
    let coord = *original.replicas.iter().next().unwrap();
    let repo = service.repo(&head.id).unwrap();
    let mut tx = repo.begin("main").await.unwrap();
    let mut metadata: Metadata = tx.get(&root(2)).await.unwrap().unwrap();
    metadata.layout = 2;
    tx.put(&root(2), &metadata).await.unwrap();
    let mut header: spatial::BlockHeader =
        tx.get(&spatial::directory(coord)).await.unwrap().unwrap();
    header.packed = false;
    tx.put(&spatial::directory(coord), &header).await.unwrap();
    tx.remove(&bulk::block_path(coord)).await.unwrap();
    let legacy = tx
        .commit("test", "legacy block fixture", Default::default())
        .await
        .unwrap();
    let before = legacy.snapshot.value_id(&entity_path(3, id)).await.unwrap();
    let packed = service.pack_block(&head.id, coord.into()).await.unwrap();
    assert_eq!(packed["geometry_changed"], false);
    let revision: ContentId = serde_json::from_value(packed["revision"].clone()).unwrap();
    let after = repo.accepted_snapshot("main", &revision).await.unwrap();
    assert_eq!(
        after
            .get::<Metadata>(&root(2))
            .await
            .unwrap()
            .unwrap()
            .layout,
        3
    );
    assert_eq!(after.value_id(&entity_path(3, id)).await.unwrap(), before);
    assert_eq!(
        canonical_cbor(&packed_pieces(&service, &head.id, &revision, coord).await[id]).unwrap(),
        canonical_cbor(&original).unwrap()
    );
    assert!(
        !legacy
            .snapshot
            .get::<spatial::BlockHeader>(&spatial::directory(coord))
            .await
            .unwrap()
            .unwrap()
            .packed
    );
    let legacy_read = bulk::read_block(&legacy.snapshot, coord, |_| {
        panic!("Legacy data must fall back")
    })
    .await
    .unwrap();
    assert!(!legacy_read);
}

#[tokio::test]
async fn brick_specifications_use_versioned_http_revalidation() {
    use axum::{body::Body, http::Request};
    use tower::ServiceExt;
    let service = service();
    let mut mesh = ergo_world::geometry::fallback_mesh("3003.dat");
    mesh.quality = "source".into();
    let mut bytes = Vec::new();
    ciborium::into_writer(&mesh, &mut bytes).unwrap();
    use sha2::{Digest, Sha256};
    let hash = hex::encode(Sha256::digest(&bytes));
    let description = ergo_assets::PartDescription::from_mesh(&mesh, hash.clone());
    service
        .store
        .backend
        .put(&Path::from(format!("objects/{hash}.cbor")), bytes.into())
        .await
        .unwrap();
    service
        .store
        .backend
        .put(
            &Path::from("assets/parts/3003.dat.json"),
            serde_json::to_vec(&description).unwrap().into(),
        )
        .await
        .unwrap();
    let (app, _) = http(&service);
    let response = app
        .clone()
        .oneshot(
            Request::get("/api/parts/3003.dat")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.headers()["cache-control"], "public, max-age=60");
    let etag = response.headers()["etag"].clone();
    let response = app
        .oneshot(
            Request::get("/api/parts/3003.dat")
                .header("if-none-match", etag)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::NOT_MODIFIED);
}

#[tokio::test]
async fn cold_packed_block_loading_reduces_object_store_gets_without_changing_geometry() {
    let store = Arc::new(CountStore::default());
    let service = SharedWorlds::new(Store {
        backend: store.clone(),
        prefix: String::new(),
        kind: "test".into(),
        measurements: Arc::new(Default::default()),
        static_tolerances: Default::default(),
    })
    .unwrap();
    let mut d = ergo_world::demo::Demo::new();
    for index in 0..4096 {
        d.add(
            "3003",
            19,
            100. + (index % 64) as f64 * 3.,
            0.,
            100. + (index / 64) as f64 * 3.,
        );
    }
    let mut data = scene(100.);
    data.bricks = d.bricks;
    data.meshes = d.meshes;
    let (head, imported) = seeded(&service, data).await;
    let first = piece(&service, &head.id, &imported.pieces[&1]).await;
    let cells = first.replicas.clone();
    let reader = SharedWorlds::new(service.store.clone()).unwrap();
    let (app, headers) = http(&reader);
    store.gets.store(0, Ordering::SeqCst);
    let (status, packed) = json(
        &app,
        "GET",
        &format!("/api/shared-worlds/{}/view", head.id),
        &headers,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(packed["scene"]["bricks"].as_array().unwrap().len(), 4096);
    let packed_gets = store.gets.load(Ordering::SeqCst);
    let mut tx = service.repo(&head.id).unwrap().begin("main").await.unwrap();
    for coord in cells {
        let mut header: spatial::BlockHeader =
            tx.get(&spatial::directory(coord)).await.unwrap().unwrap();
        header.packed = false;
        tx.put(&spatial::directory(coord), &header).await.unwrap();
    }
    let legacy = tx
        .commit("test", "legacy read fixture", Default::default())
        .await
        .unwrap();
    let reader = SharedWorlds::new(service.store.clone()).unwrap();
    let (app, headers) = http(&reader);
    store.gets.store(0, Ordering::SeqCst);
    let (status, unpacked) = json(
        &app,
        "GET",
        &format!("/api/shared-worlds/{}/view", head.id),
        &headers,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let legacy_gets = store.gets.load(Ordering::SeqCst);
    assert_eq!(packed["scene"], unpacked["scene"]);
    assert_eq!(
        unpacked["revision"],
        serde_json::json!(legacy.snapshot.id())
    );
    println!("4096-brick cold block: {packed_gets} packed GETs vs {legacy_gets} legacy GETs");
    assert!(
        packed_gets * 5 < legacy_gets,
        "Bulk pages must avoid per-brick traversal and payload reads"
    );
}
