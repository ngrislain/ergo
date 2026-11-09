//! Thin stdio/REST bridge. Rendering and all time-integrated simulations execute
//! in the dedicated local browser WASM client, never in this process.
mod models;
mod publish;
mod shared;
use anyhow::{Context, Result, ensure};
use axum::{
    Json, Router,
    extract::{DefaultBodyLimit, Path, State},
    http::{HeaderMap, HeaderValue, StatusCode, header},
    response::IntoResponse,
    routing::{get, post},
};
use ergo_world::{WorldExport, physical::ConnectionMeasurements};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    collections::{HashMap, VecDeque},
    fs::{self, OpenOptions},
    io::{self, BufRead, Write},
    path::{Path as FsPath, PathBuf},
    process::{Child, Command, Stdio},
    sync::{Arc, Mutex, mpsc},
    time::Duration,
};
use tower_http::{services::ServeDir, set_header::SetResponseHeaderLayer};

#[derive(Clone, Serialize)]
struct Job {
    id: String,
    name: String,
    arguments: Value,
    revision: u32,
}
struct Workspace {
    scene: WorldExport,
    revision: u32,
    path: PathBuf,
    measurements: ConnectionMeasurements,
}
struct Hub {
    workspace: Workspace,
    queue: VecDeque<Job>,
    pending: HashMap<String, mpsc::Sender<Value>>,
}
#[derive(Clone)]
struct Bridge {
    hub: Arc<Mutex<Hub>>,
    token: String,
    parts: ergo_assets::PartCache,
    /// Verified JSON of the installed part library, for the local WASM runtime.
    part_library: Arc<[u8]>,
    publisher: Option<Arc<publish::Publisher>>,
    common: Option<Arc<tokio::sync::Mutex<shared::Binding>>>,
}
type HttpError = (StatusCode, Json<Value>);
const DEFAULT_ORIGIN: &str = "https://ergosandbox.com";
fn tool_schemas(common: bool) -> Value {
    let mut value = ergo_world::mcp::tools();
    for tool in value["tools"].as_array_mut().unwrap() {
        if tool["name"] == "get_lego_models" {
            tool["annotations"]["openWorldHint"] = json!(true);
        }
    }
    if common {
        for tool in value["tools"].as_array_mut().unwrap() {
            if tool["name"] == "publish_construction" {
                let properties = tool["inputSchema"]["properties"].as_object_mut().unwrap();
                properties.remove("world_id");
                properties.remove("authenticated_editing");
                tool["description"] = json!(
                    "Confirm the last shared edit, rename the current shared construction, or submit an existing validated proposal to the single public Ergo Sandbox. Normal edits commit automatically. No private/alternative world can be created."
                );
            }
        }
    }
    value
}
fn http_error(e: impl ToString) -> HttpError {
    (
        StatusCode::BAD_REQUEST,
        Json(json!({"error":e.to_string()})),
    )
}
fn authorized(state: &Bridge, headers: &HeaderMap) -> std::result::Result<(), HttpError> {
    if headers.get("X-Ergo-Client").and_then(|h| h.to_str().ok()) == Some(&state.token) {
        Ok(())
    } else {
        Err((
            StatusCode::UNAUTHORIZED,
            Json(json!({"error":"Invalid local client capability"})),
        ))
    }
}

