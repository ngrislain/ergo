//! Bounded whole-world read-projection migration, with canonical Merkle proofs.
use super::*;
use futures::{StreamExt, TryStreamExt};

/// Packing may change only /3/2 (read pages) and /5 (allocated-cell headers).
/// Compare the other Merkle subtrees without downloading every model payload.
async fn canonical_basis(snapshot: &Snapshot) -> Result<ContentId> {
    let prefixes = [
        vec![0],
        vec![1],
        vec![2],
        vec![3, 0],
        vec![3, 1],
        vec![4],
        vec![6],
        vec![7],
    ];
    let mut basis = Vec::new();
    for prefix in prefixes {
        let path = OctreePath::new(prefix)?;
        let page = snapshot
            .scan_level(&path, path.octants().len(), None, 1)
            .await?;
        basis.push((path, page.leaves));
    }
    Ok(ContentId::of(&canonical_cbor(&basis)?))
}

impl SharedWorlds {
    /// Scan all allocated cells, not one camera radius. Each cell is an atomic
    /// storage-only publication; memory and read limits remain per cell/page.
    /// `check` inventories the same pinned snapshot without changing the head.
    pub async fn pack_world(&self, id: &str, check: bool) -> Result<serde_json::Value> {
        let started = std::time::Instant::now();
        let repo = self.repo(id)?;
        let snapshot = repo.checkout("main").await?;
        let result = async {
            let metadata: Metadata = snapshot.get_limited(&root(2), self.view_limits.bytes).await?
                .context("Missing world metadata")?;
            metadata.require_current_cells()?;
            anyhow::ensure!(metadata.layout == 4 && metadata.id.as_deref().is_none_or(|stored| stored == id),
                "World packing requires the matching current layout-4 namespace");
            let basis = canonical_basis(&snapshot).await?;
            let mut revision = snapshot.id().clone();
            let mut after = None;
            let (mut cells, mut already, mut unpacked, mut packed) = (0usize, 0usize, 0usize, 0usize);
            loop {
                let page = snapshot.scan_page_prefetched(&root(5), after.as_ref(), 64).await?;
                let mut headers = futures::stream::iter(page.leaves).map(|(path, cid)| {
                    let snapshot = &snapshot;
                    async move {
                        let header: spatial::BlockHeader = snapshot.blob_limited(&cid, 4096).await?;
                        anyhow::ensure!(spatial::directory(header.coord) == path, "Block directory mismatch");
                        Ok::<_, anyhow::Error>(header)
                    }
                }).buffered(READ_CONCURRENCY);
                while let Some(header) = headers.try_next().await? {
                    cells += 1;
                    if header.packed {
                        already += 1;
                        continue;
                    }
                    unpacked += 1;
                    if check { continue; }
                    let value = self.pack_block_checked(id, header.coord.into(), Some(&revision)).await?;
                    revision = serde_json::from_value(value["revision"].clone())?;
                    packed += 1;
                    eprintln!("{}", serde_json::json!({"operation":"pack-world","cell":<[i64;3]>::from(header.coord),
                        "packed":packed,"visited":cells,"revision":revision,"pieces":value["pieces"],"pages":value["pages"],"elapsed_s":started.elapsed().as_secs_f64()}));
                }
                after = page.next;
                if after.is_none() { break; }
            }
            let current = repo.checkout("main").await?;
            let current_basis = canonical_basis(&current).await;
            let same_head = current.id() == &revision;
            let root_after = current.commit().root.clone();
            current.close().await?;
            anyhow::ensure!(same_head, "World changed during packing; inspect progress before resuming");
            anyhow::ensure!(current_basis? == basis, "Canonical Merkle subtrees changed during packing");
            Ok(serde_json::json!({"world":id,"check_only":check,"cells":cells,"already_packed":already,
                "unpacked_before":unpacked,"packed":packed,"fully_packed":unpacked==packed,
                "base_revision":snapshot.id(),"revision":revision,"root_before":snapshot.commit().root,"root_after":root_after,
                "canonical_basis":basis,"canonical_subtrees_unchanged":true,"elapsed_s":started.elapsed().as_secs_f64()}))
        }.await;
        snapshot.close().await?;
        result
    }
}
