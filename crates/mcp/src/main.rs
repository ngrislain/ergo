//! Thin stdio/REST bridge. Rendering and all time-integrated simulations execute
//! in the dedicated local browser WASM client, never in this process.
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
    upstream: Option<String>,
    http: reqwest::Client,
}
type HttpError = (StatusCode, Json<Value>);
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
    let h = s.hub.lock().unwrap();
    Ok(Json(
        json!({"scene":h.workspace.scene,"revision":h.workspace.revision,"measurements":h.workspace.measurements}),
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
    normalize(&mut update.scene).map_err(http_error)?;
    let mut h = s.hub.lock().unwrap();
    if !h.pending.contains_key(&update.job_id) || h.workspace.revision != update.expected_revision {
        return Err((
            StatusCode::CONFLICT,
            Json(json!({"error":"Job expired or working-copy revision changed"})),
        ));
    }
    persist(&h.workspace.path, &update.scene).map_err(http_error)?;
    h.workspace.scene = update.scene;
    h.workspace.revision += 1;
    Ok(Json(json!({"revision":h.workspace.revision})))
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
    if let Some(base) = &s.upstream {
        let canonical = ergo_world::catalog::canonical_part(&if name.ends_with(".dat") {
            name.clone()
        } else {
            format!("{name}.dat")
        });
        if !ergo_world::catalog::PARTS
            .iter()
            .any(|(p, _)| *p == canonical)
        {
            return Err(http_error("Unsupported part"));
        }
        let response = s
            .http
            .get(format!(
                "{}/api/parts/{canonical}",
                base.trim_end_matches('/')
            ))
            .send()
            .await
            .map_err(http_error)?
            .error_for_status()
            .map_err(http_error)?;
        return Ok(Json(response.json().await.map_err(http_error)?));
    }
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
fn persist(path: &FsPath, scene: &WorldExport) -> Result<()> {
    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(FsPath::new("."));
    let mut temp = tempfile::NamedTempFile::new_in(parent)?;
    serde_json::to_writer_pretty(&mut temp, scene)?;
    temp.flush()?;
    temp.as_file().sync_all()?;
    temp.persist(path)?;
    Ok(())
}
fn normalize(scene: &mut WorldExport) -> Result<()> {
    ensure!(
        scene.schema == ergo_world::SCHEMA_VERSION,
        "Expected schema 2: X east, Y north, Z height; reimport or explicitly migrate older files"
    );
    ensure!(
        scene.bricks.len() <= ergo_world::MAX_SIMULATION_BRICKS,
        "Local working copy exceeds 5,000 bricks"
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
        mesh.body_bounds = ergo_world::geometry::fallback_mesh(part).body_bounds;
        mesh.bounds = mesh
            .vertices
            .chunks_exact(9)
            .map(|v| {
                let p = glam::DVec3::new(v[0] as f64, v[1] as f64, v[2] as f64);
                ergo_world::Aabb::new(p, p)
            })
            .reduce(ergo_world::Aabb::union)
            .unwrap();
    }
    for b in &mut scene.bricks {
        ergo_world::physical::validate_pose(b)?;
        let mesh = scene.meshes.get(&b.part).context("Missing part geometry")?;
        b.bounds = mesh.body_bounds.transformed(b.transform, b.position);
        b.render_bounds = mesh.bounds.transformed(b.transform, b.position);
    }
    if !scene.bricks.is_empty() {
        ergo_world::partition(&scene.bricks)?;
    }
    Ok(())
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
            "--web-dir" => web = args.next().context("Missing web directory")?.into(),
            "--help" => {
                eprintln!(
                    "ergo-mcp --workspace path.json [--measurements file.json] [--attach | --headless] [--web-dir web]\nThe default opens a minimal WASM client in the existing browser. Heavy work runs there, over the local REST bridge. Build it with scripts/build-web.sh."
                );
                return Ok(());
            }
            _ => anyhow::bail!("Unknown argument {arg}"),
        }
    }
    measurements.validate()?;
    ensure!(
        web.join("mcp-pkg/ergo_client.js").exists(),
        "Build the minimal WASM client first: bash scripts/build-web.sh"
    );
    let lock = OpenOptions::new()
        .create(true)
        .read(true)
        .write(true)
        .truncate(false)
        .open(path.with_extension("mcp.lock"))?;
    fs2::FileExt::try_lock_exclusive(&lock).context("Working copy is already open")?;
    let scene = if path.exists() {
        ensure!(
            path.metadata()?.len() <= 128 * 1024 * 1024,
            "Working copy exceeds 128 MiB"
        );
        let mut scene = serde_json::from_slice(&fs::read(&path)?)?;
        normalize(&mut scene)?;
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
        }
    };
    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(FsPath::new("."));
    let assets = std::env::var_os("ERGO_MCP_ASSET_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| parent.join(".ergo-assets"));
    fs::create_dir_all(&assets)?;
    let parts = ergo_assets::PartCache::new(
        Arc::new(object_store::local::LocalFileSystem::new_with_prefix(
            assets,
        )?),
        String::new(),
    )?;
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
        upstream: std::env::var("ERGO_API_URL").ok(),
        http: reqwest::Client::builder()
            .timeout(Duration::from_secs(90))
            .build()?,
    };
    let listener = std::net::TcpListener::bind("127.0.0.1:0")?;
    let address = listener.local_addr()?;
    listener.set_nonblocking(true)?;
    let app = Router::new()
        .route("/api/local/workspace", get(workspace).put(update))
        .route("/api/local/jobs/next", get(next_job))
        .route("/api/local/jobs/result", post(complete))
        .route("/api/local/parts/{name}", get(part))
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
    let server = std::thread::spawn(move || {
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .build()
            .unwrap();
        runtime.block_on(async {
            let listener = tokio::net::TcpListener::from_std(listener).unwrap();
            axum::serve(listener, app)
                .with_graceful_shutdown(async {
                    let _ = shutdown_rx.await;
                })
                .await
                .unwrap();
        });
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
                    json!({"protocolVersion":version,"capabilities":{"tools":{"listChanged":false}},"serverInfo":{"name":"ergo-local-wasm-bridge","version":env!("CARGO_PKG_VERSION")},"instructions":"Tools execute in the attached minimal local WASM client via REST. Coordinates are X east, Y north, Z height. Source magnitude is not local PGA. The bridge only stores local working copies; no global commit tool exists."}),
                )
            }
            "ping" => Ok(json!({})),
            _ if !initialized => Err((-32000, "Initialize first".into())),
            "tools/list" => Ok(ergo_world::mcp::tools()),
            "tools/call" => {
                let name = params.get("name").and_then(Value::as_str).unwrap_or("");
                let available = ergo_world::mcp::tools();
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
                        let job = Job {
                            id: uuid::Uuid::new_v4().to_string(),
                            name: name.into(),
                            arguments,
                        };
                        let job_id = job.id.clone();
                        let (tx, rx) = mpsc::channel();
                        {
                            let mut h = bridge.hub.lock().unwrap();
                            h.pending.insert(job.id.clone(), tx);
                            h.queue.push_back(job);
                        }
                        let value = rx.recv_timeout(Duration::from_secs(120));
                        if value.is_err() {
                            let mut h = bridge.hub.lock().unwrap();
                            h.pending.remove(&job_id);
                            h.queue.retain(|j| j.id != job_id);
                        }
                        let mut value=value.context("Local WASM client timed out; attach the browser client or check its console")?;
                        if name == "export_construction" {
                            value["structuredContent"]["path"] = json!(path);
                            if let Some(text) =
                                value["content"].as_array_mut().and_then(|a| a.first_mut())
                            {
                                text["text"] =
                                    json!(format!("Local working copy: {}", path.display()));
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
    let _ = server.join();
    Ok(())
}
