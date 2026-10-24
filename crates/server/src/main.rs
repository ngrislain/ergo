mod auth;
mod catalog;
mod import;
mod shared;
mod store;

use anyhow::{Context, Result};
use axum::{
    Json, Router,
    body::Body,
    extract::{DefaultBodyLimit, Multipart, Path, Query, State},
    http::{HeaderMap, StatusCode, header},
    response::{IntoResponse, Response},
    routing::{get, post},
};
use ergo_world::{GeoReference, SCHEMA_VERSION, WorldExport, WorldManifest};
use serde::{Deserialize, Serialize};
use std::{env, path::PathBuf};
use store::Store;
use tower_http::{
    compression::CompressionLayer,
    services::{ServeDir, ServeFile},
    trace::TraceLayer,
};

#[derive(Clone)]
struct AppState {
    store: Store,
    geo: GeoReference,
    library: Option<PathBuf>,
    auth: auth::Auth,
    parts: ergo_assets::PartCache,
    ground: ergo_assets::ground::GroundCache,
    shared: shared::SharedWorlds,
}

struct ApiError(anyhow::Error);
impl<E: Into<anyhow::Error>> From<E> for ApiError {
    fn from(e: E) -> Self {
        Self(e.into())
    }
}
impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        tracing::warn!(error = %self.0, "Request failed");
        let status = if self
            .0
            .chain()
            .any(|e| e.downcast_ref::<object_store::Error>().is_some())
        {
            StatusCode::BAD_GATEWAY
        } else {
            StatusCode::BAD_REQUEST
        };
        (
            status,
            Json(serde_json::json!({"error": format!("{:#}", self.0)})),
        )
            .into_response()
    }
}

#[derive(Serialize)]
struct Config {
    storage: String,
    geo: GeoReference,
    block_size: u32,
    simulation_limit: usize,
    ground_source: &'static str,
    ground_provider: String,
}

async fn config(State(s): State<AppState>) -> Json<Config> {
    Json(Config {
        storage: s.store.kind,
        geo: s.geo,
        block_size: 1024,
        simulation_limit: ergo_world::MAX_SIMULATION_BRICKS,
        ground_source: if s.ground.is_enabled() {
            "raster"
        } else {
            "disabled"
        },
        ground_provider: s.ground.provider().to_owned(),
    })
}

#[derive(Deserialize)]
struct WorldsQuery {
    limit: Option<usize>,
    cursor: Option<String>,
}
async fn worlds(
    State(s): State<AppState>,
    headers: HeaderMap,
    Query(q): Query<WorldsQuery>,
) -> Result<Response, ApiError> {
    let principal = s.auth.principal(&headers);
    if q.limit.is_some() || q.cursor.is_some() {
        return Ok(Json(
            catalog::page(
                &s.store,
                &principal,
                q.limit.unwrap_or(32),
                q.cursor.as_deref(),
            )
            .await?,
        )
        .into_response());
    }
    Ok(Json(s.store.manifests(&principal).await?).into_response())
}

async fn part_asset(
    State(s): State<AppState>,
    Path(part): Path<String>,
) -> Result<Json<ergo_assets::PartAsset>, ApiError> {
    let part = if part.ends_with(".dat") {
        part
    } else {
        format!("{part}.dat")
    };
    Ok(Json(s.parts.resolve(&part).await?))
}

