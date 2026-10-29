use super::*;
use ergo_world::{SCHEMA_VERSION, WorldExport};
use std::sync::Arc;

#[path = "tests_bulk.rs"]
mod packed;

fn service() -> SharedWorlds {
    SharedWorlds::new(Store {
        backend: Arc::new(object_store::memory::InMemory::new()),
        prefix: String::new(),
        kind: "test".into(),
        measurements: Arc::new(Default::default()),
        static_tolerances: Default::default(),
    })
    .unwrap()
}

#[tokio::test]
async fn local_sandbox_bootstraps_one_persistent_namespace() {
    let mut first = service();
    first.bootstrap_local(Default::default()).await.unwrap();
    let id = first.common_world.clone().unwrap();
    assert_eq!(first.repo(&id).unwrap().prefix(), "world");
    let mut second = SharedWorlds::new(first.store.clone()).unwrap();
    second.bootstrap_local(Default::default()).await.unwrap();
    assert_eq!(second.common_world.as_deref(), Some(id.as_str()));
    assert!(
        second
            .create(
                "development",
                NewWorld {
                    name: "No fork".into(),
                    geo: Default::default(),
                    authenticated_editing: true
                }
            )
            .await
            .is_err()
    );
}
fn scene(x: f64) -> WorldExport {
    let mut d = ergo_world::demo::Demo::new();
    d.add("3003", 4, x, 0.0, 100.0);
    d.add("3003", 1, x + 10_300.0, 0.0, 100.0);
    WorldExport {
        schema: SCHEMA_VERSION,
        name: "Assembly".into(),
        geo: Default::default(),
        bricks: d.bricks,
        meshes: d.meshes,
        joints: Vec::new(),
        fixed_bricks: Vec::new(),
        groups: Vec::new(),
        model: None,
        ground: Vec::new(),
    }
}

#[tokio::test]
async fn commit_integrity_rejects_off_grid_and_non_isometric_geometry_without_advancing_head() {
    let service = service();
    let (head, imported) = seeded(&service, scene(100.)).await;
    for (name, matrix) in [
        (
            "scale",
            glam::DMat3::from_diagonal(glam::DVec3::new(1.1, 1., 1.)),
        ),
        (
            "reflection",
            glam::DMat3::from_diagonal(glam::DVec3::new(-1., 1., 1.)),
        ),
        (
            "shear",
            glam::DMat3::from_cols(
                glam::DVec3::X,
                glam::DVec3::new(0.1, 1., 0.),
                glam::DVec3::Z,
            ),
        ),
    ] {
        let global = &imported.pieces[&1];
        let original = piece(&service, &head.id, global).await;
        let mut geometry = original.geometry.clone();
        geometry.transform = matrix;
        let request = CommitRequest {
            base: head.revision.clone(),
            pieces: vec![PieceEdit {
                id: global.clone(),
                geometry: Some(geometry),
                mesh: None,
            }],
            rules: vec![],
            joints: vec![],
            message: name.into(),
        };
        assert!(
            service.prepare(&head.id, "alice", request).await.is_err(),
            "{name} accepted"
        );
    }
    let global = &imported.pieces[&1];
    let mut request = edit(&service, &head, global, glam::DVec3::X * 0.2).await;
    request.message = "off-grid grounded pose".into();
    let error = service
        .prepare(&head.id, "alice", request)
        .await
        .err()
        .unwrap();
    assert!(error.message.contains("stud grid"), "{error:?}");
    let mut candidate = scene(200.2);
    assert!(
        service
            .import(
                &head.id,
                "alice",
                ImportRequest {
                    base: head.revision.clone(),
                    scene: candidate.clone()
                }
            )
            .await
            .is_err()
    );
    let (app, headers) = http(&service);
    let (status, _) = json(
        &app,
        "PUT",
        &format!(
            "/api/shared-worlds/{}/assemblies/{}/workspace",
            head.id, imported.assembly
        ),
        &headers,
        Some(serde_json::json!({"base":head.revision,"scene":candidate})),
    )
    .await;
    assert!(!status.is_success());
    assert_eq!(
        service
            .head(
                &head.id,
                &Principal {
                    id: Some("alice".into())
                },
                None
            )
            .await
            .unwrap()
            .revision,
        head.revision
    );
    ergo_world::placement::snap_scene(&mut candidate).unwrap();
    service
        .import(
            &head.id,
            "alice",
            ImportRequest {
                base: head.revision,
                scene: candidate,
            },
        )
        .await
        .unwrap();
}

#[tokio::test]
async fn hierarchy_scale_cancellation_cannot_bypass_publication_isometry_checks() {
    use ergo_world::assembly::{Model, Reference, Transform};
    let service = service();
    let (head, _) = seeded(&service, scene(100.)).await;
    let mut candidate = scene(200.);
    let original = Model::from_bricks(&candidate.bricks);
    let mut model = original.clone();
    let old_root = model.root.clone();
    model.subparts.insert(
        "half".into(),
        vec![Reference {
            file: old_root,
            color: 16,
            matrix: Transform::from_pose(
                glam::DMat3::from_diagonal(glam::DVec3::splat(0.5)),
                glam::DVec3::ZERO,
            ),
        }],
    );
    model.subparts.insert(
        "double".into(),
        vec![Reference {
            file: "half".into(),
            color: 16,
            matrix: Transform::from_pose(
                glam::DMat3::from_diagonal(glam::DVec3::splat(2.)),
                glam::DVec3::ZERO,
            ),
        }],
    );
    model.root = "double".into();
    assert!(model.matches(&candidate.bricks));
    candidate.model = Some(model);
    assert!(
        service
            .import(
                &head.id,
                "alice",
                ImportRequest {
                    base: head.revision.clone(),
                    scene: candidate
                }
            )
            .await
            .is_err()
    );
    assert_eq!(
        service
            .head(
                &head.id,
                &Principal {
                    id: Some("alice".into())
                },
                None
            )
            .await
            .unwrap()
            .revision,
        head.revision
    );
}

#[tokio::test]
async fn accepted_changes_use_server_profile_name_and_retain_the_source_model() {
    let service = service();
    service
        .store
        .put_user(&crate::auth::User {
            id: "alice".into(),
            name: "Alice Builder".into(),
            email: "alice@example.test".into(),
            picture: None,
            created_at: 1,
            last_login: 1,
        })
        .await
        .unwrap();
    let (head, imported) = seeded(&service, scene(10.)).await;
    assert_eq!(imported.accepted.audit.actor, "alice");
    assert_eq!(
        imported.accepted.audit.actor_name.as_deref(),
        Some("Alice Builder")
    );
    let repo = service.repo(&head.id).unwrap();
    let birth: Birth = repo.blob(&imported.pieces[&1]).await.unwrap();
    assert_eq!(birth.created_by, "alice");
    assert_eq!(birth.created_by_name.as_deref(), Some("Alice Builder"));
    let snapshot = repo.checkout("main").await.unwrap();
    let header: super::paged::RulesHeader = snapshot
        .get(&super::paged::rule_path(&imported.assembly, 0))
        .await
        .unwrap()
        .unwrap();
    let source: ergo_world::assembly::Model =
        repo.blob(&header.source_model.unwrap()).await.unwrap();
    assert_eq!(source.references().unwrap().len(), 2);
    snapshot.close().await.unwrap();
}

#[tokio::test]
async fn construction_metadata_survives_replacement_and_downloads_export_the_whole_assembly() {
    use axum::{
        body::{Body, to_bytes},
        http::Request,
    };
    use tower::ServiceExt;
    let mut service = service();
    service
        .store
        .put_user(&crate::auth::User {
            id: "alice".into(),
            name: "Alice Builder".into(),
            email: "alice@example.test".into(),
            picture: None,
            created_at: 1,
            last_login: 1,
        })
        .await
        .unwrap();
    let (head, imported) = seeded(&service, scene(10.)).await;
    let (app, headers) = http(&service);
    let path = format!("/api/shared-worlds/{}", head.id);
    let (_, listing) = json(&app, "GET", &format!("{path}/assemblies"), &headers, None).await;
    let first = &listing["items"][0];
    assert_eq!(first["created_by"], "alice");
    assert_eq!(first["created_by_name"], "Alice Builder");
    assert_eq!(first["piece_count"], 2);
    assert!(first["created_at"].as_u64().unwrap() > 0);
    let mut renamed = scene(10.);
    renamed.name = "Renamed construction".into();
    let replaced = service
        .replace_assembly(
            &head.id,
            "alice",
            &imported.assembly,
            authoring::ReplaceRequest {
                base: head.revision,
                scene: renamed,
            },
        )
        .await
        .unwrap();
    let (app, headers) = http(&service);
    let (_, listing) = json(&app, "GET", &format!("{path}/assemblies"), &headers, None).await;
    assert_eq!(listing["items"][0]["creation"], first["creation"]);
    service.limits.pieces = 1;
    let (app, headers) = http(&service);
    let (status, _) = json(
        &app,
        "GET",
        &format!("{path}/assemblies/{}/workspace", imported.assembly),
        &headers,
        None,
    )
    .await;
    assert_eq!(
        status,
        StatusCode::PAYLOAD_TOO_LARGE,
        "Authoring must retain its own count limit"
    );
    for format in ["ldraw", "ergo"] {
        let mut request = Request::get(format!(
            "{path}/assemblies/{}/export?format={format}&revision={}",
            imported.assembly, replaced.accepted.revision
        ))
        .body(Body::empty())
        .unwrap();
        request.headers_mut().extend(headers.clone());
        let response = app.clone().oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            response.headers()["x-ergo-revision"],
            replaced.accepted.revision.to_string()
        );
        assert!(
            response.headers()["content-disposition"]
                .to_str()
                .unwrap()
                .contains("attachment")
        );
        let bytes = to_bytes(response.into_body(), 16 * 1024 * 1024)
            .await
            .unwrap();
        if format == "ldraw" {
            let text = std::str::from_utf8(&bytes).unwrap();
            let document = ergo_world::ldraw::Document::parse(text, "download.mpd").unwrap();
            let refs = ergo_world::assembly::Model::from_document(&document)
                .unwrap()
                .references()
                .unwrap();
            assert_eq!(
                refs.len(),
                2,
                "Export must include the member over ten cells away"
            );
            assert!(
                text.contains("0 FILE 3003.dat"),
                "Export must embed its part geometry"
            );
        } else {
            assert_eq!(ergo_world::storage::decode(&bytes).unwrap().bricks.len(), 2);
        }
    }
}

