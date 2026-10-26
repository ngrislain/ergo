//! Unbounded persistent world, bounded edit/validation working sets. No route
//! scans or exports the whole world's geometry and no global size counter exists.
macro_rules! ensure {
    ($condition:expr, $($message:tt)*) => { if !$condition { return Err(anyhow::anyhow!($($message)*).into()); } };
}
pub(crate) mod address;
mod administration;
mod authoring;
pub(crate) mod catalog;
mod events;
mod group_commit;
mod joints;
mod model;
pub(crate) mod pacing;
mod paged;
mod retention;
mod spatial;
#[cfg(test)]
mod tests;
mod view;
mod working;

use crate::{AppState, auth::Principal, store::Store};
use address::{block_path, entity_path, root};
use anyhow::{Context, Result};
use axum::{
    Json, Router,
    extract::{Path, Query, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::{get, post, put},
};
use ergo_transactions::{
    CommitOptions, ContentId, OctreePath, Repository, Snapshot, canonical_cbor,
};
use model::*;
use retention::blob_links;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use working::Working;

#[derive(Clone, Debug)]
pub struct SharedError {
    status: StatusCode,
    message: String,
    candidate: Option<ContentId>,
    world: Option<String>,
    retry_after: Option<u64>,
}
impl SharedError {
    fn forbidden() -> Self {
        Self::status(StatusCode::FORBIDDEN, "Shared-world permission denied")
    }
    fn unauthorized() -> Self {
        Self::status(StatusCode::UNAUTHORIZED, "Sign in to access shared worlds")
    }
    fn missing() -> Self {
        Self::status(StatusCode::NOT_FOUND, "Shared-world entity is absent")
    }
    fn working_limit(what: &str) -> Self {
        Self::status(
            StatusCode::PAYLOAD_TOO_LARGE,
            &format!(
                "Edit working-set limit exceeded ({what}); the persistent world has no size limit. No dependency was truncated."
            ),
        )
    }
    fn status(status: StatusCode, message: &str) -> Self {
        Self {
            status,
            message: message.into(),
            candidate: None,
            world: None,
            retry_after: None,
        }
    }
}
impl std::fmt::Display for SharedError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}
impl std::error::Error for SharedError {}
impl From<anyhow::Error> for SharedError {
    fn from(e: anyhow::Error) -> Self {
        Self::status(StatusCode::UNPROCESSABLE_ENTITY, &format!("{e:#}"))
    }
}
impl From<ergo_transactions::Error> for SharedError {
    fn from(e: ergo_transactions::Error) -> Self {
        use ergo_transactions::Error;
        let status = match &e {
            Error::Conflict { .. }
            | Error::Contention { .. }
            | Error::InvalidBase
            | Error::BranchExists(_) => StatusCode::CONFLICT,
            Error::RevisionUnavailable => StatusCode::GONE,
            Error::ObjectTooLarge { .. }
            | Error::TraversalLimit { .. }
            | Error::CompactionLimit { .. } => StatusCode::PAYLOAD_TOO_LARGE,
            Error::Store(object_store::Error::NotFound { .. }) => StatusCode::NOT_FOUND,
            Error::PublicationUnknown { .. }
            | Error::MaintenanceBusy
            | Error::ActivePins { .. } => StatusCode::SERVICE_UNAVAILABLE,
            Error::Store(_) | Error::Corrupt(_) => StatusCode::BAD_GATEWAY,
            _ => StatusCode::BAD_REQUEST,
        };
        let mut result = Self::status(status, &e.to_string());
        if let Error::PublicationUnknown { candidate, .. } = e {
            result.candidate = Some(candidate);
        }
        result
    }
}
impl IntoResponse for SharedError {
    fn into_response(self) -> Response {
        let mut response=(self.status, Json(serde_json::json!({"error": self.message, "candidate": self.candidate, "world": self.world,"retry_after_seconds":self.retry_after}))).into_response();
        if let Some(seconds) = self.retry_after {
            response
                .headers_mut()
                .insert("Retry-After", seconds.to_string().parse().unwrap());
        }
        response
    }
}
type SharedResult<T> = std::result::Result<T, SharedError>;

