//! Bounded private-snapshot discovery; legacy full-manifest listing remains a
//! compatibility route. New manifests are advertised before publication, and
//! results verify that the immutable manifest actually exists.
use crate::{auth::Principal, store::Store};
use anyhow::{Context, Result, ensure};
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD as B64};
use ergo_transactions::{ContentId, OctreePath, Repository};
use ergo_world::{Aabb, GeoReference, SCHEMA_VERSION, WorldManifest};
use futures::TryStreamExt;
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize)]
pub struct Summary {
    pub id: String,
    pub name: String,
    pub owner: String,
    pub brick_count: usize,
    pub bounds: Aabb,
    pub geo: GeoReference,
}
#[derive(Serialize, Deserialize, Default)]
struct Cursor {
    scope: String,
    slot: usize,
    revision: Option<ContentId>,
    after: Option<OctreePath>,
}
#[derive(Serialize)]
pub struct Page {
    pub items: Vec<Summary>,
    pub next: Option<String>,
}
fn repo(store: &Store, owner: &str) -> Result<Repository> {
    let prefix = if store.prefix.is_empty() {
        "_private_catalog".to_owned()
    } else {
        format!("{}/_private_catalog", store.prefix)
    };
    Ok(Repository::new(
        store.backend.clone(),
        &format!("{prefix}/{}", ContentId::of(owner.as_bytes())),
    )?
    .with_cache_capacity(4 * 1024 * 1024)
    .with_coordinated_gc())
}
pub async fn register(store: &Store, manifest: &WorldManifest) -> Result<()> {
    let summary = Summary {
        id: manifest.id.clone(),
        name: manifest.name.chars().take(120).collect(),
        owner: manifest.owner.clone(),
        brick_count: manifest.brick_count,
        bounds: manifest.bounds,
        geo: manifest.geo,
    };
    let id = ContentId::of(manifest.id.as_bytes());
    let path = super::shared::address::entity_path(1, &id);
    super::shared::catalog::upsert(&repo(store, &manifest.owner)?, vec![(path, summary)]).await
}
pub async fn page(
    store: &Store,
    principal: &Principal,
    limit: usize,
    cursor: Option<&str>,
) -> Result<Page> {
    ensure!((1..=128).contains(&limit), "Catalog limit must be 1..128");
    let scope = principal.id.as_deref().unwrap_or("guest");
    let mut cursor = if let Some(c) = cursor {
        ensure!(c.len() <= 8192, "Invalid cursor");
        let c: Cursor = serde_json::from_slice(&B64.decode(c)?)?;
        ensure!(c.scope == scope, "Cursor scope mismatch");
        c
    } else {
        Cursor {
            scope: scope.into(),
            ..Default::default()
        }
    };
    let owners: Vec<_> = std::iter::once("examples")
        .chain(principal.id.as_deref())
        .collect();
    let mut items = Vec::new();
    let mut scanned = 0;
    while cursor.slot < owners.len() && scanned < limit {
        let owner = owners[cursor.slot];
        let repo = repo(store, owner)?;
        let snapshot = if let Some(id) = &cursor.revision {
            repo.accepted_snapshot("main", id).await?
        } else {
            match repo.checkout("main").await {
                Ok(s) => {
                    cursor.revision = Some(s.id().clone());
                    s
                }
                Err(ergo_transactions::Error::Store(object_store::Error::NotFound { .. })) => {
                    cursor.slot += 1;
                    continue;
                }
                Err(e) => return Err(e.into()),
            }
        };
        let page = snapshot
            .scan_page(
                &OctreePath::new(vec![1])?,
                cursor.after.as_ref(),
                limit - scanned,
            )
            .await?;
        for (_, cid) in page.leaves {
            scanned += 1;
            let summary: Summary = snapshot.blob_limited(&cid, 64 * 1024).await?;
            ensure!(summary.owner == owner, "Catalog owner mismatch");
            let path = store.path(&format!("worlds/{owner}/{}/manifest.json", summary.id));
            match store.backend.head(&path).await {
                Ok(_) => items.push(summary),
                Err(object_store::Error::NotFound { .. }) => {}
                Err(e) => return Err(e.into()),
            }
        }
        cursor.after = page.next;
        if cursor.after.is_none() {
            cursor.slot += 1;
            cursor.revision = None;
        }
    }
    let next = if cursor.slot < owners.len() {
        Some(B64.encode(serde_json::to_vec(&cursor)?))
    } else {
        None
    };
    Ok(Page { items, next })
}
pub async fn rebuild(store: &Store, owner: &str) -> Result<()> {
    ensure!(
        !owner.is_empty()
            && owner
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'-'),
        "Invalid owner"
    );
    let mut objects = store
        .backend
        .list(Some(&store.path(&format!("worlds/{owner}"))));
    while let Some(object) = objects.try_next().await? {
        if !object.location.as_ref().ends_with("/manifest.json") {
            continue;
        }
        let result = store.backend.get(&object.location).await?;
        ensure!(
            result.meta.size <= 128 * 1024 * 1024,
            "Legacy manifest exceeds rebuild byte budget"
        );
        let value: serde_json::Value = serde_json::from_slice(&result.bytes().await?)?;
        if value.get("schema").and_then(|v| v.as_u64()) != Some(u64::from(SCHEMA_VERSION)) {
            continue;
        }
        let manifest: WorldManifest =
            serde_json::from_value(value).context("Invalid legacy manifest")?;
        ensure!(manifest.owner == owner, "Manifest owner mismatch");
        register(store, &manifest).await?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    #[tokio::test]
    async fn private_catalog_pages_summaries_and_enforces_principal_scope() {
        let store = Store {
            backend: Arc::new(object_store::memory::InMemory::new()),
            prefix: String::new(),
            kind: "test".into(),
            measurements: Arc::new(Default::default()),
            static_tolerances: Default::default(),
        };
        for n in 0..3 {
            let mut d = ergo_world::demo::Demo::new();
            d.add("3003", 4, 2., 0., 2.);
            let scene = ergo_world::WorldExport {
                schema: SCHEMA_VERSION,
                name: format!("world {n}"),
                geo: Default::default(),
                bricks: d.bricks,
                meshes: d.meshes,
                joints: Vec::new(),
                fixed_bricks: Vec::new(),
                groups: Vec::new(),
                model: None,
                ground: Vec::new(),
            };
            store
                .save(scene, "test".into(), Vec::new(), "alice".into())
                .await
                .unwrap();
        }
        let alice = Principal {
            id: Some("alice".into()),
        };
        let first = page(&store, &alice, 1, None).await.unwrap();
        assert_eq!(first.items.len(), 1);
        let cursor = first.next.unwrap();
        assert!(
            page(
                &store,
                &Principal {
                    id: Some("bob".into())
                },
                1,
                Some(&cursor)
            )
            .await
            .is_err()
        );
        let second = page(&store, &alice, 2, Some(&cursor)).await.unwrap();
        assert_eq!(second.items.len(), 2);
        assert_ne!(first.items[0].id, second.items[0].id);
    }
}
