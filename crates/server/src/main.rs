mod auth;
mod catalog;
mod import;
mod parts;
mod positions;
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
use std::{collections::BTreeMap, env};
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
    /// The installed part library's documents and its synthesis job.
    library: parts::Parts,
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
    version: &'static str,
    part_library: &'static str,
    part_library_format: &'static str,
    construction_format: &'static str,
    model_format: &'static str,
    authoring_format: &'static str,
    edit_format: &'static str,
    common_world: Option<String>,
    storage: String,
    geo: GeoReference,
    block_size: u32,
    block_height_plates: u32,
    simulation_limit: usize,
    ground_source: &'static str,
    ground_provider: String,
    ground_revision: String,
}

async fn config(State(s): State<AppState>) -> Json<Config> {
    Json(Config {
        version: env!("CARGO_PKG_VERSION"),
        part_library: &ergo_world::part_library::library().revision,
        part_library_format: ergo_world::part_library::FORMAT,
        construction_format: ergo_world::storage::FORMAT,
        model_format: ergo_world::assembly::FORMAT,
        authoring_format: "ergo-workspace-2",
        edit_format: ergo_world::authoring::FORMAT,
        common_world: s.shared.common_world.clone(),
        storage: s.store.kind,
        geo: s.geo,
        block_size: ergo_world::BLOCK_SIZE as u32,
        block_height_plates: ergo_world::BLOCK_HEIGHT_PLATES,
        simulation_limit: ergo_world::MAX_SIMULATION_BRICKS,
        ground_source: if s.ground.is_enabled() {
            "raster"
        } else {
            "disabled"
        },
        ground_provider: s.ground.provider().to_owned(),
        ground_revision: s
            .ground
            .cache_revision(s.geo)
            .expect("Validated ground configuration"),
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
    if s.shared.common_world.is_some() {
        return Err(anyhow::anyhow!(
            "Only the shared Ergo Sandbox is available; use /api/shared-worlds"
        )
        .into());
    }
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
    headers: HeaderMap,
    Path(part): Path<String>,
) -> Result<Response, ApiError> {
    let part = if part.ends_with(".dat") {
        part
    } else {
        format!("{part}.dat")
    };
    let asset = s.parts.resolve(&part).await?;
    // Immutable mesh plus this library revision's catalogue record.
    let etag = Some(format!(
        "W/\"part-{}-{}\"",
        asset.entry.mesh_hash, asset.entry.revision
    ));
    let unchanged = etag.as_ref().is_some_and(|etag| {
        headers
            .get("if-none-match")
            .and_then(|value| value.to_str().ok())
            .is_some_and(|values| {
                values
                    .split(',')
                    .any(|value| value.trim() == etag || value.trim() == "*")
            })
    });
    let mut response = if unchanged {
        StatusCode::NOT_MODIFIED.into_response()
    } else {
        Json(asset).into_response()
    };
    if let Some(etag) = etag {
        response.headers_mut().insert(header::ETAG, etag.parse()?);
        response
            .headers_mut()
            .insert(header::CACHE_CONTROL, "public, max-age=60".parse()?);
    } else {
        response
            .headers_mut()
            .insert(header::CACHE_CONTROL, "no-store".parse()?);
    }
    Ok(response)
}

async fn search_parts(
    Query(query): Query<ergo_world::catalog::SearchQuery>,
) -> Result<Json<ergo_world::catalog::SearchPage>, ApiError> {
    Ok(Json(ergo_world::catalog::search(&query)?))
}

/// Immutable content-addressed geometry of one installed library part. Only
/// hashes named by the library are served; world objects stay authorized.
async fn part_object(
    State(s): State<AppState>,
    Path(hash): Path<String>,
) -> Result<Response, ApiError> {
    if ergo_world::part_library::by_mesh_hash(&hash).is_none() {
        return Ok(StatusCode::NOT_FOUND.into_response());
    }
    let mut response = s.parts.object(&hash).await?.into_response();
    let headers = response.headers_mut();
    headers.insert(header::CONTENT_TYPE, "application/cbor".parse()?);
    headers.insert(header::ETAG, format!("\"{hash}\"").parse()?);
    headers.insert(
        header::CACHE_CONTROL,
        "public, max-age=31536000, immutable".parse()?,
    );
    Ok(response)
}

async fn part_library_file(
    State(s): State<AppState>,
    headers: HeaderMap,
    Path((revision, file)): Path<(String, String)>,
) -> Response {
    let documents = s.library.documents();
    if revision != documents.revision {
        return StatusCode::NOT_FOUND.into_response();
    }
    let (bytes, mime) = match file.as_str() {
        "library.json" => (&*documents.library, "application/json"),
        "reviewed.json" => (&*documents.reviewed, "application/json"),
        _ => return StatusCode::NOT_FOUND.into_response(),
    };
    let etag = format!("\"{revision}-{file}\"");
    let unchanged = headers
        .get("if-none-match")
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| {
            value
                .split(',')
                .any(|value| value.trim() == etag || value.trim() == "*")
        });
    let mut response = if unchanged {
        StatusCode::NOT_MODIFIED.into_response()
    } else {
        bytes.to_vec().into_response()
    };
    response
        .headers_mut()
        .insert(header::ETAG, etag.parse().unwrap());
    response
        .headers_mut()
        .insert(header::CONTENT_TYPE, mime.parse().unwrap());
    response.headers_mut().insert(
        header::CACHE_CONTROL,
        "public, max-age=31536000, immutable".parse().unwrap(),
    );
    response
}

#[derive(Deserialize)]
struct GroundQuery {
    latitude: f64,
    longitude: f64,
    arcseconds_per_block: f64,
}
async fn ground_block(
    State(s): State<AppState>,
    headers: HeaderMap,
    Path((x, y)): Path<(i64, i64)>,
    Query(q): Query<GroundQuery>,
) -> Response {
    let block = s
        .ground
        .block(
            GeoReference {
                latitude: q.latitude,
                longitude: q.longitude,
                arcseconds_per_block: q.arcseconds_per_block,
            },
            x,
            y,
        )
        .await;
    if block.fallback_reason.is_some() {
        let mut response = Json(block).into_response();
        response
            .headers_mut()
            .insert(header::CACHE_CONTROL, "no-store".parse().unwrap());
        return response;
    }
    let bytes = serde_json::to_vec(&block).expect("Serializable ground field");
    let etag = format!("\"ground-{}\"", ergo_transactions::ContentId::of(&bytes));
    let unchanged = headers
        .get("if-none-match")
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| {
            value
                .split(',')
                .any(|value| value.trim() == etag || value.trim() == "*")
        });
    let mut response = if unchanged {
        StatusCode::NOT_MODIFIED.into_response()
    } else {
        bytes.into_response()
    };
    response
        .headers_mut()
        .insert(header::CONTENT_TYPE, "application/json".parse().unwrap());
    response
        .headers_mut()
        .insert(header::ETAG, etag.parse().unwrap());
    response.headers_mut().insert(
        header::CACHE_CONTROL,
        "public, max-age=300, must-revalidate".parse().unwrap(),
    );
    response
}
async fn manifest(
    State(s): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<Json<WorldManifest>, ApiError> {
    if s.shared.common_world.is_some() {
        return Err(anyhow::anyhow!("Alternative snapshot worlds are not available").into());
    }
    Ok(Json(
        s.store.manifest(&id, &s.auth.principal(&headers)).await?,
    ))
}

async fn object(
    State(s): State<AppState>,
    headers: HeaderMap,
    Path((id, hash)): Path<(String, String)>,
) -> Result<Response, ApiError> {
    if s.shared.common_world.is_some() {
        return Err(anyhow::anyhow!("Alternative snapshot worlds are not available").into());
    }
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
    if s.shared.common_world.is_some() {
        return Err(
            anyhow::anyhow!("Export a bounded construction from the shared Ergo Sandbox").into(),
        );
    }
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
    let (scene, parsed) = tokio::task::spawn_blocking(move || -> Result<_> {
        if is_json {
            let mut scene =
                ergo_world::storage::decode(&bytes).context("Invalid Ergo scene JSON")?;
            import::validate_export(&mut scene)?;
            Ok((Some(scene), None))
        } else {
            Ok((
                None,
                Some((filename.clone(), import::parse_ldraw(&bytes, &filename)?)),
            ))
        }
    })
    .await??;
    let (mut scene, mut notes) = if let Some(scene) = scene {
        (scene, Vec::new())
    } else {
        let (filename, parsed) = parsed.context("Parsed LDraw document")?;
        let mut library = BTreeMap::new();
        for part in parsed.library_parts() {
            library.insert(part.clone(), s.parts.resolve(&part).await?.mesh);
        }
        let import::ImportedLdraw {
            bricks,
            meshes,
            notes,
            model,
        } = tokio::task::spawn_blocking(move || import::assemble(parsed, &library)).await??;
        (
            WorldExport {
                schema: SCHEMA_VERSION,
                name: filename,
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
        )
    };
    // Exact supplied meshes remain pinned. Missing/parametric catalog parts are
    // resolved once through the persistent local/S3 asset cache before placement.
    for part in scene.meshes.keys().cloned().collect::<Vec<_>>() {
        if scene.meshes[&part].quality != "source" {
            let asset = s.parts.resolve(&part).await?;
            if let Some(warning) = asset.warning
                && notes.len() < 32
            {
                notes.push(format!("{part}: {warning}"));
            }
            if asset.mesh.quality == "source" {
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
    let snap_delta = ergo_world::placement::snap_scene(&mut scene)?;
    if snap_delta != glam::DVec3::ZERO {
        notes.push(format!(
            "Snapped whole-model placement by {:?} to the stud/plate grid",
            ergo_world::coordinates::public(snap_delta)
        ));
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
    if s.shared.common_world.is_some() {
        return Err(anyhow::anyhow!(
            "Private snapshots are disabled; commit to the shared Ergo Sandbox"
        )
        .into());
    }
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
    // `.env` from the working directory or its parents; variables already set
    // in the process environment win. Reported once logging is configured.
    let dotenv = dotenvy::dotenv();
    tracing_subscriber::fmt()
        // Operator subcommands print machine-readable JSON on stdout. Keep
        // progress diagnostics on stderr so profiles remain parseable.
        .with_writer(std::io::stderr)
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "ergo_server=info,tower_http=info".into()),
        )
        .init();
    match &dotenv {
        Ok(path) => tracing::info!(path = %path.display(), "Loaded environment file"),
        Err(error) if error.not_found() => tracing::info!("No .env file found"),
        Err(error) => {
            tracing::warn!(%error, "Could not load .env; continuing with the process environment")
        }
    }
    let store = Store::from_env()?;
    let mut warmed_shared = None;
    let mut args = env::args().skip(1);
    let command = args.next();
    if let Some(command @ ("parts-sync" | "parts-check")) = command.as_deref() {
        let option = args.next();
        anyhow::ensure!(
            args.next().is_none() && option.as_deref().is_none_or(|o| o == "--dry-run"),
            "Usage: ergo-server parts-sync [--dry-run] | ergo-server parts-check"
        );
        let report = if command == "parts-check" {
            let workers = std::thread::available_parallelism().map_or(4, |n| n.get());
            ergo_assets::library::check(store.backend.clone(), &store.prefix, workers).await?
        } else {
            parts::run_sync(&store, option.is_some()).await?.report
        };
        println!("{}", serde_json::to_string_pretty(&report)?);
        return Ok(());
    }
    if command.as_deref() == Some("parts-publish") {
        let (Some(library), Some(objects), None) = (args.next(), args.next(), args.next()) else {
            anyhow::bail!("Usage: ergo-server parts-publish LIBRARY.json OBJECTS.cbor.gz");
        };
        let library = ergo_assets::library::publish_bundle(
            &*store.backend,
            &store.prefix,
            &std::fs::read(library)?,
            &std::fs::read(objects)?,
        )
        .await?;
        println!(
            "{}",
            serde_json::json!({"revision": library.revision, "parts": library.parts.len()})
        );
        return Ok(());
    }
    let library = parts::Parts::bootstrap(&store).await?;
    if let Some(command) = command {
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
            "shared-profile-workspace" => {
                let assembly = args.next().context("Expected an assembly ID")?.parse()?;
                let actor = args.next().context("Expected an actor ID")?;
                let seconds = args.next().map(|s| s.parse()).transpose()?.unwrap_or(600);
                anyhow::ensure!(
                    args.next().is_none(),
                    "Usage: ergo-server shared-profile-workspace WORLD_ID ASSEMBLY_ID ACTOR_ID [SECONDS]"
                );
                println!(
                    "{}",
                    serde_json::to_string_pretty(
                        &shared
                            .profile_workspace(&id, &assembly, &actor, seconds)
                            .await?
                    )?
                );
            }
            "shared-profile-dependencies" => {
                let assembly = args.next().context("Expected an assembly ID")?.parse()?;
                let mut rest = args.collect::<Vec<_>>();
                let serve = rest.last().is_some_and(|s| s == "--serve");
                if serve {
                    rest.pop();
                }
                anyhow::ensure!(
                    rest.len() <= 1,
                    "Usage: ergo-server shared-profile-dependencies WORLD_ID ASSEMBLY_ID [SECONDS] [--serve]"
                );
                let seconds = rest.first().map(|s| s.parse()).transpose()?.unwrap_or(120);
                if serve {
                    let bind = env::var("ERGO_BIND")
                        .context("--serve requires an explicit loopback ERGO_BIND")?
                        .parse::<std::net::SocketAddr>()?;
                    anyhow::ensure!(
                        bind.ip().is_loopback()
                            && shared.common_world.as_deref() == Some(id.as_str()),
                        "Profile --serve requires loopback and the configured common world"
                    );
                }
                println!(
                    "{}",
                    serde_json::to_string_pretty(
                        &shared.profile_dependencies(&id, &assembly, seconds).await?
                    )?
                );
                if serve {
                    warmed_shared = Some(shared);
                }
            }
            "shared-part-inventory" => {
                anyhow::ensure!(
                    args.next().is_none(),
                    "Usage: ergo-server shared-part-inventory WORLD_ID"
                );
                println!(
                    "{}",
                    serde_json::to_string_pretty(&shared.part_inventory(&id).await?)?
                );
            }
            "shared-pack-world" => {
                let option = args.next();
                anyhow::ensure!(
                    args.next().is_none() && option.as_deref().is_none_or(|s| s == "--check"),
                    "Usage: ergo-server shared-pack-world WORLD_ID [--check]"
                );
                println!(
                    "{}",
                    serde_json::to_string_pretty(&shared.pack_world(&id, option.is_some()).await?)?
                );
            }
            "shared-pack-block" => {
                let mut coord = [0i64; 3];
                for value in &mut coord {
                    *value = args
                        .next()
                        .context("Usage: ergo-server shared-pack-block WORLD_ID X Y Z")?
                        .parse()?;
                }
                anyhow::ensure!(args.next().is_none(), "Unexpected argument");
                println!(
                    "{}",
                    serde_json::to_string_pretty(&shared.pack_block(&id, coord).await?)?
                );
            }
            "shared-pack-region" => {
                let mut coord = [0i64; 3];
                for value in &mut coord {
                    *value = args
                        .next()
                        .context("Usage: ergo-server shared-pack-region WORLD_ID X Y Z [RADIUS]")?
                        .parse()?;
                }
                let radius = args
                    .next()
                    .map(|value| value.parse::<u8>())
                    .transpose()
                    .context("Usage: ergo-server shared-pack-region WORLD_ID X Y Z [RADIUS]")?
                    .unwrap_or(1);
                anyhow::ensure!(args.next().is_none(), "Unexpected argument");
                println!(
                    "{}",
                    serde_json::to_string_pretty(&shared.pack_region(&id, coord, radius).await?)?
                );
            }
            "shared-warm-view" => {
                let mut coord = [0i64; 3];
                for value in &mut coord {
                    *value = args
                        .next()
                        .context("Usage: ergo-server shared-warm-view WORLD_ID X Y Z [RADIUS]")?
                        .parse()?;
                }
                let radius = args
                    .next()
                    .map(|value| value.parse::<u8>())
                    .transpose()
                    .context("Usage: ergo-server shared-warm-view WORLD_ID X Y Z [RADIUS]")?
                    .unwrap_or(1);
                anyhow::ensure!(args.next().is_none(), "Unexpected argument");
                println!(
                    "{}",
                    serde_json::to_string_pretty(&shared.warm_view(&id, coord, radius).await?)?
                );
            }
            "shared-install-history" => {
                let path = args
                    .next()
                    .context("Usage: ergo-server shared-install-history WORLD_ID MANIFEST.json")?;
                anyhow::ensure!(args.next().is_none(), "Unexpected argument");
                println!(
                    "{}",
                    serde_json::to_string_pretty(
                        &shared
                            .install_history(&id, std::path::Path::new(&path))
                            .await?
                    )?
                );
            }
            "shared-migrate" => {
                let target = args
                    .next()
                    .context("Usage: ergo-server shared-migrate WORLD_ID TARGET_PREFIX")?;
                anyhow::ensure!(args.next().is_none(), "Unexpected argument");
                println!(
                    "{}",
                    serde_json::to_string_pretty(&shared.migrate_namespace(&id, &target).await?)?
                );
            }
            "shared-resize-cells" => {
                anyhow::ensure!(
                    args.next().is_none(),
                    "Usage: ergo-server shared-resize-cells WORLD_ID"
                );
                println!(
                    "{}",
                    serde_json::to_string_pretty(&shared.resize_cells(&id).await?)?
                );
                return Ok(());
            }
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
            "shared-rebuild-catalog" => {
                anyhow::ensure!(args.next().is_none(), "Unexpected argument");
                shared.rebuild_catalog(&id).await?;
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
        if warmed_shared.is_none() {
            return Ok(());
        }
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
    let isolated = env::var("ERGO_ISOLATED_FIXTURES").as_deref() == Ok("1");
    anyhow::ensure!(
        !isolated || store.kind == "Local object store",
        "Isolated fixtures are only supported in disposable local test stores"
    );
    let mut shared = match warmed_shared {
        Some(shared) => shared,
        None => shared::SharedWorlds::from_env(store.clone())?,
    };
    if shared.common_world.is_none() && !isolated {
        anyhow::ensure!(
            !auth.google && store.kind == "Local object store",
            "Configure ERGO_COMMON_WORLD_ID for the hosted Ergo Sandbox"
        );
        shared.bootstrap_local(geo).await?;
    }
    if let Some(id) = &shared.common_world {
        let head = shared
            .head(
                id,
                &auth::Principal {
                    id: Some("public-viewer".into()),
                },
                None,
            )
            .await?;
        anyhow::ensure!(
            head.policy.authenticated_editing,
            "The shared Ergo Sandbox must allow signed-in editors"
        );
        anyhow::ensure!(
            head.metadata
                .id
                .as_deref()
                .is_none_or(|stored| stored == id),
            "Configured sandbox ID does not match the stored namespace"
        );
    }
    if isolated
        && shared.common_world.is_none()
        && store.kind == "Local object store"
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
            let mut scene = WorldExport {
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
            let parts = store.part_cache()?;
            for (part, mesh) in &mut scene.meshes {
                *mesh = parts.resolve(part).await?.mesh;
            }
            for brick in &mut scene.bricks {
                let mesh = &scene.meshes[&brick.part];
                brick.bounds = mesh
                    .body_bounds
                    .transformed(brick.transform, brick.position);
                brick.render_bounds = mesh.bounds.transformed(brick.transform, brick.position);
            }
            store.save(scene, "Built-in example".into(), vec!["Canonical source geometry with nominal solid-body collision proxies. The pavilion has authored fixed bonds on touching faces.".into()],"examples".into()).await?;
        }
    }
    let parts = store.part_cache()?;
    let ground = store.ground_cache()?;
    let state = AppState {
        shared,
        store,
        geo,
        library,
        auth,
        parts,
        ground,
    };
    let app = Router::new()
        .merge(shared::routes())
        .route("/api/config", get(config))
        .route(
            "/api/user-position",
            get(positions::get).put(positions::put),
        )
        .route(
            "/api/catalog",
            get(|| async { Json(ergo_world::catalog::catalog()) }),
        )
        .route("/api/worlds", get(worlds).post(snapshot))
        .route("/api/catalog/parts", get(search_parts))
        .route("/api/parts/{part}", get(part_asset))
        .route("/api/part-objects/{hash}", get(part_object))
        .route("/api/library/{revision}/{file}", get(part_library_file))
        .route(
            "/api/admin/parts-sync",
            get(parts::status).post(parts::start),
        )
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
        .route_service("/catalogue", ServeFile::new("web/catalogue.html"))
        .route_service("/catalogue/", ServeFile::new("web/catalogue.html"))
        .route_service("/mcp-inspector", ServeFile::new("web/inspector.html"))
        .route_service("/mcp-inspector/", ServeFile::new("web/inspector.html"))
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