#[derive(Clone)]
pub struct SharedWorlds {
    pub(crate) common_world: Option<String>,
    common_root: Option<Repository>,
    root: Repository,
    store: Store,
    limits: Limits,
    hubs: std::sync::Arc<events::Hubs>,
    event_period: std::time::Duration,
    queues: std::sync::Arc<group_commit::Queues>,
    group_commit: bool,
    pub(crate) pacing: pacing::Pacing,
}
impl SharedWorlds {
    async fn actor_name(&self, actor: &str) -> Result<Option<String>> {
        Ok(self
            .store
            .user(actor)
            .await?
            .map(|user| user.name.chars().take(120).collect()))
    }
    pub fn new(store: Store) -> Result<Self> {
        let prefix = if store.prefix.is_empty() {
            "shared".into()
        } else {
            format!("{}/shared", store.prefix)
        };
        Ok(Self {
            common_world: None,
            common_root: None,
            pacing: pacing::Pacing::disabled(store.clone()),
            root: Repository::new(store.backend.clone(), &prefix)?
                .with_cache_capacity(64 * 1024 * 1024)
                .with_coordinated_gc(),
            store,
            limits: Limits::default(),
            hubs: std::sync::Arc::new(events::Hubs::default()),
            event_period: std::time::Duration::from_secs(1),
            queues: std::sync::Arc::new(group_commit::Queues::default()),
            group_commit: false,
        })
    }
    pub fn from_env(store: Store) -> Result<Self> {
        let mut service = Self::new(store)?;
        service.common_world = std::env::var("ERGO_COMMON_WORLD_ID")
            .ok()
            .map(|id| uuid::Uuid::parse_str(&id).map(|id| id.to_string()))
            .transpose()?;
        if let Ok(prefix) = std::env::var("ERGO_WORLD_PREFIX") {
            anyhow::ensure!(
                service.common_world.is_some() && !prefix.trim_matches('/').is_empty(),
                "ERGO_WORLD_PREFIX requires a configured common world and a dedicated prefix"
            );
            service.common_root = Some(service.namespace(&prefix)?);
        }
        service.pacing = pacing::Pacing::from_env(service.store.clone())?;
        if let Ok(v) = std::env::var("ERGO_SHARED_GROUP_COMMIT") {
            anyhow::ensure!(
                ["0", "1", "false", "true"].contains(&v.as_str()),
                "Invalid group commit flag"
            );
            service.group_commit = v == "1" || v == "true";
        }
        if let Ok(v) = std::env::var("ERGO_SHARED_EVENT_POLL_MS") {
            let ms: u64 = v.parse()?;
            anyhow::ensure!((50..=60_000).contains(&ms), "Invalid event poll interval");
            service.event_period = std::time::Duration::from_millis(ms);
        }
        if let Ok(v) = std::env::var("ERGO_SHARED_WORKING_PIECES") {
            service.limits.pieces = v.parse()?;
        }
        if let Ok(v) = std::env::var("ERGO_SHARED_WORKING_BYTES") {
            service.limits.bytes = v.parse()?;
        }
        anyhow::ensure!(
            service.limits.pieces > 0
                && service.limits.pieces < u32::MAX as usize
                && service.limits.bytes > 0,
            "Invalid shared edit working-set limits"
        );
        Ok(service)
    }
    pub(crate) fn repo(&self, id: &str) -> SharedResult<Repository> {
        if self
            .common_world
            .as_ref()
            .is_some_and(|common| common != id)
        {
            return Err(SharedError::status(
                StatusCode::NOT_FOUND,
                "Only the shared Ergo Sandbox is available",
            ));
        }
        uuid::Uuid::parse_str(id).map_err(|_| {
            SharedError::status(StatusCode::BAD_REQUEST, "Invalid shared-world UUID")
        })?;
        if let Some(repo) = &self.common_root {
            return Ok(repo.clone());
        }
        Ok(self.root.child(id)?)
    }
    async fn authorized(
        &self,
        id: &str,
        principal: &Principal,
    ) -> SharedResult<(Repository, Snapshot, Policy)> {
        let actor = principal
            .id
            .as_deref()
            .ok_or_else(SharedError::unauthorized)?;
        let repo = self.repo(id)?;
        let snapshot = repo.checkout("main").await?;
        let policy: Policy = snapshot
            .get_limited(&root(0), self.limits.bytes)
            .await?
            .ok_or_else(SharedError::forbidden)?;
        if self.common_world.as_deref() != Some(id) && !policy.can_read(actor) {
            return Err(SharedError::forbidden());
        }
        Ok((repo, snapshot, policy))
    }
    async fn working(&self, id: &str, actor: &str, base: &ContentId) -> SharedResult<Working> {
        let (repo, _, _) = self
            .authorized(
                id,
                &Principal {
                    id: Some(actor.into()),
                },
            )
            .await?;
        let mut limits = self.limits;
        if self.pacing.unlimited(actor).await? {
            limits.pieces = usize::MAX;
        }
        let ws = Working::new(repo.begin_at("main", base).await?, limits).await?;
        if self.common_world.as_deref() != Some(id) && !ws.policy.can_read(actor) {
            return Err(SharedError::forbidden());
        }
        Ok(ws)
    }
    pub async fn create(&self, actor: &str, request: NewWorld) -> SharedResult<Head> {
        if let Some(common) = &self.common_world {
            return Err(SharedError::status(
                StatusCode::CONFLICT,
                &format!(
                    "World creation is disabled; only the public Ergo Sandbox ({common}) is available"
                ),
            ));
        }
        ensure!(
            !request.name.trim().is_empty() && request.name.chars().count() <= 120,
            "Invalid world name"
        );
        request.geo.validate()?;
        let id = uuid::Uuid::new_v4().to_string();
        self.advertise_users(&id, [actor.to_owned()]).await?;
        let repo = self.repo(&id)?;
        let initial = repo.create("main").await.map_err(|e| {
            let mut e = SharedError::from(e);
            e.world = Some(id.clone());
            e
        })?;
        initial.close().await?;
        let mut tx = repo.begin("main").await?;
        let model = PhysicalModel {
            algorithm: 1,
            measurements: self.store.measurements.as_ref().clone(),
            tolerances: self.store.static_tolerances,
        };
        model.measurements.validate()?;
        model.tolerances.validate()?;
        let model = tx.stage_blob(&model)?;
        let metadata = Metadata {
            id: Some(id.clone()),
            name: request.name,
            geo: request.geo,
            model,
            layout: 2,
        };
        let policy = Policy {
            owner: actor.into(),
            grants: Vec::new(),
            keep_last: None,
            authenticated_editing: request.authenticated_editing,
        };
        tx.put(&root(0), &policy).await?;
        tx.put(&root(2), &metadata).await?;
        tx.set_metadata(&Audit {
            actor: actor.into(),
            actor_name: self.actor_name(actor).await?,
            base: tx.base_id().clone(),
            policy_hash: ContentId::of(&canonical_cbor(&policy)?),
            model: metadata.model.clone(),
            changed_pieces: BTreeSet::new(),
            affected_pieces: BTreeSet::new(),
            watched_regions: BTreeSet::new(),
            validation: None,
            loaded_pieces: 0,
            loaded_cbor_bytes: 0,
        })?;
        let outcome = tx
            .commit(actor, "Create shared world", CommitOptions::default())
            .await
            .map_err(|e| {
                let mut e = SharedError::from(e);
                e.world = Some(id.clone());
                e
            })?;
        Ok(Head {
            id,
            revision: outcome.snapshot.id().clone(),
            policy,
            metadata,
        })
    }
    pub async fn head(
        &self,
        id: &str,
        principal: &Principal,
        revision: Option<&ContentId>,
    ) -> SharedResult<Head> {
        let (repo, current, policy) = self.authorized(id, principal).await?;
        let snapshot = if let Some(revision) = revision {
            repo.accepted_snapshot("main", revision).await?
        } else {
            current
        };
        let metadata = snapshot
            .get_limited(&root(2), self.limits.bytes)
            .await?
            .context("Missing world metadata")?;
        let policy = if revision.is_some() {
            snapshot
                .get_limited(&root(0), self.limits.bytes)
                .await?
                .context("Missing historical policy")?
        } else {
            policy
        };
        Ok(Head {
            id: id.into(),
            revision: snapshot.id().clone(),
            policy,
            metadata,
        })
    }
    pub async fn import(
        &self,
        id: &str,
        actor: &str,
        mut request: ImportRequest,
    ) -> SharedResult<Imported> {
        self.destination(id)?;
        let mut ws = self.working(id, actor, &request.base).await?;
        if request.scene.bricks.len() > ws.piece_limit() {
            return Err(SharedError::working_limit("import batch pieces"));
        }
        ensure!(
            canonical_cbor(&request.scene.geo)? == canonical_cbor(&ws.metadata.geo)?,
            "Imported coordinates must use the world's geographic origin"
        );
        crate::import::validate_export(&mut request.scene)?;
        if let Some(model) = &request.scene.model {
            model.validate_isometries()?;
        }
        ws.model.measurements.normalize_scene(&mut request.scene)?;
        let mut ids = BTreeSet::new();
        ensure!(
            request
                .scene
                .bricks
                .iter()
                .all(|b| b.id > 0 && ids.insert(b.id)),
            "Duplicate or zero import-local IDs"
        );
        if canonical_cbor(&request.scene)?.len() > self.limits.bytes {
            return Err(SharedError::working_limit("import bytes"));
        }
        let assembly = ws.tx.stage_blob(&(
            "assembly-birth-v1",
            uuid::Uuid::new_v4().to_string(),
            actor,
            &request.scene.name,
        ))?;
        let mut meshes = BTreeMap::new();
        for (name, mesh) in &request.scene.meshes {
            meshes.insert(name.clone(), ws.tx.stage_blob(mesh)?);
        }
        let mut pieces = BTreeMap::new();
        let mut spawned = Vec::new();
        let actor_name = self.actor_name(actor).await?;
        for brick in &request.scene.bricks {
            let mesh = meshes
                .get(&brick.part)
                .context("Missing imported mesh")?
                .clone();
            let birth = Birth {
                nonce: uuid::Uuid::new_v4().to_string(),
                created_by: actor.into(),
                created_by_name: actor_name.clone(),
                assembly: assembly.clone(),
                initial: Geometry::of(brick),
                mesh: mesh.clone(),
            };
            let piece_id = ws.tx.stage_blob(&birth)?;
            pieces.insert(brick.id, piece_id.clone());
            spawned.push(Piece {
                id: piece_id,
                assembly: assembly.clone(),
                local_id: brick.id,
                geometry: Geometry::of(brick),
                mesh,
                replicas: BTreeSet::new(),
            });
        }
        let source_model = ws
            .tx
            .stage_blob(&ergo_world::assembly::Model::for_scene(&request.scene))?;
        let rules = Rules {
            id: assembly.clone(),
            name: request.scene.name,
            pieces: pieces.clone(),
            meshes,
            joints: request.scene.joints,
            fixed_bricks: request.scene.fixed_bricks,
            groups: request.scene.groups,
            source_model: Some(source_model),
        };
        ws.stage_rules(rules)?;
        let changed: BTreeSet<_> = pieces.values().cloned().collect();
        for mut piece in spawned {
            ensure!(
                ws.piece(&piece.id).await?.is_none(),
                "Creation-record identity already exists"
            );
            piece.replicas = ws.cells(&piece).await?;
            ws.permit(actor, &piece)?;
            ws.charge(&piece)?;
            ws.replace(None, Some(piece)).await?;
        }
        ws.flush_rules(&BTreeSet::from([assembly.clone()])).await?;
        let affected = ws.discover(&changed).await?;
        let report = ws.validate(&affected).await?;
        self.advertise_blocks(id, ws.metadata.geo, &ws.new_blocks)
            .await?;
        let prepared = self
            .prepared(
                id,
                actor,
                request.base,
                "Import bounded assembly".into(),
                changed,
                affected,
                report,
                ws,
            )
            .await?;
        let accepted = prepared.publish_with(self).await?;
        Ok(Imported {
            accepted,
            assembly,
            pieces,
        })
    }
    pub async fn prepare(
        &self,
        id: &str,
        actor: &str,
        request: CommitRequest,
    ) -> SharedResult<Prepared> {
        self.destination(id)?;
        ensure!(
            !request.pieces.is_empty() || !request.rules.is_empty() || !request.joints.is_empty(),
            "Empty edit"
        );
        ensure!(request.message.len() <= 4096, "Commit message is too large");
        let mut ws = self.working(id, actor, &request.base).await?;
        if request
            .pieces
            .len()
            .saturating_add(request.rules.len())
            .saturating_add(request.joints.len())
            > ws.piece_limit()
        {
            return Err(SharedError::working_limit("mutation batch"));
        }
        let mut old = BTreeMap::new();
        let mut seeds = BTreeSet::new();
        let mut rule_changes = BTreeMap::new();
        for edit in &request.pieces {
            ensure!(!old.contains_key(&edit.id), "Duplicate piece mutation");
            let piece = ws.piece(&edit.id).await?.ok_or_else(SharedError::missing)?;
            ws.permit(actor, &piece)?;
            seeds.insert(edit.id.clone());
            old.insert(edit.id.clone(), piece);
        }
        for edit in &request.rules {
            ensure!(
                !rule_changes.contains_key(&edit.assembly),
                "Duplicate rules mutation"
            );
            let mut rules = ws.rules(&edit.assembly).await?.as_ref().clone();
            seeds.extend(rule_references(&rules)?);
            rules.joints = edit.joints.clone();
            rules.fixed_bricks = edit.fixed_bricks.clone();
            rules.groups = edit.groups.clone();
            seeds.extend(rule_references(&rules)?);
            rule_changes.insert(edit.assembly.clone(), rules);
        }
        let mut joint_mutations = BTreeSet::new();
        for edit in &request.joints {
            if let Some(id) = &edit.id {
                ensure!(
                    joint_mutations.insert(id.clone()),
                    "Duplicate joint mutation"
                );
                let j = ws.global_joint(id).await?;
                seeds.extend([j.a, j.b]);
            }
            if let Some(j) = &edit.value {
                ensure!(
                    ws.piece(&j.a).await?.is_some() && ws.piece(&j.b).await?.is_some(),
                    "Joint endpoint is absent"
                );
                seeds.extend([j.a.clone(), j.b.clone()]);
            }
        }
        if seeds.is_empty() && actor != ws.policy.owner {
            return Err(SharedError::forbidden());
        }
        for seed in &seeds {
            if let Some(piece) = ws.piece(seed).await? {
                ws.permit(actor, &piece)?;
            }
        }
        let before = ws.discover(&seeds).await?;
        let mut joint_ids = Vec::new();
        for edit in &request.joints {
            joint_ids.push(ws.change_joint(actor, edit).await?);
        }
        for edit in &request.pieces {
            let previous = &old[&edit.id];
            if let Some(geometry) = &edit.geometry {
                let mut next = previous.clone();
                next.geometry = geometry.clone();
                if let Some(mesh) = &edit.mesh {
                    ws.tx.retain_blob(mesh).await?;
                    next.mesh = mesh.clone();
                } else if geometry.part != previous.geometry.part {
                    next.mesh = ws
                        .assembly(&next.assembly)
                        .await?
                        .meshes
                        .get(&geometry.part)
                        .context("Part has no mesh in this assembly")?
                        .clone();
                }
                next.replicas = ws.cells(&next).await?;
                ws.permit(actor, &next)?;
                ws.swept(previous, &next).await?;
                ws.replace(Some(previous), Some(next)).await?;
            } else {
                ensure!(edit.mesh.is_none(), "A deletion cannot change a mesh");
                ws.no_external(&previous.id).await?;
                let mut rules = if let Some(rules) = rule_changes.remove(&previous.assembly) {
                    rules
                } else if !ws.assembly(&previous.assembly).await?.paged {
                    ws.rules(&previous.assembly).await?.as_ref().clone()
                } else {
                    ws.remove_member(previous).await?;
                    ws.replace(Some(previous), None).await?;
                    continue;
                };
                rules.pieces.remove(&previous.local_id);
                rule_changes.insert(previous.assembly.clone(), rules);
                ws.replace(Some(previous), None).await?;
            }
        }
        let assemblies: BTreeSet<_> = rule_changes.keys().cloned().collect();
        for (_, rules) in rule_changes {
            ws.stage_rules(rules)?;
        }
        ws.flush_rules(&assemblies).await?;
        seeds.extend(before);
        ws.reset_dependencies();
        let affected = ws.discover(&seeds).await?;
        let report = ws.validate(&affected).await?;
        self.advertise_blocks(id, ws.metadata.geo, &ws.new_blocks)
            .await?;
        let changed = request.pieces.iter().map(|p| p.id.clone()).collect();
        let mut prepared = self
            .prepared(
                id,
                actor,
                request.base,
                request.message,
                changed,
                affected,
                report,
                ws,
            )
            .await?;
        prepared.joint_ids = joint_ids;
        Ok(prepared)
    }
    #[allow(clippy::too_many_arguments)]
    async fn prepared(
        &self,
        id: &str,
        actor: &str,
        base: ContentId,
        message: String,
        changed: BTreeSet<ContentId>,
        affected: BTreeSet<ContentId>,
        validation: Option<ergo_world::validation::ValidationReport>,
        mut ws: Working,
    ) -> SharedResult<Prepared> {
        let audit = Audit {
            actor: actor.into(),
            actor_name: self.actor_name(actor).await?,
            base,
            policy_hash: ws.policy_cid.clone(),
            model: ws.metadata.model.clone(),
            changed_pieces: changed,
            affected_pieces: affected,
            watched_regions: ws.regions.clone(),
            validation,
            loaded_pieces: ws.pieces.len(),
            loaded_cbor_bytes: ws.loaded_bytes,
        };
        ws.tx.set_metadata(&audit)?;
        Ok(Prepared {
            id: id.into(),
            tx: ws.tx,
            actor: actor.into(),
            message,
            audit,
            joint_ids: Vec::new(),
        })
    }
    pub async fn grants(
        &self,
        id: &str,
        actor: &str,
        request: GrantRequest,
    ) -> SharedResult<ContentId> {
        ensure!(
            self.common_world.is_none() || request.authenticated_editing != Some(false),
            "The public Ergo Sandbox must remain editable by signed-in users"
        );
        let mut ws = self.working(id, actor, &request.base).await?;
        if ws.policy.owner != actor {
            return Err(SharedError::forbidden());
        }
        ws.policy.grants = request.grants;
        if let Some(enabled) = request.authenticated_editing {
            ws.policy.authenticated_editing = enabled;
        }
        ws.policy.validate()?;
        self.advertise_users(
            id,
            std::iter::once(ws.policy.owner.clone())
                .chain(ws.policy.grants.iter().map(|g| g.user.clone())),
        )
        .await?;
        ws.tx.put(&root(0), &ws.policy).await?;
        let result = self
            .prepared(
                id,
                actor,
                request.base,
                "Update grants".into(),
                BTreeSet::new(),
                BTreeSet::new(),
                None,
                ws,
            )
            .await?
            .publish_with(self)
            .await?;
        Ok(result.revision)
    }
    pub async fn collect(&self, id: &str) -> Result<ergo_transactions::GcReport> {
        let repo = self.repo(id)?;
        repo.checkout("main").await?.close().await?;
        let mut maintenance = repo.maintenance().await?;
        tracing::info!(
            world = id,
            token = maintenance.token(),
            "Shared-world maintenance barrier acquired"
        );
        // Every immutable mesh, creation record, and model is an explicit commit attachment.
        let options = ergo_transactions::GcOptions {
            scratch_directory: std::env::var_os("ERGO_GC_SCRATCH_DIR")
                .map(std::path::PathBuf::from),
            max_object_bytes: self.limits.bytes,
            ..Default::default()
        };
        let report = maintenance
            .collect_with_options(options, blob_links)
            .await?;
        maintenance.finish().await?;
        Ok(report)
    }
}
fn rule_references(rules: &Rules) -> Result<BTreeSet<ContentId>> {
    let ids = rules
        .joints
        .iter()
        .flat_map(|j| [j.a, j.b])
        .chain(rules.fixed_bricks.iter().copied())
        .chain(rules.groups.iter().flat_map(|g| {
            g.members.iter().filter_map(|m| match m {
                ergo_world::groups::Member::Piece { id } => Some(*id),
                _ => None,
            })
        }));
    ids.map(|id| {
        rules
            .pieces
            .get(&id)
            .cloned()
            .context("Rule references missing assembly-local ID")
    })
    .collect()
}
pub struct Prepared {
    id: String,
    tx: ergo_transactions::Transaction,
    actor: String,
    message: String,
    audit: Audit,
    joint_ids: Vec<ContentId>,
}
impl Prepared {
    pub async fn publish(self) -> SharedResult<Accepted> {
        let result = self
            .tx
            .commit(&self.actor, &self.message, CommitOptions::default())
            .await?;
        Ok(Accepted {
            id: self.id,
            revision: result.snapshot.id().clone(),
            attempts: result.attempts,
            rebases: result.rebases,
            audit: self.audit,
            joint_ids: self.joint_ids,
        })
    }
}

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/shared-worlds", get(catalog::list).post(create))
        .route("/api/shared-worlds/allowance", get(allowance))
        .route("/api/common-world/locate", get(authoring::locate))
        .route("/api/shared-worlds/{id}", get(head))
        .route("/api/shared-worlds/{id}/view", get(view::view))
        .route(
            "/api/shared-worlds/{id}/assemblies",
            get(authoring::list).post(import),
        )
        .route(
            "/api/shared-worlds/{id}/assemblies/{assembly}/workspace",
            get(authoring::workspace).put(authoring::replace),
        )
        .route(
            "/api/shared-worlds/{id}/assemblies/{assembly}/location",
            get(authoring::location),
        )
        .route(
            "/api/shared-worlds/{id}/assemblies/{assembly}",
            get(assembly),
        )
        .route(
            "/api/shared-worlds/{id}/assemblies/{assembly}/pages",
            get(rule_page),
        )
        .route("/api/shared-worlds/{id}/pieces/{piece}", get(piece))
        .route(
            "/api/shared-worlds/{id}/pieces/{piece}/joints",
            get(incident_joints),
        )
        .route("/api/shared-worlds/{id}/joints/{joint}", get(joint))
        .route("/api/shared-worlds/{id}/pieces/{piece}/mesh", get(mesh))
        .route("/api/shared-worlds/{id}/blocks", get(blocks))
        .route("/api/shared-worlds/{id}/blocks/{x}/{y}/{z}", get(block))
        .route(
            "/api/shared-worlds/{id}/blocks/{x}/{y}/{z}/pieces",
            get(block_pieces),
        )
        .route("/api/shared-worlds/{id}/commits", post(commit))
        .route("/api/shared-worlds/{id}/grants", put(grants))
        .route("/api/shared-worlds/{id}/retention", put(retention::set))
        .route("/api/shared-worlds/{id}/history", get(history))
        .route("/api/shared-worlds/{id}/events", get(events::subscribe))
}
fn reader(s: &AppState, h: &HeaderMap) -> SharedResult<Principal> {
    let p = s
        .auth
        .shared_principal(h)
        .map_err(|_| SharedError::unauthorized())?;
    if p.id.is_none() && s.shared.common_world.is_some() {
        return Ok(Principal {
            id: Some("public-viewer".into()),
        });
    }
    if p.id.is_none() {
        return Err(SharedError::unauthorized());
    }
    Ok(p)
}
async fn allowance(
    State(s): State<AppState>,
    headers: HeaderMap,
) -> SharedResult<Json<serde_json::Value>> {
    let actor = s
        .auth
        .shared_principal(&headers)
        .map_err(|_| SharedError::unauthorized())?
        .required()
        .map_err(|_| SharedError::unauthorized())?;
    let mut status = s.shared.pacing.status(&actor).await?;
    status["authoring_piece_limit"] = if status["god_mode"] == true {
        serde_json::Value::Null
    } else {
        serde_json::json!(s.shared.limits.pieces)
    };
    status["actor"] = serde_json::json!(actor);
    Ok(Json(status))
}
fn writer(s: &AppState, h: &HeaderMap) -> SharedResult<String> {
    if h.contains_key(axum::http::header::AUTHORIZATION) {
        return Err(SharedError::forbidden());
    }
    reader(s, h)?;
    s.auth
        .check_write(h)
        .map_err(|e| SharedError::status(StatusCode::FORBIDDEN, &e.to_string()))
}
fn publisher(s: &AppState, h: &HeaderMap) -> SharedResult<String> {
    reader(s, h)?;
    s.auth
        .check_publish(h)
        .map_err(|e| SharedError::status(StatusCode::FORBIDDEN, &e.to_string()))
}
async fn create(
    State(s): State<AppState>,
    h: HeaderMap,
    Json(body): Json<NewWorld>,
) -> SharedResult<Json<Head>> {
    Ok(Json(s.shared.create(&publisher(&s, &h)?, body).await?))
}
#[derive(Deserialize)]
struct RevisionQuery {
    revision: Option<ContentId>,
}
async fn snapshot(
    s: &AppState,
    id: &str,
    h: &HeaderMap,
    revision: Option<&ContentId>,
) -> SharedResult<Snapshot> {
    let (repo, current, _) = s.shared.authorized(id, &reader(s, h)?).await?;
    if let Some(id) = revision {
        Ok(repo.accepted_snapshot("main", id).await?)
    } else {
        Ok(current)
    }
}
async fn head(
    State(s): State<AppState>,
    h: HeaderMap,
    Path(id): Path<String>,
    Query(q): Query<RevisionQuery>,
) -> SharedResult<Json<Head>> {
    Ok(Json(
        s.shared
            .head(&id, &reader(&s, &h)?, q.revision.as_ref())
            .await?,
    ))
}
async fn import(
    State(s): State<AppState>,
    h: HeaderMap,
    Path(id): Path<String>,
    Json(body): Json<ImportRequest>,
) -> SharedResult<Json<Imported>> {
    Ok(Json(s.shared.import(&id, &publisher(&s, &h)?, body).await?))
}
async fn commit(
    State(s): State<AppState>,
    h: HeaderMap,
    Path(id): Path<String>,
    Json(body): Json<CommitRequest>,
) -> SharedResult<Json<Accepted>> {
    Ok(Json(
        s.shared
            .prepare(&id, &editor(&s, &h)?, body)
            .await?
            .publish_with(&s.shared)
            .await?,
    ))
}
async fn grants(
    State(s): State<AppState>,
    h: HeaderMap,
    Path(id): Path<String>,
    Json(body): Json<GrantRequest>,
) -> SharedResult<Json<ContentId>> {
    Ok(Json(s.shared.grants(&id, &writer(&s, &h)?, body).await?))
}
fn editor(s: &AppState, h: &HeaderMap) -> SharedResult<String> {
    reader(s, h)?;
    s.auth
        .check_shared_edit(h)
        .map_err(|e| SharedError::status(StatusCode::FORBIDDEN, &e.to_string()))
}
async fn piece(
    State(s): State<AppState>,
    h: HeaderMap,
    Path((id, piece)): Path<(String, ContentId)>,
    Query(q): Query<RevisionQuery>,
) -> SharedResult<Json<Option<Piece>>> {
    Ok(Json(
        snapshot(&s, &id, &h, q.revision.as_ref())
            .await?
            .get_limited(&entity_path(3, &piece), s.shared.limits.bytes)
            .await?,
    ))
}
async fn joint(
    State(s): State<AppState>,
    h: HeaderMap,
    Path((id, joint)): Path<(String, ContentId)>,
    Query(q): Query<RevisionQuery>,
) -> SharedResult<Json<Option<joints::GlobalJoint>>> {
    Ok(Json(
        snapshot(&s, &id, &h, q.revision.as_ref())
            .await?
            .get_limited(&entity_path(6, &joint), s.shared.limits.bytes)
            .await?,
    ))
}
async fn incident_joints(
    State(s): State<AppState>,
    h: HeaderMap,
    Path((id, piece)): Path<(String, ContentId)>,
    Query(q): Query<PiecePageQuery>,
) -> SharedResult<Json<serde_json::Value>> {
    let limit = q.limit.unwrap_or(64);
    ensure!((1..=128).contains(&limit), "Page limit must be 1..128");
    let after = q
        .after
        .as_deref()
        .map(str::parse::<OctreePath>)
        .transpose()?;
    let snapshot = snapshot(&s, &id, &h, Some(&q.revision)).await?;
    let page = snapshot
        .scan_page(&joints::incident(&piece), after.as_ref(), limit)
        .await?;
    let mut ids = Vec::new();
    for (_, cid) in page.leaves {
        ids.push(snapshot.blob::<ContentId>(&cid).await?);
    }
    Ok(Json(
        serde_json::json!({"revision":q.revision,"joints":ids,"next":page.next.map(|p|p.to_string())}),
    ))
}
async fn assembly(
    State(s): State<AppState>,
    h: HeaderMap,
    Path((id, assembly)): Path<(String, ContentId)>,
    Query(q): Query<RevisionQuery>,
) -> SharedResult<Json<serde_json::Value>> {
    let snapshot = snapshot(&s, &id, &h, q.revision.as_ref()).await?;
    match snapshot
        .get_limited::<paged::RulesHeader>(&paged::rule_path(&assembly, 0), s.shared.limits.bytes)
        .await
    {
        Ok(header) => Ok(Json(
            serde_json::to_value(header).map_err(anyhow::Error::from)?,
        )),
        Err(ergo_transactions::Error::CoveredByLeaf(_)) => Ok(Json(
            serde_json::to_value(
                snapshot
                    .get_limited::<Rules>(&entity_path(4, &assembly), s.shared.limits.bytes)
                    .await?,
            )
            .map_err(anyhow::Error::from)?,
        )),
        Err(e) => Err(e.into()),
    }
}

