use super::*;

fn query(values: &[(&str, String)]) -> String {
    url::form_urlencoded::Serializer::new(String::new())
        .extend_pairs(values.iter().map(|(k, v)| (*k, v.as_str())))
        .finish()
}

#[tokio::test]
async fn manifests_and_pages_are_bounded_pinned_and_never_implicitly_load_a_scene() {
    let service = service();
    let mut source = scene(0.);
    source.bricks.clear();
    let mut demo = ergo_world::demo::Demo::new();
    for i in 0..600 {
        demo.add("3003", 4, i as f64 * 4., 0., 100.);
    }
    source.bricks = demo.bricks;
    let (head, imported) = seeded(&service, source).await;
    let (app, headers) = http(&service);
    let root = format!(
        "/api/shared-worlds/{}/assemblies/{}/workspace",
        head.id, imported.assembly
    );
    let (status, manifest) = json(&app, "GET", &root, &headers, None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(manifest["format"], "ergo-workspace-2");
    assert_eq!(manifest["piece_count"], 600);
    assert_eq!(manifest["next_local_id"], 601);
    assert!(manifest.get("scene").is_none());
    let mut cursor = None;
    let mut ids = BTreeSet::new();
    let mut pages = 0;
    loop {
        let mut args = vec![
            ("limit", "128".into()),
            ("revision", imported.accepted.revision.to_string()),
        ];
        if let Some(c) = cursor {
            args.push(("cursor", c));
        }
        let (status, page) = json(
            &app,
            "GET",
            &format!("{root}/pages?{}", query(&args)),
            &headers,
            None,
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{page}");
        assert_eq!(
            page["base_revision"],
            imported.accepted.revision.to_string()
        );
        assert_eq!(page["partial"], true);
        assert!(serde_json::to_vec(&page).unwrap().len() <= ergo_world::authoring::PAGE_BYTES);
        let bricks = page["bricks"].as_array().unwrap();
        assert!(bricks.len() <= 128);
        for b in bricks {
            assert!(ids.insert(b["id"].as_u64().unwrap()));
            assert!(page["versions"].get(b["id"].to_string()).is_some());
        }
        pages += 1;
        cursor = page["next"].as_str().map(str::to_owned);
        if cursor.is_none() {
            break;
        }
        assert!(pages < 10);
    }
    assert_eq!(ids.len(), 600);
}

#[tokio::test]
async fn spatial_pages_deduplicate_boundary_replicas_and_scope_the_cursor() {
    let service = service();
    let mut demo = ergo_world::demo::Demo::new();
    demo.add("3001", 4, 127., 0., 100.);
    demo.add("3003", 4, 140., 0., 100.);
    let mut source = scene(0.);
    source.bricks = demo.bricks;
    source.meshes = demo.meshes;
    let min = ergo_world::coordinates::public(source.bricks[0].render_bounds.min);
    let max = ergo_world::coordinates::public(source.bricks[0].render_bounds.max);
    let bounds = [
        (min.min(max) - glam::DVec3::ONE).to_array(),
        (min.max(max) + glam::DVec3::ONE).to_array(),
    ];
    let (head, imported) = seeded(&service, source).await;
    let (app, headers) = http(&service);
    let root = format!(
        "/api/shared-worlds/{}/assemblies/{}/workspace/pages",
        head.id, imported.assembly
    );
    let mut cursor = None;
    let mut ids = Vec::new();
    for _ in 0..20 {
        let mut args = vec![
            ("bounds", serde_json::to_string(&bounds).unwrap()),
            ("limit", "1".into()),
        ];
        if let Some(c) = cursor {
            args.push(("cursor", c));
        }
        let (status, page) = json(
            &app,
            "GET",
            &format!("{root}?{}", query(&args)),
            &headers,
            None,
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{page}");
        ids.extend(
            page["bricks"]
                .as_array()
                .unwrap()
                .iter()
                .map(|b| b["id"].as_u64().unwrap()),
        );
        cursor = page["next"].as_str().map(str::to_owned);
        if cursor.is_none() {
            break;
        }
    }
    assert!(cursor.is_none());
    assert_eq!(ids, vec![1]);
    let mut boundary = bounds;
    boundary[0][0] = 128.;
    let mut cursor = None;
    let mut at_boundary = Vec::new();
    for _ in 0..20 {
        let args = if let Some(c) = cursor {
            vec![("cursor", c)]
        } else {
            vec![("bounds", serde_json::to_string(&boundary).unwrap())]
        };
        let (status, page) = json(
            &app,
            "GET",
            &format!("{root}?{}", query(&args)),
            &headers,
            None,
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{page}");
        at_boundary.extend(
            page["bricks"]
                .as_array()
                .unwrap()
                .iter()
                .map(|b| b["id"].as_u64().unwrap()),
        );
        cursor = page["next"].as_str().map(str::to_owned);
        if cursor.is_none() {
            break;
        }
    }
    assert_eq!(
        at_boundary,
        vec![1],
        "A query beginning exactly at a cell boundary must not lose a crossing piece"
    );
    let empty_bounds = [[64., -128., 0.], [64., 0., 128.]];
    let (status, empty) = json(
        &app,
        "GET",
        &format!(
            "{root}?{}",
            query(&[("bounds", serde_json::to_string(&empty_bounds).unwrap())])
        ),
        &headers,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{empty}");
    assert!(empty["bricks"].as_array().unwrap().is_empty());
    assert!(
        empty["next"].is_null(),
        "Absent octree prefixes must be pruned, not subdivided geometrically forever"
    );
    let (_, page) = json(&app, "GET", &format!("{root}?limit=1"), &headers, None).await;
    let cursor = page["next"].as_str().unwrap();
    let other = "f".repeat(64);
    let (status, _) = json(
        &app,
        "GET",
        &format!(
            "{root}?{}",
            query(&[("cursor", cursor.into()), ("revision", other)])
        ),
        &headers,
        None,
    )
    .await;
    assert!(!status.is_success());
}

#[tokio::test]
async fn sparse_edits_keep_unloaded_members_and_reject_stale_or_missing_versions() {
    let service = service();
    let source = scene(10.);
    let (head, imported) = seeded(&service, source.clone()).await;
    let (app, headers) = http(&service);
    let root = format!(
        "/api/shared-worlds/{}/assemblies/{}/workspace",
        head.id, imported.assembly
    );
    let (_, page) = json(
        &app,
        "GET",
        &format!("{root}/pages?limit=1"),
        &headers,
        None,
    )
    .await;
    let mut before = source.clone();
    before.bricks.truncate(1);
    let mut after = before.clone();
    after.bricks[0].color = 14;
    after.bricks[0].rgb = ergo_world::color_rgb(14);
    let versions = serde_json::from_value(page["versions"].clone()).unwrap();
    let delta = ergo_world::authoring::Delta::between(&before, &after, &versions).unwrap();
    let (status, result) = json(
        &app,
        "PATCH",
        &root,
        &headers,
        Some(serde_json::json!({"base":imported.accepted.revision,"delta":delta})),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{result}");
    let accepted: ContentId = result["accepted"]["revision"]
        .as_str()
        .unwrap()
        .parse()
        .unwrap();
    let loaded = service
        .authoring_scene(&head.id, "alice", &imported.assembly, &accepted)
        .await
        .unwrap();
    assert_eq!(loaded["scene"]["bricks"].as_array().unwrap().len(), 2);
    assert_eq!(
        piece(&service, &head.id, &imported.pieces[&2]).await.id,
        imported.pieces[&2]
    );
    let (status, _) = json(
        &app,
        "PATCH",
        &root,
        &headers,
        Some(serde_json::json!({"base":accepted,"delta":delta})),
    )
    .await;
    assert!(!status.is_success());
    let (status, _) = json(
        &app,
        "PUT",
        &root,
        &headers,
        Some(serde_json::json!({"base":accepted,"scene":source})),
    )
    .await;
    assert_eq!(status, StatusCode::METHOD_NOT_ALLOWED);
    let (_, manifest) = json(&app, "GET", &root, &headers, None).await;
    assert_eq!(manifest["base_revision"], accepted.to_string());
}
