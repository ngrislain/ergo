//! Read-only inventory of every stored construction against the installed
//! part library: alias part IDs, unknown parts and geometry that differs from
//! the library's pinned vertex stream. It never changes the head.
use super::*;
use futures::{StreamExt, TryStreamExt};
use sha2::{Digest, Sha256};

impl SharedWorlds {
    pub async fn part_inventory(&self, id: &str) -> Result<serde_json::Value> {
        let repo = self.repo(id)?;
        let snapshot = repo.checkout("main").await?;
        let result = async {
            let revision = snapshot.id().clone();
            // Per stored part string: occurrences, canonical ID and whether
            // every distinct mesh it uses has the library's vertex stream.
            let mut parts = BTreeMap::<String, (usize, BTreeSet<ContentId>)>::new();
            let mut pieces = 0usize;
            for branch in [0, 1] {
                let prefix = OctreePath::new(vec![3, branch])?;
                let mut after = None;
                loop {
                    let page = snapshot
                        .scan_page_prefetched(&prefix, after.as_ref(), 512)
                        .await?;
                    let mut rows = futures::stream::iter(page.leaves)
                        .map(|(_, cid)| {
                            let snapshot = &snapshot;
                            async move { snapshot.blob_limited::<Piece>(&cid, 1024 * 1024).await }
                        })
                        .buffered(READ_CONCURRENCY);
                    while let Some(piece) = rows.try_next().await? {
                        pieces += 1;
                        let entry = parts.entry(piece.geometry.part.clone()).or_default();
                        entry.0 += 1;
                        entry.1.insert(piece.mesh.clone());
                    }
                    after = page.next;
                    if after.is_none() {
                        break;
                    }
                }
            }
            let mut assemblies = 0usize;
            let mut alias_header_keys = BTreeSet::new();
            let mut after = None;
            loop {
                let page = snapshot
                    .scan_level_prefetched(&root(4), 87, after.as_ref(), 256)
                    .await?;
                for (path, _) in page.leaves {
                    if let Some(header) = snapshot
                        .get_limited::<paged::RulesHeader>(&path.child(0)?, self.limits.bytes)
                        .await?
                    {
                        assemblies += 1;
                        for part in header.meshes.keys() {
                            if ergo_world::catalog::canonical_id(part) != part {
                                alias_header_keys.insert(part.clone());
                            }
                        }
                    }
                }
                after = page.next;
                if after.is_none() {
                    break;
                }
            }
            let (mut alias, mut unknown, mut stale_geometry) = (vec![], vec![], vec![]);
            let mut canonical_current = 0usize;
            for (part, (count, meshes)) in &parts {
                let Some(entry) = ergo_world::part_library::get(part) else {
                    unknown.push(serde_json::json!({"part": part, "pieces": count}));
                    continue;
                };
                let canonical = &entry.catalogue.id;
                if canonical != part {
                    alias.push(serde_json::json!({"part": part, "canonical": canonical, "pieces": count}));
                }
                let mut differing = 0;
                for cid in meshes {
                    let mesh: ergo_world::geometry::Mesh =
                        snapshot.blob_limited(cid, 32 * 1024 * 1024).await?;
                    let mut hash = Sha256::new();
                    for value in &mesh.vertices {
                        hash.update(ergo_world::geometry::vertex_bytes(*value));
                    }
                    if hex::encode(hash.finalize()) != entry.vertex_hash {
                        differing += 1;
                    }
                }
                if differing > 0 {
                    stale_geometry.push(serde_json::json!({
                        "part": part, "pieces": count, "meshes": meshes.len(), "differing_meshes": differing
                    }));
                } else if canonical == part {
                    canonical_current += count;
                }
            }
            Ok::<_, anyhow::Error>(serde_json::json!({
                "revision": revision,
                "library": ergo_world::part_library::library().revision,
                "pieces": pieces,
                "assemblies": assemblies,
                "distinct_parts": parts.len(),
                "pieces_canonical_and_current": canonical_current,
                "alias_parts": alias,
                "unknown_parts": unknown,
                "stale_geometry": stale_geometry,
                "alias_header_keys": alias_header_keys,
                "migration_needed": !(parts.keys().all(|p| ergo_world::catalog::canonical_id(p) == p)
                    && stale_geometry.is_empty() && alias_header_keys.is_empty()),
            }))
        }
        .await;
        snapshot.close().await?;
        result
    }
}