#[derive(Deserialize)]
struct RulePageQuery {
    revision: ContentId,
    kind: u8,
    group: Option<u32>,
    after: Option<String>,
    limit: Option<usize>,
}
async fn rule_page(
    State(s): State<AppState>,
    h: HeaderMap,
    Path((id, assembly)): Path<(String, ContentId)>,
    Query(q): Query<RulePageQuery>,
) -> SharedResult<Json<serde_json::Value>> {
    ensure!((1..=6).contains(&q.kind), "Rule page kind must be 1..6");
    let prefix = if let Some(group) = q.group {
        ensure!(q.kind == 5, "group is only valid for group pages");
        paged::row(&assembly, 5, u64::from(group))
    } else {
        paged::rule_path(&assembly, q.kind)
    };
    let after = q
        .after
        .as_deref()
        .map(str::parse::<OctreePath>)
        .transpose()?;
    let limit = q.limit.unwrap_or(64);
    ensure!((1..=128).contains(&limit), "Page limit must be 1..128");
    let snapshot = snapshot(&s, &id, &h, Some(&q.revision)).await?;
    let page = snapshot.scan_page(&prefix, after.as_ref(), limit).await?;
    let mut rows = Vec::new();
    let mut remaining = s.shared.limits.bytes;
    for (path, cid) in page.leaves {
        let value: serde_json::Value = snapshot.blob_limited(&cid, remaining).await?;
        remaining = remaining.saturating_sub(canonical_cbor(&value)?.len());
        rows.push(serde_json::json!({"path":path.to_string(),"value":value}));
    }
    Ok(Json(
        serde_json::json!({"revision":q.revision,"rows":rows,"next":page.next.map(|p|p.to_string())}),
    ))
}

