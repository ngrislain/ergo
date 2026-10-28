//! Transactional block-read projection. Canonical pieces and mechanical spatial
//! predicates remain authoritative; these bounded pages avoid per-brick view GETs.
use super::*;
use ergo_world::BlockCoord;
use futures::{StreamExt, TryStreamExt};

pub(super) const PAGE_BYTES: usize = 1024 * 1024;
pub(super) const PAGE_PIECES: usize = 1024;
const UPDATE_BYTES: usize = 64 * 1024 * 1024;
pub(super) const FORMAT: &str = "ergo-spatial-page-1";

pub(super) fn block_path(coord: BlockCoord) -> OctreePath {
    // Entity CID addresses use only children 0/1 after their kind octant.
    // Child 2 is disjoint from all canonical /3 piece addresses.
    let mut path = vec![3, 2];
    path.extend(OctreePath::for_block(coord.into()).octants());
    OctreePath::new(path).unwrap()
}
fn shard(id: &ContentId) -> u8 {
    u8::from_str_radix(&id.as_str()[..2], 16).unwrap()
}
fn page_path(coord: BlockCoord, shard: u8) -> OctreePath {
    paged::append(
        &block_path(coord),
        &[shard >> 6, (shard >> 3) & 7, shard & 7],
    )
}

#[derive(Clone, Serialize, Deserialize)]
pub(super) struct Page {
    format: String,
    coord: BlockCoord,
    shard: u8,
    pub pieces: BTreeMap<ContentId, Piece>,
}
impl Page {
    fn empty(coord: BlockCoord, shard: u8) -> Self {
        Self {
            format: FORMAT.into(),
            coord,
            shard,
            pieces: BTreeMap::new(),
        }
    }
    fn check(&self, coord: BlockCoord) -> anyhow::Result<()> {
        anyhow::ensure!(
            self.format == FORMAT && self.coord == coord && self.pieces.len() <= PAGE_PIECES,
            "Invalid spatial page header"
        );
        for (id, piece) in &self.pieces {
            anyhow::ensure!(
                *id == piece.id && shard(id) == self.shard && piece.replicas.contains(&coord),
                "Spatial page membership mismatch"
            );
        }
        Ok(())
    }
}

#[derive(Default)]
pub(super) struct Updates {
    pages: BTreeMap<(BlockCoord, u8), Page>,
    disabled: BTreeSet<BlockCoord>,
    loaded_bytes: usize,
}
impl Updates {
    async fn disable(
        &mut self,
        tx: &mut ergo_transactions::Transaction,
        coord: BlockCoord,
    ) -> SharedResult<()> {
        self.disabled.insert(coord);
        self.pages.retain(|(cell, _), _| *cell != coord);
        if let Some(mut header) = tx
            .get::<spatial::BlockHeader>(&spatial::directory(coord))
            .await?
        {
            header.packed = false;
            tx.put(&spatial::directory(coord), &header).await?;
        }
        tx.remove(&block_path(coord)).await?;
        Ok(())
    }
    pub async fn replace(
        &mut self,
        tx: &mut ergo_transactions::Transaction,
        old: Option<&Piece>,
        new: Option<&Piece>,
    ) -> SharedResult<()> {
        let id = &new.or(old).context("Empty spatial projection edit")?.id;
        let shard = shard(id);
        let cells: BTreeSet<_> = old
            .into_iter()
            .chain(new)
            .flat_map(|piece| piece.replicas.iter().copied())
            .collect();
        for coord in cells {
            if self.disabled.contains(&coord) {
                continue;
            }
            let header = tx
                .get::<spatial::BlockHeader>(&spatial::directory(coord))
                .await?
                .context("Missing block directory")?;
            if !header.packed {
                continue;
            }
            let key = (coord, shard);
            if !self.pages.contains_key(&key) {
                let loaded = tx
                    .get_limited::<Page>(&page_path(coord, shard), PAGE_BYTES)
                    .await;
                let page = match loaded {
                    Ok(Some(page)) if page.check(coord).is_ok() && page.shard == shard => page,
                    Ok(None) => Page::empty(coord, shard),
                    _ => {
                        self.disable(tx, coord).await?;
                        continue;
                    }
                };
                self.loaded_bytes += canonical_cbor(&page)?.len();
                if self.loaded_bytes > UPDATE_BYTES {
                    self.disable(tx, coord).await?;
                    continue;
                }
                self.pages.insert(key, page);
            }
            if let Some(piece) = new.filter(|piece| piece.replicas.contains(&coord)) {
                self.loaded_bytes += canonical_cbor(piece)?.len();
                if self.loaded_bytes > UPDATE_BYTES {
                    self.disable(tx, coord).await?;
                    continue;
                }
            }
            let page = self.pages.get_mut(&key).unwrap();
            page.pieces.remove(id);
            if let Some(piece) = new.filter(|piece| piece.replicas.contains(&coord)) {
                page.pieces.insert(id.clone(), piece.clone());
            }
            if page.pieces.len() > PAGE_PIECES {
                self.disable(tx, coord).await?;
            }
        }
        Ok(())
    }
    pub async fn flush(&mut self, tx: &mut ergo_transactions::Transaction) -> SharedResult<()> {
        let pages = std::mem::take(&mut self.pages);
        for ((coord, shard), page) in pages {
            if self.disabled.contains(&coord) {
                continue;
            }
            if canonical_cbor(&page)?.len() > PAGE_BYTES {
                self.disable(tx, coord).await?;
            } else if page.pieces.is_empty() {
                tx.remove(&page_path(coord, shard)).await?;
            } else {
                tx.put(&page_path(coord, shard), &page).await?;
            }
        }
        Ok(())
    }
}

