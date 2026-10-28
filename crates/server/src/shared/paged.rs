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
    id: u32,
    name: String,
    #[serde(with = "ergo_world::coordinates::vec3")]
    pivot_world: glam::DVec3,
}

pub async fn write(tx: &mut ergo_transactions::Transaction, rules: &Rules) -> SharedResult<()> {
    use futures::TryStreamExt;
    let stream = record_edits(rules);
    futures::pin_mut!(stream);
    while let Some(edit) = stream.try_next().await? {
        if let Some(value) = edit.value {
            tx.put(&edit.path, &value).await?;
        } else {
            tx.remove(&edit.path).await?;
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
        let adjacency=rules.adjacency().map_err(|e|ergo_transactions::Error::Encoding(e.to_string()))?;
        for(&local,id)in &rules.pieces{for peer in adjacency.get(id).into_iter().flatten(){yield Edit::put(append(&row(&rules.id,2,u64::from(local)),&entity_path(0,peer).octants()[1..]),peer)?;}}
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
        let page = ws.tx.scan_page(prefix, after.as_ref(), 128).await?;
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
            let h: GroupHeader = ws
                .read(&base.child(0)?)
                .await?
                .context("Missing group header")?;
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