#[derive(Deserialize)]
struct GroundQuery {
    latitude: f64,
    longitude: f64,
    arcseconds_per_block: f64,
}
async fn ground_block(
    State(s): State<AppState>,
    Path((x, y)): Path<(i64, i64)>,
    Query(q): Query<GroundQuery>,
) -> Json<ergo_world::terrain::GroundBlock> {
    Json(
        s.ground
            .block(
                GeoReference {
                    latitude: q.latitude,
                    longitude: q.longitude,
                    arcseconds_per_block: q.arcseconds_per_block,
                },
                x,
                y,
            )
            .await,
    )
}
async fn manifest(
    State(s): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<Json<WorldManifest>, ApiError> {
    Ok(Json(
        s.store.manifest(&id, &s.auth.principal(&headers)).await?,
    ))
}

async fn object(
    State(s): State<AppState>,
    headers: HeaderMap,
    Path((id, hash)): Path<(String, String)>,
) -> Result<Response, ApiError> {
    if hash.len() != 64
        || !hash
            .bytes()
            .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
    {
        return Err(anyhow::anyhow!("Invalid object hash").into());
    }
    let manifest = s.store.manifest(&id, &s.auth.principal(&headers)).await?;
    if !manifest.geometry.values().any(|h| h == &hash)
        && !manifest.blocks.iter().any(|b| b.key == hash)
    {
        return Err(anyhow::anyhow!("Object is not referenced by this construction").into());
    }
    let bytes = s.store.get(&format!("objects/{hash}.cbor")).await?;
    Ok(Response::builder()
        .header(header::CONTENT_TYPE, "application/cbor")
        .header(
            header::CACHE_CONTROL,
            "private, max-age=31536000, immutable",
        )
        .header(header::ETAG, format!("\"{hash}\""))
        .body(Body::from(bytes))
        .unwrap())
}

async fn export(
    State(s): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<Response, ApiError> {
    let manifest = s.store.manifest(&id, &s.auth.principal(&headers)).await?;
    let scene = s.store.export(&manifest).await?;
    Ok((
        [
            (header::CONTENT_TYPE, "application/json"),
            (
                header::CONTENT_DISPOSITION,
                "attachment; filename=construction.ergo.json",
            ),
        ],
        serde_json::to_vec(&ergo_world::storage::Construction::from(&scene))?,
    )
        .into_response())
}

#[derive(Serialize)]
struct ImportPreview {
    scene: WorldExport,
    notes: Vec<String>,
}
async fn upload(
    State(s): State<AppState>,
    headers: HeaderMap,
    mut multipart: Multipart,
) -> Result<Json<ImportPreview>, ApiError> {
    s.auth.check_write(&headers)?;
    let mut file = None;
    let mut name = None;
    let mut latitude = s.geo.latitude;
    let mut longitude = s.geo.longitude;
    let mut placed = false;
    while let Some(field) = multipart.next_field().await? {
        match field.name().unwrap_or("") {
            "file" => {
                let filename = field.file_name().unwrap_or("model.ldr").to_owned();
                file = Some((filename, field.bytes().await?));
            }
            "name" => name = Some(field.text().await?),
            "latitude" => {
                latitude = field.text().await?.parse()?;
                placed = true;
            }
            "longitude" => {
                longitude = field.text().await?.parse()?;
                placed = true;
            }
            _ => {}
        }
    }
    let (filename, bytes) =
        file.context("Choose a Studio .io, LDraw .ldr/.mpd, or Ergo JSON file")?;
    let is_json = filename.to_lowercase().ends_with(".json");
    let geo = s.geo;
    let library = s.library.clone();
    let (mut scene, mut notes) = tokio::task::spawn_blocking(move || -> Result<_> {
        if is_json {
            let mut scene =
                ergo_world::storage::decode(&bytes).context("Invalid Ergo scene JSON")?;
            import::validate_export(&mut scene)?;
            Ok((scene, Vec::new()))
        } else {
            let import::ImportedLdraw {
                bricks,
                meshes,
                notes,
                model,
            } = import::import_ldraw(&bytes, &filename, library)?;
            Ok((
                WorldExport {
                    schema: SCHEMA_VERSION,
                    name: filename.clone(),
                    geo,
                    bricks,
                    meshes,
                    joints: Vec::new(),
                    fixed_bricks: Vec::new(),
                    ground: Vec::new(),
                    groups: Vec::new(),
                    model: Some(model),
                },
                notes,
            ))
        }
    })
    .await??;
    // Exact supplied meshes remain pinned. Missing/parametric catalog parts are
    // resolved once through the persistent local/S3 asset cache before placement.
    for part in scene.meshes.keys().cloned().collect::<Vec<_>>() {
        if scene.meshes[&part].quality != "ldraw" {
            let asset = s.parts.resolve(&part).await?;
            if let Some(warning) = asset.warning
                && notes.len() < 32
            {
                notes.push(format!("{part}: {warning}"));
            }
            if asset.mesh.quality == "ldraw" {
                scene.meshes.insert(part, asset.mesh);
            }
        }
    }
    for brick in &mut scene.bricks {
        let mesh = &scene.meshes[&brick.part];
        brick.bounds = mesh
            .body_bounds
            .transformed(brick.transform, brick.position);
        brick.render_bounds = mesh.bounds.transformed(brick.transform, brick.position);
    }
    if let Some(name) = name.filter(|s| !s.trim().is_empty()) {
        scene.name = name.chars().take(120).collect();
    }
    if !is_json || placed {
        let mut delta = s.geo.to_world(latitude, longitude)?;
        let bounds = ergo_world::bounds(&scene.bricks).context("Empty model")?;
        delta -= glam::DVec3::new(bounds.min.x, bounds.min.y, bounds.max.z);
        if let Some(model) = &mut scene.model {
            model.apply_transform(ergo_world::assembly::Transform::from_pose(
                glam::DMat3::IDENTITY,
                ergo_world::ldraw::conversion().inverse() * delta,
            ))?;
        }
        for brick in &mut scene.bricks {
            brick.translated(delta);
        }
        for joint in &mut scene.joints {
            joint.anchor += delta;
        }
        scene.geo = s.geo;
    }
    ergo_world::catalog::validate(&scene.bricks)?;
    let pairs: std::collections::HashSet<_> = scene
        .joints
        .iter()
        .map(|j| (j.a.min(j.b), j.a.max(j.b)))
        .collect();
    scene.joints.extend(
        ergo_world::constraints::inferred_joints(&scene.bricks)
            .into_iter()
            .filter(|j| !pairs.contains(&(j.a.min(j.b), j.a.max(j.b)))),
    );
    if let Err(error) = s.store.calibrate(&mut scene) {
        notes.push(error.to_string());
    }
    Ok(Json(ImportPreview { scene, notes }))
}

async fn snapshot(
    State(s): State<AppState>,
    headers: HeaderMap,
    Json(mut scene): Json<WorldExport>,
) -> Result<Json<WorldManifest>, ApiError> {
    let owner = s.auth.check_write(&headers)?;
    import::validate_export(&mut scene)?;
    Ok(Json(
        s.store
            .save(scene, "Saved workspace".into(), Vec::new(), owner)
            .await?,
    ))
}

#[tokio::main]
async fn main() -> Result<()> {
    dotenvy::dotenv().ok();
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "ergo_server=info,tower_http=info".into()),
        )
        .init();
    let store = Store::from_env()?;
    let mut args = env::args().skip(1);
    if let Some(command) = args.next() {
        let id = args.next().context("Expected a shared-world UUID")?;
        if command == "user-plan" {
            let plan = args
                .next()
                .context("Usage: ergo-server user-plan USER_ID free|paid")?;
            anyhow::ensure!(
                args.next().is_none() && ["free", "paid"].contains(&plan.as_str()),
                "Usage: ergo-server user-plan USER_ID free|paid"
            );
            shared::pacing::Pacing::from_env(store)?
                .set_unlimited(&id, plan == "paid")
                .await?;
            println!("Updated edit plan for {id}: {plan}");
            return Ok(());
        }
        if command == "private-rebuild-catalog" {
            anyhow::ensure!(args.next().is_none(), "Unexpected argument");
            catalog::rebuild(&store, &id).await?;
            return Ok(());
        }
        let shared = shared::SharedWorlds::from_env(store.clone())?;
        let repo = shared.repo(&id)?;
        match command.as_str() {
            "shared-gc" => {
                anyhow::ensure!(
                    args.next().is_none(),
                    "Usage: ergo-server shared-gc WORLD_ID"
                );
                println!(
                    "{}",
                    serde_json::to_string_pretty(&shared.collect(&id).await?)?
                );
            }
            "shared-maintenance-status" => {
                anyhow::ensure!(
                    args.next().is_none(),
                    "Usage: ergo-server shared-maintenance-status WORLD_ID"
                );
                println!(
                    "{}",
                    serde_json::to_string_pretty(&repo.maintenance_status().await?)?
                );
            }
            "shared-prune" => {
                anyhow::ensure!(
                    args.next().is_none(),
                    "Usage: ergo-server shared-prune WORLD_ID"
                );
                println!(
                    "{}",
                    serde_json::to_string_pretty(&shared.prune(&id).await?)?
                );
            }
            "shared-index-history" => {
                anyhow::ensure!(args.next().is_none(), "Unexpected argument");
                let mut maintenance = repo.maintenance().await?;
                maintenance.index_history("main").await?;
                maintenance.finish().await?;
            }
            "shared-rebuild-catalog" => {
                anyhow::ensure!(args.next().is_none(), "Unexpected argument");
                shared.rebuild_catalog(&id).await?;
            }
            "shared-upgrade-rules" => {
                anyhow::ensure!(args.next().is_none(), "Unexpected argument");
                println!("{}", shared.upgrade_rules(&id).await?);
            }
            "shared-upgrade-spatial" => {
                anyhow::ensure!(args.next().is_none(), "Unexpected argument");
                println!("{}", shared.upgrade_spatial(&id).await?);
            }
            "shared-compact" => {
                let region: ergo_transactions::OctreePath = args
                    .next()
                    .context("Expected an octree region path")?
                    .parse()?;
                let max_nodes = args
                    .next()
                    .map(|s| s.parse())
                    .transpose()?
                    .unwrap_or(10_000);
                anyhow::ensure!(
                    args.next().is_none(),
                    "Usage: ergo-server shared-compact WORLD_ID REGION [MAX_NODES]"
                );
                let mut maintenance = repo.maintenance().await?;
                tracing::info!(
                    token = maintenance.token(),
                    "Bounded shared-world compaction barrier acquired"
                );
                let result = maintenance.compact_region("main", &region, max_nodes).await;
                let release = maintenance.finish().await;
                if let Err(error) = result {
                    if let Err(release) = release {
                        tracing::warn!(error = %release, "Maintenance remains fenced");
                    }
                    return Err(error.into());
                }
                release?;
            }
            "shared-recover-pin" | "shared-recover-maintenance" => {
                let token = args.next().context("Expected a pin ID or maintenance token; interrupted holders and requests must already be stopped/settled")?;
                anyhow::ensure!(args.next().is_none(), "Unexpected extra argument");
                if command == "shared-recover-pin" {
                    repo.recover_abandoned_pin(&token).await?;
                } else {
                    repo.recover_maintenance(&token).await?;
                }
            }
            _ => anyhow::bail!("Unknown server command: {command}"),
        }
        return Ok(());
    }
    let auth = auth::Auth::from_env()?;
    if !auth.google {
        auth::seed_development_user(&store).await?;
        tracing::info!("Authentication: local development account");
    }
    let geo = GeoReference {
        latitude: env::var("ERGO_ORIGIN_LAT")
            .ok()
            .map(|s| s.parse())
            .transpose()?
            .unwrap_or(48.8566),
        longitude: env::var("ERGO_ORIGIN_LON")
            .ok()
            .map(|s| s.parse())
            .transpose()?
            .unwrap_or(2.3522),
        arcseconds_per_block: env::var("ERGO_ARCSECONDS_PER_BLOCK")
            .ok()
            .map(|s| s.parse())
            .transpose()?
            .unwrap_or(1.0),
    };
    geo.validate()?;
    if store.kind == "Local object store"
        && store
            .manifests(&auth::Principal { id: None })
            .await?
            .is_empty()
    {
        for (name, demo) in [
            ("Paris garden pavilion", ergo_world::demo::pavilion()),
            ("Gravity laboratory", ergo_world::demo::physics_lab()),
        ] {
            let joints = if name.contains("pavilion") {
                ergo_world::demo::authored_bonds(&demo.bricks)
            } else {
                Vec::new()
            };
            let scene = WorldExport {
                schema: SCHEMA_VERSION,
                name: name.into(),
                geo,
                bricks: demo.bricks,
                meshes: demo.meshes,
                joints,
                fixed_bricks: Vec::new(),
                ground: Vec::new(),
                groups: Vec::new(),
                model: None,
            };
            store.save(scene, "Built-in example".into(), vec!["Catalog parts use simplified solid-body geometry; studs are visual. The pavilion has authored fixed bonds on touching faces.".into()],"examples".into()).await?;
        }
    }
    let parts = store.part_cache()?;
    let ground = store.ground_cache()?;
    let state = AppState {
        shared: shared::SharedWorlds::from_env(store.clone())?,
        store,
        geo,
        library: env::var_os("LDRAW_DIR").map(PathBuf::from),
        auth,
        parts,
        ground,
    };
    let app = Router::new()
        .merge(shared::routes())
        .route("/api/config", get(config))
        .route(
            "/api/catalog",
            get(|| async { Json(ergo_world::catalog::catalog()) }),
        )
        .route("/api/worlds", get(worlds).post(snapshot))
        .route("/api/parts/{part}", get(part_asset))
        .route("/api/ground/{x}/{y}", get(ground_block))
        .route("/api/worlds/{id}", get(manifest))
        .route("/api/worlds/{id}/export", get(export))
        .route("/api/worlds/{id}/objects/{hash}", get(object))
        .route("/api/import", post(upload))
        .route("/api/auth/me", get(auth::me))
        .route("/api/auth/logout", post(auth::logout))
        .route("/api/auth/mcp-token", post(auth::mcp_token))
        .route("/auth/google", get(auth::login))
        .route("/auth/google/callback", get(auth::callback))
        .route_service("/app", ServeFile::new("web/index.html"))
        .route_service("/login", ServeFile::new("web/index.html"))
        .fallback_service(ServeDir::new("web").append_index_html_on_directories(true))
        .layer(DefaultBodyLimit::max(128 * 1024 * 1024))
        .layer(CompressionLayer::new())
        .layer(TraceLayer::new_for_http())
        .with_state(state);
    let addr = env::var("ERGO_BIND").unwrap_or_else(|_| "127.0.0.1:3000".into());
    let listener = tokio::net::TcpListener::bind(&addr).await?;
    tracing::info!("Ergo listening at http://{addr}");
    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await?;
    Ok(())
}

async fn shutdown_signal() {
    #[cfg(unix)]
    {
        let mut terminate =
            tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
                .expect("Install SIGTERM handler");
        tokio::select! {
            _ = tokio::signal::ctrl_c() => {},
            _ = terminate.recv() => {},
        }
    }
    #[cfg(not(unix))]
    tokio::signal::ctrl_c().await.ok();
}
