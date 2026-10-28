//! Native HTTP publication only. The remote server owns authorization, physical
//! validation, and storage; no local simulation can authorize a remote commit.
use anyhow::{Context, Result, ensure};
use ergo_world::WorldExport;
use reqwest::{Client, Method, Url};
use serde::Deserialize;
use serde_json::{Value, json};
use std::{env, path::PathBuf, time::Duration};

const MAX_RESPONSE_BYTES: usize = 128 * 1024 * 1024;

#[derive(Clone)]
pub struct Publisher {
    base: Url,
    token: Option<String>,
    client: Client,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Arguments {
    world_id: Option<String>,
    base_revision: Option<String>,
    name: Option<String>,
    expected_local_revision: Option<u32>,
    authenticated_editing: Option<bool>,
}

#[derive(Debug)]
pub(crate) struct RemoteFailure {
    pub(crate) status: u16,
    body: Value,
}
impl std::fmt::Display for RemoteFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "Publication API returned HTTP {}: {}",
            self.status,
            self.body
                .get("error")
                .and_then(Value::as_str)
                .unwrap_or("Unexpected response")
        )
    }
}
impl std::error::Error for RemoteFailure {}

fn cid(value: &str) -> Result<&str> {
    ensure!(
        value.len() == 64
            && value
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)),
        "Invalid revision/content ID"
    );
    Ok(value)
}

pub fn open_login(url: Option<String>) -> Result<()> {
    let url = url
        .or_else(|| env::var("ERGO_PUBLISH_URL").ok())
        .context("Set --publish-url or ERGO_PUBLISH_URL for browser sign-in")?;
    // Reuse the transport/origin validation. This placeholder is never sent.
    let destination = Publisher::new(&url, Some("interactive-login".into()))?;
    let mut login = destination.base.clone();
    login.set_path("/login");
    login.set_fragment(Some("connect"));
    eprintln!("Sign in and generate/download an MCP access token at {login}");
    let command = if cfg!(target_os = "macos") {
        "open"
    } else {
        "xdg-open"
    };
    let status = std::process::Command::new(command)
        .arg(login.as_str())
        .status()
        .context("Could not open browser; open the displayed login URL manually")?;
    ensure!(
        status.success(),
        "Could not open browser; open the displayed login URL manually"
    );
    eprintln!(
        "Save the downloaded ergo-mcp-token file, set ERGO_PUBLISH_TOKEN_FILE to its path, then start the MCP bridge normally."
    );
    Ok(())
}

impl Publisher {
    pub async fn user_position(&self) -> Value {
        match self.request(Method::GET, "/api/user-position", None).await {
            Ok(value) => {
                json!({"content":[{"type":"text","text":value.to_string()}],"structuredContent":value,"isError":false})
            }
            Err(error) => {
                let value = json!({"error":format!("{error:#}")});
                json!({"content":[{"type":"text","text":value.to_string()}],"structuredContent":value,"isError":true})
            }
        }
    }
    pub async fn allowance(&self) -> Value {
        match self
            .request(Method::GET, "/api/shared-worlds/allowance", None)
            .await
        {
            Ok(value) => {
                json!({"content":[{"type":"text","text":value.to_string()}],"structuredContent":value,"isError":false})
            }
            Err(error) => {
                let value = json!({"error":format!("{error:#}"),"http_status":error.downcast_ref::<RemoteFailure>().map(|e|e.status)});
                json!({"content":[{"type":"text","text":value.to_string()}],"structuredContent":value,"isError":true})
            }
        }
    }
    pub fn from_env(url: Option<String>, token_file: Option<PathBuf>) -> Result<Option<Self>> {
        let url = url.or_else(|| env::var("ERGO_PUBLISH_URL").ok());
        let token_file =
            token_file.or_else(|| env::var_os("ERGO_PUBLISH_TOKEN_FILE").map(PathBuf::from));
        let token = if let Some(path) = token_file {
            Some(std::fs::read_to_string(path).context("Read MCP publication token file")?)
        } else {
            env::var("ERGO_PUBLISH_TOKEN").ok()
        };
        let Some(url) = url else {
            ensure!(token.is_none(), "Set ERGO_PUBLISH_URL for MCP publishing");
            return Ok(None);
        };
        Ok(Some(Self::new(&url, token)?))
    }

