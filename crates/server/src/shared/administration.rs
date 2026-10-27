//! Operator-only namespace migration/cleanup. No network route exposes these.
use super::*;

impl SharedWorlds {
    /// Install an already staged and semantically audited current-format history.
    /// The resume file makes the deployment cutover explicit while the world is
    /// fenced; cancellation leaves the barrier in place for operator recovery.
    pub async fn install_history(
        &self,
        id: &str,
        manifest: &std::path::Path,
    ) -> Result<serde_json::Value> {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Request {
            format: String,
            expected_head: ContentId,
            revisions: Vec<ContentId>,
            resume_file: std::path::PathBuf,
        }
        let request: Request = serde_json::from_slice(&std::fs::read(manifest)?)?;
        anyhow::ensure!(
            request.format == "ergo-history-install-1" && !request.revisions.is_empty(),
            "Invalid history installation manifest"
        );
        let repo = self.repo(id)?;
        let mut maintenance = repo.maintenance().await?;
        if maintenance.branch_revision("main").await? != request.expected_head {
            maintenance.finish().await?;
            anyhow::bail!(
                "The world changed after staging; rebuild the migration before deployment"
            );
        }
        let tip = request.revisions.last().unwrap();
        std::fs::write(
            manifest.with_extension("ready.json"),
            serde_json::to_vec_pretty(&serde_json::json!({
                "state":"fenced-ready","world_id":id,"expected_head":request.expected_head,"new_head":tip,"maintenance_token":maintenance.token()
            }))?,
        )?;
        loop {
            if std::fs::read_to_string(&request.resume_file)
                .is_ok_and(|value| value.trim() == tip.as_str())
            {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(200)).await;
        }
        let revision = maintenance
            .replace_history("main", &request.expected_head, &request.revisions)
            .await?;
        let gc = maintenance
            .collect_with_options(
                ergo_transactions::GcOptions {
                    max_object_bytes: self.limits.bytes,
                    ..Default::default()
                },
                blob_links,
            )
            .await?;
        maintenance.finish().await?;
        Ok(
            serde_json::json!({"state":"installed","world_id":id,"revision":revision,"retained_revisions":request.revisions.len(),"gc":gc}),
        )
    }
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
            .with_cache_capacity(VERIFIED_CACHE_BYTES)
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
}
