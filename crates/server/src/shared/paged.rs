//! Application record pages. Rule replacement is a bounded authoring operation;
//! ordinary geometry edits load only their member, adjacency, and joint rows.
use super::*;
use ergo_world::{
    JointSpec,
    groups::{Group, Member},
};

pub fn append(path: &OctreePath, suffix: &[u8]) -> OctreePath {
    let mut p = path.octants().to_vec();
    p.extend(suffix);
    OctreePath::new(p).unwrap()
}
pub fn number(n: u64) -> Vec<u8> {
    (0..22)
        .rev()
        .map(|q| ((u128::from(n) >> (q * 3)) & 7) as u8)
        .collect()
}
pub fn rule_path(id: &ContentId, kind: u8) -> OctreePath {
    entity_path(4, id).child(kind).unwrap()
}
pub fn row(id: &ContentId, kind: u8, n: u64) -> OctreePath {
    append(
        &rule_path(id, kind),
        &(0..11)
            .rev()
            .map(|q| ((n >> (q * 3)) & 7) as u8)
            .collect::<Vec<_>>(),
    )
}

#[derive(Clone, Serialize, Deserialize)]
pub struct RulesHeader {
    pub id: ContentId,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub creation: Option<Creation>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub piece_count: Option<usize>,
    pub meshes: BTreeMap<String, ContentId>,
    #[serde(deserialize_with = "current_paged")]
    pub paged: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_model: Option<ContentId>,
}
pub(super) fn current_paged<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> std::result::Result<bool, D::Error> {
    let paged = bool::deserialize(deserializer)?;
    if !paged {
        return Err(serde::de::Error::custom(
            "Only paged shared records are supported",
        ));
    }
    Ok(paged)
}
impl RulesHeader {
    pub fn of(r: &Rules) -> Self {
        Self {
            id: r.id.clone(),
            name: r.name.clone(),
            creation: r.creation.clone(),
            piece_count: Some(r.pieces.len()),
            meshes: r.meshes.clone(),
            paged: true,
            source_model: r.source_model.clone(),
        }
    }
}
#[derive(Clone, Serialize, Deserialize)]
pub struct MemberRow {
    pub local_id: u32,
    pub piece: ContentId,
}
#[derive(Clone, Serialize, Deserialize)]
pub struct BoundJoint {
    pub a: ContentId,
    pub b: ContentId,
    pub spec: JointSpec,
}
#[derive(Clone, Serialize, Deserialize)]
pub struct GroupHeader {
    pub(super) id: u32,
    pub(super) name: String,
    #[serde(with = "ergo_world::coordinates::vec3")]
    pub(super) pivot_world: glam::DVec3,
}

pub async fn write(tx: &mut ergo_transactions::Transaction, rules: &Rules) -> SharedResult<()> {
    let records = collect_records(tx, rules).await?;
    for row in records.iter() {
        let (path, value) = row?;
        if let Some(value) = value {
            tx.put(&path, &value).await?;
        } else {
            tx.remove(&path).await?;
        }
    }
    Ok(())
}

async fn collect_records(
    tx: &ergo_transactions::Transaction,
    rules: &Rules,
) -> SharedResult<ergo_transactions::spill::SpillMap<OctreePath, Option<ciborium::value::Value>>> {
    use futures::TryStreamExt;
    let mut records = tx.local_map()?;
    let stream = record_edits(rules);
    futures::pin_mut!(stream);
    while let Some(edit) = stream.try_next().await? {
        records.insert(edit.path, edit.value)?;
    }
    Ok(records)
}

/// Write `rules` over the stored `previous` rules of the same assembly: only
/// records that differ are put, and records that no longer exist are removed,
/// so a one-piece edit of a large assembly touches a handful of rows instead
/// of rewriting every member and adjacency row. Without `previous` (a new
/// assembly) every record is written.
pub async fn write_changes(
    tx: &mut ergo_transactions::Transaction,
    previous: Option<&Rules>,
    rules: &Rules,
) -> SharedResult<()> {
    let Some(previous) = previous else {
        return write(tx, rules).await;
    };
    ensure!(previous.id == rules.id, "Rules belong to another assembly");
    let mut old = collect_records(tx, previous).await?;
    let current = collect_records(tx, rules).await?;
    for row in current.iter() {
        let (path, value) = row?;
        let before = old.remove(&path)?;
        if before.as_ref() == Some(&value) {
            continue;
        }
        if let Some(value) = value {
            tx.put(&path, &value).await?;
        } else {
            tx.remove(&path).await?;
        }
    }
    let retained: BTreeSet<_> = rules.groups.iter().map(|g| g.id).collect();
    for group in previous.groups.iter().filter(|g| !retained.contains(&g.id)) {
        let base = row(&rules.id, 5, u64::from(group.id));
        // One directory tombstone keeps the removal generation fence without
        // retaining a headerless structural prefix in bounded directory scans.
        tx.remove(&base).await?;
        old.remove_prefix(base.octants())?;
    }
    for row in old.iter() {
        let (path, value) = row?;
        if value.is_some() {
            tx.remove(&path).await?;
        }
    }
    Ok(())
}

