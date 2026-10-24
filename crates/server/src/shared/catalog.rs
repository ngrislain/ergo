//! Non-authoritative discovery indexes are pre-advertised before publication.
//! Stale advertisements are harmless: every result checks current world policy
//! and the referenced allocated-block directory. All pages use ordered Merkle
//! indexes, not assumptions about ObjectStore::list ordering.
use super::*;
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD as B64};

#[derive(Clone, Serialize, Deserialize)]
struct Advertisement {
    world: String,
    coord: Option<ergo_world::BlockCoord>,
}
#[derive(Default, Serialize, Deserialize)]
struct Cursor {
    scope: String,
    slot: usize,
    directory_revision: Option<ContentId>,
    directory_after: Option<OctreePath>,
    tile: Option<String>,
    tile_revision: Option<ContentId>,
    after: Option<OctreePath>,
}
#[derive(Deserialize)]
pub(super) struct CatalogQuery {
    cursor: Option<String>,
    limit: Option<usize>,
    quad: Option<String>,
}
#[derive(Serialize)]
struct Item {
    world: String,
    revision: ContentId,
    metadata: Metadata,
    coord: Option<ergo_world::BlockCoord>,
}
#[derive(Serialize)]
pub(super) struct Page {
    items: Vec<Item>,
    next: Option<String>,
}