#[tokio::test]
async fn discovery_metadata_revalidates_before_storage_walks_and_rechecks_permissions() {
    use axum::{body::Body, http::Request};
    use tower::ServiceExt;
    let service = service();
    let (head, imported) = seeded(&service, scene(100.)).await;
    let (app, headers) = http(&service);
    let paths = [
        format!("/api/shared-worlds/{}/assemblies", head.id),
        format!(
            "/api/shared-worlds/{}/assemblies/{}/location",
            head.id, imported.assembly
        ),
    ];
    let mut versions = Vec::new();
    for path in &paths {
        let mut request = Request::get(path).body(Body::empty()).unwrap();
        request.headers_mut().extend(headers.clone());
        let first = app.clone().oneshot(request).await.unwrap();
        assert_eq!(first.status(), StatusCode::OK);
        assert_eq!(first.headers()["cache-control"], "private, no-cache");
        let etag = first.headers()["etag"].clone();
        versions.push(etag.clone());
        let mut request = Request::get(path)
            .header("if-none-match", etag.clone())
            .body(Body::empty())
            .unwrap();
        request.headers_mut().extend(headers.clone());
        let fresh = app.clone().oneshot(request).await.unwrap();
        assert!([StatusCode::NOT_MODIFIED, StatusCode::OK].contains(&fresh.status()));
        if fresh.status() == StatusCode::NOT_MODIFIED {
            assert_eq!(fresh.headers()["x-ergo-metadata-cache"], "revalidated");
        }
        let denied = app
            .clone()
            .oneshot(
                Request::get(path)
                    .header("if-none-match", etag)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert!([StatusCode::UNAUTHORIZED, StatusCode::FORBIDDEN].contains(&denied.status()));
    }
    let mut changed = scene(120.);
    changed.name = "Changed metadata".into();
    service
        .replace_assembly(
            &head.id,
            "alice",
            &imported.assembly,
            authoring::ReplaceRequest {
                base: head.revision,
                scene: changed,
            },
        )
        .await
        .unwrap();
    for (path, etag) in paths.iter().zip(versions) {
        let mut request = Request::get(path)
            .header("if-none-match", etag.clone())
            .body(Body::empty())
            .unwrap();
        request.headers_mut().extend(headers.clone());
        let changed = app.clone().oneshot(request).await.unwrap();
        assert_eq!(changed.status(), StatusCode::OK);
        assert_ne!(changed.headers()["etag"], etag);
    }
}

#[tokio::test]
async fn navigation_without_a_previous_hint_finds_the_canonical_warm_region() {
    use axum::{body::Body, http::Request};
    use tower::ServiceExt;
    let service = service();
    let (head, _) = seeded(&service, scene(100.)).await;
    let (app, headers) = http(&service);
    let path = format!("/api/shared-worlds/{}/view?x=0&y=-1&z=0", head.id);
    let (status, packet) = json(&app, "GET", &path, &headers, None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(packet["scene"]["bricks"].as_array().unwrap().len(), 1);
    let mut request = Request::get(format!("/api/shared-worlds/{}/view?x=0&y=0&z=0", head.id))
        .body(Body::empty())
        .unwrap();
    request.headers_mut().extend(headers);
    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.headers()["x-ergo-read-cache"], "neighbor");
}

#[tokio::test]
async fn construction_download_preserves_per_instance_geometry_variants() {
    use axum::{
        body::{Body, to_bytes},
        http::Request,
    };
    use tower::ServiceExt;
    let service = service();
    let mut initial = scene(10.);
    for index in 0..2 {
        let mut repeated = initial.bricks[1].clone();
        repeated.id = index + 3;
        repeated.translated(glam::DVec3::X * f64::from((index + 1) * 10));
        initial.bricks.push(repeated);
    }
    let (head, imported) = seeded(&service, initial).await;
    let original = piece(&service, &head.id, &imported.pieces[&1]).await;
    let third = piece(&service, &head.id, &imported.pieces[&3]).await;
    let mut alternate = scene(50.);
    alternate.bricks.truncate(1);
    for mesh in alternate.meshes.values_mut() {
        mesh.quality = "fixture geometry variant".into();
        for vertex in mesh.vertices.chunks_exact_mut(9) {
            vertex[0] *= 0.99;
        }
    }
    let alternate = service
        .import(
            &head.id,
            "alice",
            ImportRequest {
                base: head.revision,
                scene: alternate,
            },
        )
        .await
        .unwrap();
    let variant = piece(&service, &head.id, &alternate.pieces[&1]).await.mesh;
    let accepted = service
        .prepare(
            &head.id,
            "alice",
            CommitRequest {
                base: alternate.accepted.revision,
                pieces: vec![
                    PieceEdit {
                        id: original.id,
                        geometry: Some(original.geometry),
                        mesh: Some(variant.clone()),
                    },
                    PieceEdit {
                        id: third.id,
                        geometry: Some(third.geometry),
                        mesh: Some(variant),
                    },
                ],
                rules: vec![],
                joints: vec![],
                message: "Use a second geometry variant".into(),
            },
        )
        .await
        .unwrap()
        .publish()
        .await
        .unwrap();
    let (app, headers) = http(&service);
    let mut request = Request::get(format!(
        "/api/shared-worlds/{}/assemblies/{}/export?format=ldraw&revision={}",
        head.id, imported.assembly, accepted.revision
    ))
    .body(Body::empty())
    .unwrap();
    request.headers_mut().extend(headers);
    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let bytes = to_bytes(response.into_body(), 16 * 1024 * 1024)
        .await
        .unwrap();
    let text = std::str::from_utf8(&bytes).unwrap();
    let document = ergo_world::ldraw::Document::parse(text, "variants.mpd").unwrap();
    let refs = ergo_world::assembly::Model::from_document(&document)
        .unwrap()
        .references()
        .unwrap();
    assert_eq!(refs.len(), 4);
    assert_ne!(refs[0].name, refs[1].name);
    assert_eq!(refs[0].name, refs[2].name);
    assert_eq!(refs[1].name, refs[3].name);
    assert!(text.contains("export geometry variant of 3003.dat"));
}
async fn seeded(service: &SharedWorlds, initial: WorldExport) -> (Head, Imported) {
    let head = service
        .create(
            "alice",
            NewWorld {
                authenticated_editing: false,
                name: "Huge world".into(),
                geo: Default::default(),
            },
        )
        .await
        .unwrap();
    let imported = service
        .import(
            &head.id,
            "alice",
            ImportRequest {
                base: head.revision,
                scene: initial,
            },
        )
        .await
        .unwrap();
    let head = service
        .head(
            &head.id,
            &Principal {
                id: Some("alice".into()),
            },
            None,
        )
        .await
        .unwrap();
    (head, imported)
}

#[tokio::test]
async fn upright_gravity_support_does_not_authorize_uncalibrated_hanging_bonds() {
    let service = service();
    let mut d = ergo_world::demo::Demo::new();
    for level in 0..9 {
        d.add("3003", 19, 0., f64::from(level), 0.);
    }
    let mut initial = scene(0.);
    initial.bricks = d.bricks;
    initial.meshes = d.meshes;
    let (head, accepted) = seeded(&service, initial).await;
    assert_eq!(
        accepted
            .accepted
            .audit
            .validation
            .as_ref()
            .unwrap()
            .joint_strength_required,
        Some(false)
    );
    let mut hung = ergo_world::demo::Demo::new();
    hung.add("3003", 19, 100., 0., 0.);
    hung.add("3003", 19, 102., 0.5, 0.);
    let mut candidate = scene(0.);
    candidate.bricks = hung.bricks;
    candidate.meshes = hung.meshes;
    candidate.joints = vec![ergo_world::JointSpec {
        a: 1,
        b: 2,
        anchor: glam::DVec3::new(102., 0.75, 1.),
        kind: ergo_world::JointKind::Fixed,
        break_force: Some(1e20),
        break_torque: Some(1e20),
        strength_source: Some("untrusted".into()),
    }];
    let error = service
        .import(
            &head.id,
            "alice",
            ImportRequest {
                base: accepted.accepted.revision,
                scene: candidate,
            },
        )
        .await
        .err()
        .expect("Hanging joint must require measured capacity");
    assert!(
        error
            .to_string()
            .contains("Missing measured connector capacity"),
        "{error}"
    );
}

#[tokio::test]
async fn god_mode_removes_the_ordinary_authoring_piece_cap() {
    let mut service = service();
    service.limits.pieces = 1;
    let head = service
        .create(
            "alice",
            NewWorld {
                name: "God mode".into(),
                geo: Default::default(),
                authenticated_editing: true,
            },
        )
        .await
        .unwrap();
    let request = ImportRequest {
        base: head.revision.clone(),
        scene: scene(10.),
    };
    assert!(service.import(&head.id, "alice", request).await.is_err());
    service.pacing.set_unlimited("alice", true).await.unwrap();
    let imported = service
        .import(
            &head.id,
            "alice",
            ImportRequest {
                base: head.revision,
                scene: scene(10.),
            },
        )
        .await
        .unwrap();
    assert_eq!(imported.pieces.len(), 2);
    let loaded = service
        .authoring_scene(
            &head.id,
            "alice",
            &imported.assembly,
            &imported.accepted.revision,
        )
        .await
        .unwrap();
    assert_eq!(loaded["scene"]["bricks"].as_array().unwrap().len(), 2);
    service.pacing.set_unlimited("alice", false).await.unwrap();
    assert!(
        service
            .authoring_scene(
                &head.id,
                "alice",
                &imported.assembly,
                &imported.accepted.revision
            )
            .await
            .is_err()
    );
}

#[tokio::test]
async fn dense_neighbor_queries_charge_immutable_spatial_rows_once() {
    let mut service = service();
    let mut d = ergo_world::demo::Demo::new();
    for x in 0..8 {
        for y in 0..8 {
            d.add(
                "3068b",
                19,
                f64::from(x) * 2. + 1.,
                0.,
                f64::from(y) * 2. + 1.,
            );
        }
    }
    let mut initial = scene(0.);
    initial.bricks = d.bricks;
    initial.meshes = d.meshes;
    let bounds = ergo_world::bounds(&initial.bricks).unwrap().expanded(0.1);
    let (head, _) = seeded(&service, initial).await;
    service.limits.bytes = 128 * 1024;
    let mut ws = service
        .working(&head.id, "alice", &head.revision)
        .await
        .unwrap();
    for _ in 0..128 {
        assert_eq!(ws.neighbors(bounds).await.unwrap().len(), 64);
    }
}

#[tokio::test]
async fn bounded_assembly_replacement_preserves_ids_and_is_atomic() {
    let mut service = service();
    let (head, imported) = seeded(&service, scene(10.)).await;
    service.common_world = Some(head.id.clone());
    let mut candidate = scene(10.);
    candidate.bricks[0].translated(glam::DVec3::X * 3.);
    let mut extra = ergo_world::demo::Demo::new();
    extra.add("3003", 14, 50., 0., 100.);
    extra.bricks[0].id = 3;
    candidate.bricks.push(extra.bricks.remove(0));
    let next = service
        .replace_assembly(
            &head.id,
            "alice",
            &imported.assembly,
            authoring::ReplaceRequest {
                base: imported.accepted.revision.clone(),
                scene: candidate.clone(),
            },
        )
        .await
        .unwrap();
    assert_eq!(next.assembly, imported.assembly);
    assert_eq!(next.pieces[&1], imported.pieces[&1]);
    assert_eq!(next.pieces[&2], imported.pieces[&2]);
    assert_eq!(next.pieces.len(), 3);
    let loaded = service
        .authoring_scene(&head.id, "alice", &next.assembly, &next.accepted.revision)
        .await
        .unwrap();
    let loaded: WorldExport = serde_json::from_value(loaded["scene"].clone()).unwrap();
    assert_eq!(loaded.bricks.len(), 3);
    assert_eq!(loaded.bricks[0].position, candidate.bricks[0].position);
    let accepted = next.accepted.revision.clone();
    candidate.bricks[0].translated(glam::DVec3::Y * 20.);
    assert!(
        service
            .replace_assembly(
                &head.id,
                "alice",
                &next.assembly,
                authoring::ReplaceRequest {
                    base: accepted.clone(),
                    scene: candidate
                }
            )
            .await
            .is_err()
    );
    assert_eq!(
        service
            .head(
                &head.id,
                &Principal {
                    id: Some("alice".into())
                },
                None
            )
            .await
            .unwrap()
            .revision,
        accepted
    );
    let mut cleared = loaded;
    cleared.bricks.clear();
    let deleted = service
        .replace_assembly(
            &head.id,
            "alice",
            &next.assembly,
            authoring::ReplaceRequest {
                base: accepted,
                scene: cleared,
            },
        )
        .await
        .unwrap();
    assert!(deleted.pieces.is_empty());
    let redirected = service
        .create(
            "alice",
            NewWorld {
                name: "ignored private world".into(),
                geo: Default::default(),
                authenticated_editing: false,
            },
        )
        .await
        .err()
        .unwrap();
    assert_eq!(redirected.status, StatusCode::CONFLICT);
}

#[tokio::test]
async fn batch_replacement_swaps_assemblies_and_adds_pieces_in_one_validated_commit() {
    let service = service();
    let (head, first) = seeded(&service, scene(0.)).await;
    let second = service
        .import(
            &head.id,
            "alice",
            ImportRequest {
                base: head.revision,
                scene: scene(10.),
            },
        )
        .await
        .unwrap();
    let base = second.accepted.revision.clone();
    // Either individual swap overlaps the other accepted construction.
    assert!(
        service
            .replace_assembly(
                &head.id,
                "alice",
                &first.assembly,
                authoring::ReplaceRequest {
                    base: base.clone(),
                    scene: scene(10.)
                }
            )
            .await
            .is_err()
    );
    let (app, headers) = http(&service);
    let mut larger = scene(10.);
    let mut extra = ergo_world::demo::Demo::new();
    extra.add("3003", 14, 50., 0., 100.);
    extra.bricks[0].id = 3;
    larger.bricks.extend(extra.bricks);
    let path = format!("/api/shared-worlds/{}/assemblies/workspaces", head.id);
    // An invalid final member must not publish even the valid first member.
    let (status, _) = json(
        &app,
        "PUT",
        &path,
        &headers,
        Some(serde_json::json!({
            "base":base,"assemblies":[
                {"assembly_id":first.assembly,"scene":larger},
                {"assembly_id":second.assembly,"scene":scene(0.2)}
            ]
        })),
    )
    .await;
    assert!(!status.is_success());
    assert_eq!(
        service
            .head(
                &head.id,
                &Principal {
                    id: Some("alice".into())
                },
                None
            )
            .await
            .unwrap()
            .revision,
        base
    );
    let (status, response) = json(
        &app,
        "PUT",
        &path,
        &headers,
        Some(serde_json::json!({
            "base":base,"assemblies":[
                {"assembly_id":first.assembly,"scene":larger},
                {"assembly_id":second.assembly,"scene":scene(0.)}
            ]
        })),
    )
    .await;
    assert!(status.is_success(), "{response:?}");
    let accepted: ContentId =
        serde_json::from_value(response["accepted"]["revision"].clone()).unwrap();
    assert_ne!(accepted, base);
    assert_eq!(
        response["assemblies"][0]["pieces"]["1"],
        serde_json::json!(first.pieces[&1])
    );
    assert_eq!(
        response["assemblies"][1]["pieces"]["1"],
        serde_json::json!(second.pieces[&1])
    );
    assert_eq!(
        response["assemblies"][0]["pieces"]
            .as_object()
            .unwrap()
            .len(),
        3
    );
    for (assembly, expected) in [(&first.assembly, larger), (&second.assembly, scene(0.))] {
        let workspace = service
            .authoring_scene(&head.id, "alice", assembly, &accepted)
            .await
            .unwrap();
        let actual: WorldExport = serde_json::from_value(workspace["scene"].clone()).unwrap();
        assert_eq!(actual.bricks.len(), expected.bricks.len());
        for (actual, expected) in actual.bricks.iter().zip(&expected.bricks) {
            assert_eq!(actual.position, expected.position);
        }
        assert!(actual.model.as_ref().unwrap().matches(&actual.bricks));
    }
    // The batch is one history transition, with one combined audit.
    assert_eq!(
        response["accepted"]["audit"]["base"],
        serde_json::json!(base)
    );
    assert_eq!(
        response["accepted"]["audit"]["changed_pieces"]
            .as_array()
            .unwrap()
            .len(),
        5
    );
}

#[tokio::test]
async fn batch_replacement_rejects_duplicate_identities_and_aggregate_budgets() {
    let mut service = service();
    let (head, first) = seeded(&service, scene(0.)).await;
    let second = service
        .import(
            &head.id,
            "alice",
            ImportRequest {
                base: head.revision,
                scene: scene(10.),
            },
        )
        .await
        .unwrap();
    let base = second.accepted.revision;
    let replacement = || authoring::AssemblyReplacement {
        assembly_id: first.assembly.clone(),
        scene: scene(20.),
    };
    let duplicate = service
        .replace_assemblies(
            &head.id,
            "alice",
            authoring::BatchReplaceRequest {
                base: base.clone(),
                assemblies: vec![replacement(), replacement()],
            },
        )
        .await
        .err()
        .unwrap();
    assert!(duplicate.message.contains("Duplicate assembly"));
    service.limits.pieces = 3;
    assert!(
        service
            .replace_assemblies(
                &head.id,
                "alice",
                authoring::BatchReplaceRequest {
                    base: base.clone(),
                    assemblies: vec![
                        replacement(),
                        authoring::AssemblyReplacement {
                            assembly_id: second.assembly,
                            scene: scene(30.)
                        }
                    ],
                }
            )
            .await
            .is_err()
    );
    assert_eq!(
        service
            .head(
                &head.id,
                &Principal {
                    id: Some("alice".into())
                },
                None
            )
            .await
            .unwrap()
            .revision,
        base
    );
}

#[tokio::test]
async fn browser_views_are_authenticated_bounded_and_preserve_mesh_variants() {
    let mut service = service();
    let (head, _) = seeded(&service, scene(100.)).await;
    let mut alternate = scene(200.);
    alternate.bricks.truncate(1);
    alternate.bricks[0].translated(glam::DVec3::new(10., 0., 10.));
    for mesh in alternate.meshes.values_mut() {
        for vertex in mesh.vertices.chunks_exact_mut(9) {
            vertex[6] = 0.2;
            vertex[7] = 0.7;
            vertex[8] = 0.1;
        }
    }
    service
        .import(
            &head.id,
            "alice",
            ImportRequest {
                base: head.revision,
                scene: alternate,
            },
        )
        .await
        .unwrap();
    let (app, alice) = http(&service);
    let (_, bob) = crate::auth::Auth::test_session("bob");
    let url = format!("/api/shared-worlds/{}/view", head.id);
    let (status, view) = json(&app, "GET", &url, &alice, None).await;
    assert_eq!(status, StatusCode::OK, "{view}");
    assert_eq!(view["view_kind"], "bounded_read_only_region");
    // The distant original piece is outside the requested 3x3x3 block region.
    assert_eq!(view["scene"]["bricks"].as_array().unwrap().len(), 2);
    assert_eq!(view["scene"]["meshes"].as_object().unwrap().len(), 2);
    assert!(view["scene"]["joints"].as_array().unwrap().is_empty());
    assert_eq!(
        json(&app, "GET", &url, &bob, None).await.0,
        StatusCode::FORBIDDEN
    );
    service.view_limits.pieces = 1;
    let (limited, alice) = http(&service);
    assert_eq!(
        json(&limited, "GET", &url, &alice, None).await.0,
        StatusCode::PAYLOAD_TOO_LARGE
    );
}

#[tokio::test]
async fn dense_browser_regions_exceed_authoring_caps_without_truncation_or_cache_budget_bypass() {
    let mut service = service();
    service.pacing.set_unlimited("alice", true).await.unwrap();
    let mut demo = ergo_world::demo::Demo::new();
    for index in 0..5001 {
        demo.add(
            "3068b",
            19,
            (index % 100) as f64 * 3.,
            0.,
            (index / 100) as f64 * 3.,
        );
    }
    let mut dense = scene(0.);
    dense.bricks = demo.bricks;
    dense.meshes = demo.meshes;
    let (head, imported) = seeded(&service, dense).await;
    service.pacing.set_unlimited("alice", false).await.unwrap();
    assert_eq!(service.limits.pieces, 5000);
    assert!(
        service
            .authoring_scene(&head.id, "alice", &imported.assembly, &head.revision)
            .await
            .is_err()
    );
    let (app, alice) = http(&service);
    let url = format!("/api/shared-worlds/{}/view", head.id);
    let (status, packet) = json(&app, "GET", &url, &alice, None).await;
    assert_eq!(status, StatusCode::OK, "{packet}");
    assert_eq!(packet["scene"]["bricks"].as_array().unwrap().len(), 5001);
    assert!(
        packet["fully_resident_assemblies"]
            .as_array()
            .unwrap()
            .contains(&serde_json::json!(imported.assembly))
    );
    // The same cache is shared by these handles. Lower limits must neither hit
    // an oversized packet nor silently serve only its first 5,000 pieces.
    service.view_limits.pieces = 5000;
    let (limited, alice) = http(&service);
    assert_eq!(
        json(&limited, "GET", &url, &alice, None).await.0,
        StatusCode::PAYLOAD_TOO_LARGE
    );
    service.view_limits.pieces = 5001;
    service.view_limits.bytes = 1;
    let (limited, alice) = http(&service);
    assert_eq!(
        json(&limited, "GET", &url, &alice, None).await.0,
        StatusCode::PAYLOAD_TOO_LARGE
    );
    assert_eq!(
        service
            .repo(&head.id)
            .unwrap()
            .checkout("main")
            .await
            .unwrap()
            .id(),
        &head.revision
    );
}

#[tokio::test]
async fn compressed_cbor_views_cache_revalidate_and_invalidate_on_appearance_edits() {
    use axum::{
        body::{Body, to_bytes},
        http::Request,
    };
    use std::io::Read;
    use tower::ServiceExt;
    let service = service();
    let (head, imported) = seeded(&service, scene(100.)).await;
    let (app, alice) = http(&service);
    let url = format!("/api/shared-worlds/{}/view?format=cbor", head.id);
    let mut request = Request::get(&url)
        .header("accept-encoding", "gzip")
        .body(Body::empty())
        .unwrap();
    request.headers_mut().extend(alice.clone());
    let first = app.clone().oneshot(request).await.unwrap();
    assert_eq!(first.status(), StatusCode::OK);
    assert_eq!(first.headers()["content-type"], "application/cbor");
    assert_eq!(first.headers()["content-encoding"], "gzip");
    assert_eq!(first.headers()["x-ergo-read-cache"], "miss");
    let etag = first.headers()["etag"].clone();
    let bytes = to_bytes(first.into_body(), 16 * 1024 * 1024).await.unwrap();
    let mut raw = Vec::new();
    flate2::read::GzDecoder::new(bytes.as_ref())
        .read_to_end(&mut raw)
        .unwrap();
    let packet: view::ViewPacket = ciborium::from_reader(raw.as_slice()).unwrap();
    assert_eq!(packet.scene.bricks.len(), 1);
    assert_eq!(packet.revision, head.revision);
    let mut request = Request::get(&url)
        .header("accept-encoding", "gzip;q=0, *;q=1")
        .body(Body::empty())
        .unwrap();
    request.headers_mut().extend(alice.clone());
    let cached = app.clone().oneshot(request).await.unwrap();
    assert_eq!(cached.headers()["x-ergo-read-cache"], "memory");
    assert!(!cached.headers().contains_key("content-encoding"));
    let bytes = to_bytes(cached.into_body(), 16 * 1024 * 1024)
        .await
        .unwrap();
    let decoded: view::ViewPacket = ciborium::from_reader(bytes.as_ref()).unwrap();
    assert_eq!(
        serde_json::to_value(decoded.scene).unwrap(),
        serde_json::to_value(packet.scene).unwrap()
    );
    let mut request = Request::get(&url)
        .header("if-none-match", etag.clone())
        .body(Body::empty())
        .unwrap();
    request.headers_mut().extend(alice.clone());
    assert_eq!(
        app.clone().oneshot(request).await.unwrap().status(),
        StatusCode::NOT_MODIFIED
    );
    let (_, bob) = crate::auth::Auth::test_session("bob");
    let mut request = Request::get(&url)
        .header("if-none-match", etag)
        .body(Body::empty())
        .unwrap();
    request.headers_mut().extend(bob);
    assert_eq!(
        app.clone().oneshot(request).await.unwrap().status(),
        StatusCode::FORBIDDEN,
        "Cache/304 must not bypass current authorization"
    );
    let old = piece(&service, &head.id, &imported.pieces[&1]).await;
    let mut geometry = old.geometry;
    geometry.color = 14;
    geometry.rgb = Some(ergo_world::color_rgb(14));
    let accepted = service
        .prepare(
            &head.id,
            "alice",
            CommitRequest {
                base: head.revision.clone(),
                pieces: vec![PieceEdit {
                    id: old.id,
                    geometry: Some(geometry),
                    mesh: None,
                }],
                rules: vec![],
                joints: vec![],
                message: "Color-only change".into(),
            },
        )
        .await
        .unwrap()
        .publish()
        .await
        .unwrap();
    let (status, current) = json(
        &app,
        "GET",
        &format!("/api/shared-worlds/{}/view", head.id),
        &alice,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(current["revision"], serde_json::json!(accepted.revision));
    assert_eq!(current["scene"]["bricks"][0]["color"], 14);
    let (_, historical) = json(
        &app,
        "GET",
        &format!(
            "/api/shared-worlds/{}/view?revision={}",
            head.id, head.revision
        ),
        &alice,
        None,
    )
    .await;
    assert_eq!(
        historical["scene"]["bricks"][0]["color"], 4,
        "Accepted historical cache must retain its actual version"
    );
}

#[tokio::test]
async fn neighboring_views_reuse_only_proven_identical_geometry_and_persist_pinned_current_reads() {
    use axum::{
        body::{Body, to_bytes},
        http::Request,
    };
    use tower::ServiceExt;
    let service = service();
    let mut three = scene(100.);
    three.bricks.truncate(1);
    for (id, x) in [(2, 1900.), (3, 2200.)] {
        let mut brick = scene(x).bricks.remove(0);
        brick.id = id;
        three.bricks.push(brick);
    }
    let (head, _) = seeded(&service, three).await;
    assert_eq!(
        service.warm_view(&head.id, [0; 3]).await.unwrap()["pieces"],
        2
    );
    let (app, headers) = http(&service);
    for (coord, expected, source) in [
        ([0, -1, 0], 2, "neighbor"),
        ([1, 0, 0], 3, "miss"),
        ([-1, 0, 0], 1, "miss"),
    ] {
        let url = format!(
            "/api/shared-worlds/{}/view?x={}&y={}&z={}&revision={}&from_x=0&from_y=0&from_z=0",
            head.id, coord[0], coord[1], coord[2], head.revision
        );
        let mut request = Request::get(url).body(Body::empty()).unwrap();
        request.headers_mut().extend(headers.clone());
        let response = app.clone().oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(response.headers()["x-ergo-read-cache"], source);
        assert!(
            response.headers()["server-timing"]
                .to_str()
                .unwrap()
                .contains("view_cache;dur=")
        );
        let packet: serde_json::Value =
            serde_json::from_slice(&to_bytes(response.into_body(), 1024 * 1024).await.unwrap())
                .unwrap();
        assert_eq!(
            packet["scene"]["bricks"].as_array().unwrap().len(),
            expected
        );
    }
    let key = read_cache::ViewKey {
        world: head.id.clone(),
        revision: head.revision.clone(),
        coord: [1, 0, 0],
        pieces: service.view_limits.pieces,
        read_bytes: service.view_limits.bytes,
    };
    let mut persisted = false;
    for _ in 0..20 {
        if service
            .read_cache
            .persistent(&service.store, &key)
            .await
            .0
            .is_some()
        {
            persisted = true;
            break;
        }
        tokio::task::yield_now().await;
    }
    assert!(
        persisted,
        "Explicit-current revision reads must still populate the persistent cache"
    );
}

#[tokio::test]
async fn operator_warm_view_persists_verified_read_data_without_a_world_commit() {
    use axum::{body::Body, http::Request};
    use tower::ServiceExt;
    let service = service();
    let (head, _) = seeded(&service, scene(100.)).await;
    let report = service.warm_view(&head.id, [0; 3]).await.unwrap();
    assert_eq!(report["revision"], serde_json::json!(head.revision));
    assert_eq!(report["pieces"], 1);
    assert_eq!(
        service
            .repo(&head.id)
            .unwrap()
            .checkout("main")
            .await
            .unwrap()
            .id(),
        &head.revision
    );
    let restarted = SharedWorlds::new(service.store.clone()).unwrap();
    let (app, headers) = http(&restarted);
    let mut request = Request::get(format!("/api/shared-worlds/{}/view?format=cbor", head.id))
        .body(Body::empty())
        .unwrap();
    request.headers_mut().extend(headers);
    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.headers()["x-ergo-read-cache"], "persistent");
    let mut bounded = service.clone();
    bounded.view_limits.bytes = 1;
    assert!(bounded.warm_view(&head.id, [0; 3]).await.is_err());
    assert!(
        service
            .warm_view(&head.id, [1_000_000_000, 0, 0])
            .await
            .is_err()
    );
}

#[tokio::test]
async fn read_progress_is_authorized_ephemeral_and_contains_no_geometry() {
    let service = service();
    let (head, _) = seeded(&service, scene(100.)).await;
    let key = read_cache::ViewKey {
        world: head.id.clone(),
        revision: head.revision.clone(),
        coord: [0; 3],
        pieces: service.view_limits.pieces,
        read_bytes: service.view_limits.bytes,
    }
    .memory_key()
    .unwrap();
    let progress = service.read_cache.start_progress(&key);
    progress.report("piece records: 1024/5000");
    let (app, alice) = http(&service);
    let (_, bob) = crate::auth::Auth::test_session("bob");
    let url = format!("/api/shared-worlds/{}/view?status=true", head.id);
    let (status, result) = json(&app, "GET", &url, &alice, None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(result["phase"], "piece records: 1024/5000");
    assert!(result.get("scene").is_none());
    progress.scanning(12, 12288, None);
    let (_, discovering) = json(&app, "GET", &url, &alice, None).await;
    assert_eq!(discovering["progress"]["stage"], "discovering");
    assert_eq!(discovering["progress"]["completed"], 12);
    assert_eq!(discovering["progress"]["total"], 27);
    assert_eq!(discovering["progress"]["found_pieces"], 12288);
    progress.pieces(2048, 6000, 13);
    let (_, pieces) = json(&app, "GET", &url, &alice, None).await;
    assert_eq!(pieces["progress"]["stage"], "pieces");
    assert_eq!(pieces["progress"]["completed"], 2048);
    assert_eq!(pieces["progress"]["total"], 6000);
    assert_eq!(
        json(&app, "GET", &url, &bob, None).await.0,
        StatusCode::FORBIDDEN
    );
    drop(progress);
    assert!(json(&app, "GET", &url, &alice, None).await.1["phase"].is_null());
    assert!(json(&app, "GET", &url, &alice, None).await.1["progress"].is_null());
}

#[tokio::test]
async fn cached_historical_views_recheck_revoked_grants_before_304_or_body() {
    use axum::{body::Body, http::Request};
    use tower::ServiceExt;
    let service = service();
    let (head, _) = seeded(&service, scene(100.)).await;
    let granted = service
        .grants(
            &head.id,
            "alice",
            GrantRequest {
                base: head.revision,
                grants: vec![Grant {
                    user: "bob".into(),
                    regions: vec!["/".parse().unwrap()],
                }],
                authenticated_editing: None,
            },
        )
        .await
        .unwrap();
    let (app, _) = http(&service);
    let (_, bob) = crate::auth::Auth::test_session("bob");
    let url = format!(
        "/api/shared-worlds/{}/view?revision={granted}&format=cbor",
        head.id
    );
    let mut request = Request::get(&url).body(Body::empty()).unwrap();
    request.headers_mut().extend(bob.clone());
    let cached = app.clone().oneshot(request).await.unwrap();
    assert_eq!(cached.status(), StatusCode::OK);
    let etag = cached.headers()["etag"].clone();
    service
        .grants(
            &head.id,
            "alice",
            GrantRequest {
                base: granted,
                grants: vec![],
                authenticated_editing: None,
            },
        )
        .await
        .unwrap();
    let mut request = Request::get(&url)
        .header("if-none-match", etag)
        .body(Body::empty())
        .unwrap();
    request.headers_mut().extend(bob.clone());
    assert_eq!(
        app.oneshot(request).await.unwrap().status(),
        StatusCode::FORBIDDEN
    );
    let (app, _) = http(&service);
    assert_eq!(
        json(&app, "GET", &url, &bob, None).await.0,
        StatusCode::FORBIDDEN
    );
}

#[tokio::test]
async fn signed_in_shared_editing_is_paced_and_paid_plans_do_not_bypass_permissions() {
    let mut service = service();
    service.pacing = pacing::Pacing::test_budget(service.store.clone(), 2, 86400);
    let (head, imported) = seeded(&service, scene(100.)).await;
    service.pacing.set_unlimited("bob", true).await.unwrap();
    assert!(
        service
            .prepare(
                &head.id,
                "bob",
                edit(&service, &head, &imported.pieces[&1], glam::DVec3::X).await
            )
            .await
            .is_err()
    );
    service.pacing.set_unlimited("bob", false).await.unwrap();
    service
        .grants(
            &head.id,
            "alice",
            GrantRequest {
                base: head.revision,
                grants: Vec::new(),
                authenticated_editing: Some(true),
            },
        )
        .await
        .unwrap();
    for _ in 0..2 {
        let head = service
            .head(
                &head.id,
                &Principal {
                    id: Some("bob".into()),
                },
                None,
            )
            .await
            .unwrap();
        service
            .prepare(
                &head.id,
                "bob",
                edit(&service, &head, &imported.pieces[&1], glam::DVec3::X).await,
            )
            .await
            .unwrap()
            .publish_with(&service)
            .await
            .unwrap();
    }
    let head = service
        .head(
            &head.id,
            &Principal {
                id: Some("bob".into()),
            },
            None,
        )
        .await
        .unwrap();
    let request = edit(&service, &head, &imported.pieces[&1], glam::DVec3::X).await;
    let error = service
        .prepare(&head.id, "bob", request.clone())
        .await
        .unwrap()
        .publish_with(&service)
        .await
        .err()
        .unwrap();
    assert_eq!(error.status, StatusCode::TOO_MANY_REQUESTS);
    service.pacing.set_unlimited("bob", true).await.unwrap();
    service
        .prepare(&head.id, "bob", request)
        .await
        .unwrap()
        .publish_with(&service)
        .await
        .unwrap();
}
async fn piece(service: &SharedWorlds, world: &str, id: &ContentId) -> Piece {
    service
        .repo(world)
        .unwrap()
        .checkout("main")
        .await
        .unwrap()
        .get(&entity_path(3, id))
        .await
        .unwrap()
        .unwrap()
}
async fn edit(
    service: &SharedWorlds,
    head: &Head,
    id: &ContentId,
    delta: glam::DVec3,
) -> CommitRequest {
    let mut geometry = piece(service, &head.id, id).await.geometry;
    geometry.position += delta;
    CommitRequest {
        joints: Vec::new(),
        base: head.revision.clone(),
        pieces: vec![PieceEdit {
            id: id.clone(),
            geometry: Some(geometry),
            mesh: None,
        }],
        rules: Vec::new(),
        message: "move".into(),
    }
}

#[tokio::test]
async fn independent_domains_rebase_and_birth_identity_survives_edits() {
    let service = service();
    let (head, imported) = seeded(&service, scene(100.0)).await;
    let left = imported.pieces[&1].clone();
    let right = imported.pieces[&2].clone();
    let old = piece(&service, &head.id, &left).await;
    let a = service
        .prepare(
            &head.id,
            "alice",
            edit(&service, &head, &left, glam::DVec3::X).await,
        )
        .await
        .unwrap();
    let b = service
        .prepare(
            &head.id,
            "alice",
            edit(&service, &head, &right, glam::DVec3::X).await,
        )
        .await
        .unwrap();
    assert_eq!(a.audit.validation.as_ref().unwrap().checked_bricks, 1);
    assert_eq!(b.audit.validation.as_ref().unwrap().checked_bricks, 1);
    a.publish().await.unwrap();
    assert!(b.publish().await.unwrap().rebases > 0);
    let new = piece(&service, &head.id, &left).await;
    assert_eq!(new.id, old.id);
    assert_eq!(new.geometry.position.x, old.geometry.position.x + 1.0);
    let repo = service.repo(&head.id).unwrap();
    let birth: Birth = repo.blob(&left).await.unwrap();
    assert_eq!(ContentId::of(&canonical_cbor(&birth).unwrap()), left);
    assert_eq!(birth.initial.position, old.geometry.position);
}

#[tokio::test]
async fn rule_edits_register_group_dependencies_and_world_model_is_frozen() {
    let mut service = service();
    let (head, imported) = seeded(&service, scene(100.0)).await;
    // A later deployment config cannot silently reinterpret previously certified regions.
    service.store.static_tolerances.position_brick_units = 100.0;
    let request = edit(&service, &head, &imported.pieces[&1], glam::DVec3::Y * 10.0).await;
    assert!(service.prepare(&head.id, "alice", request).await.is_err());
    let moving = service
        .prepare(
            &head.id,
            "alice",
            edit(&service, &head, &imported.pieces[&1], glam::DVec3::X).await,
        )
        .await
        .unwrap();
    let request = CommitRequest {
        joints: Vec::new(),
        base: head.revision.clone(),
        pieces: Vec::new(),
        rules: vec![RulesEdit {
            assembly: imported.assembly,
            joints: Vec::new(),
            fixed_bricks: Vec::new(),
            groups: vec![ergo_world::groups::Group {
                id: 1,
                name: "Connected authoring dependency".into(),
                pivot_world: glam::DVec3::ZERO,
                members: vec![
                    ergo_world::groups::Member::Piece { id: 1 },
                    ergo_world::groups::Member::Piece { id: 2 },
                ],
            }],
        }],
        message: "group".into(),
    };
    let prepared = service.prepare(&head.id, "alice", request).await.unwrap();
    assert_eq!(
        prepared.audit.validation.as_ref().unwrap().checked_bricks,
        2
    );
    prepared.publish().await.unwrap();
    assert_eq!(
        moving.publish().await.err().unwrap().status,
        StatusCode::CONFLICT
    );
}

#[tokio::test]
async fn all_geometry_can_be_deleted_without_a_minimum_or_world_size_counter() {
    let service = service();
    let (head, imported) = seeded(&service, scene(100.0)).await;
    let (app, headers) = http(&service);
    let url = format!("/api/shared-worlds/{}/assemblies?limit=1", head.id);
    let (status, before) = json(&app, "GET", &url, &headers, None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(before["items"].as_array().unwrap().len(), 1);
    let request = CommitRequest {
        joints: Vec::new(),
        base: head.revision,
        pieces: imported
            .pieces
            .values()
            .map(|id| PieceEdit {
                id: id.clone(),
                geometry: None,
                mesh: None,
            })
            .collect(),
        rules: Vec::new(),
        message: "clear bounded assembly".into(),
    };
    service
        .prepare(&head.id, "alice", request)
        .await
        .unwrap()
        .publish()
        .await
        .unwrap();
    let repo = service.repo(&head.id).unwrap();
    let snapshot = repo.checkout("main").await.unwrap();
    assert!(
        snapshot
            .scan_page(&root(1), None, 1)
            .await
            .unwrap()
            .leaves
            .is_empty()
    );
    assert!(
        service
            .head(
                &head.id,
                &Principal {
                    id: Some("alice".into())
                },
                None
            )
            .await
            .is_ok()
    );
    let (status, after) = json(&app, "GET", &url, &headers, None).await;
    assert_eq!(status, StatusCode::OK);
    assert!(after["items"].as_array().unwrap().is_empty());
}

#[tokio::test]
async fn total_world_can_exceed_working_set_and_edit_never_reads_remote_assemblies() {
    let mut service = service();
    service.limits.pieces = 4;
    let (mut head, first) = seeded(&service, scene(100.0)).await;
    for i in 1..5 {
        service
            .import(
                &head.id,
                "alice",
                ImportRequest {
                    base: head.revision,
                    scene: scene(100_000.0 * i as f64),
                },
            )
            .await
            .unwrap();
        head = service
            .head(
                &head.id,
                &Principal {
                    id: Some("alice".into()),
                },
                None,
            )
            .await
            .unwrap();
    }
    // Ten world pieces, a four-piece edit budget. No aggregate count participates.
    let prepared = service
        .prepare(
            &head.id,
            "alice",
            edit(&service, &head, &first.pieces[&1], glam::DVec3::X).await,
        )
        .await
        .unwrap();
    assert!(prepared.audit.loaded_pieces <= 4);
    assert_eq!(
        prepared.audit.validation.as_ref().unwrap().checked_bricks,
        1
    );
    assert!(!prepared.audit.watched_regions.contains(&root(1)));
    prepared.publish().await.unwrap();
}

#[tokio::test]
async fn oversized_connected_domain_is_rejected_without_truncation_or_publication() {
    let mut service = service();
    let mut initial = scene(100.0);
    initial.groups.push(ergo_world::groups::Group {
        id: 1,
        name: "Distant assembly dependency".into(),
        pivot_world: glam::DVec3::ZERO,
        members: vec![
            ergo_world::groups::Member::Piece { id: 1 },
            ergo_world::groups::Member::Piece { id: 2 },
        ],
    });
    let (head, imported) = seeded(&service, initial).await;
    service.limits.pieces = 1;
    let request = edit(&service, &head, &imported.pieces[&1], glam::DVec3::X).await;
    let error = service
        .prepare(&head.id, "alice", request)
        .await
        .err()
        .unwrap();
    assert_eq!(error.status, StatusCode::PAYLOAD_TOO_LARGE);
    assert_eq!(
        service
            .repo(&head.id)
            .unwrap()
            .checkout("main")
            .await
            .unwrap()
            .id(),
        &head.revision
    );
}

#[tokio::test]
async fn empty_spatial_predicates_detect_new_neighbors_and_no_global_counter_couples_far_imports() {
    let service = service();
    let (head, imported) = seeded(&service, scene(1020.0)).await;
    let moving = service
        .prepare(
            &head.id,
            "alice",
            edit(&service, &head, &imported.pieces[&1], glam::DVec3::X * 2.0).await,
        )
        .await
        .unwrap();
    let mut neighbor = scene(1024.0);
    neighbor.bricks.truncate(1);
    service
        .import(
            &head.id,
            "alice",
            ImportRequest {
                base: head.revision.clone(),
                scene: neighbor,
            },
        )
        .await
        .unwrap();
    assert_eq!(
        moving.publish().await.err().unwrap().status,
        StatusCode::CONFLICT
    );
    let current = service
        .head(
            &head.id,
            &Principal {
                id: Some("alice".into()),
            },
            None,
        )
        .await
        .unwrap();
    let prepared = service
        .prepare(
            &head.id,
            "alice",
            edit(&service, &current, &imported.pieces[&2], glam::DVec3::X).await,
        )
        .await
        .unwrap();
    service
        .import(
            &head.id,
            "alice",
            ImportRequest {
                base: current.revision,
                scene: scene(50_000.0),
            },
        )
        .await
        .unwrap();
    prepared.publish().await.unwrap();
}

#[tokio::test]
async fn region_grants_and_revocation_are_checked_at_atomic_publication() {
    let service = service();
    let (head, imported) = seeded(&service, scene(100.0)).await;
    let id = &imported.pieces[&1];
    let record = piece(&service, &head.id, id).await;
    let request = edit(&service, &head, id, glam::DVec3::X).await;
    assert_eq!(
        service
            .prepare(&head.id, "bob", request)
            .await
            .err()
            .unwrap()
            .status,
        StatusCode::FORBIDDEN
    );
    let revision = service
        .grants(
            &head.id,
            "alice",
            GrantRequest {
                authenticated_editing: None,
                base: head.revision,
                grants: vec![Grant {
                    user: "bob".into(),
                    regions: record
                        .replicas
                        .iter()
                        .map(|c| OctreePath::for_block((*c).into()))
                        .collect(),
                }],
            },
        )
        .await
        .unwrap();
    let granted = service
        .head(
            &head.id,
            &Principal {
                id: Some("bob".into()),
            },
            None,
        )
        .await
        .unwrap();
    let prepared = service
        .prepare(
            &head.id,
            "bob",
            edit(&service, &granted, id, glam::DVec3::X).await,
        )
        .await
        .unwrap();
    service
        .grants(
            &head.id,
            "alice",
            GrantRequest {
                authenticated_editing: None,
                base: revision,
                grants: Vec::new(),
            },
        )
        .await
        .unwrap();
    assert_eq!(
        prepared.publish().await.err().unwrap().status,
        StatusCode::CONFLICT
    );
}

#[tokio::test]
async fn support_removal_checks_old_and_new_domains_across_spatial_blocks() {
    let mut service = service();
    service.store.measurements = Arc::new(ergo_world::physical::ConnectionMeasurements {
        parts: Vec::new(),
        entries: vec![ergo_world::physical::ConnectionMeasurement {
            part_a: "3003.dat".into(),
            part_b: "3003.dat".into(),
            kind: "fixed".into(),
            overlap_studs: [2, 2],
            conservative_force_limit_n: 100.0,
            conservative_torque_limit_nm: 100.0,
            source: "synthetic test fixture, not production measurements".into(),
            samples: 1,
        }],
    });
    let mut initial = scene(1023.0);
    let mut top = initial.bricks[0].clone();
    top.id = 3;
    top.translated(glam::DVec3::Y);
    initial.bricks.push(top);
    let (head, imported) = seeded(&service, initial).await;
    let request = edit(&service, &head, &imported.pieces[&1], glam::DVec3::X * 10.0).await;
    assert!(service.prepare(&head.id, "alice", request).await.is_err());
    assert_eq!(
        service
            .repo(&head.id)
            .unwrap()
            .checkout("main")
            .await
            .unwrap()
            .id(),
        &head.revision
    );
}

#[tokio::test]
async fn boundary_references_update_atomically_and_versions_remain_historical() {
    let service = service();
    let (head, imported) = seeded(&service, scene(1023.0)).await;
    let id = &imported.pieces[&1];
    let previous = piece(&service, &head.id, id).await;
    assert_eq!(previous.replicas.len(), 2);
    service
        .prepare(
            &head.id,
            "alice",
            edit(&service, &head, id, glam::DVec3::X).await,
        )
        .await
        .unwrap()
        .publish()
        .await
        .unwrap();
    let current = piece(&service, &head.id, id).await;
    assert_eq!(current.replicas.len(), 1);
    let repo = service.repo(&head.id).unwrap();
    let old = repo.snapshot(&head.revision).await.unwrap();
    assert_eq!(
        old.get::<Piece>(&entity_path(3, id))
            .await
            .unwrap()
            .unwrap()
            .replicas
            .len(),
        2
    );
    for coord in previous.replicas.difference(&current.replicas) {
        assert!(
            repo.checkout("main")
                .await
                .unwrap()
                .scan_page(&block_path(*coord), None, 1)
                .await
                .unwrap()
                .leaves
                .is_empty()
        );
    }
}

#[tokio::test]
async fn rest_is_authenticated_paged_and_never_exports_whole_world_geometry() {
    use axum::{
        body::{Body, to_bytes},
        http::Request,
    };
    use tower::ServiceExt;
    let service = service();
    let (head, imported) = seeded(&service, scene(100.0)).await;
    let store = service.store.clone();
    let (auth, alice) = crate::auth::Auth::test_session("alice");
    let (_, bob) = crate::auth::Auth::test_session("bob");
    let app = routes().with_state(AppState {
        shared: service,
        parts: store.part_cache().unwrap(),
        ground: store.ground_cache().unwrap(),
        store,
        auth,
        geo: Default::default(),
        library: None,
    });
    let uri = format!("/api/shared-worlds/{}", head.id);
    assert_eq!(
        app.clone()
            .oneshot(Request::get(&uri).body(Body::empty()).unwrap())
            .await
            .unwrap()
            .status(),
        StatusCode::UNAUTHORIZED
    );
    let mut request = Request::get(&uri).body(Body::empty()).unwrap();
    request.headers_mut().extend(bob);
    assert_eq!(
        app.clone().oneshot(request).await.unwrap().status(),
        StatusCode::FORBIDDEN
    );
    let mut request = Request::get(&uri).body(Body::empty()).unwrap();
    request.headers_mut().extend(alice.clone());
    let response = app.clone().oneshot(request).await.unwrap();
    let value: serde_json::Value =
        serde_json::from_slice(&to_bytes(response.into_body(), 1024 * 1024).await.unwrap())
            .unwrap();
    assert!(
        value.get("scene").is_none()
            && value.get("pieces").is_none()
            && value.get("metadata").is_some()
    );
    let mut request = Request::get(format!("{uri}/blocks?revision={}&limit=1", head.revision))
        .body(Body::empty())
        .unwrap();
    request.headers_mut().extend(alice.clone());
    let response = app.clone().oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let page: serde_json::Value =
        serde_json::from_slice(&to_bytes(response.into_body(), 1024 * 1024).await.unwrap())
            .unwrap();
    assert_eq!(page["blocks"].as_array().unwrap().len(), 1);
    assert!(page["next"].is_string());
    let mut request = Request::get(format!(
        "{uri}/blocks?revision={}&limit=1&after={}",
        head.revision,
        page["next"].as_str().unwrap()
    ))
    .body(Body::empty())
    .unwrap();
    request.headers_mut().extend(alice.clone());
    let response = app.clone().oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let second: serde_json::Value =
        serde_json::from_slice(&to_bytes(response.into_body(), 1024 * 1024).await.unwrap())
            .unwrap();
    assert!(second["next"].is_null());
    assert_ne!(page["blocks"][0]["coord"], second["blocks"][0]["coord"]);
    for revision in [format!("?revision={}", head.revision), String::new()] {
        let mut request = Request::get(format!(
            "{uri}/pieces/{}/mesh{revision}",
            imported.pieces[&1]
        ))
        .body(Body::empty())
        .unwrap();
        request.headers_mut().extend(alice.clone());
        let response = app.clone().oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let etag = response.headers()["etag"]
            .to_str()
            .unwrap()
            .trim_matches('"')
            .to_owned();
        assert_eq!(response.headers()["content-type"], "application/cbor");
        assert_eq!(
            response.headers()["cache-control"]
                .to_str()
                .unwrap()
                .contains("immutable"),
            !revision.is_empty()
        );
        let bytes = to_bytes(response.into_body(), 1024 * 1024).await.unwrap();
        assert_eq!(ContentId::of(&bytes).as_str(), etag);
    }
    let body = CommitRequest {
        joints: Vec::new(),
        base: head.revision.clone(),
        pieces: vec![PieceEdit {
            id: imported.pieces[&1].clone(),
            geometry: None,
            mesh: None,
        }],
        rules: Vec::new(),
        message: "delete".into(),
    };
    let mut request = Request::post(format!("{uri}/commits"))
        .header("content-type", "application/json")
        .body(Body::from(serde_json::to_vec(&body).unwrap()))
        .unwrap();
    request.headers_mut().extend(alice.clone());
    request.headers_mut().remove("X-Ergo-Request");
    assert_eq!(
        app.clone().oneshot(request).await.unwrap().status(),
        StatusCode::FORBIDDEN
    );
    let mut request = Request::post(format!("{uri}/commits"))
        .header("content-type", "application/json")
        .body(Body::from(serde_json::to_vec(&body).unwrap()))
        .unwrap();
    request.headers_mut().extend(alice);
    assert_eq!(app.oneshot(request).await.unwrap().status(), StatusCode::OK);
}

#[tokio::test]
async fn mcp_delegations_publish_and_read_but_cannot_change_grants_or_other_users_worlds() {
    let service = service();
    let (head, _) = seeded(&service, scene(100.)).await;
    let store = service.store.clone();
    let (auth, alice) = crate::auth::Auth::test_session("alice");
    let (_, bob) = crate::auth::Auth::test_session("bob");
    let app = routes()
        .route("/api/auth/mcp-token", post(crate::auth::mcp_token))
        .with_state(AppState {
            shared: service,
            parts: store.part_cache().unwrap(),
            ground: store.ground_cache().unwrap(),
            store,
            auth,
            geo: Default::default(),
            library: None,
        });
    let mut delegated = Vec::new();
    for session in [&alice, &bob] {
        let (status, grant) = json(
            &app,
            "POST",
            "/api/auth/mcp-token",
            session,
            Some(serde_json::json!({"expires_in_seconds":60})),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        let mut headers = axum::http::HeaderMap::new();
        headers.insert(
            "authorization",
            format!("Bearer {}", grant["token"].as_str().unwrap())
                .parse()
                .unwrap(),
        );
        headers.insert("X-Ergo-Request", "1".parse().unwrap());
        delegated.push(headers);
    }
    let uri = format!("/api/shared-worlds/{}", head.id);
    assert_eq!(
        json(&app, "GET", &uri, &delegated[0], None).await.0,
        StatusCode::OK
    );
    assert_eq!(
        json(&app, "GET", &uri, &delegated[1], None).await.0,
        StatusCode::FORBIDDEN
    );
    let (status, imported) = json(
        &app,
        "POST",
        &format!("{uri}/assemblies"),
        &delegated[0],
        Some(serde_json::json!({"base":head.revision,"scene":scene(100_000.)})),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{imported}");
    assert!(imported["accepted"]["audit"]["validation"].is_object());
    let revision = &imported["accepted"]["revision"];
    assert_eq!(
        json(
            &app,
            "PUT",
            &format!("{uri}/grants"),
            &delegated[0],
            Some(serde_json::json!({"base":revision,"grants":[]}))
        )
        .await
        .0,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        json(
            &app,
            "POST",
            &format!("{uri}/commits"),
            &delegated[0],
            Some(serde_json::json!({"base":revision,"pieces":[]}))
        )
        .await
        .0,
        StatusCode::UNPROCESSABLE_ENTITY
    );
    assert_eq!(
        json(
            &app,
            "PUT",
            &format!("{uri}/retention"),
            &delegated[0],
            Some(serde_json::json!({"base":revision,"keep_last":1}))
        )
        .await
        .0,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        json(
            &app,
            "POST",
            &format!("{uri}/assemblies"),
            &delegated[1],
            Some(serde_json::json!({"base":revision,"scene":scene(200_000.)}))
        )
        .await
        .0,
        StatusCode::FORBIDDEN
    );
}

fn http(service: &SharedWorlds) -> (Router, HeaderMap) {
    let store = service.store.clone();
    let (auth, headers) = crate::auth::Auth::test_session("alice");
    (
        routes()
            .route("/api/parts/{part}", get(crate::part_asset))
            .with_state(AppState {
                shared: service.clone(),
                parts: store.part_cache().unwrap(),
                ground: store.ground_cache().unwrap(),
                store,
                auth,
                geo: Default::default(),
                library: None,
            }),
        headers,
    )
}
async fn json(
    app: &Router,
    method: &str,
    url: &str,
    headers: &HeaderMap,
    body: Option<serde_json::Value>,
) -> (StatusCode, serde_json::Value) {
    use axum::{
        body::{Body, to_bytes},
        http::Request,
    };
    use tower::ServiceExt;
    let mut request = Request::builder()
        .method(method)
        .uri(url)
        .header("content-type", "application/json")
        .body(
            body.map(|v| Body::from(v.to_string()))
                .unwrap_or_else(Body::empty),
        )
        .unwrap();
    request.headers_mut().extend(headers.clone());
    let response = app.clone().oneshot(request).await.unwrap();
    let status = response.status();
    let bytes = to_bytes(response.into_body(), 16 * 1024 * 1024)
        .await
        .unwrap();
    (status, serde_json::from_slice(&bytes).unwrap())
}
async fn unpinned(repo: &Repository) {
    for _ in 0..1024 {
        if repo.active_pins().await.unwrap().is_empty() {
            return;
        }
        tokio::task::yield_now().await;
    }
    panic!("pins still active");
}

#[tokio::test]
async fn dense_block_edits_load_only_selected_piece_and_rule_pages() {
    let mut service = service();
    let mut initial = scene(100.);
    let mut d = ergo_world::demo::Demo::new();
    for y in 0..10 {
        for x in 0..10 {
            d.add("3003", 4, 10. + x as f64 * 80., 0., 10. + y as f64 * 80.);
        }
    }
    initial.bricks = d.bricks;
    initial.meshes = d.meshes;
    let (head, imported) = seeded(&service, initial).await;
    service.limits.pieces = 2;
    let left = service
        .prepare(
            &head.id,
            "alice",
            edit(&service, &head, &imported.pieces[&1], glam::DVec3::X).await,
        )
        .await
        .unwrap();
    let right = service
        .prepare(
            &head.id,
            "alice",
            edit(&service, &head, &imported.pieces[&100], glam::DVec3::X).await,
        )
        .await
        .unwrap();
    assert!(left.audit.loaded_pieces <= 2 && right.audit.loaded_pieces <= 2);
    left.publish().await.unwrap();
    right.publish().await.unwrap();
    let (app, headers) = http(&service);
    let coord: [i64; 3] = piece(&service, &head.id, &imported.pieces[&1])
        .await
        .replicas
        .into_iter()
        .next()
        .unwrap()
        .into();
    let url = format!(
        "/api/shared-worlds/{}/blocks/{}/{}/{}/pieces?revision={}&limit=7",
        head.id, coord[0], coord[1], coord[2], head.revision
    );
    let (status, page) = json(&app, "GET", &url, &headers, None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(page["pieces"].as_array().unwrap().len(), 7);
    assert!(page["next"].is_string());
    let (status, page) = json(
        &app,
        "GET",
        &format!(
            "/api/shared-worlds/{}/assemblies/{}/pages?revision={}&kind=1&limit=5",
            head.id, imported.assembly, head.revision
        ),
        &headers,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(page["rows"].as_array().unwrap().len(), 5);
    assert!(page["next"].is_string());
}

#[tokio::test]
async fn cross_assembly_joints_track_both_ends_and_prevent_dangling_deletions() {
    let mut service = service();
    service.store.measurements = Arc::new(ergo_world::physical::ConnectionMeasurements {
        parts: Vec::new(),
        entries: vec![ergo_world::physical::ConnectionMeasurement {
            part_a: "3003.dat".into(),
            part_b: "3003.dat".into(),
            kind: "fixed".into(),
            overlap_studs: [0, 2],
            conservative_force_limit_n: 100.,
            conservative_torque_limit_nm: 100.,
            source: "synthetic test-only fixture".into(),
            samples: 1,
        }],
    });
    let (head, first) = seeded(&service, scene(100.)).await;
    let second = service
        .import(
            &head.id,
            "alice",
            ImportRequest {
                base: head.revision,
                scene: scene(100_000.),
            },
        )
        .await
        .unwrap();
    let head = service
        .head(
            &head.id,
            &Principal {
                id: Some("alice".into()),
            },
            None,
        )
        .await
        .unwrap();
    let a = first.pieces[&1].clone();
    let b = second.pieces[&1].clone();
    let before = service
        .prepare(
            &head.id,
            "alice",
            edit(&service, &head, &a, glam::DVec3::X).await,
        )
        .await
        .unwrap();
    let input = joints::JointInput {
        a: a.clone(),
        b: b.clone(),
        anchor: glam::DVec3::ZERO,
        kind: ergo_world::JointKind::Fixed,
    };
    let request = CommitRequest {
        base: head.revision,
        pieces: Vec::new(),
        rules: Vec::new(),
        joints: vec![joints::JointEdit {
            id: None,
            value: Some(input),
        }],
        message: "cross-assembly joint".into(),
    };
    let created = service
        .prepare(&head.id, "alice", request)
        .await
        .unwrap()
        .publish()
        .await
        .unwrap();
    assert_eq!(created.joint_ids.len(), 1);
    assert_eq!(
        before.publish().await.err().unwrap().status,
        StatusCode::CONFLICT
    );
    let storage = service.repo(&head.id).unwrap();
    unpinned(&storage).await;
    service.collect(&head.id).await.unwrap();
    let head = service
        .head(
            &head.id,
            &Principal {
                id: Some("alice".into()),
            },
            None,
        )
        .await
        .unwrap();
    let mut deletion = CommitRequest {
        base: head.revision,
        pieces: vec![
            PieceEdit {
                id: a.clone(),
                geometry: None,
                mesh: None,
            },
            PieceEdit {
                id: b,
                geometry: None,
                mesh: None,
            },
        ],
        rules: Vec::new(),
        joints: Vec::new(),
        message: "delete endpoints".into(),
    };
    assert!(
        service
            .prepare(&head.id, "alice", deletion.clone())
            .await
            .is_err()
    );
    deletion.joints.push(joints::JointEdit {
        id: Some(created.joint_ids[0].clone()),
        value: None,
    });
    service
        .prepare(&head.id, "alice", deletion)
        .await
        .unwrap()
        .publish()
        .await
        .unwrap();
}

#[tokio::test]
async fn compact_authoring_guard_excludes_members_but_keeps_touching_other_assemblies() {
    let service = service();
    let mut one = scene(100.);
    one.bricks.truncate(1);
    let (head, first) = seeded(&service, one).await;
    let independent = service
        .authoring_scene(&head.id, "alice", &first.assembly, &head.revision)
        .await
        .unwrap();
    assert_eq!(independent["external_dependencies"], false);
    let mut adjacent = scene(102.);
    adjacent.bricks.truncate(1);
    let second = service
        .import(
            &head.id,
            "alice",
            ImportRequest {
                base: head.revision,
                scene: adjacent,
            },
        )
        .await
        .unwrap();
    let guarded = service
        .authoring_scene(
            &head.id,
            "alice",
            &first.assembly,
            &second.accepted.revision,
        )
        .await
        .unwrap();
    assert_eq!(guarded["external_dependencies"], true);
}

#[tokio::test]
async fn authoring_dependency_guard_stops_at_the_first_verified_external_piece() {
    let mut service = service();
    let mut owner = scene(100.);
    let delta = glam::DVec3::X * (130. - owner.bricks[1].position.x);
    owner.bricks[1].translated(delta);
    let (head, imported) = seeded(&service, owner).await;
    let mut neighbors = scene(110.);
    let delta = glam::DVec3::X * (114. - neighbors.bricks[1].position.x);
    neighbors.bricks[1].translated(delta);
    let other = service
        .import(
            &head.id,
            "alice",
            ImportRequest {
                base: head.revision,
                scene: neighbors,
            },
        )
        .await
        .unwrap();
    // Two owned pieces fit with one external witness. Materializing the whole
    // neighborhood would unnecessarily exceed this authoring read budget.
    service.limits.pieces = 3;
    let guarded = service
        .authoring_scene(
            &head.id,
            "alice",
            &imported.assembly,
            &other.accepted.revision,
        )
        .await
        .unwrap();
    assert_eq!(guarded["external_dependencies"], true);
    assert_eq!(guarded["scene"]["bricks"].as_array().unwrap().len(), 2);
}

#[tokio::test]
async fn compact_authoring_guard_keeps_remote_explicit_dependencies() {
    let mut service = service();
    service.store.measurements = Arc::new(ergo_world::physical::ConnectionMeasurements {
        parts: vec![],
        entries: vec![ergo_world::physical::ConnectionMeasurement {
            part_a: "3003.dat".into(),
            part_b: "3003.dat".into(),
            kind: "fixed".into(),
            overlap_studs: [0, 2],
            conservative_force_limit_n: 100.,
            conservative_torque_limit_nm: 100.,
            source: "synthetic test-only fixture".into(),
            samples: 1,
        }],
    });
    let mut one = scene(100.);
    one.bricks.truncate(1);
    let (head, first) = seeded(&service, one).await;
    let mut far = scene(100_000.);
    far.bricks.truncate(1);
    let second = service
        .import(
            &head.id,
            "alice",
            ImportRequest {
                base: head.revision,
                scene: far,
            },
        )
        .await
        .unwrap();
    let independent = service
        .authoring_scene(
            &head.id,
            "alice",
            &first.assembly,
            &second.accepted.revision,
        )
        .await
        .unwrap();
    assert_eq!(independent["external_dependencies"], false);
    let linked = service
        .prepare(
            &head.id,
            "alice",
            CommitRequest {
                base: second.accepted.revision,
                pieces: vec![],
                rules: vec![],
                message: "remote dependency fixture".into(),
                joints: vec![joints::JointEdit {
                    id: None,
                    value: Some(joints::JointInput {
                        a: first.pieces[&1].clone(),
                        b: second.pieces[&1].clone(),
                        anchor: glam::DVec3::ZERO,
                        kind: ergo_world::JointKind::Fixed,
                    }),
                }],
            },
        )
        .await
        .unwrap()
        .publish()
        .await
        .unwrap();
    let guarded = service
        .authoring_scene(&head.id, "alice", &first.assembly, &linked.revision)
        .await
        .unwrap();
    assert_eq!(guarded["external_dependencies"], true);
}

#[tokio::test]
async fn retention_keeps_birth_mesh_and_model_links_after_import_history_is_collected() {
    let service = service();
    let (head, imported) = seeded(&service, scene(100.)).await;
    let old = head.revision.clone();
    let id = imported.pieces[&1].clone();
    let original = piece(&service, &head.id, &id).await;
    let (app, headers) = http(&service);
    let (status, _) = json(
        &app,
        "PUT",
        &format!("/api/shared-worlds/{}/retention", head.id),
        &headers,
        Some(serde_json::json!({"base":head.revision,"keep_last":2})),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let head = service
        .head(
            &head.id,
            &Principal {
                id: Some("alice".into()),
            },
            None,
        )
        .await
        .unwrap();
    service
        .prepare(
            &head.id,
            "alice",
            edit(&service, &head, &id, glam::DVec3::X).await,
        )
        .await
        .unwrap()
        .publish()
        .await
        .unwrap();
    let repo = service.repo(&head.id).unwrap();
    unpinned(&repo).await;
    assert!(service.prune(&head.id).await.unwrap().removed_revisions > 0);
    unpinned(&repo).await;
    service.collect(&head.id).await.unwrap();
    assert_eq!(
        service
            .head(
                &head.id,
                &Principal {
                    id: Some("alice".into())
                },
                Some(&old)
            )
            .await
            .err()
            .unwrap()
            .status,
        StatusCode::GONE
    );
    let current = piece(&service, &head.id, &id).await;
    assert_eq!(current.id, id);
    let birth: Birth = repo.blob(&id).await.unwrap();
    assert_eq!(birth.mesh, original.mesh);
    let _: ergo_world::geometry::Mesh = repo.blob(&current.mesh).await.unwrap();
    let _: PhysicalModel = repo.blob(&head.metadata.model).await.unwrap();
    {
        use axum::{body::Body, http::Request};
        use futures::StreamExt;
        use tower::ServiceExt;
        let mut request =
            Request::get(format!("/api/shared-worlds/{}/events?after={old}", head.id))
                .body(Body::empty())
                .unwrap();
        request.headers_mut().extend(headers.clone());
        let response = app.clone().oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let mut stream = response.into_body().into_data_stream();
        let reset = tokio::time::timeout(std::time::Duration::from_secs(2), stream.next())
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        assert!(String::from_utf8_lossy(&reset).contains("reset"));
    }
}

#[tokio::test]
async fn catalogs_are_paged_and_stale_grants_never_leak_worlds() {
    let service = service();
    let (head, _) = seeded(&service, scene(100.)).await;
    let (app, alice) = http(&service);
    let (_, bob) = crate::auth::Auth::test_session("bob");
    let (status, page) = json(&app, "GET", "/api/shared-worlds?limit=1", &alice, None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(page["items"][0]["world"], head.id);
    let (_, empty) = json(&app, "GET", "/api/shared-worlds?quad=&limit=32", &bob, None).await;
    assert!(empty["items"].as_array().unwrap().is_empty());
    let revision = service
        .grants(
            &head.id,
            "alice",
            GrantRequest {
                authenticated_editing: None,
                base: head.revision,
                grants: vec![Grant {
                    user: "bob".into(),
                    regions: vec![OctreePath::root()],
                }],
            },
        )
        .await
        .unwrap();
    let (_, visible) = json(&app, "GET", "/api/shared-worlds?limit=32", &bob, None).await;
    assert_eq!(visible["items"][0]["world"], head.id);
    let (_, geo) = json(&app, "GET", "/api/shared-worlds?quad=&limit=32", &bob, None).await;
    assert!(!geo["items"].as_array().unwrap().is_empty());
    service
        .grants(
            &head.id,
            "alice",
            GrantRequest {
                authenticated_editing: None,
                base: revision,
                grants: Vec::new(),
            },
        )
        .await
        .unwrap();
    let (_, hidden) = json(&app, "GET", "/api/shared-worlds?limit=32", &bob, None).await;
    assert!(hidden["items"].as_array().unwrap().is_empty());
}

#[tokio::test]
async fn grouped_service_publication_preserves_each_audit() {
    let mut service = service();
    service.group_commit = true;
    let (head, imported) = seeded(&service, scene(100.)).await;
    let a = service
        .prepare(
            &head.id,
            "alice",
            edit(&service, &head, &imported.pieces[&1], glam::DVec3::X).await,
        )
        .await
        .unwrap();
    let b = service
        .prepare(
            &head.id,
            "alice",
            edit(&service, &head, &imported.pieces[&2], glam::DVec3::X).await,
        )
        .await
        .unwrap();
    let (a, b) = tokio::join!(a.publish_with(&service), b.publish_with(&service));
    let a = a.unwrap();
    let b = b.unwrap();
    assert_ne!(a.revision, b.revision);
    let repo = service.repo(&head.id).unwrap();
    assert!(repo.was_committed("main", &a.revision).await.unwrap());
    assert!(repo.was_committed("main", &b.revision).await.unwrap());
}

#[tokio::test]
async fn live_event_stream_rechecks_grants_and_holds_no_persistent_snapshot_pin() {
    use axum::{body::Body, http::Request};
    use futures::StreamExt;
    use tower::ServiceExt;
    let mut service = service();
    service.event_period = std::time::Duration::from_millis(5);
    let (head, _) = seeded(&service, scene(100.)).await;
    let revision = service
        .grants(
            &head.id,
            "alice",
            GrantRequest {
                authenticated_editing: None,
                base: head.revision,
                grants: vec![Grant {
                    user: "bob".into(),
                    regions: vec![OctreePath::root()],
                }],
            },
        )
        .await
        .unwrap();
    let (app, _) = http(&service);
    let (_, bob) = crate::auth::Auth::test_session("bob");
    let mut request = Request::get(format!("/api/shared-worlds/{}/events", head.id))
        .body(Body::empty())
        .unwrap();
    request.headers_mut().extend(bob);
    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let mut stream = response.into_body().into_data_stream();
    let first = tokio::time::timeout(std::time::Duration::from_secs(2), stream.next())
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    assert!(String::from_utf8_lossy(&first).contains("revision"));
    let repo = service.repo(&head.id).unwrap();
    unpinned(&repo).await;
    service
        .grants(
            &head.id,
            "alice",
            GrantRequest {
                authenticated_editing: None,
                base: revision,
                grants: Vec::new(),
            },
        )
        .await
        .unwrap();
    let revoked = tokio::time::timeout(std::time::Duration::from_secs(2), stream.next())
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    assert!(String::from_utf8_lossy(&revoked).contains("revoked"));
}

#[tokio::test]
async fn noncurrent_shared_layouts_are_rejected() {
    let service = service();
    let (head, _) = seeded(&service, scene(100.)).await;
    let current = serde_json::to_value(&head.metadata).unwrap();
    for layout in [0, 1, 4] {
        let mut value = current.clone();
        value["layout"] = serde_json::json!(layout);
        assert!(serde_json::from_value::<Metadata>(value).is_err());
    }
    let mut missing = current;
    missing.as_object_mut().unwrap().remove("layout");
    assert!(serde_json::from_value::<Metadata>(missing).is_err());
}