async fn workspace(
    State(s): State<Bridge>,
    headers: HeaderMap,
) -> std::result::Result<Json<Value>, HttpError> {
    authorized(&s, &headers)?;
    let (floor, group_floor) = if let Some(common) = &s.common {
        let b = common.lock().await;
        (b.next_local_id, b.next_group_id)
    } else {
        (1, 1)
    };
    let h = s.hub.lock().unwrap();
    Ok(Json(
        json!({"scene":h.workspace.scene,"revision":h.workspace.revision,"measurements":h.workspace.measurements,"next_local_id":floor,"next_group_id":group_floor}),
    ))
}
#[derive(Deserialize)]
struct Update {
    job_id: String,
    expected_revision: u32,
    scene: WorldExport,
}
async fn update(
    State(s): State<Bridge>,
    headers: HeaderMap,
    Json(mut update): Json<Update>,
) -> std::result::Result<Json<Value>, HttpError> {
    authorized(&s, &headers)?;
    let measurements = s.hub.lock().unwrap().workspace.measurements.clone();
    normalize(&mut update.scene, &measurements).map_err(http_error)?;
    {
        let h = s.hub.lock().unwrap();
        if !h.pending.contains_key(&update.job_id)
            || h.workspace.revision != update.expected_revision
        {
            return Err((
                StatusCode::CONFLICT,
                Json(json!({"error":"Job expired or shared cache changed"})),
            ));
        }
    }
    let mut remote = None;
    let mut cache_warning = None;
    if let Some(common) = &s.common {
        let mut binding = common.lock().await;
        ensure_update_revision(&s, &update)?;
        let path = s.hub.lock().unwrap().workspace.path.clone();
        let before = s.hub.lock().unwrap().workspace.scene.clone();
        if update.scene.bricks.len() > ergo_world::authoring::RESIDENT_PIECES {
            return Err(http_error(
                "Shared working view exceeds its resident piece limit",
            ));
        }
        if binding.assembly_id.is_some() {
            ergo_world::authoring::Delta::between(&before, &update.scene, &binding.versions)
                .map_err(http_error)?;
        }
        let mut journal = binding.clone();
        journal.pending = true;
        atomic_write(&shared::state_path(&path), &journal).map_err(http_error)?;
        let result = s
            .publisher
            .as_ref()
            .unwrap()
            .commit_shared(
                &mut binding,
                &before,
                update.scene.clone(),
                update.expected_revision + 1,
            )
            .await;
        let saved = atomic_write(&shared::state_path(&path), &*binding);
        remote = Some(result.map_err(|error| http_error(format!("{error:#}")))?);
        if let Err(error) = saved {
            cache_warning = Some(format!(
                "Shared edit confirmed; local binding cache could not be flushed: {error}"
            ));
        }
    }
    let mut h = s.hub.lock().unwrap();
    if !h.pending.contains_key(&update.job_id) || h.workspace.revision != update.expected_revision {
        return Err((
            StatusCode::CONFLICT,
            Json(json!({"error":"Job expired or working-copy revision changed"})),
        ));
    }
    let saved = if s.common.is_some() {
        persist_page(&h.workspace.path, &update.scene)
    } else {
        persist(&h.workspace.path, &update.scene)
    };
    if let Err(error) = saved {
        if remote.is_none() {
            return Err(http_error(error));
        }
        cache_warning = Some(format!(
            "Shared edit confirmed; local scene cache could not be flushed: {error}"
        ));
    }
    h.workspace.scene = update.scene;
    h.workspace.revision += 1;
    Ok(Json(
        json!({"revision":h.workspace.revision,"shared_commit":remote,"cache_warning":cache_warning}),
    ))
}
fn ensure_update_revision(s: &Bridge, update: &Update) -> std::result::Result<(), HttpError> {
    let h = s.hub.lock().unwrap();
    if h.workspace.revision != update.expected_revision || !h.pending.contains_key(&update.job_id) {
        return Err((
            StatusCode::CONFLICT,
            Json(json!({"error":"Shared cache changed before commit"})),
        ));
    }
    Ok(())
}
async fn next_job(
    State(s): State<Bridge>,
    headers: HeaderMap,
) -> std::result::Result<axum::response::Response, HttpError> {
    authorized(&s, &headers)?;
    let mut h = s.hub.lock().unwrap();
    Ok(if let Some(job) = h.queue.pop_front() {
        Json(job).into_response()
    } else {
        StatusCode::NO_CONTENT.into_response()
    })
}
#[derive(Deserialize)]
struct Completion {
    id: String,
    result: Value,
}
async fn complete(
    State(s): State<Bridge>,
    headers: HeaderMap,
    Json(done): Json<Completion>,
) -> std::result::Result<StatusCode, HttpError> {
    authorized(&s, &headers)?;
    if let Some(sender) = s.hub.lock().unwrap().pending.remove(&done.id) {
        let _ = sender.send(done.result);
    }
    Ok(StatusCode::NO_CONTENT)
}
async fn part(
    State(s): State<Bridge>,
    headers: HeaderMap,
    Path(name): Path<String>,
) -> std::result::Result<Json<Value>, HttpError> {
    authorized(&s, &headers)?;
    let name = if name.ends_with(".dat") {
        name
    } else {
        format!("{name}.dat")
    };
    Ok(Json(
        serde_json::to_value(s.parts.resolve(&name).await.map_err(http_error)?)
            .map_err(http_error)?,
    ))
}
async fn serve_part_library(
    State(s): State<Bridge>,
    headers: HeaderMap,
) -> std::result::Result<axum::response::Response, HttpError> {
    authorized(&s, &headers)?;
    let mut response = s.part_library.to_vec().into_response();
    response.headers_mut().insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("application/json"),
    );
    Ok(response)
}
fn persist(path: &FsPath, scene: &WorldExport) -> Result<()> {
    atomic_write(path, &ergo_world::storage::Construction::from(scene))
}
fn persist_page(path: &FsPath, scene: &WorldExport) -> Result<()> {
    atomic_write(
        path,
        &json!({"format":"ergo-authoring-cache-1","schema":ergo_world::SCHEMA_VERSION,"partial":true,"construction":ergo_world::storage::Construction::from(scene)}),
    )
}
fn atomic_write(path: &FsPath, value: &impl Serialize) -> Result<()> {
    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(FsPath::new("."));
    let mut temp = tempfile::NamedTempFile::new_in(parent)?;
    serde_json::to_writer_pretty(&mut temp, value)?;
    temp.flush()?;
    temp.as_file().sync_all()?;
    temp.persist(path)?;
    Ok(())
}
fn normalize(scene: &mut WorldExport, measurements: &ConnectionMeasurements) -> Result<()> {
    ensure!(
        scene.schema == ergo_world::SCHEMA_VERSION,
        "Expected schema 2: X east, Y north, Z height; reimport or explicitly migrate older files"
    );
    scene.geo.validate()?;
    ergo_world::catalog::validate(&scene.bricks)?;
    ergo_world::constraints::validate_joints(&scene.bricks, &scene.joints, &scene.fixed_bricks)?;
    ergo_world::groups::validate(&scene.bricks, &scene.groups)?;
    ensure!(scene.ground.len() <= 128, "Too many ground blocks");
    for block in &scene.ground {
        block.validate()?;
    }
    let mut vertices = 0;
    for (part, mesh) in &mut scene.meshes {
        ensure!(
            mesh.schema == ergo_world::SCHEMA_VERSION
                && mesh.part == *part
                && !mesh.vertices.is_empty()
                && mesh.vertices.len().is_multiple_of(27)
                && mesh.vertices.iter().all(|v| v.is_finite()),
            "Invalid part mesh"
        );
        vertices += mesh.vertices.len() / 9;
        ensure!(vertices <= 4_000_000, "Too much geometry");
        mesh.bounds = mesh
            .vertices
            .chunks_exact(9)
            .map(|v| {
                let p = glam::DVec3::new(v[0] as f64, v[1] as f64, v[2] as f64);
                ergo_world::Aabb::new(p, p)
            })
            .reduce(ergo_world::Aabb::union)
            .unwrap();
        mesh.body_bounds = ergo_world::geometry::nominal_body_bounds(part).unwrap_or(mesh.bounds);
        mesh.physical = measurements
            .parts
            .iter()
            .find(|record| record.part == *part)
            .cloned();
        if let Some(record) = &mesh.physical {
            mesh.body_bounds = record.body_bounds;
        }
    }
    for b in &mut scene.bricks {
        ergo_world::physical::validate_affine_pose(b)?;
        let mesh = scene.meshes.get(&b.part).context("Missing part geometry")?;
        b.bounds = mesh.body_bounds.transformed(b.transform, b.position);
        b.render_bounds = mesh.bounds.transformed(b.transform, b.position);
    }
    if !scene.bricks.is_empty() {
        ergo_world::partition(&scene.bricks)?;
    }
    Ok(())
}

