//! Operator-only, revision-pinned dependency timing; no world edits or validation.
use super::*;

impl SharedWorlds {
    /// Exercise the exact complete authoring read without exposing its geometry
    /// or changing accepted state. The ordinary actor entitlement still applies.
    pub async fn profile_workspace(
        &self,
        id: &str,
        assembly: &ContentId,
        actor: &str,
        seconds: u64,
    ) -> SharedResult<serde_json::Value> {
        ensure!(
            (1..=1800).contains(&seconds),
            "Profile deadline must be 1–1800 seconds"
        );
        let head = self
            .head(
                id,
                &crate::auth::Principal {
                    id: Some(actor.into()),
                },
                None,
            )
            .await?;
        let started = std::time::Instant::now();
        let result = tokio::time::timeout(
            std::time::Duration::from_secs(seconds),
            self.authoring_scene(id, actor, assembly, &head.revision),
        )
        .await;
        let (complete, timed_out, error, pieces) = match result {
            Ok(Ok(value)) => (
                true,
                false,
                None,
                value["scene"]["bricks"].as_array().map(Vec::len),
            ),
            Ok(Err(error)) => (false, false, Some(error.to_string()), None),
            Err(_) => (false, true, None, None),
        };
        Ok(
            serde_json::json!({"read_only":true,"published":false,"world":id,"assembly":assembly,
            "revision":head.revision,"complete":complete,"timed_out":timed_out,"error":error,
            "pieces":pieces,"elapsed_milliseconds":started.elapsed().as_millis(),
            "cache":self.repo(id)?.cache_stats()}),
        )
    }
    pub async fn profile_dependencies(
        &self,
        id: &str,
        assembly: &ContentId,
        seconds: u64,
    ) -> SharedResult<serde_json::Value> {
        ensure!(
            (1..=1800).contains(&seconds),
            "Profile deadline must be 1–1800 seconds"
        );
        let repo = self.repo(id)?;
        let snapshot = repo.checkout("main").await?;
        let revision = snapshot.id().clone();
        let limits = Limits {
            pieces: 100_000,
            bytes: self.limits.bytes,
        };
        let mut ws = Working::new(repo.begin_at("main", &revision).await?, limits).await?;
        let started = std::time::Instant::now();
        let mut rules_ms = None;
        let mut prefetch_ms = None;
        let result = tokio::time::timeout(std::time::Duration::from_secs(seconds), async {
            let rules = ws.rules(assembly).await?;
            rules_ms = Some(started.elapsed().as_millis());
            let seeds = rules.pieces.values().cloned().collect::<BTreeSet<_>>();
            ws.prefetch_pieces(seeds.iter().cloned().collect()).await;
            prefetch_ms = Some(started.elapsed().as_millis());
            ws.warm_member_spatial(&seeds).await?;
            ws.discover(&seeds).await
        })
        .await;
        let (complete, timed_out, error, affected) = match result {
            Ok(Ok(ids)) => (true, false, None, Some(ids.len())),
            Ok(Err(error)) => (false, false, Some(error.message), None),
            Err(_) => (false, true, None, None),
        };
        let report = serde_json::json!({"read_only":true,"published":false,"authoritative_validation":false,
            "world":id,"assembly":assembly,"revision":revision,"complete":complete,"timed_out":timed_out,
            "error":error,"affected_pieces":affected,"loaded_pieces":ws.pieces.len(),"loaded_cbor_bytes":ws.loaded_bytes,
            "elapsed_milliseconds":started.elapsed().as_millis(),"rules_milliseconds":rules_ms,"prefetch_milliseconds":prefetch_ms,"discovery":ws.discovery_report(),"cache":repo.cache_stats(),"profile_piece_limit":limits.pieces,"profile_byte_limit":limits.bytes});
        ws.tx.close().await?;
        snapshot.close().await?;
        Ok(report)
    }
}
