//! The installed part library: served documents, startup bootstrap and the
//! operator-only synthesis job (`parts-sync`), with hot swapping.
use crate::{ApiError, AppState, store::Store};
use anyhow::{Context, Result};
use axum::{
    Json,
    extract::State,
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeSet,
    sync::{Arc, Mutex, RwLock},
    time::{SystemTime, UNIX_EPOCH},
};

/// The exact JSON documents of the installed revision.
#[derive(Clone)]
pub struct Documents {
    pub revision: String,
    pub library: Arc<[u8]>,
    /// Reviewed tier, for static illustrations.
    pub reviewed: Arc<[u8]>,
}

#[derive(Clone, Default, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SyncState {
    #[default]
    Idle,
    Running,
    Succeeded,
    Failed,
}

#[derive(Clone, Default, Serialize)]
pub struct SyncStatus {
    pub state: SyncState,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub started_at: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub finished_at: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dry_run: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub report: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[derive(Clone)]
pub struct Parts {
    documents: Arc<RwLock<Documents>>,
    status: Arc<Mutex<SyncStatus>>,
    /// Signed-in user IDs allowed to run the synthesis (`ERGO_OPERATOR_IDS`).
    operators: Arc<BTreeSet<String>>,
}

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

/// Pull the official inputs and synthesize; publish unless `dry_run`.
pub async fn run_sync(store: &Store, dry_run: bool) -> Result<ergo_assets::library::Synthesis> {
    let key = std::env::var("REBRICKABLE_API_KEY")
        .ok()
        .filter(|k| !k.is_empty());
    if key.is_some() {
        tracing::info!("Using REBRICKABLE_API_KEY for BrickLink deduplication");
    } else {
        tracing::warn!(
            "REBRICKABLE_API_KEY is unset: BrickLink numbers will not be deduplicated or listed"
        );
    }
    let workers = std::thread::available_parallelism().map_or(4, |n| n.get());
    tracing::info!(dry_run, "Pulling part-library inputs");
    let inputs = ergo_assets::library::pull(key.as_deref()).await?;
    tracing::info!(workers, "Synthesizing the part library");
    let synthesis = ergo_assets::library::sync(
        store.backend.clone(),
        &store.prefix,
        inputs,
        workers,
        dry_run,
    )
    .await?;
    tracing::info!(
        revision = %synthesis.library.revision,
        parts = synthesis.library.parts.len(),
        objects_written = synthesis.report.objects_written,
        dry_run,
        "Part-library synthesis finished"
    );
    Ok(synthesis)
}

impl Parts {
    /// Install the published library, synthesizing and publishing it first
    /// when the store has none (a new deployment's first start).
    pub async fn bootstrap(store: &Store) -> Result<Self> {
        let current = ergo_assets::library::load_current(&*store.backend, &store.prefix).await?;
        let (library, json) = match current {
            Some(found) => found,
            None => {
                tracing::warn!(
                    "No part library is published in this store; running the first synthesis before serving"
                );
                let synthesis = run_sync(store, false).await?;
                (synthesis.library, synthesis.json)
            }
        };
        let documents = install(library, json)?;
        let operators = std::env::var("ERGO_OPERATOR_IDS")
            .unwrap_or_default()
            .split(',')
            .map(str::trim)
            .filter(|id| !id.is_empty())
            .map(str::to_owned)
            .collect();
        Ok(Self {
            documents: Arc::new(RwLock::new(documents)),
            status: Default::default(),
            operators: Arc::new(operators),
        })
    }

    pub fn documents(&self) -> Documents {
        self.documents.read().unwrap().clone()
    }

    #[cfg(test)]
    pub fn for_tests() -> Self {
        let library = ergo_world::part_library::library().clone();
        let json = serde_json::to_vec(&library).unwrap();
        Self {
            documents: Arc::new(RwLock::new(install(library, json).unwrap())),
            status: Default::default(),
            operators: Arc::new(BTreeSet::from(["operator".to_owned()])),
        }
    }
}

/// Verify, install (hot swap) and return the documents to serve.
fn install(library: ergo_world::part_library::Library, json: Vec<u8>) -> Result<Documents> {
    let reviewed = serde_json::to_vec(&ergo_world::part_library::reviewed_tier(&library)?)?;
    let revision = library.revision.clone();
    let parts = library.parts.len();
    ergo_world::part_library::install(library)?;
    tracing::info!(%revision, parts, "Installed part library");
    Ok(Documents {
        revision,
        library: json.into(),
        reviewed: reviewed.into(),
    })
}

fn authorize(s: &AppState, headers: &HeaderMap) -> Result<(), ApiError> {
    let user = s.auth.check_write(headers)?;
    // Local development without Google sign-in is its own operator.
    if s.auth.development() || s.library.operators.contains(&user) {
        Ok(())
    } else {
        Err(anyhow::anyhow!(
            "Running the part-library synthesis requires an operator account (ERGO_OPERATOR_IDS)"
        )
        .into())
    }
}

#[derive(Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct SyncRequest {
    #[serde(default)]
    dry_run: bool,
}

/// `GET /api/admin/parts-sync`: state of the current or last synthesis job.
pub async fn status(
    State(s): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<SyncStatus>, ApiError> {
    authorize(&s, &headers)?;
    Ok(Json(s.library.status.lock().unwrap().clone()))
}

/// `POST /api/admin/parts-sync`: start one synthesis job in the background.
/// A published revision is installed immediately (hot swap).
pub async fn start(
    State(s): State<AppState>,
    headers: HeaderMap,
    body: Option<Json<SyncRequest>>,
) -> Result<Response, ApiError> {
    authorize(&s, &headers)?;
    let dry_run = body.map(|Json(b)| b.dry_run).unwrap_or_default();
    {
        let mut status = s.library.status.lock().unwrap();
        if matches!(status.state, SyncState::Running) {
            return Ok((StatusCode::CONFLICT, Json(status.clone())).into_response());
        }
        *status = SyncStatus {
            state: SyncState::Running,
            started_at: Some(now()),
            dry_run: Some(dry_run),
            ..Default::default()
        };
    }
    let (store, parts) = (s.store.clone(), s.library.clone());
    tokio::spawn(async move {
        let result = async {
            let synthesis = run_sync(&store, dry_run).await?;
            if !dry_run {
                let documents = install(synthesis.library, synthesis.json)?;
                *parts.documents.write().unwrap() = documents;
            }
            serde_json::to_value(&synthesis.report).context("Report")
        }
        .await;
        let mut status = parts.status.lock().unwrap();
        status.finished_at = Some(now());
        match result {
            Ok(report) => {
                status.state = SyncState::Succeeded;
                status.report = Some(report);
            }
            Err(error) => {
                tracing::error!(error = %format!("{error:#}"), "Part-library synthesis failed");
                status.state = SyncState::Failed;
                status.error = Some(format!("{error:#}"));
            }
        }
    });
    let status = s.library.status.lock().unwrap().clone();
    Ok((StatusCode::ACCEPTED, Json(status)).into_response())
}