/// One callback per bounded page; callers deduplicate block replicas and enforce
/// their existing operation budgets before constructing a scene.
pub(super) async fn read_block(
    snapshot: &Snapshot,
    coord: BlockCoord,
    mut consume: impl FnMut(Page) -> SharedResult<()>,
) -> SharedResult<bool> {
    let header = snapshot
        .get_limited::<spatial::BlockHeader>(&spatial::directory(coord), 4096)
        .await?;
    let Some(header) = header.filter(|header| header.packed) else {
        return Ok(false);
    };
    ensure!(header.coord == coord, "Block directory mismatch");
    let mut after = None;
    loop {
        let page = snapshot
            .scan_page_prefetched(&block_path(coord), after.as_ref(), 64)
            .await?;
        let mut values = futures::stream::iter(page.leaves)
            .map(|(path, cid)| async move {
                let value: Page = snapshot.blob_limited(&cid, PAGE_BYTES).await?;
                value.check(coord)?;
                ensure!(
                    page_path(coord, value.shard) == path,
                    "Spatial page path mismatch"
                );
                Ok::<_, SharedError>(value)
            })
            .buffered(READ_CONCURRENCY);
        while let Some(value) = values.try_next().await? {
            consume(value)?;
        }
        after = page.next;
        if after.is_none() {
            break;
        }
    }
    Ok(true)
}

