//! Bounded group discovery and complete descendant loading. Membership is never
//! a physical weld and a loaded group is not a complete mechanical domain.
use super::*;
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use ergo_world::groups::{Group, Member};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct QueryGroups {
    revision: ContentId,
    cursor: Option<String>,
    limit: Option<usize>,
}
#[derive(Serialize, Deserialize)]
struct GroupCursor {
    world: String,
    actor: String,
    assembly: ContentId,
    revision: ContentId,
    after: OctreePath,
}
fn private(value: serde_json::Value) -> Response {
    let mut r = Json(value).into_response();
    r.headers_mut()
        .insert("cache-control", "private, no-store".parse().unwrap());
    r
}

pub(super) async fn list(
    State(s): State<AppState>,
    h: HeaderMap,
    Path((id, assembly)): Path<(String, ContentId)>,
    Query(q): Query<QueryGroups>,
) -> SharedResult<Response> {
    let actor = reader(&s, &h)?.required()?;
    let limit = q.limit.unwrap_or(32);
    ensure!((1..=128).contains(&limit), "Group page limit must be 1–128");
    s.shared
        .head(
            &id,
            &Principal {
                id: Some(actor.clone()),
            },
            Some(&q.revision),
        )
        .await?;
    let snapshot = s
        .shared
        .repo(&id)?
        .accepted_snapshot("main", &q.revision)
        .await?;
    ensure!(
        snapshot
            .value_id(&paged::rule_path(&assembly, 0))
            .await?
            .is_some(),
        "Assembly is absent"
    );
    let after = if let Some(raw) = q.cursor {
        ensure!(raw.len() <= 4096, "Group cursor is too large");
        let c: GroupCursor =
            serde_json::from_slice(&URL_SAFE_NO_PAD.decode(raw).map_err(anyhow::Error::from)?)
                .map_err(anyhow::Error::from)?;
        ensure!(
            c.world == id && c.actor == actor && c.assembly == assembly && c.revision == q.revision,
            "Group cursor scope mismatch"
        );
        Some(c.after)
    } else {
        None
    };
    let prefix = paged::rule_path(&assembly, 5);
    let page = snapshot
        .scan_level(&prefix, prefix.octants().len() + 11, after.as_ref(), limit)
        .await?;
    let mut groups = Vec::new();
    for (base, _) in page.leaves {
        let header: Option<paged::GroupHeader> =
            snapshot.get_limited(&base.child(0)?, 4096).await?;
        let Some(header) = header else {
            ensure!(
                snapshot.scan_page(&base, None, 1).await?.leaves.is_empty(),
                "Missing group header"
            );
            continue;
        };
        ensure!(
            base == paged::row(&assembly, 5, u64::from(header.id)),
            "Group key mismatch"
        );
        groups.push(serde_json::json!({"id":header.id,"name":header.name,"pivot_world":ergo_world::coordinates::public(header.pivot_world),"members_loaded":false}));
    }
    let next = page.next.map(|after| {
        URL_SAFE_NO_PAD.encode(
            serde_json::to_vec(&GroupCursor {
                world: id.clone(),
                actor,
                assembly: assembly.clone(),
                revision: q.revision.clone(),
                after,
            })
            .expect("serializable cursor"),
        )
    });
    snapshot.close().await?;
    Ok(private(
        serde_json::json!({"world_id":id,"assembly_id":assembly,"base_revision":q.revision,"groups":groups,"next":next,"scope":"assembly_group_headers","geometry_loaded":false}),
    ))
}

