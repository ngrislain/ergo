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
async fn unpacked_current_block_backfill_changes_only_the_read_projection() {
    let service = service();
    let (head, imported) = seeded(&service, scene(100.)).await;
    let id = &imported.pieces[&1];
    let original = piece(&service, &head.id, id).await;
    let coord = *original.replicas.iter().next().unwrap();
    let repo = service.repo(&head.id).unwrap();
    let mut tx = repo.begin("main").await.unwrap();
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
        4
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
async fn region_backfill_packs_every_allocated_cell_in_radius_and_is_idempotent() {
    let service = service();
    let mut data = scene(100.);
    let mut demo = ergo_world::demo::Demo::new();
    for offset in [0., 128., 256., 384.] {
        demo.add("3003", 19, 100. + offset, 0., 100.);
    }
    demo.add("3003", 19, 10_000., 0., 100.);
    data.bricks = demo.bricks;
    data.meshes = demo.meshes;
    let (head, imported) = seeded(&service, data).await;
    let repo = service.repo(&head.id).unwrap();
    let mut near = std::collections::BTreeSet::new();
    let mut far = std::collections::BTreeSet::new();
    for index in 1..=5 {
        let piece = piece(&service, &head.id, &imported.pieces[&index]).await;
        if index == 5 {
            far.extend(piece.replicas.iter().copied());
        } else {
            near.extend(piece.replicas.iter().copied());
        }
    }
    let mut tx = repo.begin("main").await.unwrap();
    for coord in near.iter().chain(far.iter()) {
        let mut header: spatial::BlockHeader =
            tx.get(&spatial::directory(*coord)).await.unwrap().unwrap();
        header.packed = false;
        tx.put(&spatial::directory(*coord), &header).await.unwrap();
        tx.remove(&bulk::block_path(*coord)).await.unwrap();
    }
    let legacy = tx
        .commit("test", "legacy region fixture", Default::default())
        .await
        .unwrap();
    let mut original = BTreeMap::new();
    for index in 1..=4 {
        let id = imported.pieces[&index].clone();
        let piece = piece(&service, &head.id, &id).await;
        original.insert(id, piece);
    }
    let report = service.pack_region(&head.id, [1, 0, 0], 2).await.unwrap();
    assert_eq!(report["packed"], serde_json::json!(near.len()));
    assert_eq!(report["already_packed"], serde_json::json!(0));
    let blocks = report["blocks"].as_array().unwrap();
    // Each cell pack is its own storage-only commit; the last block's revision
    // is the head after the whole region backfill.
    let revision: ContentId =
        serde_json::from_value(blocks[blocks.len() - 1]["revision"].clone()).unwrap();
    let after = repo.accepted_snapshot("main", &revision).await.unwrap();
    for coord in &near {
        let values = packed_pieces(&service, &head.id, &revision, *coord).await;
        assert!(!values.is_empty());
        for (id, value) in &values {
            assert_eq!(
                canonical_cbor(value).unwrap(),
                canonical_cbor(&original[id]).unwrap()
            );
        }
    }
    for coord in &far {
        assert!(
            !after
                .get::<spatial::BlockHeader>(&spatial::directory(*coord))
                .await
                .unwrap()
                .unwrap()
                .packed
        );
    }
    assert!(
        !legacy
            .snapshot
            .get::<spatial::BlockHeader>(&spatial::directory(*far.iter().next().unwrap()))
            .await
            .unwrap()
            .unwrap()
            .packed
    );
    let again = service.pack_region(&head.id, [1, 0, 0], 2).await.unwrap();
    assert_eq!(again["packed"], serde_json::json!(0));
    assert_eq!(again["already_packed"], serde_json::json!(near.len()));
}

#[tokio::test]
async fn world_packing_pages_all_cells_preserves_models_and_invalidates_current_merkle_tokens() {
    use axum::{body::Body, http::Request};
    use tower::ServiceExt;
    let service = service();
    let mut data = scene(100.);
    let mut demo = ergo_world::demo::Demo::new();
    demo.add("3003", 19, 100., 0., 100.);
    demo.add("3003", 19, 1_000_000., 0., 100.);
    data.bricks = demo.bricks;
    data.meshes = demo.meshes;
    let (head, imported) = seeded(&service, data).await;
    let repo = service.repo(&head.id).unwrap();
    let mut tx = repo.begin("main").await.unwrap();
    let mut cells = BTreeSet::new();
    for id in imported.pieces.values() {
        cells.extend(piece(&service, &head.id, id).await.replicas);
    }
    // Cross directory-page boundaries and include allocated empty cells far
    // outside any camera radius. The migration must neither omit nor invent one.
    cells.extend((0..70).map(|i| ergo_world::BlockCoord::from([-1_000_000 - i, 0, 0])));
    for coord in &cells {
        tx.put(
            &spatial::directory(*coord),
            &spatial::BlockHeader {
                coord: *coord,
                paged: true,
                packed: false,
            },
        )
        .await
        .unwrap();
        tx.remove(&bulk::block_path(*coord)).await.unwrap();
    }
    let legacy = tx
        .commit("test", "unpacked world fixture", Default::default())
        .await
        .unwrap();
    let old_revision = legacy.snapshot.id().clone();
    let mut piece_ids = BTreeMap::new();
    for id in imported.pieces.values() {
        piece_ids.insert(
            id.clone(),
            legacy.snapshot.value_id(&entity_path(3, id)).await.unwrap(),
        );
    }
    let (app, headers) = http(&service);
    let view_url = format!("/api/shared-worlds/{}/view?format=cbor", head.id);
    let workspace_url = format!(
        "/api/shared-worlds/{}/assemblies/{}/workspace",
        head.id, imported.assembly
    );
    let mut tokens = Vec::new();
    for url in [&view_url, &workspace_url] {
        let mut request = Request::builder().method("HEAD").uri(url);
        for (key, value) in &headers {
            request = request.header(key, value);
        }
        let response = app
            .clone()
            .oneshot(request.body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        tokens.push(response.headers()["etag"].clone());
    }
    let survey = service.pack_world(&head.id, true).await.unwrap();
    assert_eq!(survey["unpacked_before"], serde_json::json!(cells.len()));
    assert_eq!(survey["fully_packed"], false);
    assert_eq!(survey["base_revision"], survey["revision"]);
    let report = service.pack_world(&head.id, false).await.unwrap();
    assert_eq!(report["cells"], serde_json::json!(cells.len()));
    assert_eq!(report["packed"], serde_json::json!(cells.len()));
    assert_eq!(report["fully_packed"], true);
    assert_eq!(report["canonical_subtrees_unchanged"], true);
    assert_ne!(report["root_before"], report["root_after"]);
    let current = repo.checkout("main").await.unwrap();
    for (id, cid) in piece_ids {
        assert_eq!(current.value_id(&entity_path(3, &id)).await.unwrap(), cid);
    }
    for coord in &cells {
        assert!(
            current
                .get::<spatial::BlockHeader>(&spatial::directory(*coord))
                .await
                .unwrap()
                .unwrap()
                .packed
        );
        assert!(
            !legacy
                .snapshot
                .get::<spatial::BlockHeader>(&spatial::directory(*coord))
                .await
                .unwrap()
                .unwrap()
                .packed
        );
    }
    let revision = current.id().clone();
    current.close().await.unwrap();
    for (url, token) in [&view_url, &workspace_url].into_iter().zip(tokens) {
        let mut request = Request::builder()
            .method("HEAD")
            .uri(url)
            .header("If-None-Match", token);
        for (key, value) in &headers {
            request = request.header(key, value);
        }
        let response = app
            .clone()
            .oneshot(request.body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(
            response.status(),
            StatusCode::OK,
            "Old local tokens must miss after the storage Merkle root changes"
        );
        assert_eq!(response.headers()["x-ergo-revision"], revision.as_str());
    }
    let again = service.pack_world(&head.id, false).await.unwrap();
    assert_eq!(again["packed"], 0);
    assert_eq!(again["already_packed"], serde_json::json!(cells.len()));
    assert_eq!(again["base_revision"], again["revision"]);
    assert!(
        service
            .pack_block_checked(&head.id, [0; 3], Some(&old_revision))
            .await
            .is_err()
    );
    let verified = service.pack_world(&head.id, true).await.unwrap();
    assert_eq!(verified["fully_packed"], true);
    assert_eq!(verified["revision"], serde_json::json!(revision));
    legacy.snapshot.close().await.unwrap();
}

#[tokio::test]
async fn brick_specifications_use_versioned_http_revalidation() {
    use axum::{body::Body, http::Request};
    use tower::ServiceExt;
    // The test service store holds the reviewed-tier synthesized objects.
    let service = service();
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
            (index % 64) as f64 * 2.,
            0.,
            (index / 64) as f64 * 2.,
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