impl SharedWorlds {
    /// One-block storage projection backfill. Geometry, births and physics are
    /// untouched; an optimistic whole-root watch rejects concurrent changes.
    pub async fn pack_block(&self, id: &str, coord: [i64; 3]) -> anyhow::Result<serde_json::Value> {
        anyhow::ensure!(
            coord.iter().all(|n| n.unsigned_abs() < 1_000_000_000),
            "Block coordinate out of range"
        );
        let coord: BlockCoord = coord.into();
        let repo = self.repo(id)?;
        let mut tx = repo.begin("main").await?;
        tx.watch(&OctreePath::new(Vec::new())?).await?;
        let base = tx.base_id().clone();
        let snapshot = repo.accepted_snapshot("main", &base).await?;
        let mut metadata: Metadata = snapshot
            .get(&root(2))
            .await?
            .context("Missing world metadata")?;
        let mut header: spatial::BlockHeader = snapshot
            .get(&spatial::directory(coord))
            .await?
            .context("Block is not allocated")?;
        anyhow::ensure!(header.coord == coord, "Block directory mismatch");
        if header.packed {
            anyhow::ensure!(
                metadata.layout >= 3,
                "Packed block requires spatial layout 3"
            );
            snapshot.close().await?;
            return Ok(serde_json::json!({"already_packed":true,"revision":base}));
        }
        let mut pages = BTreeMap::<u8, Page>::new();
        let mut ids = BTreeSet::new();
        let mut after = None;
        let mut bytes = 0usize;
        loop {
            let page = snapshot
                .scan_page_prefetched(&super::block_path(coord), after.as_ref(), 512)
                .await?;
            let mut records = futures::stream::iter(page.leaves)
                .map(|(path, _)| {
                    let snapshot = &snapshot;
                    async move {
                        let id = spatial::reference_piece(&path, coord)?;
                        let piece: Piece = snapshot
                            .get_limited(&entity_path(3, &id), PAGE_BYTES)
                            .await?
                            .context("Missing spatial piece")?;
                        ensure!(piece.id == id, "Spatial piece identity mismatch");
                        Ok::<_, SharedError>(piece)
                    }
                })
                .buffered(READ_CONCURRENCY);
            while let Some(piece) = records.try_next().await? {
                anyhow::ensure!(
                    piece.replicas.contains(&coord) && ids.insert(piece.id.clone()),
                    "Invalid or duplicate spatial membership"
                );
                bytes += canonical_cbor(&piece)?.len();
                anyhow::ensure!(
                    ids.len() <= self.view_limits.pieces && bytes <= self.view_limits.bytes,
                    "Block packing exceeds configured read budgets"
                );
                pages
                    .entry(shard(&piece.id))
                    .or_insert_with(|| Page::empty(coord, shard(&piece.id)))
                    .pieces
                    .insert(piece.id.clone(), piece);
            }
            after = page.next;
            if after.is_none() {
                break;
            }
        }
        for page in pages.values() {
            page.check(coord)?;
            anyhow::ensure!(
                canonical_cbor(page)?.len() <= PAGE_BYTES,
                "Spatial shard too large to pack"
            );
        }
        tx.remove(&block_path(coord)).await?;
        for (&shard, page) in &pages {
            tx.put(&page_path(coord, shard), page).await?;
        }
        header.packed = true;
        tx.put(&spatial::directory(coord), &header).await?;
        // Layout 3 is a writer fence: older layout-2-only servers reject it,
        // rather than editing canonical pieces without maintaining these pages.
        if metadata.layout < 3 {
            metadata.layout = 3;
            tx.put(&root(2), &metadata).await?;
        }
        let policy_hash = snapshot
            .value_id(&root(0))
            .await?
            .context("Missing world policy")?;
        tx.set_metadata(&Audit {
            actor: "storage-operator".into(),
            actor_name: None,
            base: base.clone(),
            policy_hash,
            model: metadata.model,
            changed_pieces: BTreeSet::new(),
            affected_pieces: BTreeSet::new(),
            watched_regions: BTreeSet::new(),
            validation: None,
            loaded_pieces: 0,
            loaded_cbor_bytes: bytes,
        })?;
        snapshot.close().await?;
        let accepted = tx
            .commit(
                "storage-operator",
                "Pack spatial block read projection",
                CommitOptions::default(),
            )
            .await?;
        Ok(
            serde_json::json!({"revision":accepted.snapshot.id(),"base_revision":base,"block":<[i64;3]>::from(coord),"pieces":ids.len(),"pages":pages.len(),"logical_bytes":bytes,"geometry_changed":false}),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    #[tokio::test]
    async fn projection_overflow_falls_back_without_rejecting_canonical_edits() {
        let repo =
            Repository::new(Arc::new(object_store::memory::InMemory::new()), "overflow").unwrap();
        repo.create("main").await.unwrap();
        let mut tx = repo.begin("main").await.unwrap();
        let coord: BlockCoord = [0, 0, 0].into();
        tx.put(
            &spatial::directory(coord),
            &spatial::BlockHeader {
                coord,
                paged: true,
                packed: true,
            },
        )
        .await
        .unwrap();
        let mut d = ergo_world::demo::Demo::new();
        d.add("3003", 4, 0., 0., 0.);
        let mut updates = Updates::default();
        for index in 0..=PAGE_PIECES {
            let piece = Piece {
                id: format!("00{index:062x}").parse().unwrap(),
                assembly: ContentId::of(b"assembly fixture"),
                local_id: index as u32 + 1,
                geometry: Geometry::of(&d.bricks[0]),
                mesh: ContentId::of(b"mesh fixture"),
                replicas: BTreeSet::from([coord]),
            };
            updates.replace(&mut tx, None, Some(&piece)).await.unwrap();
        }
        updates.flush(&mut tx).await.unwrap();
        assert!(
            !tx.get::<spatial::BlockHeader>(&spatial::directory(coord))
                .await
                .unwrap()
                .unwrap()
                .packed
        );
        assert!(
            tx.scan_page(&block_path(coord), None, 128)
                .await
                .unwrap()
                .leaves
                .is_empty()
        );
    }
}