#[cfg(test)]
mod normalization_tests {
    use super::*;
    #[test]
    fn reopening_catalogue_specialties_preserves_nominal_bodies_not_visual_studs() {
        // Synthetic meshes isolate cache normalization; no source geometry or
        // physical acceptance is claimed by this fixture.
        for part in ["60594", "3901", "3829c01"] {
            let mut demo = ergo_world::demo::Demo::new();
            demo.add(part, 14, 0., 0., 0.);
            let mut scene = WorldExport {
                schema: ergo_world::SCHEMA_VERSION,
                name: "Normalization fixture".into(),
                geo: Default::default(),
                bricks: demo.bricks,
                meshes: demo.meshes,
                joints: vec![],
                fixed_bricks: vec![],
                ground: vec![],
                groups: vec![],
                model: None,
            };
            normalize(&mut scene, &Default::default()).unwrap();
            let id = format!("{part}.dat");
            let expected = ergo_world::geometry::nominal_body_bounds(&id).unwrap();
            assert_eq!(scene.meshes[&id].body_bounds.min, expected.min);
            assert_eq!(scene.meshes[&id].body_bounds.max, expected.max);
            let positioned =
                expected.transformed(scene.bricks[0].transform, scene.bricks[0].position);
            assert_eq!(scene.bricks[0].bounds.max, positioned.max);
        }
    }
}

