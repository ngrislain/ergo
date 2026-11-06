//! Human-approved browser delegation, returned once over a loopback POST.
//! Tokens never appear in URLs, stdout, or workspace files.
use super::*;
use axum::{
    Form, Router,
    extract::{DefaultBodyLimit, State},
    http::{HeaderMap, StatusCode, header},
    response::{Html, IntoResponse, Response},
    routing::post,
};
use sha2::{Digest, Sha256};
use std::{io::Write, path::Path, sync::Arc};
use tokio::sync::{Mutex, oneshot};

const LOGIN_TIMEOUT: Duration = Duration::from_secs(600);

fn default_token_file(base: &Url) -> Result<PathBuf> {
    let config = env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| env::var_os("HOME").map(|home| PathBuf::from(home).join(".config")))
        .context("Set ERGO_PUBLISH_TOKEN_FILE when no home/config directory is available")?;
    Ok(config.join("ergo/tokens").join(format!(
        "{:x}.token",
        Sha256::digest(base.origin().ascii_serialization().as_bytes())
    )))
}

fn read_token(path: &Path) -> Result<Option<String>> {
    match std::fs::read_to_string(path) {
        Ok(value) => Ok((!value.trim().is_empty()).then(|| value.trim().to_owned())),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error).context("Read MCP publication token file"),
    }
}

fn save_token(path: &Path, token: &str) -> Result<()> {
    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    std::fs::create_dir_all(parent)?;
    // NamedTempFile is mode 0600 on Unix; rename never follows a destination symlink.
    let mut file = tempfile::NamedTempFile::new_in(parent)?;
    file.write_all(token.as_bytes())?;
    file.as_file().sync_all()?;
    file.persist(path).map_err(|e| e.error)?;
    Ok(())
}

pub(super) fn configure(
    url: Option<String>,
    token_file: Option<PathBuf>,
    force: bool,
) -> Result<Option<Publisher>> {
    let url = url.or_else(|| env::var("ERGO_PUBLISH_URL").ok());
    let token_file =
        token_file.or_else(|| env::var_os("ERGO_PUBLISH_TOKEN_FILE").map(PathBuf::from));
    let environment_token = env::var("ERGO_PUBLISH_TOKEN")
        .ok()
        .filter(|s| !s.trim().is_empty());
    let Some(url) = url else {
        ensure!(
            token_file.is_none() && environment_token.is_none(),
            "Set ERGO_PUBLISH_URL for MCP publishing"
        );
        return Ok(None);
    };
    // Validate the origin before reading files or opening a browser. This
    // placeholder is never sent in an HTTP request.
    let mut publisher = Publisher::new(&url, Some("interactive-login".into()))?;
    publisher.token = None;
    let base_host = publisher.base.host_str().unwrap_or_default();
    let loopback = base_host == "localhost"
        || base_host
            .trim_matches(['[', ']'])
            .parse::<std::net::IpAddr>()
            .is_ok_and(|ip| ip.is_loopback());
    let explicit_file = token_file.is_some();
    let path = if let Some(path) = token_file {
        Some(path)
    } else if force || (!loopback && environment_token.is_none()) {
        Some(default_token_file(&publisher.base)?)
    } else {
        default_token_file(&publisher.base)
            .ok()
            .filter(|path| path.exists())
    };
    let token = if force {
        None
    } else if explicit_file || environment_token.is_none() {
        path.as_ref().map(|p| read_token(p)).transpose()?.flatten()
    } else {
        environment_token
    };
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()?;
    if let Some(token) = token {
        publisher = Publisher::new(&url, Some(token))?;
        match runtime.block_on(publisher.request(Method::GET, "/api/shared-worlds/allowance", None))
        {
            Ok(_) => return Ok(Some(publisher)),
            Err(error)
                if error
                    .downcast_ref::<RemoteFailure>()
                    .is_some_and(|e| e.status == 401) =>
            {
                eprintln!("MCP credentials expired or were rejected; opening browser sign-in.");
                publisher.token = None;
            }
            Err(error) => return Err(error),
        }
    } else if loopback && !force && path.is_none() {
        // Local development may deliberately omit delegation, but a Google-
        // enabled loopback server still needs the same browser approval flow.
        let account = runtime.block_on(publisher.request(Method::GET, "/api/auth/me", None))?;
        if account["development"] == true {
            return Ok(Some(publisher));
        }
    }
    let token = runtime.block_on(authorize(&publisher))?;
    let path = match path {
        Some(path) => path,
        None => default_token_file(&publisher.base)?,
    };
    save_token(&path, &token).context("Save approved MCP token")?;
    publisher.token = Some(token);
    eprintln!("MCP authorization saved privately to {}.", path.display());
    Ok(Some(publisher))
}