    fn new(url: &str, token: Option<String>) -> Result<Self> {
        let base = Url::parse(url).context("Invalid ERGO_PUBLISH_URL")?;
        let host = base.host_str().unwrap_or_default();
        let loopback = host == "localhost"
            || host
                .trim_matches(['[', ']'])
                .parse::<std::net::IpAddr>()
                .is_ok_and(|ip| ip.is_loopback());
        ensure!(
            base.host().is_some()
                && base.username().is_empty()
                && base.password().is_none()
                && base.path() == "/"
                && base.query().is_none()
                && base.fragment().is_none()
                && (base.scheme() == "https" || (base.scheme() == "http" && loopback)),
            "ERGO_PUBLISH_URL must be an HTTPS origin (HTTP is allowed only on loopback)"
        );
        let token = token.map(|s| s.trim().to_owned()).filter(|s| !s.is_empty());
        ensure!(
            loopback || token.is_some(),
            "Remote publishing requires ERGO_PUBLISH_TOKEN or ERGO_PUBLISH_TOKEN_FILE"
        );
        if let Some(token) = &token {
            ensure!(
                token.len() < 4096
                    && token
                        .bytes()
                        .all(|b| b.is_ascii_alphanumeric() || b"-_.".contains(&b)),
                "Invalid MCP publication token format"
            );
        }
        Ok(Self {
            base,
            token,
            client: Client::builder()
                .timeout(http_timeout(
                    env::var("ERGO_MCP_HTTP_TIMEOUT_SECONDS").ok().as_deref(),
                )?)
                .retry(reqwest::retry::never())
                .redirect(reqwest::redirect::Policy::none())
                .build()?,
        })
    }

    pub(crate) fn url(&self, path: &str) -> Result<Url> {
        Ok(self.base.join(path.trim_start_matches('/'))?)
    }

    pub(crate) async fn request(
        &self,
        method: Method,
        path: &str,
        body: Option<Value>,
    ) -> Result<Value> {
        let mut request = self
            .client
            .request(method.clone(), self.url(path)?)
            .header("X-Ergo-Request", "1");
        if let Some(token) = &self.token {
            request = request.bearer_auth(token);
        }
        if let Some(body) = body {
            let bytes = serde_json::to_vec(&body)?;
            ensure!(
                bytes.len() <= 128 * 1024 * 1024,
                "Publication request exceeds 128 MiB"
            );
            request = request
                .header("Content-Type", "application/json")
                .body(bytes);
        }
        let mut response = request.send().await.map_err(|error| {
            let kind = if error.is_timeout() {
                "timed out"
            } else {
                "failed"
            };
            anyhow::Error::from(error.without_url()).context(format!(
                "Publication {method} request {kind}; no automatic retry"
            ))
        })?;
        let status = response.status();
        ensure!(
            response
                .content_length()
                .is_none_or(|length| length <= MAX_RESPONSE_BYTES as u64),
            "Publication response exceeds 128 MiB"
        );
        let mut bytes = Vec::new();
        while let Some(chunk) = response.chunk().await.map_err(|e| e.without_url())? {
            ensure!(
                bytes.len() + chunk.len() <= MAX_RESPONSE_BYTES,
                "Publication response exceeds 128 MiB"
            );
            bytes.extend_from_slice(&chunk);
        }
        let body = serde_json::from_slice(&bytes)
            .unwrap_or_else(|_| json!({"error":"Non-JSON response from publication API"}));
        if !status.is_success() {
            return Err(RemoteFailure {
                status: status.as_u16(),
                body,
            }
            .into());
        }
        Ok(body)
    }

    pub async fn publish(&self, scene: WorldExport, revision: u32, arguments: Value) -> Value {
        let mut details = json!({"published":false,"publication_state":"not_attempted", "local_revision":revision,"working_copy_changed":false});
        let result = self.perform(scene, revision, arguments, &mut details).await;
        match result {
            Ok(()) => {
                details["published"] = json!(true);
                details["publication_state"] = json!("confirmed");
                json!({"content":[{"type":"text","text":details.to_string()}],"structuredContent":details,"isError":false})
            }
            Err(error) => {
                if let Some(remote) = error.downcast_ref::<RemoteFailure>() {
                    details["http_status"] = json!(remote.status);
                    details["server_response"] = remote.body.clone();
                    if details["publication_state"] != "accepted_unconfirmed"
                        && (400..500).contains(&remote.status)
                    {
                        details["publication_state"] = json!("rejected");
                        details["published"] = json!(false);
                    }
                    if details.get("world_id").is_none()
                        && let Some(world) = remote.body.get("world").and_then(Value::as_str)
                    {
                        details["world_id"] = json!(world);
                    }
                }
                details["error"] = json!(format!("{error:#}"));
                details["automatic_retry"] = json!(false);
                details["next_action"] = json!(if details["publication_state"] == "unknown"
                    || details["publication_state"] == "accepted_unconfirmed"
                {
                    "Do not repeat this non-idempotent import automatically. Inspect the world/accepted revision or candidate in authenticated shared-world history before retrying."
                } else {
                    "Correct the reported configuration, permissions, base revision, or server validation failure before publishing again."
                });
                json!({"content":[{"type":"text","text":details.to_string()}],"structuredContent":details,"isError":true})
            }
        }
    }

