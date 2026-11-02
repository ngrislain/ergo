//! Operator-only, atomic cell-index migration. Accepted poses and identities are
//! not reauthored; old immutable revisions remain retained in normal history.
use super::*;
use futures::{StreamExt, TryStreamExt};

fn resized_region(region: &OctreePath, factor: i128) -> Result<Option<OctreePath>> {
    anyhow::ensure!(
        region.octants().len() <= 64 && factor > 0,
        "Invalid cell-region migration"
    );
    let mut origin = [0u128; 3];
    for (depth, q) in region.octants().iter().enumerate() {
        for (axis, value) in origin.iter_mut().enumerate() {
            *value |= u128::from((q >> axis) & 1) << (63 - depth);
        }
    }
    let side = 1i128 << (64 - region.octants().len());
    let lower = i128::from(i64::MIN);
    let upper = i128::from(i64::MAX) + 1;
    let lo = origin.map(|v| ((v as i128 - (1i128 << 63)) * factor).clamp(lower, upper));
    let hi = origin.map(|v| ((v as i128 - (1i128 << 63) + side) * factor).clamp(lower, upper));
    if (0..3).any(|a| lo[a] >= hi[a]) {
        return Ok(None);
    }
    let widths = std::array::from_fn::<_, 3, _>(|a| (hi[a] - lo[a]) as u128);
    anyhow::ensure!(
        widths.iter().all(|w| *w == widths[0]) && widths[0].is_power_of_two(),
        "Cell permission cover is not an exact cube"
    );
    let depth = 64 - widths[0].trailing_zeros() as usize;
    let address = OctreePath::for_block(lo.map(|v| v as i64));
    Ok(Some(OctreePath::new(address.octants()[..depth].to_vec())?))
}