async fn mesh(
    State(s): State<AppState>,
    h: HeaderMap,
    Path((id, piece)): Path<(String, ContentId)>,
    Query(q): Query<RevisionQuery>,
) -> SharedResult<Response> {
    let immutable_revision = q.revision.is_some();
    let snapshot = snapshot(&s, &id, &h, q.revision.as_ref()).await?;
    let piece: Piece = snapshot
        .get_limited(&entity_path(3, &piece), s.shared.limits.bytes)
        .await?
        .ok_or_else(SharedError::missing)?;
    let mesh: ergo_world::geometry::Mesh = snapshot
        .blob_limited(&piece.mesh, s.shared.limits.bytes)
        .await?;
    Ok((
        [
            (
                axum::http::header::CONTENT_TYPE,
                "application/cbor".to_owned(),
            ),
            (
                axum::http::header::CACHE_CONTROL,
                if immutable_revision {
                    "private, max-age=31536000, immutable"
                } else {
                    "private, no-cache"
                }
                .to_owned(),
            ),
            (axum::http::header::ETAG, format!("\"{}\"", piece.mesh)),
        ],
        canonical_cbor(&mesh)?,
    )
        .into_response())
}
async fn block(
    State(s): State<AppState>,
    h: HeaderMap,
    Path((id, x, y, z)): Path<(String, i64, i64, i64)>,
    Query(q): Query<RevisionQuery>,
) -> SharedResult<Json<serde_json::Value>> {
    let snapshot = snapshot(&s, &id, &h, q.revision.as_ref()).await?;
    let coord = [x, y, z].into();
    Ok(Json(
        serde_json::to_value(
            snapshot
                .get_limited::<spatial::BlockHeader>(
                    &spatial::directory(coord),
                    s.shared.limits.bytes,
                )
                .await?,
        )
        .map_err(anyhow::Error::from)?,
    ))
}
#[derive(Deserialize)]
struct PiecePageQuery {
    revision: ContentId,
    after: Option<String>,
    limit: Option<usize>,
}
async fn block_pieces(
    State(s): State<AppState>,
    h: HeaderMap,
    Path((id, x, y, z)): Path<(String, i64, i64, i64)>,
    Query(q): Query<PiecePageQuery>,
) -> SharedResult<Json<serde_json::Value>> {
    let limit = q.limit.unwrap_or(64);
    ensure!((1..=128).contains(&limit), "Page limit must be 1..128");
    let snapshot = snapshot(&s, &id, &h, Some(&q.revision)).await?;
    let coord = [x, y, z].into();
    let after = q
        .after
        .as_deref()
        .map(str::parse::<OctreePath>)
        .transpose()?;
    let page = snapshot
        .scan_page(&block_path(coord), after.as_ref(), limit)
        .await?;
    let mut rows = Vec::new();
    let mut remaining = s.shared.limits.bytes;
    for (_, cid) in page.leaves {
        let row: spatial::SpatialRef = snapshot.blob_limited(&cid, remaining).await?;
        remaining = remaining.saturating_sub(canonical_cbor(&row)?.len());
        rows.push(row);
    }
    Ok(Json(
        serde_json::json!({"revision":q.revision,"pieces":rows,"next":page.next.map(|p|p.to_string())}),
    ))
}
#[derive(Deserialize)]
struct BlockQuery {
    revision: ContentId,
    region: Option<String>,
    after: Option<String>,
    limit: Option<usize>,
}
#[derive(Serialize)]
struct BlockPage {
    revision: ContentId,
    blocks: Vec<spatial::BlockHeader>,
    next: Option<String>,
}
async fn blocks(
    State(s): State<AppState>,
    h: HeaderMap,
    Path(id): Path<String>,
    Query(q): Query<BlockQuery>,
) -> SharedResult<Json<BlockPage>> {
    let local: OctreePath = q.region.as_deref().unwrap_or("/").parse()?;
    ensure!(
        local.octants().len() <= 64,
        "Block query region is too deep"
    );
    let snapshot = snapshot(&s, &id, &h, Some(&q.revision)).await?;
    let mut region = vec![5];
    region.extend(local.octants());
    let region = OctreePath::new(region)?;
    let after = q
        .after
        .as_deref()
        .map(str::parse::<OctreePath>)
        .transpose()?;
    let limit = q.limit.unwrap_or(64);
    ensure!(
        (1..=128).contains(&limit),
        "Block page limit must be 1..128"
    );
    let page = snapshot
        .scan_level(&region, 65, after.as_ref(), limit)
        .await?;
    let mut blocks = Vec::new();
    for (path, _) in page.leaves {
        blocks.push(spatial::BlockHeader {
            coord: spatial::coordinate(&path)?,
            paged: true,
        });
    }
    Ok(Json(BlockPage {
        revision: q.revision,
        blocks,
        next: page.next.map(|p| p.to_string()),
    }))
}
#[derive(Deserialize)]
struct HistoryQuery {
    cursor: Option<ContentId>,
    limit: Option<usize>,
}
#[derive(Serialize)]
struct HistoryEntry {
    revision: ContentId,
    commit: ergo_transactions::Commit,
    audit: Option<Audit>,
}
#[derive(Serialize)]
struct HistoryPage {
    entries: Vec<HistoryEntry>,
    next: Option<ContentId>,
    retained_from: u64,
}
async fn history(
    State(s): State<AppState>,
    h: HeaderMap,
    Path(id): Path<String>,
    Query(q): Query<HistoryQuery>,
) -> SharedResult<Json<HistoryPage>> {
    let limit = q.limit.unwrap_or(20);
    ensure!((1..=100).contains(&limit), "History limit must be 1..100");
    let (repo, _pin, _) = s.shared.authorized(&id, &reader(&s, &h)?).await?;
    let page = repo.revisions("main", q.cursor.as_ref(), limit).await?;
    let mut entries = Vec::new();
    let mut remaining = s.shared.limits.bytes;
    for id in page.revisions {
        let snapshot = repo.snapshot(&id).await?;
        let bytes = canonical_cbor(snapshot.commit())?.len();
        if bytes > remaining {
            return Err(SharedError::working_limit("history page bytes"));
        }
        remaining -= bytes;
        let audit: Option<Audit> = match &snapshot.commit().metadata {
            Some(cid) => {
                let a = snapshot.blob_limited(cid, remaining).await?;
                remaining = remaining.saturating_sub(canonical_cbor(&a)?.len());
                Some(a)
            }
            None => None,
        };
        entries.push(HistoryEntry {
            revision: id,
            commit: snapshot.commit().clone(),
            audit,
        });
    }
    Ok(Json(HistoryPage {
        entries,
        next: page.next,
        retained_from: page.retained_from,
    }))
}