    async fn perform(
        &self,
        mut scene: WorldExport,
        revision: u32,
        arguments: Value,
        details: &mut Value,
    ) -> Result<()> {
        let args: Arguments = serde_json::from_value(arguments)?;
        ensure!(
            args.expected_local_revision
                .is_none_or(|expected| expected == revision),
            "Local working-copy revision changed"
        );
        ensure!(
            !scene.bricks.is_empty(),
            "Cannot publish an empty construction"
        );
        let view_coord: [i64; 3] =
            ergo_world::BlockCoord::at(scene.bricks[0].bounds.center()).into();
        if let Some(name) = args.name {
            scene.name = name;
        }
        ensure!(
            !scene.name.trim().is_empty() && scene.name.chars().count() <= 120,
            "Construction name must contain 1..120 characters"
        );
        if let Some(base) = &args.base_revision {
            cid(base)?;
        }
        let (world, base) = if let Some(world) = args.world_id {
            ensure!(
                args.authenticated_editing.is_none(),
                "authenticated_editing is a new-world setting; existing-world access changes require its owner"
            );
            let world = uuid::Uuid::parse_str(&world)
                .context("Invalid shared-world UUID")?
                .to_string();
            details["world_id"] = json!(world);
            let head = self
                .request(Method::GET, &format!("/api/shared-worlds/{world}"), None)
                .await?;
            ensure!(
                head["id"] == world,
                "Publication API returned the wrong world"
            );
            let base = args.base_revision.unwrap_or(
                head["revision"]
                    .as_str()
                    .context("Missing world revision")?
                    .to_owned(),
            );
            cid(&base)?;
            (world, base)
        } else {
            ensure!(
                args.base_revision.is_none(),
                "base_revision requires world_id"
            );
            details["publication_state"] = json!("unknown");
            details["published"] = Value::Null;
            details["phase"] = json!("create_world");
            let head = self
                .request(
                    Method::POST,
                    "/api/shared-worlds",
                    Some(json!({"name":scene.name,"geo":scene.geo,"authenticated_editing":args.authenticated_editing.unwrap_or(true)})),
                )
                .await?;
            let world =
                uuid::Uuid::parse_str(head["id"].as_str().context("Missing created world ID")?)?
                    .to_string();
            details["world_id"] = json!(world);
            details["world_created"] = json!(true);
            let base = head["revision"]
                .as_str()
                .context("Missing created world revision")?
                .to_owned();
            cid(&base)?;
            (world, base)
        };
        details["base_revision"] = json!(base);
        details["phase"] = json!("import_assembly");
        details["publication_state"] = json!("unknown");
        details["published"] = Value::Null;
        let imported = self
            .request(
                Method::POST,
                &format!("/api/shared-worlds/{world}/assemblies"),
                Some(json!({"base":base,"scene":scene})),
            )
            .await?;
        ensure!(
            imported["accepted"]["id"] == world,
            "Publication API returned the wrong accepted world"
        );
        let accepted = cid(imported["accepted"]["revision"]
            .as_str()
            .context("Missing accepted revision")?)?
        .to_owned();
        let assembly = cid(imported["assembly"]
            .as_str()
            .context("Missing assembly ID")?)?
        .to_owned();
        details["accepted_revision"] = json!(accepted);
        details["assembly_id"] = json!(assembly);
        details["pieces"] = imported["pieces"].clone();
        details["audit"] = imported["accepted"]["audit"].clone();
        details["publication_state"] = json!("accepted_unconfirmed");
        details["phase"] = json!("read_back");
        let head = self
            .request(
                Method::GET,
                &format!("/api/shared-worlds/{world}?revision={accepted}"),
                None,
            )
            .await?;
        ensure!(
            head["id"] == world && head["revision"] == accepted,
            "Accepted world revision could not be confirmed"
        );
        let path = format!("/api/shared-worlds/{world}/assemblies/{assembly}?revision={accepted}");
        let read_back = self.request(Method::GET, &path, None).await?;
        ensure!(
            read_back["id"] == assembly,
            "Accepted assembly could not be read back"
        );
        details["assembly_url"] = json!(self.url(&path)?.as_str());
        details["view_url"] = json!(
            self.url(&format!(
                "/app?shared_world={world}&x={}&y={}&z={}",
                view_coord[0], view_coord[1], view_coord[2]
            ))?
            .as_str()
        );
        details["world_url"] = json!(
            self.url(&format!("/api/shared-worlds/{world}?revision={accepted}"))?
                .as_str()
        );
        Ok(())
    }
}