#[derive(Clone)]
struct CallbackState {
    origin: String,
    host: String,
    result: Arc<Mutex<Option<oneshot::Sender<String>>>>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Credentials {
    token: String,
}

async fn callback(
    State(state): State<CallbackState>,
    headers: HeaderMap,
    Form(form): Form<Credentials>,
) -> Response {
    if headers.get(header::ORIGIN).and_then(|v| v.to_str().ok()) != Some(&state.origin)
        || headers.get(header::HOST).and_then(|v| v.to_str().ok()) != Some(&state.host)
        || form.token.len() >= 4096
        || form.token.is_empty()
        || !form
            .token
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"-_.".contains(&b))
    {
        return StatusCode::FORBIDDEN.into_response();
    }
    let Some(sender) = state.result.lock().await.take() else {
        return StatusCode::GONE.into_response();
    };
    let _ = sender.send(form.token);
    (
        [
            (header::CACHE_CONTROL, "no-store"),
            (header::REFERRER_POLICY, "no-referrer"),
            (header::CONTENT_SECURITY_POLICY, "default-src 'none'; frame-ancestors 'none'"),
        ],
        Html("<!doctype html><title>Ergo MCP authorization</title><h1>Authorization received</h1><p>The MCP server is checking and saving your token. You can close this tab.</p>"),
    ).into_response()
}

async fn authorize(publisher: &Publisher) -> Result<String> {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let address = listener.local_addr()?;
    let capability = format!(
        "{}{}",
        uuid::Uuid::new_v4().simple(),
        uuid::Uuid::new_v4().simple()
    );
    let callback_url = format!("http://{address}/callback/{capability}");
    let (tx, rx) = oneshot::channel();
    let app = Router::new()
        .route(&format!("/callback/{capability}"), post(callback))
        .layer(DefaultBodyLimit::max(8192))
        .with_state(CallbackState {
            origin: publisher.base.origin().ascii_serialization(),
            host: address.to_string(),
            result: Arc::new(Mutex::new(Some(tx))),
        });
    let (shutdown_tx, shutdown_rx) = oneshot::channel::<()>();
    let mut server = tokio::spawn(async move {
        axum::serve(listener, app)
            .with_graceful_shutdown(async {
                let _ = shutdown_rx.await;
            })
            .await
    });
    let mut url = publisher.base.join("login")?;
    url.query_pairs_mut()
        .append_pair("mcp_callback", &callback_url);
    url.set_fragment(Some("connect"));
    eprintln!("Sign in and authorize this MCP server in your browser: {url}");
    let opener = if cfg!(target_os = "macos") {
        "open"
    } else {
        "xdg-open"
    };
    match std::process::Command::new(opener)
        .arg(url.as_str())
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
    {
        Ok(status) if status.success() => {}
        _ => eprintln!(
            "Could not open a browser. Open the displayed URL manually within 10 minutes."
        ),
    }
    let result = tokio::time::timeout(LOGIN_TIMEOUT, rx).await;
    // Finish the accepted POST without relying on a timing guess. Bound shutdown
    // as well, so an unrelated stalled local request cannot hold login open.
    let _ = shutdown_tx.send(());
    if tokio::time::timeout(Duration::from_secs(2), &mut server)
        .await
        .is_err()
    {
        server.abort();
    }
    let token = result
        .context("Browser sign-in timed out after 10 minutes; restart MCP to try again")?
        .context("Browser sign-in was cancelled")?;
    let authenticated = Publisher::new(publisher.base.as_str(), Some(token.clone()))?;
    authenticated
        .request(Method::GET, "/api/shared-worlds/allowance", None)
        .await
        .context("The browser returned a token the configured server did not accept")?;
    Ok(token)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tokens_are_saved_atomically_and_privately() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("private/token");
        assert!(read_token(&path).unwrap().is_none());
        save_token(&path, "first-token").unwrap();
        save_token(&path, "second-token").unwrap();
        assert_eq!(read_token(&path).unwrap().as_deref(), Some("second-token"));
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                std::fs::metadata(path).unwrap().permissions().mode() & 0o777,
                0o600
            );
        }
    }

    #[test]
    fn callback_requires_the_application_origin_and_exact_host_and_is_single_use() {
        tokio::runtime::Runtime::new().unwrap().block_on(async {
            let (sender, receiver) = oneshot::channel();
            let state = CallbackState {
                origin: "https://ergosandbox.com".into(),
                host: "127.0.0.1:1234".into(),
                result: Arc::new(Mutex::new(Some(sender))),
            };
            let mut headers = HeaderMap::new();
            headers.insert(header::HOST, state.host.parse().unwrap());
            for origin in [None, Some("null"), Some("https://attacker.example")] {
                if let Some(origin) = origin {
                    headers.insert(header::ORIGIN, origin.parse().unwrap());
                }
                assert_eq!(
                    callback(
                        State(state.clone()),
                        headers.clone(),
                        Form(Credentials {
                            token: "valid-token".into()
                        })
                    )
                    .await
                    .status(),
                    StatusCode::FORBIDDEN
                );
            }
            headers.insert(header::ORIGIN, state.origin.parse().unwrap());
            headers.insert(header::HOST, "attacker.example:1234".parse().unwrap());
            assert_eq!(
                callback(
                    State(state.clone()),
                    headers.clone(),
                    Form(Credentials {
                        token: "valid-token".into()
                    })
                )
                .await
                .status(),
                StatusCode::FORBIDDEN
            );
            headers.insert(header::HOST, state.host.parse().unwrap());
            assert_eq!(
                callback(
                    State(state.clone()),
                    headers.clone(),
                    Form(Credentials {
                        token: "valid-token".into()
                    })
                )
                .await
                .status(),
                StatusCode::OK
            );
            assert_eq!(receiver.await.unwrap(), "valid-token");
            assert_eq!(
                callback(
                    State(state),
                    headers,
                    Form(Credentials {
                        token: "another-token".into()
                    })
                )
                .await
                .status(),
                StatusCode::GONE
            );
        });
    }
}