impl SharedWorlds {
    async fn discard_incompatible_cell_caches(&self, id: &str) -> Result<usize> {
        let mut deleted = 0usize;
        for suffix in [
            "cache/read-views/v1".to_owned(),
            format!("read-cache/metadata-v1/{id}"),
            "ground/blocks".to_owned(),
        ] {
            let ground = suffix == "ground/blocks";
            let prefix = self.store.path(&suffix);
            let exact = format!("{prefix}/");
            let backend = &self.store.backend;
            let mut tasks = backend
                .list(Some(&prefix))
                .map_err(anyhow::Error::from)
                .try_filter(move |meta| {
                    futures::future::ready(meta.location.as_ref().starts_with(&exact))
                })
                .map_ok(|meta| async move {
                    if ground && meta.size <= 128 * 1024 {
                        let bytes = backend.get(&meta.location).await?.bytes().await?;
                        if ciborium::from_reader::<ergo_world::terrain::GroundBlock, _>(
                            bytes.as_ref(),
                        )
                        .is_ok()
                        {
                            return Ok::<_, anyhow::Error>(0);
                        }
                    }
                    backend.delete(&meta.location).await?;
                    Ok(1)
                })
                .try_buffer_unordered(READ_CONCURRENCY);
            while let Some(count) = tasks.try_next().await? {
                deleted += count;
            }
        }
        Ok(deleted)
    }
    pub async fn resize_cells(&self, id: &str) -> Result<serde_json::Value> {
        let repo = self.repo(id)?;
        let mut tx = repo.begin("main").await?;
        tx.watch(&OctreePath::new(vec![])?).await?;
        let base = tx.base_id().clone();
        let snapshot = repo.accepted_snapshot("main", &base).await?;
        // The old format is decoded here only, never by the running read/edit
        // paths. No legacy mode survives after this atomic storage cutover.
        #[derive(Deserialize)]
        struct SourceMetadata {
            id: Option<String>,
            name: String,
            geo: ergo_world::GeoReference,
            model: ContentId,
            layout: u32,
            #[serde(default)]
            cell_size: Option<u32>,
        }
        let source: SourceMetadata = snapshot
            .get(&root(2))
            .await?
            .context("Missing world metadata")?;
        let old_size = source.cell_size.unwrap_or(1024);
        let target = ergo_world::BLOCK_SIZE as u32;
        if old_size == target && source.layout == 4 {
            snapshot.close().await?;
            let deleted = self.discard_incompatible_cell_caches(id).await?;
            return Ok(
                serde_json::json!({"already_current":true,"revision":base,"cell_size":target,"incompatible_caches_deleted":deleted}),
            );
        }
        anyhow::ensure!(
            old_size == 1024 && target == 128 && [2, 3].contains(&source.layout),
            "Only the reviewed 1024-to-128 cell migration is supported"
        );
        anyhow::ensure!(
            source.id.as_deref().is_none_or(|stored| stored == id),
            "Migration world identity mismatch"
        );
        let mut metadata = Metadata {
            id: Some(id.into()),
            name: source.name,
            geo: source.geo,
            model: source.model,
            layout: 4,
            cell_size: target,
        };
        let mut policy: Policy = snapshot
            .get(&root(0))
            .await?
            .context("Missing world policy")?;
        let factor = i128::from(old_size / target);
        for grant in &mut policy.grants {
            let mut regions = BTreeSet::new();
            for region in &grant.regions {
                if let Some(region) = resized_region(region, factor)? {
                    regions.insert(region);
                }
            }
            grant.regions = regions.into_iter().collect();
        }
        policy.grants.retain(|g| !g.regions.is_empty());
        policy.validate()?;
        tx.remove(&root(1)).await?;
        tx.remove(&root(5)).await?;
        tx.remove(&OctreePath::new(vec![3, 2])?).await?;
        let mut meshes = BTreeMap::<ContentId, ergo_world::geometry::Mesh>::new();
        let mut blocks = BTreeSet::new();
        let (mut pieces, mut bytes) = (0usize, 0usize);
        // Only the canonical /3/{0,1} branches; /3/2 contains old read projections.
        for branch in [0, 1] {
            let prefix = OctreePath::new(vec![3, branch])?;
            let mut after = None;
            loop {
                let page = snapshot
                    .scan_page_prefetched(&prefix, after.as_ref(), 512)
                    .await?;
                let mut rows = futures::stream::iter(page.leaves)
                    .map(|(path, cid)| {
                        let snapshot = &snapshot;
                        async move {
                            let piece: Piece = snapshot.blob_limited(&cid, 1024 * 1024).await?;
                            Ok::<_, anyhow::Error>((path, piece))
                        }
                    })
                    .buffered(READ_CONCURRENCY);
                while let Some((path, mut piece)) = rows.try_next().await? {
                    anyhow::ensure!(
                        path == entity_path(3, &piece.id),
                        "Canonical piece path mismatch"
                    );
                    pieces += 1;
                    bytes += canonical_cbor(&piece)?.len();
                    anyhow::ensure!(
                        pieces <= 1_000_000 && bytes <= 512 * 1024 * 1024,
                        "Cell resize exceeds its bounded operator budget"
                    );
                    if !meshes.contains_key(&piece.mesh) {
                        let mesh: ergo_world::geometry::Mesh =
                            snapshot.blob_limited(&piece.mesh, 32 * 1024 * 1024).await?;
                        bytes += canonical_cbor(&mesh)?.len();
                        anyhow::ensure!(
                            bytes <= 512 * 1024 * 1024,
                            "Cell resize mesh budget exceeded"
                        );
                        meshes.insert(piece.mesh.clone(), mesh);
                    }
                    let brick = piece.geometry.brick(1, &meshes[&piece.mesh])?;
                    piece.replicas = ergo_world::partition(&[brick.clone()])?
                        .into_keys()
                        .collect();
                    tx.put(&path, &piece).await?;
                    for &coord in &piece.replicas {
                        let (path, row) = spatial::record(&piece, coord, brick.render_bounds);
                        tx.put(&path, &row).await?;
                        blocks.insert(coord);
                    }
                }
                tracing::info!(
                    pieces,
                    metadata_bytes = bytes,
                    blocks = blocks.len(),
                    "Cell resize staged progress"
                );
                after = page.next;
                if after.is_none() {
                    break;
                }
            }
        }
        for &coord in &blocks {
            tx.put(
                &spatial::directory(coord),
                &spatial::BlockHeader {
                    coord,
                    paged: true,
                    packed: false,
                },
            )
            .await?;
        }
        metadata.cell_size = target;
        metadata.layout = 4; // Old 1024-cell writers reject this layout.
        tx.put(&root(2), &metadata).await?;
        let policy_hash = tx.put(&root(0), &policy).await?;
        tx.set_metadata(&Audit {
            actor: "storage-operator".into(),
            actor_name: None,
            base: base.clone(),
            policy_hash,
            model: metadata.model.clone(),
            changed_pieces: BTreeSet::new(),
            affected_pieces: BTreeSet::new(),
            watched_regions: BTreeSet::from([OctreePath::new(vec![])?]),
            validation: None,
            loaded_pieces: pieces,
            loaded_cbor_bytes: bytes,
        })?;
        self.advertise_blocks(id, metadata.geo, &blocks).await?;
        snapshot.close().await?;
        tracing::info!(pieces, blocks=blocks.len(), previous_revision=%base, "Publishing atomic 128-cell resize");
        let accepted = tx
            .commit(
                "storage-operator",
                "Resize cell indexes from 1024 to 128 units",
                CommitOptions::default(),
            )
            .await?;
        let revision = accepted.snapshot.id().clone();
        accepted.snapshot.close().await?;
        let cache_cleanup = match self.discard_incompatible_cell_caches(id).await {
            Ok(deleted) => serde_json::json!({"deleted":deleted}),
            Err(error) => {
                serde_json::json!({"warning":error.to_string(),"migration_already_published":true})
            }
        };
        Ok(
            serde_json::json!({"revision":revision,"previous_revision":base,"cell_size":target,
            "height_plates":ergo_world::BLOCK_HEIGHT_PLATES,"pieces":pieces,"blocks":blocks.len(),
            "geometry_and_identities_preserved":true,"permission_regions_rescaled_exactly":true,
            "packed_projection":"unpacked; pack individual cells normally","incompatible_cache_cleanup":cache_cleanup}),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn cell_permission_prefixes_preserve_exact_physical_regions() {
        for coordinate in [[0, 0, 0], [-1, -1, -1], [100, -200, 300]] {
            let original = OctreePath::for_block(coordinate);
            let resized = resized_region(&original, 8).unwrap().unwrap();
            assert_eq!(resized.octants().len(), 61);
            for dx in 0..8 {
                let new = OctreePath::for_block([
                    coordinate[0] * 8 + dx,
                    coordinate[1] * 8 + 7,
                    coordinate[2] * 8,
                ]);
                assert!(new.octants().starts_with(resized.octants()));
            }
            let outside = OctreePath::for_block([
                coordinate[0] * 8 + 8,
                coordinate[1] * 8,
                coordinate[2] * 8,
            ]);
            assert!(!outside.octants().starts_with(resized.octants()));
        }
        assert_eq!(
            resized_region(&OctreePath::new(vec![]).unwrap(), 8)
                .unwrap()
                .unwrap()
                .octants()
                .len(),
            0
        );
    }
}