fn http_timeout(value: Option<&str>) -> Result<Duration> {
    let seconds = value.map(str::parse::<u64>).transpose()?.unwrap_or(300);
    ensure!(
        (1..=1800).contains(&seconds),
        "ERGO_MCP_HTTP_TIMEOUT_SECONDS must be 1..1800"
    );
    Ok(Duration::from_secs(seconds))
}

#[cfg(test)]
mod tests {
    #[test]
    fn http_deadlines_are_explicit_and_bounded() {
        assert_eq!(super::http_timeout(None).unwrap().as_secs(), 300);
        assert_eq!(super::http_timeout(Some("600")).unwrap().as_secs(), 600);
        for value in ["0", "1801", "invalid"] {
            assert!(super::http_timeout(Some(value)).is_err());
        }
    }
    use super::*;
    use axum::{
        Json, Router,
        body::to_bytes,
        extract::{Request, State},
        http::StatusCode,
        response::{IntoResponse, Response},
    };
    use std::sync::{Arc, Mutex};

    const WORLD: &str = "12345678-1234-4234-8234-123456789abc";
    #[test]
    fn complete_workspace_reads_allow_large_scenes_but_remain_byte_bounded() {
        tokio::runtime::Runtime::new().unwrap().block_on(async {
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            let address = listener.local_addr().unwrap();
            let app = Router::new()
                .route(
                    "/workspace",
                    axum::routing::get(|| async {
                        Json(json!({"scene_payload":"x".repeat(9 * 1024 * 1024)}))
                    }),
                )
                .route(
                    "/oversized",
                    axum::routing::get(|| async { vec![0u8; MAX_RESPONSE_BYTES + 1] }),
                );
            let server = tokio::spawn(async move {
                axum::serve(listener, app).await.unwrap();
            });
            let publisher = Publisher::new(&format!("http://{address}"), None).unwrap();
            let read = publisher
                .request(Method::GET, "/workspace", None)
                .await
                .unwrap();
            assert_eq!(
                read["scene_payload"].as_str().unwrap().len(),
                9 * 1024 * 1024
            );
            let error = publisher
                .request(Method::GET, "/oversized", None)
                .await
                .unwrap_err();
            assert!(error.to_string().contains("response exceeds 128 MiB"));
            server.abort();
        });
    }
    #[derive(Clone)]
    struct Mock {
        mode: &'static str,
        requests: Arc<Mutex<Vec<(String, Value)>>>,
    }
    async fn api(State(mock): State<Mock>, request: Request) -> Response {
        assert_eq!(request.headers()["authorization"], "Bearer test-token");
        assert_eq!(request.headers()["X-Ergo-Request"], "1");
        let method = request.method().clone();
        let path = request.uri().path().to_owned();
        let body = to_bytes(request.into_body(), 1024 * 1024).await.unwrap();
        let body: Value = serde_json::from_slice(&body).unwrap_or(Value::Null);
        mock.requests
            .lock()
            .unwrap()
            .push((format!("{method} {path}"), body));
        if mock.mode == "redirect" {
            return (
                StatusCode::TEMPORARY_REDIRECT,
                [("location", "/must-not-follow")],
            )
                .into_response();
        }
        if mock.mode == "forbidden" {
            return (
                StatusCode::FORBIDDEN,
                Json(json!({"error":"permission denied"})),
            )
                .into_response();
        }
        if path.ends_with("/assemblies") && ["unknown", "conflict"].contains(&mock.mode) {
            return (
                if mock.mode == "conflict" {
                    StatusCode::CONFLICT
                } else {
                    StatusCode::SERVICE_UNAVAILABLE
                },
                Json(
                    json!({"error":"publication failure","candidate":"4".repeat(64),"world":WORLD}),
                ),
            )
                .into_response();
        }
        if mock.mode == "readback_missing" && method == Method::GET {
            return (
                StatusCode::SERVICE_UNAVAILABLE,
                Json(json!({"error":"read-back unavailable"})),
            )
                .into_response();
        }
        let response = if path.contains("/assemblies/") {
            json!({"id":"2".repeat(64)})
        } else if path.ends_with("/assemblies") {
            json!({"accepted":{"id":WORLD,"revision":"1".repeat(64),"audit":{"actor":"alice"}},"assembly":"2".repeat(64),"pieces":{"1":"3".repeat(64)}})
        } else {
            json!({"id":WORLD,"revision":if method == Method::POST {"0".repeat(64)}else{"1".repeat(64)}})
        };
        Json(response).into_response()
    }
    fn scene() -> WorldExport {
        let demo = ergo_world::demo::physics_lab();
        WorldExport {
            schema: ergo_world::SCHEMA_VERSION,
            name: "MCP test".into(),
            geo: Default::default(),
            bricks: demo.bricks,
            meshes: demo.meshes,
            joints: Vec::new(),
            fixed_bricks: Vec::new(),
            ground: Vec::new(),
            groups: Vec::new(),
            model: None,
        }
    }
    async fn exercise(mode: &'static str, args: Value) -> (Value, Vec<(String, Value)>) {
        let mock = Mock {
            mode,
            requests: Arc::new(Mutex::new(Vec::new())),
        };
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let app = Router::new().fallback(api).with_state(mock.clone());
        let server = tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        let publisher =
            Publisher::new(&format!("http://{address}"), Some("test-token".into())).unwrap();
        let result = publisher.publish(scene(), 7, args).await;
        server.abort();
        let requests = mock.requests.lock().unwrap().clone();
        (result, requests)
    }
    #[test]
    fn publishing_is_confirmed_by_accepted_revision_and_assembly_readback() {
        tokio::runtime::Runtime::new().unwrap().block_on(async {
            let (result, requests) = exercise(
                "ok",
                json!({"name":"Remote sculpture","expected_local_revision":7}),
            )
            .await;
            assert_eq!(result["isError"], false, "{result}");
            let receipt = &result["structuredContent"];
            assert_eq!(receipt["publication_state"], "confirmed");
            assert_eq!(receipt["accepted_revision"], "1".repeat(64));
            assert_eq!(receipt["assembly_id"], "2".repeat(64));
            assert!(
                receipt["assembly_url"]
                    .as_str()
                    .unwrap()
                    .ends_with(&format!("?revision={}", "1".repeat(64)))
            );
            assert_eq!(requests.len(), 4);
            assert_eq!(requests[0].1["name"], "Remote sculpture");
            assert_eq!(requests[1].1["scene"]["name"], "Remote sculpture");
            assert!(requests[2].0.starts_with("GET "));
            assert!(requests[3].0.contains("/assemblies/"));
        });
    }
    #[test]
    fn rejections_unknown_outcomes_and_redirects_are_never_retried() {
        tokio::runtime::Runtime::new().unwrap().block_on(async {
            for (mode, state, calls) in [
                ("forbidden", "rejected", 1),
                ("unknown", "unknown", 2),
                ("redirect", "unknown", 1),
                ("readback_missing", "accepted_unconfirmed", 3),
            ] {
                let (result, requests) = exercise(mode, json!({})).await;
                assert_eq!(result["isError"], true, "{result}");
                assert_eq!(result["structuredContent"]["publication_state"], state);
                assert_eq!(requests.len(), calls);
                if mode == "unknown" {
                    assert!(result["structuredContent"]["published"].is_null());
                    assert_eq!(result["structuredContent"]["world_id"], WORLD);
                    assert_eq!(
                        result["structuredContent"]["server_response"]["candidate"],
                        "4".repeat(64)
                    );
                }
            }
            let base = "5".repeat(64);
            let (result, requests) =
                exercise("conflict", json!({"world_id":WORLD,"base_revision":base})).await;
            assert_eq!(result["structuredContent"]["http_status"], 409);
            assert_eq!(requests.len(), 2);
            assert_eq!(requests[1].1["base"], base);
            let (result, requests) = exercise("ok", json!({"expected_local_revision":6})).await;
            assert_eq!(result["isError"], true);
            assert!(requests.is_empty());
        });
    }
    #[test]
    fn configuration_prevents_credentials_in_urls_and_insecure_remote_transport() {
        for url in [
            "http://ergo.example",
            "https://user:secret@ergo.example",
            "https://ergo.example/path",
            "https://ergo.example?token=secret",
            "https://ergo.example#fragment",
        ] {
            assert!(Publisher::new(url, Some("token".into())).is_err(), "{url}");
        }
        assert!(Publisher::new("https://ergo.example", None).is_err());
        assert!(Publisher::new("http://127.0.0.1:3000", None).is_ok());
    }
}