impl SharedWorlds {
    pub async fn authoring_group(
        &self,
        id: &str,
        actor: &str,
        assembly: &ContentId,
        root_group: u32,
        revision: &ContentId,
    ) -> SharedResult<serde_json::Value> {
        let head = self
            .head(
                id,
                &Principal {
                    id: Some(actor.into()),
                },
                Some(revision),
            )
            .await?;
        let snapshot = self.repo(id)?.accepted_snapshot("main", revision).await?;
        #[derive(Deserialize)]
        struct Name {
            id: ContentId,
            name: String,
        }
        let header: Name = snapshot
            .get_limited(&paged::rule_path(assembly, 0), self.limits.bytes)
            .await?
            .ok_or_else(SharedError::missing)?;
        ensure!(header.id == *assembly, "Assembly identity mismatch");
        let mut stack = vec![(root_group, 0usize)];
        let mut seen = BTreeSet::new();
        let mut groups = Vec::new();
        let mut ids = BTreeSet::new();
        let mut records = 0usize;
        let mut bytes = 0usize;
        while let Some((group, depth)) = stack.pop() {
            ensure!(
                depth < 64 && seen.insert(group),
                "Cyclic/duplicate group closure"
            );
            if seen.len() > 512 {
                return Err(SharedError::working_limit("group closure count"));
            }
            let base = paged::row(assembly, 5, u64::from(group));
            let h: paged::GroupHeader = snapshot
                .get_limited(&base.child(0)?, 4096)
                .await?
                .ok_or_else(SharedError::missing)?;
            ensure!(h.id == group, "Group identity mismatch");
            let mut members = Vec::new();
            let mut after = None;
            loop {
                let page = snapshot
                    .scan_page(&base.child(1)?, after.as_ref(), 128)
                    .await?;
                for (path, cid) in page.leaves {
                    records += 1;
                    if records > 4096 {
                        return Err(SharedError::working_limit("group member closure"));
                    }
                    let member: Member = snapshot.blob_limited(&cid, 4096).await?;
                    let (kind, local) = match member {
                        Member::Piece { id } => (0, id),
                        Member::Group { id } => (1, id),
                    };
                    ensure!(
                        path == paged::append(
                            &base.child(1)?.child(kind)?,
                            &paged::number(u64::from(local))
                        ),
                        "Group member key mismatch"
                    );
                    match member {
                        Member::Piece { id } => {
                            ids.insert(id);
                        }
                        Member::Group { id } => stack.push((id, depth + 1)),
                    };
                    members.push(member);
                }
                if ids.len() > ergo_world::authoring::RESIDENT_PIECES {
                    return Err(SharedError::working_limit("group resident piece limit"));
                }
                after = page.next;
                if after.is_none() {
                    break;
                }
            }
            let g = Group {
                id: group,
                name: h.name,
                members,
                pivot_world: h.pivot_world,
            };
            bytes += canonical_cbor(&g)?.len();
            if bytes > ergo_world::authoring::PAGE_BYTES {
                return Err(SharedError::working_limit("group metadata bytes"));
            }
            groups.push(g);
        }
        ergo_world::groups::validate_ids(ids.iter().copied(), &groups)?;
        let mut joints = BTreeMap::new();
        let mut fixed = Vec::new();
        let mut boundary_pieces = BTreeSet::new();
        for &local in &ids {
            let member: paged::MemberRow = snapshot
                .get_limited(&paged::row(assembly, 1, u64::from(local)), 4096)
                .await?
                .ok_or_else(SharedError::missing)?;
            ensure!(member.local_id == local, "Member key mismatch");
            if !snapshot
                .scan_page(&super::joints::incident(&member.piece), None, 1)
                .await?
                .leaves
                .is_empty()
            {
                boundary_pieces.insert(local);
            }
            if snapshot
                .get_limited::<bool>(&paged::row(assembly, 4, u64::from(local)), 128)
                .await?
                .unwrap_or(false)
            {
                fixed.push(local);
            }
            let prefix = paged::row(assembly, 3, u64::from(local));
            let mut after = None;
            loop {
                let page = snapshot.scan_page(&prefix, after.as_ref(), 128).await?;
                for (_, cid) in page.leaves {
                    records += 1;
                    if records > 16384 {
                        return Err(SharedError::working_limit("group constraint records"));
                    }
                    let j: paged::BoundJoint = snapshot.blob_limited(&cid, 64 * 1024).await?;
                    bytes += canonical_cbor(&j)?.len();
                    if bytes > ergo_world::authoring::PAGE_BYTES {
                        return Err(SharedError::working_limit("group constraint bytes"));
                    }
                    ensure!(
                        (j.spec.a == local && j.a == member.piece)
                            || (j.spec.b == local && j.b == member.piece),
                        "Joint endpoint mismatch"
                    );
                    if ids.contains(&j.spec.a) && ids.contains(&j.spec.b) {
                        let pair = (j.spec.a.min(j.spec.b), j.spec.a.max(j.spec.b));
                        if let Some(old) = joints.get(&pair) {
                            ensure!(
                                canonical_cbor(old)? == canonical_cbor(&j.spec)?,
                                "Conflicting group joint records"
                            );
                        }
                        joints.insert(pair, j.spec);
                    } else {
                        boundary_pieces.insert(local);
                    }
                }
                after = page.next;
                if after.is_none() {
                    break;
                }
            }
        }
        let mut blocked_groups = Vec::new();
        for group in &groups {
            let (descendants, _) = ergo_world::groups::descendants(&groups, group.id)?;
            if descendants.iter().any(|id| boundary_pieces.contains(id)) {
                blocked_groups.push(group.id);
            }
        }
        let value = serde_json::json!({"world_id":id,"assembly_id":assembly,"base_revision":revision,"name":header.name,"geo":head.metadata.geo,"root_group_id":root_group,"groups":groups,"piece_ids":ids,"joints":joints.into_values().collect::<Vec<_>>(),"fixed_bricks":fixed,"blocked_transform_groups":blocked_groups,"group_complete":true,"physical_domain_complete":false});
        ensure!(
            serde_json::to_vec(&value)
                .map_err(anyhow::Error::from)?
                .len()
                <= ergo_world::authoring::PAGE_BYTES,
            "Group metadata exceeds page budget"
        );
        snapshot.close().await?;
        Ok(value)
    }
}

pub(super) async fn load(
    State(s): State<AppState>,
    h: HeaderMap,
    Path((id, assembly, group)): Path<(String, ContentId, u32)>,
    Query(q): Query<RevisionQuery>,
) -> SharedResult<Response> {
    let actor = reader(&s, &h)?.required()?;
    let revision = q
        .revision
        .context("A group read requires its pinned revision")?;
    Ok(private(
        s.shared
            .authoring_group(&id, &actor, &assembly, group, &revision)
            .await?,
    ))
}