pub(crate) async fn upsert<T: Serialize>(
    repo: &Repository,
    values: Vec<(OctreePath, T)>,
) -> Result<()> {
    match repo.checkout("main").await {
        Ok(s) => s.close().await?,
        Err(ergo_transactions::Error::Store(object_store::Error::NotFound { .. })) => {
            match repo.create("main").await {
                Ok(s) => s.close().await?,
                Err(ergo_transactions::Error::BranchExists(_)) => {}
                Err(e) => return Err(e.into()),
            }
        }
        Err(e) => return Err(e.into()),
    }
    let mut tx = repo.begin("main").await?;
    for (path, value) in values {
        if tx.value_id(&path).await?.is_none() {
            tx.put(&path, &value).await?;
        }
    }
    tx.commit(
        "catalog",
        "Advertise derived directory entries",
        CommitOptions::default(),
    )
    .await?
    .snapshot
    .close()
    .await?;
    Ok(())
}
fn user_key(user: &str) -> String {
    ContentId::of(user.as_bytes()).to_string()
}
fn key<T: Serialize>(value: &T) -> Result<OctreePath> {
    Ok(entity_path(1, &ContentId::of(&canonical_cbor(value)?)))
}
fn quad(latitude: f64, longitude: f64) -> String {
    let n = 4096.;
    let x = (((longitude + 180.) / 360. * n).floor() as i64).rem_euclid(4096) as u32;
    let lat = latitude.clamp(-85.05112878, 85.05112878).to_radians();
    let y = ((1. - (lat.tan() + 1. / lat.cos()).ln() / std::f64::consts::PI) / 2. * n)
        .floor()
        .clamp(0., n - 1.) as u32;
    (0..12)
        .rev()
        .map(|bit| char::from(b'0' + ((x >> bit) & 1) as u8 + 2 * ((y >> bit) & 1) as u8))
        .collect()
}
fn tiles(geo: ergo_world::GeoReference, coord: ergo_world::BlockCoord) -> BTreeSet<String> {
    let c: [i64; 3] = coord.into();
    let mut result = BTreeSet::new();
    for dx in [0, 1] {
        for dy in [0, 1] {
            let p = ergo_world::coordinates::internal(glam::DVec3::new(
                (c[0] + dx) as f64 * ergo_world::BLOCK_SIZE,
                (c[1] + dy) as f64 * ergo_world::BLOCK_SIZE,
                0.,
            ));
            let [lat, lon] = geo.to_gps(p);
            result.insert(quad(lat, lon));
        }
    }
    result
}
impl SharedWorlds {
    fn catalog_repo(&self, suffix: &str) -> Result<Repository> {
        Ok(self.root.child(&format!("_catalog/{suffix}"))?)
    }
    pub(super) async fn advertise_users(
        &self,
        id: &str,
        users: impl IntoIterator<Item = String>,
    ) -> Result<()> {
        if self.common_world.is_some() || self.common_root.is_some() {
            return Ok(());
        }
        for user in users.into_iter().collect::<BTreeSet<_>>() {
            let repo = self.catalog_repo(&format!("users/{}", user_key(&user)))?;
            upsert(
                &repo,
                vec![(
                    key(&id)?,
                    Advertisement {
                        world: id.into(),
                        coord: None,
                    },
                )],
            )
            .await?;
        }
        Ok(())
    }
    pub(super) async fn advertise_blocks(
        &self,
        id: &str,
        geo: ergo_world::GeoReference,
        blocks: &BTreeSet<ergo_world::BlockCoord>,
    ) -> Result<()> {
        if self.common_world.is_some() || self.common_root.is_some() {
            return Ok(());
        }
        let mut buckets: BTreeMap<String, Vec<_>> = BTreeMap::new();
        for &coord in blocks {
            for q in tiles(geo, coord) {
                buckets.entry(q).or_default().push((
                    key(&(id, coord))?,
                    Advertisement {
                        world: id.into(),
                        coord: Some(coord),
                    },
                ));
            }
        }
        for (q, entries) in buckets {
            let slot = &q[..2];
            let dir = self.catalog_repo(&format!("geo-directory/{slot}"))?;
            let path = OctreePath::new(
                q.as_bytes()[2..]
                    .iter()
                    .map(|q| q - b'0')
                    .collect::<Vec<_>>(),
            )?;
            upsert(&dir, vec![(path, q.clone())]).await?;
            upsert(&self.catalog_repo(&format!("geo/{q}"))?, entries).await?;
        }
        Ok(())
    }
    async fn catalog_snapshot(
        &self,
        repo: &Repository,
        revision: &mut Option<ContentId>,
    ) -> SharedResult<Option<Snapshot>> {
        match revision {
            Some(id) => Ok(Some(repo.accepted_snapshot("main", id).await?)),
            None => match repo.checkout("main").await {
                Ok(s) => {
                    *revision = Some(s.id().clone());
                    Ok(Some(s))
                }
                Err(ergo_transactions::Error::Store(object_store::Error::NotFound { .. })) => {
                    Ok(None)
                }
                Err(e) => Err(e.into()),
            },
        }
    }
    async fn catalog_item(
        &self,
        advertisement: Advertisement,
        principal: &Principal,
    ) -> SharedResult<Option<Item>> {
        let authorized = self.authorized(&advertisement.world, principal).await;
        let (_, snapshot, _) = match authorized {
            Ok(v) => v,
            Err(e) if e.status == StatusCode::FORBIDDEN || e.status == StatusCode::NOT_FOUND => {
                return Ok(None);
            }
            Err(e) => return Err(e),
        };
        let metadata: Metadata = snapshot
            .get_limited(&root(2), self.limits.bytes)
            .await?
            .context("Missing metadata")?;
        if let Some(coord) = advertisement.coord {
            let path = if metadata.layout >= 2 {
                spatial::directory(coord)
            } else {
                block_path(coord)
            };
            if snapshot.value_id(&path).await?.is_none() {
                return Ok(None);
            }
        }
        Ok(Some(Item {
            world: advertisement.world,
            revision: snapshot.id().clone(),
            metadata,
            coord: advertisement.coord,
        }))
    }
    async fn catalog_page(&self, principal: &Principal, q: CatalogQuery) -> SharedResult<Page> {
        if let Some(id) = &self.common_world {
            let head = self.head(id, principal, None).await?;
            return Ok(Page {
                items: vec![Item {
                    world: id.clone(),
                    revision: head.revision,
                    metadata: head.metadata,
                    coord: None,
                }],
                next: None,
            });
        }
        let actor = principal
            .id
            .as_deref()
            .ok_or_else(SharedError::unauthorized)?;
        let limit = q.limit.unwrap_or(32);
        ensure!((1..=128).contains(&limit), "Catalog limit must be 1..128");
        let prefix = q.quad.as_deref();
        if let Some(p) = prefix {
            ensure!(
                p.len() <= 12 && p.bytes().all(|b| (b'0'..=b'3').contains(&b)),
                "Invalid geographic quad prefix"
            );
        }
        let scope = format!("{}:{}", user_key(actor), prefix.unwrap_or("user"));
        let mut cursor = if let Some(c) = q.cursor {
            ensure!(c.len() <= 8192, "Invalid cursor length");
            let bytes = B64.decode(c).map_err(anyhow::Error::from)?;
            let c: Cursor = serde_json::from_slice(&bytes).map_err(anyhow::Error::from)?;
            ensure!(c.scope == scope, "Cursor scope mismatch");
            c
        } else {
            Cursor {
                scope,
                ..Default::default()
            }
        };
        if let Some(tile) = &cursor.tile {
            ensure!(
                tile.len() == 12
                    && tile.bytes().all(|b| (b'0'..=b'3').contains(&b))
                    && prefix.is_some_and(|p| tile.starts_with(p)),
                "Invalid tile cursor"
            );
        }
        let mut items = Vec::new();
        let mut scanned = 0;
        let mut steps = 0;
        let mut finished = false;
        match prefix {
            None => {
                let repo = self.catalog_repo(&format!("users/{}", user_key(actor)))?;
                if let Some(snapshot) = self
                    .catalog_snapshot(&repo, &mut cursor.tile_revision)
                    .await?
                {
                    let page = snapshot
                        .scan_page(&root(1), cursor.after.as_ref(), limit)
                        .await?;
                    for (_, cid) in page.leaves {
                        let ad: Advertisement =
                            snapshot.blob_limited(&cid, self.limits.bytes).await?;
                        if let Some(item) = self.catalog_item(ad, principal).await? {
                            items.push(item);
                        }
                    }
                    cursor.after = page.next;
                    finished = cursor.after.is_none();
                } else {
                    finished = true;
                }
            }
            Some(prefix) => {
                let slots: Vec<String> = (0..16)
                    .map(|n| format!("{}{}", n / 4, n % 4))
                    .filter(|s| prefix.starts_with(s) || s.starts_with(prefix))
                    .collect();
                while scanned < limit && steps < 128 {
                    steps += 1;
                    if cursor.slot >= slots.len() {
                        finished = true;
                        break;
                    }
                    if cursor.tile.is_none() {
                        let repo =
                            self.catalog_repo(&format!("geo-directory/{}", slots[cursor.slot]))?;
                        let Some(snapshot) = self
                            .catalog_snapshot(&repo, &mut cursor.directory_revision)
                            .await?
                        else {
                            cursor.slot += 1;
                            continue;
                        };
                        let path = OctreePath::new(
                            prefix
                                .as_bytes()
                                .get(2..)
                                .unwrap_or_default()
                                .iter()
                                .map(|b| b - b'0')
                                .collect::<Vec<_>>(),
                        )?;
                        let page = snapshot
                            .scan_page(&path, cursor.directory_after.as_ref(), 1)
                            .await?;
                        if let Some((p, cid)) = page.leaves.into_iter().next() {
                            let tile: String = snapshot.blob(&cid).await?;
                            ensure!(
                                tile.starts_with(prefix) && tile.len() == 12,
                                "Invalid geographic directory"
                            );
                            cursor.directory_after = Some(p);
                            cursor.tile = Some(tile);
                        } else {
                            cursor.slot += 1;
                            cursor.directory_revision = None;
                            cursor.directory_after = None;
                            continue;
                        }
                    }
                    let repo =
                        self.catalog_repo(&format!("geo/{}", cursor.tile.as_ref().unwrap()))?;
                    if let Some(snapshot) = self
                        .catalog_snapshot(&repo, &mut cursor.tile_revision)
                        .await?
                    {
                        let page = snapshot
                            .scan_page(&root(1), cursor.after.as_ref(), limit - scanned)
                            .await?;
                        for (_, cid) in page.leaves {
                            scanned += 1;
                            let ad: Advertisement =
                                snapshot.blob_limited(&cid, self.limits.bytes).await?;
                            if let Some(item) = self.catalog_item(ad, principal).await? {
                                items.push(item);
                            }
                        }
                        cursor.after = page.next;
                        if cursor.after.is_some() {
                            break;
                        }
                    }
                    cursor.tile = None;
                    cursor.tile_revision = None;
                    cursor.after = None;
                }
            }
        }
        let next = if finished {
            None
        } else {
            Some(B64.encode(serde_json::to_vec(&cursor).map_err(anyhow::Error::from)?))
        };
        Ok(Page { items, next })
    }
    pub async fn rebuild_catalog(&self, id: &str) -> Result<()> {
        let repo = self.repo(id)?;
        let snapshot = repo.checkout("main").await?;
        let policy: Policy = snapshot.get(&root(0)).await?.context("Missing policy")?;
        let metadata: Metadata = snapshot.get(&root(2)).await?.context("Missing metadata")?;
        self.advertise_users(
            id,
            std::iter::once(policy.owner).chain(policy.grants.into_iter().map(|g| g.user)),
        )
        .await?;
        let mut after = None;
        let prefix = root(if metadata.layout >= 2 { 5 } else { 1 });
        loop {
            let page = snapshot
                .scan_level(&prefix, 65, after.as_ref(), 128)
                .await?;
            let blocks = page
                .leaves
                .into_iter()
                .map(|(p, _)| spatial::coordinate(&p))
                .collect::<SharedResult<BTreeSet<_>>>()?;
            self.advertise_blocks(id, metadata.geo, &blocks).await?;
            after = page.next;
            if after.is_none() {
                break;
            }
        }
        snapshot.close().await?;
        Ok(())
    }
}
pub(super) async fn list(
    State(s): State<AppState>,
    headers: HeaderMap,
    Query(q): Query<CatalogQuery>,
) -> SharedResult<Json<Page>> {
    Ok(Json(
        s.shared.catalog_page(&reader(&s, &headers)?, q).await?,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn block_footprints_use_public_south_and_north_corners() {
        let geo = ergo_world::GeoReference {
            latitude: 0.5 / 3600.,
            longitude: 0.,
            arcseconds_per_block: 1.,
        };
        let tiles = tiles(geo, [0, -1, 0].into());
        assert!(tiles.contains(&quad(-0.5 / 3600., 0.)));
        assert!(tiles.contains(&quad(0.5 / 3600., 0.)));
    }
}
