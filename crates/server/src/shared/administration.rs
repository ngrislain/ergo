//! Operator-only namespace migration/cleanup. No network route exposes these.
use super::*;
use futures::TryStreamExt;

impl SharedWorlds {
    pub async fn bootstrap_local(&mut self, geo: ergo_world::GeoReference) -> Result<()> {
        self.common_root = Some(self.namespace("world")?);
        let repo = self.common_root.as_ref().unwrap();
        match repo.checkout("main").await {
            Ok(snapshot) => {
                let metadata: Metadata = snapshot
                    .get(&root(2))
                    .await?
                    .context("Missing sandbox metadata")?;
                self.common_world = Some(metadata.id.context("Missing local sandbox ID")?);
                snapshot.close().await?;
            }
            Err(ergo_transactions::Error::Store(object_store::Error::NotFound { .. })) => {
                let head = self
                    .create(
                        "development",
                        NewWorld {
                            name: "Ergo Sandbox".into(),
                            geo,
                            authenticated_editing: true,
                        },
                    )
                    .await?;
                self.common_world = Some(head.id);
            }
            Err(error) => return Err(error.into()),
        }
        Ok(())
    }
    pub(super) fn namespace(&self, relative: &str) -> Result<Repository> {
        let prefix = if self.store.prefix.is_empty() {
            relative.into()
        } else {
            format!("{}/{}", self.store.prefix, relative.trim_matches('/'))
        };
        Ok(Repository::new(self.store.backend.clone(), &prefix)?
            .with_cache_capacity(64 * 1024 * 1024)
            .with_coordinated_gc())
    }
    pub async fn migrate_namespace(&self, id: &str, target: &str) -> Result<serde_json::Value> {
        let source = self.repo(id)?;
        source.checkout("main").await?.close().await?;
        let mut maintenance = source.maintenance().await?;
        tracing::info!(
            source = source.prefix(),
            token = maintenance.token(),
            "Namespace migration barrier acquired; source stays fenced until rollout"
        );
        let gc = maintenance
            .collect_with_options(
                ergo_transactions::GcOptions {
                    max_object_bytes: self.limits.bytes,
                    ..Default::default()
                },
                blob_links,
            )
            .await?;
        let target = self.namespace(target)?;
        let migration = maintenance.migrate_to(&target, self.limits.bytes).await?;
        // Intentionally leave the old namespace fenced. New configuration must
        // point to the verified target before old keys can be retired.
        Ok(
            serde_json::json!({"gc":gc,"migration":migration,"source_maintenance_token":maintenance.token()}),
        )
    }

    pub async fn cleanup_legacy(&self, id: &str) -> Result<serde_json::Value> {
        let repo = self.repo(id)?;
        let expected = if self.store.prefix.is_empty() {
            "world".into()
        } else {
            format!("{}/world", self.store.prefix)
        };
        anyhow::ensure!(
            self.common_root.is_some() && repo.prefix() == expected,
            "Cleanup requires the migrated world namespace to be active"
        );
        let snapshot = repo.checkout("main").await?;
        let policy: Policy = snapshot
            .get(&root(0))
            .await?
            .context("Missing sandbox policy")?;
        let revision = snapshot.id().clone();
        snapshot.close().await?;
        let mut ws = self.working(id, &policy.owner, &revision).await?;
        if ws.metadata.name != "Ergo Sandbox" || ws.metadata.id.as_deref() != Some(id) {
            ws.metadata.name = "Ergo Sandbox".into();
            ws.metadata.id = Some(id.into());
            ws.tx.put(&root(2), &ws.metadata).await?;
            self.prepared(
                id,
                &policy.owner,
                revision,
                "Name the single Ergo Sandbox".into(),
                BTreeSet::new(),
                BTreeSet::new(),
                None,
                ws,
            )
            .await?
            .publish_with(self)
            .await?;
        } else {
            drop(ws);
        }
        let base = if self.store.prefix.is_empty() {
            String::new()
        } else {
            format!("{}/", self.store.prefix)
        };
        let old_shared = format!("{base}shared/");
        let old_worlds = format!("{base}worlds/");
        let development = format!("{base}users/development.json");
        let retired = std::env::var("ERGO_RETIRED_WORLD_IDS")
            .unwrap_or_default()
            .split(',')
            .filter(|id| !id.is_empty())
            .map(str::to_owned)
            .collect::<BTreeSet<_>>();
        // Preflight names before deleting anything. Unknown user constructions
        // must be inspected/migrated, not silently treated as old demo data.
        let mut inventory = self.store.backend.list(None);
        while let Some(object) = inventory.try_next().await? {
            let key = object.location.as_ref();
            if let Some(suffix) = key.strip_prefix(&old_shared) {
                let namespace = suffix.split('/').next().unwrap_or_default();
                anyhow::ensure!(
                    namespace == id || namespace == "_catalog" || retired.contains(namespace),
                    "Unmigrated legacy namespace {namespace}; inspect it before retirement"
                );
            }
            if let Some(suffix) = key.strip_prefix(&old_worlds) {
                anyhow::ensure!(
                    suffix.starts_with("examples/"),
                    "Unmigrated private snapshot {key}; preserve/import it before cleanup"
                );
            }
        }
        let old_repo = self.namespace(&format!("shared/{id}"))?;
        let old_head = object_store::path::Path::from(format!("{old_shared}{id}/refs/main.cbor"));
        if let Ok(source) = self.store.backend.get(&old_head).await {
            #[derive(Deserialize)]
            struct SavedHead {
                commit: ContentId,
            }
            let head: SavedHead = crate::store::decode(&source.bytes().await?)?;
            anyhow::ensure!(
                old_repo.maintenance_status().await?.exclusive.is_some(),
                "The old common namespace must remain fenced before retirement"
            );
            repo.accepted_snapshot("main", &head.commit)
                .await?
                .close()
                .await?;
        }
        let mut objects = self.store.backend.list(None);
        let mut deleted = 0usize;
        let mut bytes = 0u64;
        while let Some(object) = objects.try_next().await? {
            let key = object.location.as_ref();
            if key.starts_with(&old_shared)
                || key.starts_with(&old_worlds)
                || key.starts_with("ergo-transaction-tests/")
                || key.starts_with("ergo-transaction-bench/")
                || key == development
            {
                self.store.backend.delete(&object.location).await?;
                deleted += 1;
                bytes += object.size;
            }
        }
        repo.checkout("main").await?.close().await?;
        Ok(
            serde_json::json!({"active_prefix":repo.prefix(),"deleted_objects":deleted,"deleted_bytes":bytes}),
        )
    }
}