struct Browser(Option<Child>);
impl Drop for Browser {
    fn drop(&mut self) {
        if let Some(child) = &mut self.0 {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}
fn launch(url: &str, headless: bool, attach: bool, profile: &FsPath) -> Result<Browser> {
    if attach {
        eprintln!("Attach the minimal local WASM client: {url}");
        return Ok(Browser(None));
    }
    if headless {
        let executable = std::env::var("ERGO_CHROME_BIN").unwrap_or_else(|_| {
            if cfg!(target_os = "macos") {
                "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome".into()
            } else {
                "google-chrome".into()
            }
        });
        let child = Command::new(executable)
            .args([
                "--headless=new",
                "--no-first-run",
                "--no-default-browser-check",
                "--disable-background-networking",
                "--enable-unsafe-swiftshader",
            ])
            .arg(format!("--user-data-dir={}", profile.display()))
            .arg(url)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .context("Could not launch headless Chrome; set ERGO_CHROME_BIN or use --attach")?;
        Ok(Browser(Some(child)))
    } else {
        #[cfg(target_os = "macos")]
        let status = Command::new("open").arg(url).status()?;
        #[cfg(not(target_os = "macos"))]
        let status = Command::new("xdg-open").arg(url).status()?;
        ensure!(
            status.success(),
            "Could not open local client; use --attach"
        );
        Ok(Browser(None))
    }
}

fn main() -> Result<()> {
    let mut args = std::env::args().skip(1);
    let mut path = PathBuf::from("construction.ergo.json");
    let mut measurements = ConnectionMeasurements::default();
    let mut headless = false;
    let mut attach = false;
    let mut publish_url = None;
    let mut publish_token_file = None;
    let mut login = false;
    let mut local = false;
    let mut web = std::env::var_os("ERGO_WEB_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../web"));
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--workspace" => path = args.next().context("Missing workspace path")?.into(),
            "--measurements" => {
                measurements = serde_json::from_slice(&fs::read(
                    args.next().context("Missing measurements path")?,
                )?)?
            }
            "--headless" => headless = true,
            "--attach" => attach = true,
            "--publish-url" => publish_url = Some(args.next().context("Missing publication URL")?),
            "--login" => login = true,
            "--local" => local = true,
            "--publish-token-file" => {
                publish_token_file = Some(PathBuf::from(
                    args.next().context("Missing publication token file")?,
                ))
            }
            "--web-dir" => web = args.next().context("Missing web directory")?.into(),
            "--help" => {
                eprintln!(
                    "ergo-mcp --workspace path.json [--measurements file.json] [--attach | --headless] [--web-dir web] [--publish-url HTTPS_ORIGIN] [--publish-token-file FILE] [--local]\nergo-mcp --login --publish-url HTTPS_ORIGIN [--publish-token-file FILE]\nMissing or rejected credentials open browser sign-in and securely save the approved token. --login completes that setup and exits; --local disables publishing and login. Heavy work runs in the minimal local WASM client; publication uses the authenticated REST API. Build the client with cargo xtask build-web."
                );
                return Ok(());
            }
            _ => anyhow::bail!("Unknown argument {arg}"),
        }
    }
    if login {
        ensure!(!local, "--login and --local cannot be combined");
        return publish::open_login(publish_url, publish_token_file);
    }
    measurements.validate()?;
    let publisher = if local {
        None
    } else {
        publish::Publisher::from_env(publish_url, publish_token_file)?
    };
    // One continuously driven runtime serves HTTP jobs and remote requests even
    // while the stdio thread waits for a browser result.
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()?;
    ensure!(
        web.join("mcp-pkg/ergo_client.js").exists(),
        "Build the minimal WASM client first: cargo xtask build-web"
    );
    let lock = OpenOptions::new()
        .create(true)
        .read(true)
        .write(true)
        .truncate(false)
        .open(path.with_extension("mcp.lock"))?;
    fs2::FileExt::try_lock_exclusive(&lock).context("Working copy is already open")?;
    let bound = publisher.is_some()
        && shared::state_path(&path).exists()
        && shared::load_binding(&shared::state_path(&path))?
            .assembly_id
            .is_some();
    let mut scene = if path.exists() && !bound {
        ensure!(
            path.metadata()?.len() <= 128 * 1024 * 1024,
            "Working copy exceeds 128 MiB"
        );
        let bytes = fs::read(&path)?;
        let raw: Value = serde_json::from_slice(&bytes)?;
        let mut scene = if raw["format"] == "ergo-authoring-cache-1" {
            ensure!(
                publisher.is_some() && shared::state_path(&path).exists(),
                "A partial authoring cache is not a standalone construction; use export_construction"
            );
            ensure!(
                raw["partial"] == true
                    && raw["schema"] == ergo_world::SCHEMA_VERSION
                    && raw["construction"]["pieces"]
                        .as_array()
                        .is_some_and(Vec::is_empty),
                "Unbound nonempty partial cache requires explicit recovery; no geometry was discarded"
            );
            ergo_world::storage::decode(&serde_json::to_vec(&raw["construction"])?)?
        } else {
            ergo_world::storage::decode(&bytes)?
        };
        normalize(&mut scene, &measurements)?;
        scene
    } else {
        WorldExport {
            schema: ergo_world::SCHEMA_VERSION,
            name: "Agent construction".into(),
            geo: Default::default(),
            bricks: Vec::new(),
            meshes: Default::default(),
            joints: Vec::new(),
            fixed_bricks: Vec::new(),
            ground: Vec::new(),
            groups: Vec::new(),
            model: None,
        }
    };
    let common = if let Some(publisher) = &publisher {
        if let Some(world) = runtime.block_on(publisher.common_world())? {
            let sidecar = shared::state_path(&path);
            let mut binding = if sidecar.exists() {
                let binding = shared::load_binding(&sidecar)?;
                ensure!(
                    binding.world_id == world && binding.origin == publisher.url("/")?.as_str(),
                    "Shared cache belongs to another world/origin"
                );
                binding
            } else {
                runtime.block_on(publisher.new_binding(world))?
            };
            ensure!(
                !binding.pending || binding.assembly_id.is_some(),
                "An import outcome is unresolved; reconcile its shared-world history before opening this cache"
            );
            if let Some(assembly) = binding.assembly_id.clone() {
                if let Ok(requested) = std::env::var("ERGO_SHARED_ASSEMBLY_ID") {
                    ensure!(
                        requested == assembly,
                        "This cache is bound to another assembly; use a new path or open_shared_construction explicitly"
                    );
                }
                scene = runtime.block_on(publisher.load_shared_file(
                    &mut binding,
                    &assembly,
                    Some(&path),
                ))?;
                persist_page(&path, &scene)?;
            } else if let Ok(assembly) = std::env::var("ERGO_SHARED_ASSEMBLY_ID") {
                ensure!(
                    scene.bricks.is_empty(),
                    "Use a new cache path for this shared assembly"
                );
                scene = runtime.block_on(publisher.load_shared_file(
                    &mut binding,
                    &assembly,
                    Some(&path),
                ))?;
                persist_page(&path, &scene)?;
            } else if let Ok(position) = std::env::var("ERGO_POSITION_WORLD") {
                ensure!(
                    scene.bricks.is_empty(),
                    "Use a new cache path for a position-based shared workspace, or call open_shared_construction explicitly"
                );
                let position: [f64; 3] = serde_json::from_str(&position)?;
                let located = runtime.block_on(publisher.locate_shared(position))?;
                if let Some(assembly) = located["assembly_id"].as_str() {
                    scene = runtime.block_on(publisher.load_shared_file(
                        &mut binding,
                        assembly,
                        Some(&path),
                    ))?;
                } else {
                    scene.geo = serde_json::from_value(located["geo"].clone())?;
                }
                persist_page(&path, &scene)?;
            }
            atomic_write(&sidecar, &binding)?;
            Some(Arc::new(tokio::sync::Mutex::new(binding)))
        } else {
            None
        }
    } else {
        None
    };
    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(FsPath::new("."));
    let assets = std::env::var_os("ERGO_MCP_ASSET_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| parent.join(".ergo-assets"));
    fs::create_dir_all(&assets)?;
    // The hosted origin's synthesized library is mirrored and verified
    // locally; later sessions keep working offline from the mirror.
    let origin = match &publisher {
        Some(publisher) => publisher.url("/")?.to_string(),
        None => std::env::var("ERGO_API_URL").unwrap_or_else(|_| DEFAULT_ORIGIN.into()),
    };
    let http = reqwest::Client::builder()
        .timeout(Duration::from_secs(90))
        .build()?;
    let store: Arc<dyn object_store::ObjectStore> = Arc::new(
        object_store::local::LocalFileSystem::new_with_prefix(assets)?,
    );
    let (library, part_library) =
        runtime.block_on(ergo_assets::library::mirror(&*store, &http, &origin))?;
    ergo_world::part_library::install(library)?;
    let parts =
        ergo_assets::PartCache::new(store, String::new())?.with_origin(http.clone(), &origin);
    let bridge = Bridge {
        hub: Arc::new(Mutex::new(Hub {
            workspace: Workspace {
                scene,
                revision: 0,
                path: path.clone(),
                measurements,
            },
            queue: VecDeque::new(),
            pending: HashMap::new(),
        })),
        token: format!(
            "{}{}",
            uuid::Uuid::new_v4().simple(),
            uuid::Uuid::new_v4().simple()
        ),
        parts,
        part_library: part_library.into(),
        publisher: publisher.clone().map(Arc::new),
        common,
    };
    let listener = std::net::TcpListener::bind("127.0.0.1:0")?;
    let address = listener.local_addr()?;
    listener.set_nonblocking(true)?;
    let app = Router::new()
        .route("/api/local/workspace", get(workspace).put(update))
        .route("/api/local/jobs/next", get(next_job))
        .route("/api/local/jobs/result", post(complete))
        .route("/api/local/parts/{name}", get(part))
        .route("/api/local/library.json", get(serve_part_library))
        .fallback_service(ServeDir::new(web))
        .layer(DefaultBodyLimit::max(128 * 1024 * 1024))
        .layer(SetResponseHeaderLayer::overriding(
            header::CACHE_CONTROL,
            HeaderValue::from_static("no-store"),
        ))
        .layer(SetResponseHeaderLayer::overriding(
            header::REFERRER_POLICY,
            HeaderValue::from_static("no-referrer"),
        ))
        .with_state(bridge.clone());
    let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel::<()>();
    let server = runtime.spawn(async move {
        let listener = tokio::net::TcpListener::from_std(listener).unwrap();
        axum::serve(listener, app)
            .with_graceful_shutdown(async {
                let _ = shutdown_rx.await;
            })
            .await
            .unwrap();
    });
    let profile = tempfile::tempdir()?;
    let _browser = launch(
        &format!("http://{address}/mcp.html#mcp={}", bridge.token),
        headless,
        attach,
        profile.path(),
    )?;
    let input = io::stdin();
    let mut input = input.lock();
    let mut output = io::stdout().lock();
    let mut initialized = false;
    loop {
        let mut bytes = Vec::new();
        let read = std::io::Read::take(&mut input, 1_048_577).read_until(b'\n', &mut bytes)?;
        if read == 0 {
            break;
        }
        ensure!(read <= 1_048_576, "MCP request exceeds 1 MiB");
        let request: Value = match serde_json::from_slice(&bytes) {
            Ok(v) => v,
            Err(_) => {
                writeln!(
                    output,
                    "{}",
                    json!({"jsonrpc":"2.0","id":null,"error":{"code":-32700,"message":"Invalid JSON"}})
                )?;
                output.flush()?;
                continue;
            }
        };
        let Some(id) = request.get("id") else {
            continue;
        };
        let method = request.get("method").and_then(Value::as_str).unwrap_or("");
        let params = request.get("params").cloned().unwrap_or_else(|| json!({}));
        let result: std::result::Result<Value, (i32, String)> = match method {
            "initialize" => {
                initialized = true;
                let version = params
                    .get("protocolVersion")
                    .and_then(Value::as_str)
                    .filter(|v| ["2025-11-25", "2025-06-18", "2024-11-05"].contains(v))
                    .unwrap_or("2025-06-18");
                Ok(
                    json!({"protocolVersion":version,"capabilities":{"tools":{"listChanged":false}},"serverInfo":{"name":"ergo-local-wasm-bridge","version":env!("CARGO_PKG_VERSION")},"instructions":format!("{} Coordinates are X east, Y north, Z height. Editing/rendering execute in WASM. {}",ergo_world::mcp::BUILDING_GUIDANCE,if bridge.common.is_some(){"This is the common S3 world: accepted edits commit automatically with server validation and author attribution. Open an existing construction with open_shared_construction using assembly_id or position_world. The workspace file is only a cache; never treat local JSON edits as shared commits."}else{"Local development mode: publish_construction explicitly publishes through the configured API; only a confirmed result establishes publication."})}),
                )
            }
            "ping" => Ok(json!({})),
            _ if !initialized => Err((-32000, "Initialize first".into())),
            "tools/list" => Ok(tool_schemas(bridge.common.is_some())),
            "tools/call" => {
                let name = params.get("name").and_then(Value::as_str).unwrap_or("");
                let available = tool_schemas(bridge.common.is_some());
                if let Some(tool) = available["tools"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .find(|t| t["name"] == name)
                {
                    let arguments = params
                        .get("arguments")
                        .cloned()
                        .unwrap_or_else(|| json!({}));
                    let response: Result<Value> = (|| {
                        ergo_world::mcp::validate_input(
                            &arguments,
                            &tool["inputSchema"],
                            "arguments",
                        )?;
                        if name == "get_lego_models" {
                            let value = runtime.block_on(models::call(arguments.clone()))?;
                            return Ok(
                                json!({"content":[{"type":"text","text":value.to_string()}],"structuredContent":value,"isError":false}),
                            );
                        }
                        if name == "open_shared_construction" {
                            let publisher = publisher
                                .as_ref()
                                .context("Configure production URL and token first")?;
                            let common = bridge
                                .common
                                .as_ref()
                                .context("The server has no configured common world")?;
                            return runtime.block_on(async {
                                let mut binding = common.lock().await;
                                let assembly = if let Some(id) = arguments["assembly_id"].as_str() { id.to_owned() }
                                else {
                                    let position: [f64;3] = serde_json::from_value(arguments["position_world"].clone()).context("Supply assembly_id or position_world")?;
                                    publisher.locate_shared(position).await?["assembly_id"].as_str().context("No shared construction in this bounded search area")?.to_owned()
                                };
                                let path=bridge.hub.lock().unwrap().workspace.path.clone();
                                let scene = publisher.load_shared_file(&mut binding,&assembly,Some(&path)).await?;
                                let mut h = bridge.hub.lock().unwrap();
                                 persist_page(&h.workspace.path,&scene)?;
                                atomic_write(&shared::state_path(&h.workspace.path),&*binding)?;
                                h.workspace.scene=scene; h.workspace.revision+=1;
                                let value=json!({"world_id":binding.world_id,"assembly_id":assembly,"base_revision":binding.base_revision,
                                    "brick_count":h.workspace.scene.bricks.len(),"external_dependencies":binding.external_dependencies,
                                    "shared_editing":true,"local_revision":h.workspace.revision,"total_pieces":binding.total_pieces,"partial":true,"next_local_id":binding.next_local_id,"next_tool":"read_construction_page"});
                                Ok(json!({"content":[{"type":"text","text":value.to_string()}],"structuredContent":value,"isError":false}))
                            });
                        }
                        if name == "read_construction_page" {
                            let publisher = publisher
                                .as_ref()
                                .context("Paged reads require a shared server")?;
                            let common = bridge
                                .common
                                .as_ref()
                                .context("Open a shared construction first")?;
                            return runtime.block_on(async{
                                let mut binding=common.lock().await;
                                ensure!(!binding.pending,"Reconcile the preceding uncertain edit before changing the working view");
                                let mut next_binding=binding.clone();
                                let (mut scene,mut details)=publisher.load_page(&mut next_binding,&arguments).await?;
                                let mut h=bridge.hub.lock().unwrap();
                                if arguments["append"]==true{
                                    let mut old=h.workspace.scene.clone();
                                    for brick in scene.bricks{old.bricks.retain(|b|b.id!=brick.id);old.bricks.push(brick);}
                                    for (part,mesh) in scene.meshes{
                                        if let Some(previous)=old.meshes.get(&part){ensure!(ergo_world::authoring::version(previous)?==ergo_world::authoring::version(&mesh)?,"Cannot append incompatible meshes for one part name; replace the working view");}
                                        old.meshes.insert(part,mesh);
                                    }
                                    old.model=None;scene=old;
                                     let mut versions=binding.versions.clone();versions.extend(next_binding.versions);next_binding.versions=versions;
                                     next_binding.blocked_transform_groups=binding.blocked_transform_groups.clone();
                                }
                                ensure!(scene.bricks.len()<=ergo_world::authoring::RESIDENT_PIECES,"Resident limit exceeded; replace the view instead of appending");
                                ensure!(serde_json::to_vec(&scene)?.len()<=32*1024*1024,"Resident geometry exceeds 32 MiB; request a smaller page");
                                persist_page(&h.workspace.path,&scene)?;atomic_write(&shared::state_path(&h.workspace.path),&next_binding)?;
                                *binding=next_binding;h.workspace.scene=scene;h.workspace.revision+=1;
                                details["loaded_pieces"]=json!(h.workspace.scene.bricks.len());details["local_revision"]=json!(h.workspace.revision);
                                Ok(json!({"content":[{"type":"text","text":details.to_string()}],"structuredContent":details,"isError":false}))
                            });
                        }
                        if name == "list_groups"
                            && let Some(common) = &bridge.common
                        {
                            let value: Result<Option<Value>> = runtime.block_on(async {
                                let b = common.lock().await;
                                if b.assembly_id.is_none() {
                                    return Ok(None);
                                }
                                Ok(Some(
                                    publisher
                                        .as_ref()
                                        .context("Publishing is not configured")?
                                        .list_group_headers(&b, &arguments)
                                        .await?,
                                ))
                            });
                            if let Some(value) = value? {
                                return Ok(
                                    json!({"content":[{"type":"text","text":value.to_string()}],"structuredContent":value,"isError":false}),
                                );
                            }
                        }
                        if name == "read_construction_group" {
                            let common = bridge
                                .common
                                .as_ref()
                                .context("Group closure reads require shared mode")?;
                            return runtime.block_on(async{
                                let mut binding=common.lock().await;ensure!(!binding.pending,"Reconcile the preceding edit first");
                                let mut next=binding.clone();let group=u32::try_from(arguments["group_id"].as_u64().context("Missing group ID")?)?;
                                let(scene,mut details)=publisher.as_ref().context("Publishing is not configured")?.load_group(&mut next,group).await?;
                                let mut h=bridge.hub.lock().unwrap();persist_page(&h.workspace.path,&scene)?;atomic_write(&shared::state_path(&h.workspace.path),&next)?;
                                *binding=next;h.workspace.scene=scene;h.workspace.revision+=1;details["local_revision"]=json!(h.workspace.revision);
                                Ok(json!({"content":[{"type":"text","text":details.to_string()}],"structuredContent":details,"isError":false}))
                            });
                        }
                        if name == "export_construction"
                            && let Some(common) = &bridge.common
                        {
                            return runtime.block_on(async{
                                let binding=common.lock().await;
                                if let Some(assembly)=&binding.assembly_id{
                                    let value=publisher.as_ref().context("Publishing is not configured")?.request(reqwest::Method::GET,&format!("/api/shared-worlds/{}/assemblies/{assembly}/export?format=ergo&revision={}",binding.world_id,binding.base_revision),None).await?;
                                    let path=bridge.hub.lock().unwrap().workspace.path.with_extension("export.ergo.json");atomic_write(&path,&value)?;
                                    let details=json!({"path":path,"assembly_id":assembly,"base_revision":binding.base_revision,"partial":false,"published":false,"purpose":"explicit_full_export"});
                                    return Ok(json!({"content":[{"type":"text","text":details.to_string()}],"structuredContent":details,"isError":false}));
                                }
                                anyhow::bail!("No shared assembly is open; export the local proposal in --local mode")
                            });
                        }
                        if name == "publish_construction"
                            || name == "get_edit_allowance"
                            || name == "get_user_position"
                        {
                            let publisher = publisher.as_ref().context("Publishing is not configured. Set ERGO_PUBLISH_URL and ERGO_PUBLISH_TOKEN_FILE (or ERGO_PUBLISH_TOKEN); loopback development can omit the token")?;
                            if name == "get_edit_allowance" {
                                return Ok(runtime.block_on(publisher.allowance()));
                            }
                            if name == "get_user_position" {
                                return Ok(runtime.block_on(publisher.user_position()));
                            }
                            if let Some(common) = &bridge.common {
                                return runtime.block_on(async {
                                    let mut binding = common.lock().await;
                                    ensure!(!binding.pending,"A shared outcome is unresolved");
                                    let (mut scene,revision,path)={let h=bridge.hub.lock().unwrap();(h.workspace.scene.clone(),h.workspace.revision,h.workspace.path.clone())};
                                    ensure!(arguments["expected_local_revision"].as_u64().is_none_or(|expected|expected==u64::from(revision)),"Local cache changed");
                                    ensure!(arguments["base_revision"].as_str().is_none_or(|base|base==binding.base_revision),"Shared base differs; reopen the construction");
                                    let renamed=arguments["name"].as_str().is_some_and(|name|name!=scene.name);
                                    let before=scene.clone();
                                    if let Some(name)=arguments["name"].as_str(){scene.name=name.into();}
                                    let value=if !renamed && binding.assembly_id.is_some() {
                                        let assembly=binding.assembly_id.as_ref().unwrap();
                                        let read=publisher.request(reqwest::Method::GET,&format!("/api/shared-worlds/{}/assemblies/{assembly}/workspace?revision={}",binding.world_id,binding.base_revision),None).await?;
                                        ensure!(read["base_revision"]==binding.base_revision,"Shared receipt read-back failed");
                                        binding.receipt.clone().unwrap_or_else(||json!({"published":true,"publication_state":"confirmed_existing_revision","world_id":binding.world_id,"assembly_id":assembly,"accepted_revision":binding.base_revision,"view_url":format!("{}/app?shared_world={}",binding.origin.trim_end_matches('/'),binding.world_id)}))
                                    } else {
                                        let mut journal=binding.clone();journal.pending=true;
                                        atomic_write(&shared::state_path(&path),&journal)?;
                                        let result=publisher.commit_shared(&mut binding,&before,scene.clone(),revision).await;
                                        let flushed=atomic_write(&shared::state_path(&path),&*binding);
                                        let mut value=result?;
                                        if let Err(error)=flushed {value["cache_warning"]=json!(error.to_string());}
                                        let mut h=bridge.hub.lock().unwrap();h.workspace.scene=scene;h.workspace.revision+=1;
                                        if let Err(error)=persist_page(&path,&h.workspace.scene){value["cache_warning"]=json!(error.to_string());}
                                        value
                                    };
                                    Ok(json!({"content":[{"type":"text","text":value.to_string()}],"structuredContent":value,"isError":false}))
                                });
                            }
                            let (scene, revision) = {
                                let hub = bridge.hub.lock().unwrap();
                                (hub.workspace.scene.clone(), hub.workspace.revision)
                            };
                            return Ok(
                                runtime.block_on(publisher.publish(scene, revision, arguments))
                            );
                        }
                        if let Some(common) = &bridge.common {
                            {
                                let b = runtime.block_on(common.lock());
                                ensure!(
                                    name != "transform_group"
                                        || arguments["group_id"].as_u64().is_none_or(|id| !b
                                            .blocked_transform_groups
                                            .contains(&(id as u32))),
                                    "This group has unloaded or globally managed constraints; use a complete joint-aware domain operation before transforming it"
                                );
                                ensure!(
                                    name != "create_group"
                                        || b.next_group_id <= u64::from(u32::MAX),
                                    "Assembly group ID space exhausted"
                                );
                            }
                            let external = runtime
                                .block_on(async { common.lock().await.external_dependencies });
                            ensure!(
                                !external
                                    || ![
                                        "validate_construction",
                                        "move_brick",
                                        "rotate_about_pivot",
                                        "simulate_gravity",
                                        "test_solidity",
                                        "test_stability_by_gravity",
                                        "test_stability_by_earthquake",
                                        "test_small_earthquake_stability",
                                        "test_moderate_earthquake_stability"
                                    ]
                                    .contains(&name),
                                "This is a partial shared working view. Whole-construction validation/simulation and implicit connected-component moves require a complete authoritative closure. Use explicit loaded groups for deliberate group edits; shared commits still validate the full affected domain."
                            );
                        }
                        let job = Job {
                            id: uuid::Uuid::new_v4().to_string(),
                            name: name.into(),
                            arguments,
                            revision: bridge.hub.lock().unwrap().workspace.revision,
                        };
                        let job_id = job.id.clone();
                        let (tx, rx) = mpsc::channel();
                        {
                            let mut h = bridge.hub.lock().unwrap();
                            h.pending.insert(job.id.clone(), tx);
                            h.queue.push_back(job);
                        }
                        // A shared mutation includes publication and authoritative
                        // read-back (up to 300s each), plus local WASM work. The
                        // outer deadline must not cut off an accepted operation.
                        let value = rx.recv_timeout(Duration::from_secs(3600));
                        if value.is_err() {
                            let mut h = bridge.hub.lock().unwrap();
                            h.pending.remove(&job_id);
                            h.queue.retain(|j| j.id != job_id);
                        }
                        let mut value=value.context(if bridge.common.is_some(){"Shared MCP request timed out; acceptance may be unknown. Reopen/reconcile the authoritative assembly before any retry; no automatic retry was performed"}else{"Local WASM client timed out; attach the browser client or check its console"})?;
                        if name == "export_construction" && value["isError"] != true {
                            value["structuredContent"]["path"] = json!(path);
                            if let Some(text) =
                                value["content"].as_array_mut().and_then(|a| a.first_mut())
                            {
                                text["text"] =
                                    json!(format!("Local working copy: {}", path.display()));
                            }
                        }
                        if let Some(content) = value.get_mut("structuredContent") {
                            let feedback = if name == "export_construction" {
                                &mut content["feedback"]
                            } else {
                                content
                            };
                            if feedback.get("global_commit_available").is_some() {
                                feedback["global_commit_available"] = json!(publisher.is_some());
                            }
                        }
                        if let Some(common) = &bridge.common {
                            let binding = runtime.block_on(common.lock());
                            let loaded = bridge.hub.lock().unwrap().workspace.scene.bricks.len();
                            let scope = json!({"partial":binding.assembly_id.is_some(),"loaded_pieces":loaded,"total_pieces":binding.total_pieces,"base_revision":binding.base_revision,"next":binding.next,"local_diagnostics":"loaded_view_only; no complete boundary-load certificate"});
                            if let Some(content) = value
                                .get_mut("structuredContent")
                                .and_then(Value::as_object_mut)
                            {
                                content.insert("working_view".into(), scope.clone());
                            }
                            if let Some(items) = value["content"].as_array_mut() {
                                items.insert(0,json!({"type":"text","text":json!({"working_view":scope}).to_string()}));
                            }
                        }
                        // Keep the text fallback consistent for MCP hosts that
                        // do not consume structuredContent.
                        if let Some(items) = value["content"].as_array_mut() {
                            for item in items {
                                if let Some(text) = item["text"].as_str()
                                    && let Ok(mut feedback) = serde_json::from_str::<Value>(text)
                                    && feedback.get("global_commit_available").is_some()
                                {
                                    feedback["global_commit_available"] =
                                        json!(publisher.is_some());
                                    item["text"] = json!(feedback.to_string());
                                }
                            }
                        }
                        Ok(value)
                    })();
                    Ok(response.unwrap_or_else(|e|json!({"content":[{"type":"text","text":format!("{e:#}")}],"isError":true})))
                } else {
                    Err((-32602, format!("Unknown tool: {name}")))
                }
            }
            _ => Err((-32601, format!("Unknown method: {method}"))),
        };
        let response = match result {
            Ok(result) => json!({"jsonrpc":"2.0","id":id,"result":result}),
            Err((code, message)) => {
                json!({"jsonrpc":"2.0","id":id,"error":{"code":code,"message":message}})
            }
        };
        writeln!(output, "{response}")?;
        output.flush()?;
    }
    let _ = shutdown_tx.send(());
    drop(_browser);
    let _ = runtime.block_on(server);
    Ok(())
}