fn record_edits(
    rules: &Rules,
) -> impl futures::Stream<Item = ergo_transactions::Result<ergo_transactions::RecordEdit>> + '_ {
    async_stream::try_stream! {
        use ergo_transactions::RecordEdit as Edit;
        yield Edit::remove(entity_path(4,&rules.id));yield Edit::put(rule_path(&rules.id,0),&RulesHeader::of(rules))?;
        for(&local,id)in &rules.pieces{yield Edit::put(row(&rules.id,1,u64::from(local)),&MemberRow{local_id:local,piece:id.clone()})?;}
        for edge in rules.dependency_edges(){let(local,peer)=edge.map_err(|e|ergo_transactions::Error::Encoding(e.to_string()))?;let peer=&rules.pieces[&peer];yield Edit::put(append(&row(&rules.id,2,u64::from(local)),&entity_path(0,peer).octants()[1..]),peer)?;}
        for(i,spec)in rules.joints.iter().enumerate(){yield Edit::put(row(&rules.id,6,i as u64),spec)?;let joint=BoundJoint{a:rules.pieces[&spec.a].clone(),b:rules.pieces[&spec.b].clone(),spec:spec.clone()};for endpoint in[spec.a,spec.b]{yield Edit::put(append(&row(&rules.id,3,u64::from(endpoint)),&number(i as u64)),&joint)?;}}
        for &local in &rules.fixed_bricks{yield Edit::put(row(&rules.id,4,u64::from(local)),&true)?;}
        for g in &rules.groups{let base=row(&rules.id,5,u64::from(g.id));yield Edit::put(base.child(0)?,&GroupHeader{id:g.id,name:g.name.clone(),pivot_world:g.pivot_world})?;for member in &g.members{let(kind,id)=match member{Member::Piece{id}=>(0,*id),Member::Group{id}=>(1,*id)};yield Edit::put(append(&base.child(1)?.child(kind)?,&number(u64::from(id))),member)?;}}
    }
}

pub(super) async fn rows<T: serde::de::DeserializeOwned + Serialize>(
    ws: &mut working::Working,
    prefix: &OctreePath,
) -> SharedResult<Vec<T>> {
    use futures::{StreamExt, TryStreamExt};
    let mut after = None;
    let mut result = Vec::new();
    loop {
        let page = ws
            .tx
            .scan_page_prefetched(prefix, after.as_ref(), 1024)
            .await?;
        let limit = ws.remaining();
        let tx = &ws.tx;
        let values: Vec<T> = futures::stream::iter(page.leaves)
            .map(|(_, cid)| async move { tx.blob_limited(&cid, limit).await })
            .buffered(READ_CONCURRENCY)
            .try_collect()
            .await?;
        for value in values {
            ws.charge(&value)?;
            result.push(value);
        }
        after = page.next;
        if after.is_none() {
            break;
        }
    }
    Ok(result)
}

pub(super) async fn reconstruct(
    ws: &mut working::Working,
    header: &RulesHeader,
) -> SharedResult<Rules> {
    let members: Vec<MemberRow> = rows(ws, &rule_path(&header.id, 1)).await?;
    let pieces = members.into_iter().map(|m| (m.local_id, m.piece)).collect();
    let joints = rows(ws, &rule_path(&header.id, 6)).await?;
    let mut fixed_bricks = Vec::new();
    let mut after = None;
    loop {
        let page = ws
            .tx
            .scan_page(&rule_path(&header.id, 4), after.as_ref(), 128)
            .await?;
        for (path, _) in page.leaves {
            let n = path.octants()[path.octants().len() - 11..]
                .iter()
                .fold(0u128, |n, q| (n << 3) | u128::from(*q));
            fixed_bricks.push(u32::try_from(n).context("invalid fixed identity")?);
        }
        after = page.next;
        if after.is_none() {
            break;
        }
    }
    let mut groups = Vec::new();
    let mut after = None;
    let prefix = rule_path(&header.id, 5);
    loop {
        let page = ws
            .tx
            .scan_level(&prefix, prefix.octants().len() + 11, after.as_ref(), 128)
            .await?;
        for (base, _) in page.leaves {
            let header: Option<GroupHeader> = ws.read(&base.child(0)?).await?;
            let Some(h) = header else {
                // Record-level tombstones can retain an empty directory. It is
                // absent only when no live record remains; orphaned membership
                // is corruption, not an excuse to truncate the group inventory.
                ensure!(
                    ws.tx.scan_page(&base, None, 1).await?.leaves.is_empty(),
                    "Missing group header"
                );
                continue;
            };
            let members = rows(ws, &base.child(1)?).await?;
            groups.push(Group {
                id: h.id,
                name: h.name,
                members,
                pivot_world: h.pivot_world,
            });
        }
        after = page.next;
        if after.is_none() {
            break;
        }
    }
    Ok(Rules {
        id: header.id.clone(),
        name: header.name.clone(),
        creation: header.creation.clone(),
        pieces,
        meshes: header.meshes.clone(),
        joints,
        fixed_bricks,
        groups,
        source_model: header.source_model.clone(),
    })
}
